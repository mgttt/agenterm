//! Unified per-engine `ScriptEngineBackend` trait + static-dispatch enum.
//!
//! Trait-M1-M4 of `plan/design-script-engine-trait.md`. This module defines
//! the shared invocation types (§2.2), the `ScriptEngineBackend` trait
//! (§2.3), the per-engine impls, and the `ScriptEngine` static-dispatch enum
//! (§2.4). `script_worker.rs`'s `execute_inner` dispatches only through this
//! trait (Trait-M3, commit 50ab1f7e).
//!
//! Trait-M4 folded the lua and qjs engine-specific invocation logic
//! (host-function wiring, `agenterm_lua`/`agenterm_qjs` calls) directly into
//! `LuaEngineBackend`/`QjsEngineBackend`'s `check`/`execute` — the old
//! `try_execute_lua_invocation`/`try_execute_qjs_invocation` and their
//! `LuaInvocationOptions`/`QjsInvocationOptions`/`LuaInvocationResult`/
//! `QjsInvocationResult` types are deleted from `script_backend.rs`; nothing
//! outside this module's own tests referenced them (verified by grep across
//! `src/`, `tests/`, `crates/`).
//!
//! `RhEngineBackend` is the one exception: it still delegates to
//! `try_execute_rh_invocation` in `script_backend.rs`, which is NOT deleted
//! and NOT folded here. `crates/agenterm-rh/src/main.rs` — compiled as the
//! `agenterm-rh` *binary target of the root `agenterm` package* (see root
//! `Cargo.toml`'s `[[bin]] name = "agenterm-rh" path =
//! "crates/agenterm-rh/src/main.rs"`, distinct from the `agenterm-rh`
//! library crate under `crates/agenterm-rh/`) — calls
//! `agenterm::script_backend::try_execute_rh_invocation` directly and needs
//! its typed `agenterm_rh::RhError` return (it propagates the error with
//! `?` into its own `Result<_, RhError>`), which this trait's `String`-typed
//! `ScriptEngineError` cannot losslessly round-trip. Folding rh's logic into
//! `RhEngineBackend` while also keeping `try_execute_rh_invocation` for that
//! caller would mean maintaining the same logic twice; keeping the existing
//! thin-delegation shape for rh only avoids that duplication. See the M4
//! task report for the full grep trail.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::script_backend::{RhInvocationOptions, ScriptBackend, try_execute_rh_invocation};
use crate::script_protocol::{ScriptBudgets, ScriptOperation};

// ---------------------------------------------------------------------
// §2.2 — shared types
// ---------------------------------------------------------------------

/// Shared invocation options across all three engines. Replaced the
/// previously-duplicated `RhInvocationOptions`/`LuaInvocationOptions`/
/// `QjsInvocationOptions`. Trait-M4 deleted the lua/qjs versions entirely
/// (folded their logic in directly); `RhInvocationOptions` still exists in
/// `script_backend.rs` — `RhEngineBackend` below converts into it — because
/// `try_execute_rh_invocation` has a real external caller (see module doc).
#[derive(Clone, Debug, Default)]
pub struct ScriptInvocationOptions {
    pub project_root: Option<PathBuf>,
    pub arguments: Option<Value>,
    pub budgets: Option<ScriptBudgets>,
}

/// Unified invocation result. `value` is `Option<serde_json::Value>` for
/// all three engines — lua's native `i64` is widened via
/// `serde_json::Value::from` in `LuaEngineBackend::execute`.
#[derive(Debug)]
pub struct ScriptInvocationResult {
    pub stdout: String,
    pub value: Option<Value>,
}

/// Unified error type. Trait boundary collapses `agenterm_rh::RhError`
/// (typed enum) and lua/qjs's `String` down to `String` — see design §2.2
/// "哪里不吸收" for the rationale (lossy but not a new loss: callers
/// already flatten all three into `ScriptFailureCategory::Configuration`).
pub type ScriptEngineError = String;

/// Fleet bridge callback shared by all three engines: (operation_id,
/// params_json) -> result_json. Unified to `Arc` (absorbs rh's `Box` vs
/// lua/qjs's `Arc` asymmetry noted in design §1.3 finding 1); rh's adapter
/// wraps this `Arc` in a closure to hand to `script_rh_host::FleetBridgeFn`
/// (`Box`), since `script_rh_host.rs` itself is out of scope this phase.
pub type ScriptFleetBridgeFn = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

// ---------------------------------------------------------------------
// §2.3 — trait body
// ---------------------------------------------------------------------

/// Unified per-engine "single invocation" interface (check one source,
/// execute one source). Does not cover check-many/corpus-scan (already
/// unified at the `agenterm-script-common` crate level) or pack/qualify CLI
/// verbs (engine-specific pack shapes, see design §3 non-goals).
pub trait ScriptEngineBackend {
    /// The corresponding `ScriptBackend` variant.
    fn backend_id(&self) -> ScriptBackend;

    /// Entry-file extensions this engine claims, mirroring
    /// `ScriptBackend::from_entry_path`'s routing table.
    fn entry_extensions(&self) -> &'static [&'static str];

    /// Whether this engine is selected via `AGENTERM_SCRIPT_BACKEND`.
    /// Default implementation reads the global `ScriptBackend::from_env()`.
    fn enabled(&self) -> bool {
        ScriptBackend::from_env() == self.backend_id()
    }

    /// Check operation. `source` may be empty only for rh's cached-pack
    /// deployment shape (see design §1.2 item 2) — non-rh engines return
    /// `Err` on empty source without any special-casing at the trait level.
    fn check(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
    ) -> Result<(), ScriptEngineError>;

    /// Run/Eval operation. `ScriptOperation::Api` is short-circuited by
    /// `execute_inner`'s caller before reaching any backend (design §1.3
    /// finding 2) so it is not represented in this trait's surface.
    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError>;
}

/// Compile-time object-safety assertion (design §2.4 concludes the trait is
/// object-safe; this function's mere existence is the proof — it never runs).
#[allow(dead_code)]
fn _assert_object_safe(_backend: &dyn ScriptEngineBackend) {}

// ---------------------------------------------------------------------
// §4 Trait-M2 — per-engine thin adapters
// ---------------------------------------------------------------------

fn not_enabled_error(backend: ScriptBackend) -> ScriptEngineError {
    format!("{} backend not enabled", backend.as_str())
}

/// Build the `args_len`/`arg` host-function closures shared by the lua and
/// qjs backends from a script invocation's `arguments` value.
///
/// `LuaHostFunctions` and `QjsHostFunctions` declare `args_len`/`arg` with
/// structurally identical trait-object types: `Arc<dyn Fn() -> i64 + Send +
/// Sync>` for `args_len`, and `Arc<dyn Fn(i64) -> Result<String, String> +
/// Send + Sync>` for `arg` (aliased per-crate as `ArgFn`). Type aliases
/// don't create distinct types, so the same pair of closures can be
/// assigned directly to either engine's host-function struct.
///
/// Moved here (Trait-M4) from `script_backend.rs` — only `LuaEngineBackend`/
/// `QjsEngineBackend::execute` call this now.
type ScriptArgsAccessors = (
    Arc<dyn Fn() -> i64 + Send + Sync>,
    Arc<dyn Fn(i64) -> Result<String, String> + Send + Sync>,
);

fn script_args_accessors(arguments: Value) -> ScriptArgsAccessors {
    let args_for_len = arguments.clone();
    let args_for_arg = arguments;
    let args_len: Arc<dyn Fn() -> i64 + Send + Sync> =
        Arc::new(move || args_for_len.as_array().map(|a| a.len() as i64).unwrap_or(0));
    let arg: Arc<dyn Fn(i64) -> Result<String, String> + Send + Sync> =
        Arc::new(move |index: i64| {
            args_for_arg
                .as_array()
                .and_then(|a| a.get(index as usize))
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned())
                .ok_or_else(|| format!("argument {index} is unavailable"))
        });
    (args_len, arg)
}

/// rh engine adapter. Delegates to `try_execute_rh_invocation` — does not
/// re-derive native-pack resolution, host binding, or output-budget logic.
///
/// Unlike `LuaEngineBackend`/`QjsEngineBackend` (Trait-M4 folded their logic
/// in directly), this adapter is intentionally left delegating: see the
/// module doc comment for why (`crates/agenterm-rh/src/main.rs`'s bin
/// target is a real external caller of `try_execute_rh_invocation` that
/// needs its typed `agenterm_rh::RhError`).
pub struct RhEngineBackend;

impl ScriptEngineBackend for RhEngineBackend {
    fn backend_id(&self) -> ScriptBackend {
        ScriptBackend::Rh
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        &["rh", "rhai"]
    }

    fn check(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
    ) -> Result<(), ScriptEngineError> {
        let rh_options = RhInvocationOptions {
            project_root: options.project_root.clone(),
            arguments: options.arguments.clone(),
            budgets: options.budgets.clone(),
        };
        match try_execute_rh_invocation(ScriptOperation::Check, source, rh_options, None) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(not_enabled_error(self.backend_id())),
            Err(error) => Err(error.to_string()),
        }
    }

    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError> {
        let rh_options = RhInvocationOptions {
            project_root: options.project_root.clone(),
            arguments: options.arguments.clone(),
            budgets: options.budgets.clone(),
        };
        // rh's try_execute_rh_invocation wants Option<script_rh_host::FleetBridgeFn>
        // (Box<dyn Fn...>); wrap the shared Arc in a closure to bridge the two
        // smart-pointer types (design §2.2 — rh internal adapter does this, not
        // script_rh_host.rs itself).
        let rh_bridge: Option<crate::script_rh_host::FleetBridgeFn> = fleet_bridge.map(|bridge| {
            let boxed: crate::script_rh_host::FleetBridgeFn =
                Box::new(move |op_id: &str, params: &str| bridge(op_id, params));
            boxed
        });
        match try_execute_rh_invocation(ScriptOperation::Eval, source, rh_options, rh_bridge) {
            Ok(Some(result)) => Ok(ScriptInvocationResult {
                stdout: result.stdout,
                value: result.value,
            }),
            Ok(None) => Err(not_enabled_error(self.backend_id())),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Lua engine adapter. Invocation logic folded in directly (Trait-M4) from
/// the former `try_execute_lua_invocation` in `script_backend.rs` — no
/// other caller referenced it (grep-verified across `src/`, `tests/`,
/// `crates/`; `tests/lua_task_entry_regression.rs` referenced it too, but
/// that file was already failing to compile beforehand — pre-existing,
/// unrelated `ScriptBackend::Rhai` reference — so it did not count as a
/// live caller).
#[cfg(feature = "script-lua")]
pub struct LuaEngineBackend;

#[cfg(feature = "script-lua")]
impl ScriptEngineBackend for LuaEngineBackend {
    fn backend_id(&self) -> ScriptBackend {
        ScriptBackend::Lua
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        &["lua"]
    }

    fn check(
        &self,
        source: &str,
        _options: &ScriptInvocationOptions,
    ) -> Result<(), ScriptEngineError> {
        if !self.enabled() {
            return Err(not_enabled_error(self.backend_id()));
        }

        let engine = agenterm_lua::LuaEngine::new().map_err(|e| e.to_string())?;

        engine.check(source).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError> {
        if !self.enabled() {
            return Err(not_enabled_error(self.backend_id()));
        }

        let engine = agenterm_lua::LuaEngine::new().map_err(|e| e.to_string())?;

        let mut host = agenterm_lua::LuaHostFunctions::default();

        // Wire fleet bridge
        if let Some(bridge) = fleet_bridge {
            host.fleet_call = Some(Arc::new(
                move |op_id: String, params: String| -> Result<String, String> {
                    bridge(op_id.as_str(), params.as_str())
                },
            ));
        }

        // Wire args_len / arg from options.arguments
        if let Some(arguments) = options.arguments.clone() {
            let (args_len, arg) = script_args_accessors(arguments);
            host.args_len = Some(args_len);
            host.arg = Some(arg);
        }

        let result = engine
            .eval(source, &host)
            .map_err(|e| format!("lua_eval: {e}"))?;

        Ok(ScriptInvocationResult {
            stdout: result.stdout,
            value: Some(Value::from(result.value)),
        })
    }
}

/// qjs engine adapter. Invocation logic folded in directly (Trait-M4) from
/// the former `try_execute_qjs_invocation` in `script_backend.rs` — no
/// other caller referenced it (grep-verified across `src/`, `tests/`,
/// `crates/`).
///
/// Structurally mirrors `LuaEngineBackend` — same "not enabled -> error",
/// same fleet-bridge/args wiring shape — because qjs, like lua, is an
/// interpreted engine with no AOT/native-codegen step (unlike rh's
/// `RhEngineBackend`, which resolves/loads a compiled native pack). `value`
/// is `Option<serde_json::Value>` rather than lua's widened-from-`i64`
/// because `agenterm_qjs::eval_entry_with_host` already produces a typed
/// JSON value (via `JSON.stringify`) — a strict superset, not a divergence:
/// any lua-shaped i64 result is also representable here.
pub struct QjsEngineBackend;

impl ScriptEngineBackend for QjsEngineBackend {
    fn backend_id(&self) -> ScriptBackend {
        ScriptBackend::Qjs
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        &["js", "mjs"]
    }

    fn check(
        &self,
        source: &str,
        _options: &ScriptInvocationOptions,
    ) -> Result<(), ScriptEngineError> {
        if !self.enabled() {
            return Err(not_enabled_error(self.backend_id()));
        }

        agenterm_qjs::check(source, "invocation.js").map_err(|e| e.to_string())?;
        Ok(())
    }

    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError> {
        if !self.enabled() {
            return Err(not_enabled_error(self.backend_id()));
        }

        let mut host = agenterm_qjs::QjsHostFunctions::default();

        // Wire fleet bridge
        if let Some(bridge) = fleet_bridge {
            host.fleet_call = Some(Arc::new(
                move |op_id: &str, params: &str| -> Result<String, String> {
                    bridge(op_id, params)
                },
            ));
        }

        // Wire args_len / arg from options.arguments
        if let Some(arguments) = options.arguments.clone() {
            let (args_len, arg) = script_args_accessors(arguments);
            host.args_len = Some(args_len);
            host.arg = Some(arg);
        }

        let result = agenterm_qjs::eval_entry_with_host(source, "invocation.js", &host)
            .map_err(|e| format!("qjs_eval: {e}"))?;

        Ok(ScriptInvocationResult {
            stdout: result.stdout,
            value: result.value,
        })
    }
}

/// sql engine adapter — see `plan/design-sql-execution-target.md` (the M1
/// design doc this impl now implements) and
/// `crates/agenterm-sql/src/lib.rs`'s module doc (the honest "what's real
/// vs. placeholder" writeup this impl matches).
///
/// `check` is real: delegates to `agenterm_sql::check`, which really parses
/// via `sqlparser`. `execute` is ALSO real as of M1: delegates to
/// `agenterm_sql::execute_entry`, which runs `source` against a private,
/// in-process, ephemeral SQLite database (`rusqlite`, `bundled`) — see that
/// function's module doc for the full value-mapping/budget/dialect-skew
/// writeup. `fleet_bridge` remains unused here — M2 (host-state-as-virtual-
/// tables) is the design doc's own plan for wiring it in, not M1; §2.6 of
/// the earlier trait-design doc predicted "sql 不需要 trait 新增方法，只需要
/// execute 内部把 fleet_bridge 参数忽略掉", and that still holds for M1's
/// scope even though execute() itself is no longer a placeholder.
#[cfg(feature = "script-sql")]
pub struct SqlEngineBackend;

#[cfg(feature = "script-sql")]
impl ScriptEngineBackend for SqlEngineBackend {
    fn backend_id(&self) -> ScriptBackend {
        ScriptBackend::Sql
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        &["sql"]
    }

    fn check(
        &self,
        source: &str,
        _options: &ScriptInvocationOptions,
    ) -> Result<(), ScriptEngineError> {
        if !self.enabled() {
            return Err(not_enabled_error(self.backend_id()));
        }

        agenterm_sql::check(source, "invocation.sql").map_err(|e| e.to_string())?;
        Ok(())
    }

    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        _fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError> {
        if !self.enabled() {
            return Err(not_enabled_error(self.backend_id()));
        }

        // agenterm_sql::ExecuteBudgets is a small crate-local mirror of
        // ScriptBudgets (agenterm-sql can't depend on this crate's types —
        // see that struct's doc) covering only the M1-enforced subset:
        // wall_time_ms, collection_items, and output_bytes (standing in for
        // both output_bytes and string_bytes — see its doc for why).
        let sql_budgets = options
            .budgets
            .as_ref()
            .map(|budgets| agenterm_sql::ExecuteBudgets {
                wall_time_ms: budgets.wall_time_ms,
                collection_items: budgets.collection_items,
                output_bytes: budgets.output_bytes,
            });

        let outcome = agenterm_sql::execute_entry(source, "invocation.sql", sql_budgets.as_ref())
            .map_err(|e| e.to_string())?;

        Ok(ScriptInvocationResult {
            stdout: outcome.stdout,
            value: outcome.value,
        })
    }
}

// ---------------------------------------------------------------------
// §2.4 — enum static dispatch
// ---------------------------------------------------------------------

/// Static-dispatch registry over the three engines. Not a `dyn` trait
/// object list — see design §2.4 for why enum+match is preferred as the
/// default over `Box<dyn ScriptEngineBackend>` (the trait remains
/// object-safe as a documented, unused escape hatch).
pub enum ScriptEngine {
    Rh(RhEngineBackend),
    #[cfg(feature = "script-lua")]
    Lua(LuaEngineBackend),
    Qjs(QjsEngineBackend),
    #[cfg(feature = "script-sql")]
    Sql(SqlEngineBackend),
}

impl ScriptEngine {
    pub fn all() -> Vec<ScriptEngine> {
        #[allow(unused_mut)]
        let mut engines = vec![Self::Rh(RhEngineBackend), Self::Qjs(QjsEngineBackend)];
        #[cfg(feature = "script-lua")]
        engines.push(Self::Lua(LuaEngineBackend));
        #[cfg(feature = "script-sql")]
        engines.push(Self::Sql(SqlEngineBackend));
        engines
    }

    /// Construct the engine variant corresponding to `id`.
    pub fn for_backend(id: ScriptBackend) -> Self {
        match id {
            ScriptBackend::Rh => Self::Rh(RhEngineBackend),
            #[cfg(feature = "script-lua")]
            ScriptBackend::Lua => Self::Lua(LuaEngineBackend),
            ScriptBackend::Qjs => Self::Qjs(QjsEngineBackend),
            #[cfg(feature = "script-sql")]
            ScriptBackend::Sql => Self::Sql(SqlEngineBackend),
        }
    }
}

/// Free-function alias for `ScriptEngine::for_backend`, matching the task
/// brief's literal `engine_for(backend)` naming.
pub fn engine_for(backend: ScriptBackend) -> ScriptEngine {
    ScriptEngine::for_backend(backend)
}

impl ScriptEngineBackend for ScriptEngine {
    fn backend_id(&self) -> ScriptBackend {
        match self {
            Self::Rh(backend) => backend.backend_id(),
            #[cfg(feature = "script-lua")]
            Self::Lua(backend) => backend.backend_id(),
            Self::Qjs(backend) => backend.backend_id(),
            #[cfg(feature = "script-sql")]
            Self::Sql(backend) => backend.backend_id(),
        }
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Rh(backend) => backend.entry_extensions(),
            #[cfg(feature = "script-lua")]
            Self::Lua(backend) => backend.entry_extensions(),
            Self::Qjs(backend) => backend.entry_extensions(),
            #[cfg(feature = "script-sql")]
            Self::Sql(backend) => backend.entry_extensions(),
        }
    }

    fn check(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
    ) -> Result<(), ScriptEngineError> {
        match self {
            Self::Rh(backend) => backend.check(source, options),
            #[cfg(feature = "script-lua")]
            Self::Lua(backend) => backend.check(source, options),
            Self::Qjs(backend) => backend.check(source, options),
            #[cfg(feature = "script-sql")]
            Self::Sql(backend) => backend.check(source, options),
        }
    }

    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError> {
        match self {
            Self::Rh(backend) => backend.execute(source, options, fleet_bridge),
            #[cfg(feature = "script-lua")]
            Self::Lua(backend) => backend.execute(source, options, fleet_bridge),
            Self::Qjs(backend) => backend.execute(source, options, fleet_bridge),
            #[cfg(feature = "script-sql")]
            Self::Sql(backend) => backend.execute(source, options, fleet_bridge),
        }
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_backend::{RhInvocationOptions as RhOpts, try_execute_rh_invocation};

    // Mirrors script_backend.rs's ENV_LOCK pattern (serialize env-var
    // manipulation across tests in this module — this is a *different*
    // mutex instance than script_backend.rs's, but since `cargo test`
    // runs all `#[test]` functions in one process across all `mod`s
    // sharing the same env var, tests in this module also risk racing
    // against script_backend.rs's own env-mutating tests. Each guard here
    // still gives serialization *within* this file, and both files restore
    // the prior value before releasing the lock, minimizing cross-file
    // interference to a narrow window.)
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        prior: Option<String>,
    }

    impl EnvGuard {
        fn set(value: &str) -> Self {
            let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
            unsafe {
                std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
            }
            Self { prior }
        }

        fn clear() -> Self {
            let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
            unsafe {
                std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
            }
            Self { prior }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prior {
                Some(value) => unsafe {
                    std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
                },
                None => unsafe {
                    std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
                },
            }
        }
    }

    #[test]
    fn rh_engine_enabled_by_default_with_no_env_set() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::clear();
        assert!(RhEngineBackend.enabled());
        #[cfg(feature = "script-lua")]
        assert!(!LuaEngineBackend.enabled());
        assert!(!QjsEngineBackend.enabled());
        #[cfg(feature = "script-sql")]
        assert!(!SqlEngineBackend.enabled());
    }

    #[test]
    fn script_invocation_options_default_field_shape() {
        let options = ScriptInvocationOptions::default();
        assert!(options.project_root.is_none());
        assert!(options.arguments.is_none());
        assert!(options.budgets.is_none());
    }

    // ---- rh ----

    const RH_VALID_SOURCE: &str = "fn entry() { 42 }";
    const RH_BROKEN_SOURCE: &str = "fn entry() { 1 ";

    #[test]
    fn rh_engine_enabled_matches_env() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("rh");
        let engine = RhEngineBackend;
        assert_eq!(engine.backend_id(), ScriptBackend::Rh);
        assert!(engine.enabled());

        #[cfg(feature = "script-lua")]
        {
            let _env = EnvGuard::set("lua");
            assert!(!RhEngineBackend.enabled());
        }
    }

    #[test]
    fn rh_engine_check_valid_and_broken_source() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("rh");
        let engine = RhEngineBackend;
        let options = ScriptInvocationOptions::default();

        engine
            .check(RH_VALID_SOURCE, &options)
            .expect("valid rh source should check clean");
        assert!(
            engine.check(RH_BROKEN_SOURCE, &options).is_err(),
            "broken rh source should fail check"
        );
    }

    #[test]
    fn rh_engine_execute_matches_direct_call() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("rh");
        let engine = RhEngineBackend;
        let options = ScriptInvocationOptions::default();

        let via_trait = engine
            .execute(RH_VALID_SOURCE, &options, None)
            .expect("trait execute should succeed");

        let direct = try_execute_rh_invocation(
            ScriptOperation::Eval,
            RH_VALID_SOURCE,
            RhOpts::default(),
            None,
        )
        .expect("direct call should not error")
        .expect("rh backend should be enabled");

        assert_eq!(via_trait.stdout, direct.stdout);
        assert_eq!(via_trait.value, direct.value);
    }

    #[test]
    fn rh_engine_entry_extensions_match_from_entry_path() {
        for ext in RhEngineBackend.entry_extensions() {
            let path = format!("script.{ext}");
            assert_eq!(
                ScriptBackend::from_entry_path(&path),
                ScriptBackend::Rh,
                "extension {ext} should route to rh"
            );
        }
    }

    // ---- lua ----

    const LUA_VALID_SOURCE: &str = "return 42";
    const LUA_BROKEN_SOURCE: &str = "return !!";

    #[test]
    #[cfg(feature = "script-lua")]
    fn lua_engine_enabled_matches_env() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("lua");
        let engine = LuaEngineBackend;
        assert_eq!(engine.backend_id(), ScriptBackend::Lua);
        assert!(engine.enabled());

        let _env = EnvGuard::set("qjs");
        assert!(!LuaEngineBackend.enabled());
    }

    #[test]
    #[cfg(feature = "script-lua")]
    fn lua_engine_check_valid_and_broken_source() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("lua");
        let engine = LuaEngineBackend;
        let options = ScriptInvocationOptions::default();

        engine
            .check(LUA_VALID_SOURCE, &options)
            .expect("valid lua source should check clean");
        assert!(
            engine.check(LUA_BROKEN_SOURCE, &options).is_err(),
            "broken lua source should fail check"
        );
    }

    #[test]
    #[cfg(feature = "script-lua")]
    fn lua_engine_execute_returns_evaluated_value() {
        // Trait-M4: was an equivalence test against try_execute_lua_invocation
        // (now folded/deleted); asserts the same expected shape (stdout
        // empty, value == 42 widened to serde_json::Value) directly.
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("lua");
        let engine = LuaEngineBackend;
        let options = ScriptInvocationOptions::default();

        let result = engine
            .execute(LUA_VALID_SOURCE, &options, None)
            .expect("trait execute should succeed");

        assert_eq!(result.stdout, "");
        assert_eq!(result.value, Some(Value::from(42i64)));
    }

    #[test]
    #[cfg(feature = "script-lua")]
    fn lua_engine_execute_errors_when_not_enabled() {
        // Migrated from script_backend.rs's lua_backend_not_enabled_without_env
        // (was: try_execute_lua_invocation returns Ok(None) when the lua
        // backend isn't selected). The trait surface has no Option-wrapping
        // "not enabled" case, so the equivalent is an Err from execute().
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::clear();
        let engine = LuaEngineBackend;
        let options = ScriptInvocationOptions::default();

        assert!(engine.execute(LUA_VALID_SOURCE, &options, None).is_err());
    }

    #[test]
    #[cfg(feature = "script-lua")]
    fn lua_engine_entry_extensions_match_from_entry_path() {
        for ext in LuaEngineBackend.entry_extensions() {
            let path = format!("script.{ext}");
            assert_eq!(
                ScriptBackend::from_entry_path(&path),
                ScriptBackend::Lua,
                "extension {ext} should route to lua"
            );
        }
    }

    // ---- qjs ----

    const QJS_VALID_SOURCE: &str = "function entry() { return 42; }";
    const QJS_BROKEN_SOURCE: &str = "function entry() { return 1 ";

    #[test]
    fn qjs_engine_enabled_matches_env() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("qjs");
        let engine = QjsEngineBackend;
        assert_eq!(engine.backend_id(), ScriptBackend::Qjs);
        assert!(engine.enabled());

        let _env = EnvGuard::set("rh");
        assert!(!QjsEngineBackend.enabled());
    }

    #[test]
    fn qjs_engine_check_valid_and_broken_source() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("qjs");
        let engine = QjsEngineBackend;
        let options = ScriptInvocationOptions::default();

        engine
            .check(QJS_VALID_SOURCE, &options)
            .expect("valid qjs source should check clean");
        assert!(
            engine.check(QJS_BROKEN_SOURCE, &options).is_err(),
            "broken qjs source should fail check"
        );
    }

    #[test]
    fn qjs_engine_execute_returns_evaluated_value() {
        // Trait-M4: was an equivalence test against try_execute_qjs_invocation
        // (now folded/deleted); asserts the same expected shape (stdout
        // empty, value == json!(42)) directly.
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("qjs");
        let engine = QjsEngineBackend;
        let options = ScriptInvocationOptions::default();

        let result = engine
            .execute(QJS_VALID_SOURCE, &options, None)
            .expect("trait execute should succeed");

        assert_eq!(result.stdout, "");
        assert_eq!(result.value, Some(serde_json::json!(42)));
    }

    #[test]
    fn qjs_engine_execute_errors_when_not_enabled() {
        // Migrated from script_backend.rs's qjs_backend_not_enabled_without_env
        // (was: try_execute_qjs_invocation returns Ok(None) when the qjs
        // backend isn't selected). The trait surface has no Option-wrapping
        // "not enabled" case, so the equivalent is an Err from execute().
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::clear();
        let engine = QjsEngineBackend;
        let options = ScriptInvocationOptions::default();

        assert!(engine.execute(QJS_VALID_SOURCE, &options, None).is_err());
    }

    #[test]
    fn qjs_engine_execute_wires_fleet_bridge_and_args() {
        // Migrated from script_backend.rs's
        // try_execute_qjs_invocation_wires_fleet_bridge_and_args — the
        // richest surviving scenario: fleet_call AND args_len/arg wired
        // together through a real qjs script.
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("qjs");
        let engine = QjsEngineBackend;

        let bridge: ScriptFleetBridgeFn = Arc::new(|op_id: &str, params: &str| {
            if op_id == "protocol.info" && params == "{}" {
                Ok("{\"version\":1}".to_owned())
            } else {
                Err(format!("unknown_op: {op_id}"))
            }
        });
        let mut options = ScriptInvocationOptions::default();
        options.arguments = Some(serde_json::json!(["first", "second"]));

        let result = engine
            .execute(
                "function entry() { \
                     const info = __host.fleet_call('protocol.info', '{}'); \
                     return __host.args_len() + ':' + __host.arg(0) + ':' + info; \
                 }",
                &options,
                Some(bridge),
            )
            .expect("execute should succeed");

        assert_eq!(
            result.value,
            Some(serde_json::json!("2:first:{\"version\":1}"))
        );
    }

    #[test]
    fn qjs_engine_entry_extensions_match_from_entry_path() {
        for ext in QjsEngineBackend.entry_extensions() {
            let path = format!("script.{ext}");
            assert_eq!(
                ScriptBackend::from_entry_path(&path),
                ScriptBackend::Qjs,
                "extension {ext} should route to qjs"
            );
        }
    }

    // ---- sql ----

    const SQL_VALID_SOURCE: &str = "SELECT 1;";
    const SQL_BROKEN_SOURCE: &str = "SELEC 1 FORM;";

    #[test]
    #[cfg(feature = "script-sql")]
    fn sql_engine_enabled_matches_env() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("sql");
        let engine = SqlEngineBackend;
        assert_eq!(engine.backend_id(), ScriptBackend::Sql);
        assert!(engine.enabled());

        let _env = EnvGuard::set("rh");
        assert!(!SqlEngineBackend.enabled());
    }

    #[test]
    #[cfg(feature = "script-sql")]
    fn sql_engine_check_valid_and_broken_source() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("sql");
        let engine = SqlEngineBackend;
        let options = ScriptInvocationOptions::default();

        engine
            .check(SQL_VALID_SOURCE, &options)
            .expect("valid sql source should check clean");
        assert!(
            engine.check(SQL_BROKEN_SOURCE, &options).is_err(),
            "broken sql source should fail check"
        );
    }

    #[test]
    #[cfg(feature = "script-sql")]
    fn sql_engine_check_errors_when_not_enabled() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::clear();
        let engine = SqlEngineBackend;
        let options = ScriptInvocationOptions::default();

        assert!(engine.check(SQL_VALID_SOURCE, &options).is_err());
    }

    #[test]
    #[cfg(feature = "script-sql")]
    fn sql_engine_execute_returns_rows_for_a_select() {
        // M1: execute() is real (plan/design-sql-execution-target.md).
        // Replaces the old placeholder-pinning test
        // (sql_engine_execute_returns_the_not_implemented_error), which
        // asserted the opposite — that execute() always failed closed.
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("sql");
        let engine = SqlEngineBackend;
        let options = ScriptInvocationOptions::default();

        let result = engine
            .execute(SQL_VALID_SOURCE, &options, None)
            .expect("sql execute should succeed for a valid SELECT");
        assert_eq!(result.stdout, "");
        assert_eq!(result.value, Some(serde_json::json!([{"1": 1}])));
    }

    #[test]
    #[cfg(feature = "script-sql")]
    fn sql_engine_execute_runs_multi_statement_scripts_in_order() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("sql");
        let engine = SqlEngineBackend;
        let options = ScriptInvocationOptions::default();

        let result = engine
            .execute(
                "CREATE TABLE widgets (id INTEGER, name TEXT); \
                 INSERT INTO widgets (id, name) VALUES (1, 'gizmo'); \
                 SELECT id, name FROM widgets;",
                &options,
                None,
            )
            .expect("multi-statement sql execute should succeed");
        assert_eq!(
            result.value,
            Some(serde_json::json!([{"id": 1, "name": "gizmo"}]))
        );
    }

    #[test]
    #[cfg(feature = "script-sql")]
    fn sql_engine_execute_errors_not_panics_on_bad_sql_at_execute_time() {
        // "bad sql at execute time" here means execution-time (SQLite)
        // rejection, not a check()-catchable parse error — querying a
        // table that doesn't exist parses fine, fails when run.
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("sql");
        let engine = SqlEngineBackend;
        let options = ScriptInvocationOptions::default();

        let error = engine
            .execute("SELECT * FROM does_not_exist;", &options, None)
            .expect_err("querying a nonexistent table should error, not panic");
        assert!(!error.is_empty());
    }

    #[test]
    #[cfg(feature = "script-sql")]
    fn sql_engine_execute_errors_when_not_enabled() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::clear();
        let engine = SqlEngineBackend;
        let options = ScriptInvocationOptions::default();

        let error = engine
            .execute(SQL_VALID_SOURCE, &options, None)
            .expect_err("execute must fail when the backend isn't enabled");
        // "not enabled" must win over "not implemented" when both are
        // true, matching lua/qjs's same enabled()-gate-first ordering.
        assert!(error.contains("not enabled"), "{error}");
    }

    #[test]
    #[cfg(feature = "script-sql")]
    fn sql_engine_entry_extensions_match_from_entry_path() {
        for ext in SqlEngineBackend.entry_extensions() {
            let path = format!("script.{ext}");
            assert_eq!(
                ScriptBackend::from_entry_path(&path),
                ScriptBackend::Sql,
                "extension {ext} should route to sql"
            );
        }
    }

    // ---- ScriptEngine enum (static dispatch) ----

    #[test]
    fn script_engine_for_backend_and_engine_for_agree() {
        for id in [
            ScriptBackend::Rh,
            #[cfg(feature = "script-lua")]
            ScriptBackend::Lua,
            ScriptBackend::Qjs,
            #[cfg(feature = "script-sql")]
            ScriptBackend::Sql,
        ] {
            assert_eq!(ScriptEngine::for_backend(id).backend_id(), id);
            assert_eq!(engine_for(id).backend_id(), id);
        }
    }

    #[test]
    fn script_engine_all_covers_every_backend_id() {
        let ids: Vec<ScriptBackend> = ScriptEngine::all().iter().map(|e| e.backend_id()).collect();
        let mut expected = vec![ScriptBackend::Rh, ScriptBackend::Qjs];
        #[cfg(feature = "script-lua")]
        expected.push(ScriptBackend::Lua);
        #[cfg(feature = "script-sql")]
        expected.push(ScriptBackend::Sql);
        assert_eq!(ids, expected);
    }

    #[test]
    fn script_engine_entry_extensions_match_from_entry_path_for_all() {
        for engine in ScriptEngine::all() {
            for ext in engine.entry_extensions() {
                let path = format!("script.{ext}");
                assert_eq!(
                    ScriptBackend::from_entry_path(&path),
                    engine.backend_id(),
                    "extension {ext} should route to {:?}",
                    engine.backend_id()
                );
            }
        }
    }
}
