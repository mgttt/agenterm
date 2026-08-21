//! The public embedder surface (design D9 / D10 / D17).
//!
//! Frozen: `Engine::{new, new_with_host, with_options, set_fuel, set_timeout,
//! cancel_handle, check, eval, eval_with_scope, eval_file, compile,
//! compile_file}`, plus `Host`, `Value`, and `compile → Unsupported`.
//!
//! **`Engine` is `Send` and not `Sync`** (D17). It holds the owned
//! [`crate::ir::IrModule`], never a `rhai::AST`, and there is no thread-local
//! host — the two reasons rh could not previously be a library.

use std::path::Path;
use std::time::Duration;

use crate::backend::{Backend, CancelHandle, Limits, Scope};
use crate::host::{Host, NullHost};
use crate::interp::InterpBackend;
use crate::ir::IrModule;
use crate::lang_error::Error;
use crate::lower::lower_ast;
use crate::subset::validate_ast_lang;
use crate::value::Value;

/// Runtime knobs. CLI and embedder defaults differ on purpose (D22): a shell
/// tool must not inherit the untrusted-worker budget.
#[derive(Clone, Debug)]
pub struct Options {
    /// `None` = off. Embedder default `Some(1_000_000)`; CLI default `None`.
    pub fuel: Option<u64>,
    pub call_depth: usize,
    /// `RH_MAX_EXPR_DEPTH`.
    pub parse_expression_depth: usize,
    /// `ScriptBudgets.expression_depth`.
    pub runtime_expression_depth: usize,
    /// `None` = off. Embedder default `Some(2s)`; CLI default `None`.
    pub wall_time: Option<Duration>,
    pub output_bytes: usize,
    pub fs_read_cap: usize,
    /// `eval_file` defaults true; `eval` / `-e` default false (D21).
    pub entry_required: bool,
}

impl Options {
    /// Embedder defaults: fuel and wall-time **on**, matching
    /// `ScriptBudgets::default()` (1e6 operations, 2s).
    pub fn embedder() -> Self {
        Self {
            fuel: Some(1_000_000),
            call_depth: 64,
            parse_expression_depth: crate::check::RH_MAX_EXPR_DEPTH,
            runtime_expression_depth: 64,
            wall_time: Some(Duration::from_millis(2_000)),
            output_bytes: 64 * 1024,
            fs_read_cap: crate::host_api::RH_HOST_FS_READ_CAP as usize,
            entry_required: false,
        }
    }

    /// Shebang / CLI defaults: fuel and wall-time **off** (D22).
    pub fn cli() -> Self {
        Self {
            fuel: None,
            wall_time: None,
            entry_required: true,
            ..Self::embedder()
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self::embedder()
    }
}

/// An opaque compiled program. Holds the owned IR today; a blob later.
#[derive(Clone, Debug)]
pub struct Compiled {
    ir: IrModule,
}

pub struct Engine {
    backend: Box<dyn Backend>,
    host: Box<dyn Host>,
    options: Options,
    cancel: CancelHandle,
}

impl Engine {
    /// An engine with `StdHost` and embedder defaults (fuel 1e6, wall-time
    /// 2s). `StdHost` is Node-like and unrestricted for local use; pass a
    /// custom `Host` to `new_with_host` to sandbox.
    pub fn new() -> Self {
        Self::new_with_host(crate::host_std::StdHost::new())
    }

    /// An engine whose host implements **nothing**. Useful for evaluating
    /// untrusted pure-value programs, and for proving that the core-type
    /// method surface does not depend on a host.
    pub fn sandboxed() -> Self {
        Self::new_with_host(NullHost)
    }

    pub fn new_with_host(host: impl Host + 'static) -> Self {
        Self {
            backend: Box::new(InterpBackend),
            host: Box::new(host),
            options: Options::embedder(),
            cancel: CancelHandle::default(),
        }
    }

    #[must_use]
    pub fn with_options(mut self, options: Options) -> Self {
        self.options = options;
        self
    }

    pub fn set_fuel(&mut self, operations: Option<u64>) -> &mut Self {
        self.options.fuel = operations;
        self
    }

    pub fn set_timeout(&mut self, wall_time: Option<Duration>) -> &mut Self {
        self.options.wall_time = wall_time;
        self
    }

    pub fn cancel_handle(&self) -> CancelHandle {
        self.cancel.clone()
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    /// Parse, enforce Language 1, and lower. The `rhai::AST` is dropped before
    /// this returns — it is never stored on the `Engine`.
    ///
    /// This is **strict** Language 1: no `compat_validate`. The AgenTerm
    /// compatibility bypass stays in [`crate::check::check`].
    pub fn check(&self, source: &str) -> Result<(), Error> {
        self.lower(source).map(|_| ())
    }

    fn lower(&self, source: &str) -> Result<IrModule, Error> {
        let ast = crate::check::parse_rh_ast(source)?;
        validate_ast_lang(&ast)?;
        let ir = lower_ast(&ast)?;
        drop(ast);
        Ok(ir)
    }

    pub fn eval(&mut self, source: &str) -> Result<Value, Error> {
        let mut scope = Scope::new();
        self.eval_with_scope(source, &mut scope)
    }

    pub fn eval_with_scope(&mut self, source: &str, scope: &mut Scope) -> Result<Value, Error> {
        let ir = self.lower(source)?;
        self.run(&ir, scope)
    }

    pub fn eval_file(&mut self, path: &Path) -> Result<Value, Error> {
        let source = std::fs::read_to_string(path)
            .map_err(|error| Error::Io(format!("{}: {error}", path.display())))?;
        let ir = self.lower(&source)?;
        if self.options.entry_required && !ir.has_function("entry") {
            return Err(Error::runtime(format!(
                "{}: script must define `fn entry()`",
                path.display()
            )));
        }
        let mut scope = Scope::new();
        self.run(&ir, &mut scope)
    }

    fn run(&mut self, ir: &IrModule, scope: &mut Scope) -> Result<Value, Error> {
        let limits = Limits {
            fuel: self.options.fuel,
            wall_time: self.options.wall_time,
            cancel: self.cancel.clone(),
        };
        self.backend.eval(ir, scope, self.host.as_mut(), &limits)
    }

    /// Reserved AOT/JIT seam. **Always** `Unsupported` on default builds
    /// (D10): embedders must never depend on rustc, Cranelift, or `dlopen`.
    pub fn compile(&mut self, source: &str) -> Result<Compiled, Error> {
        let ir = self.lower(source)?;
        self.backend.compile(&ir)?;
        Ok(Compiled { ir })
    }

    pub fn compile_file(&mut self, path: &Path) -> Result<Compiled, Error> {
        let source = std::fs::read_to_string(path)
            .map_err(|error| Error::Io(format!("{}: {error}", path.display())))?;
        self.compile(&source)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl Compiled {
    pub fn eval(&self, engine: &mut Engine) -> Result<Value, Error> {
        let mut scope = Scope::new();
        engine.run(&self.ir, &mut scope)
    }
}
