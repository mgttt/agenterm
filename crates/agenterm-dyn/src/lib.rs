use std::collections::HashMap;
use std::ffi::c_void;

mod error;
mod eval;
#[cfg(unix)]
mod exec;
mod hosts;
mod native;
mod parse;
mod sym;
mod value;

pub use error::DynError;
pub use eval::{MAX_TOTAL_REPEAT_ITERATIONS, REPEAT_MAX};
#[cfg(unix)]
pub use exec::{
    BufferState, CodeBuffer, NameEntry, NameTable, aarch64_mov_x0_ret, x86_64_call_thunk,
    x86_64_mov_rax_ret,
};
pub use hosts::{
    ALL_CELLS, CU_ADJACENT_PROBE_CATALOG, CuAdjacentProbeCell, HostArch, HostCell, HostOs,
    LAYER3_CANDIDATES, LINUX_AARCH64, LINUX_ATSPI_EXISTENCE_LIBS, LINUX_X86_64, MACOS_AARCH64,
    MACOS_X86_64, PLATFORM_CANDIDATES, ProbeFact, SecondaryProbe, SizeProbe, SystemProbe,
    SystemProbeStatus, WINDOWS_AARCH64, WINDOWS_X86_64, cell, cu_adjacent_probe, live_cell,
};
pub use sym::Symbol;
pub use value::Value;

/// Maximum number of distinct bindings retained by one [`Dyn`] environment.
pub const MAX_BINDINGS: usize = 4_096;
/// Maximum number of distinct interned symbols retained by one [`Dyn`] environment.
pub const MAX_SYMBOLS: usize = 4_096;
/// Maximum UTF-8 byte length of a binding, symbol, library, or native symbol name.
pub const MAX_NAME_BYTES: usize = 255;

/// In-process live-native evaluation environment.
pub struct Dyn {
    interner: sym::Interner,
    pub(crate) bindings: HashMap<String, Value>,
    pub(crate) libs: native::LibraryCache,
}

impl Dyn {
    pub fn new() -> Self {
        Self {
            interner: sym::Interner::new(),
            bindings: HashMap::new(),
            libs: native::LibraryCache::new(),
        }
    }

    /// Intern `name` into a stable [`Symbol`].
    pub fn intern(&mut self, name: &str) -> Result<Symbol, DynError> {
        self.interner.intern(name)
    }

    /// Bind `name` to an existing native pointer/handle (for example a `winsize` buffer).
    ///
    /// Binding alone is safe: [`Dyn::eval`] cannot execute `dlcall`. If the
    /// pointer is later consumed by native code, all pointer obligations belong
    /// to the caller of [`Dyn::eval_native`].
    pub fn bind(&mut self, name: &str, ptr: *mut c_void) -> Result<(), DynError> {
        if name.is_empty() {
            return Err(DynError::InvalidBindingName);
        }
        Self::ensure_name(name)?;
        self.ensure_binding_capacity(name)?;
        self.bindings
            .insert(name.to_owned(), Value::Ptr(ptr as usize));
        Ok(())
    }

    /// Reject a new binding when the environment is full, while always permitting replacement.
    pub(crate) fn ensure_binding_capacity(&self, name: &str) -> Result<(), DynError> {
        if self.bindings.contains_key(name) || self.bindings.len() < MAX_BINDINGS {
            return Ok(());
        }
        Err(DynError::StateLimit {
            resource: "bindings",
            limit: MAX_BINDINGS,
        })
    }

    pub(crate) fn ensure_name(name: &str) -> Result<(), DynError> {
        if name.as_bytes().contains(&0) {
            return Err(DynError::NameContainsNul);
        }
        if name.len() <= MAX_NAME_BYTES {
            Ok(())
        } else {
            Err(DynError::NameTooLong {
                limit: MAX_NAME_BYTES,
            })
        }
    }

    /// Evaluate pure S-expression `source` in this environment.
    ///
    /// This safe entry point rejects an AST containing `dlcall` before any
    /// expression executes, including native forms hidden in dead branches.
    pub fn eval(&mut self, source: &str) -> Result<Value, DynError> {
        let expr = parse::parse(source)?;
        if parse::contains_dlcall(&expr) {
            return Err(DynError::NativeRequiresUnsafe);
        }
        let mut budget = eval::RepeatBudget::new();
        eval::eval_expr(self, &expr, &mut budget)
    }

    /// Evaluate source that may invoke the native `dlcall` primitive.
    ///
    /// # Safety
    /// The caller must supply the exact fixed, non-variadic C ABI for every
    /// native symbol (except the documented Unix `ioctl` compatibility
    /// case). Every `ptr` argument and result must be valid, correctly
    /// aligned, live for the native call, and obey the callee's aliasing and
    /// mutability requirements. The caller also owns library availability,
    /// thread-affinity, and all process/resource side effects of the symbol,
    /// including any required cleanup. `dlcall` does not validate these
    /// contracts and must not be used for arbitrary variadic APIs.
    pub unsafe fn eval_native(&mut self, source: &str) -> Result<Value, DynError> {
        let expr = parse::parse(source)?;
        let mut budget = eval::RepeatBudget::new();
        eval::eval_expr(self, &expr, &mut budget)
    }
}

impl Default for Dyn {
    fn default() -> Self {
        Self::new()
    }
}
