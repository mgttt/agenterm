//! Unified per-engine `ScriptEngineBackend` trait + static-dispatch enum.
//!
//! Trait-M1/Trait-M2 of `plan/design-script-engine-trait.md`. This module
//! defines the shared invocation types (§2.2), the `ScriptEngineBackend`
//! trait (§2.3), thin per-engine adapter impls that delegate to the
//! existing `try_execute_{rh,lua,qjs}_invocation` functions in
//! `script_backend.rs` (§4 Trait-M2 — those functions are NOT modified or
//! re-derived here), and the `ScriptEngine` static-dispatch enum (§2.4).
//!
//! `script_backend.rs`'s three `try_execute_*` functions, their
//! `*InvocationOptions`/`*InvocationResult` types, and `script_worker.rs`'s
//! `execute_inner` are untouched by this phase (Trait-M3/M4 in the design
//! doc would wire `execute_inner` to `ScriptEngine` — not done here).

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::script_backend::{
    LuaInvocationOptions, QjsInvocationOptions, RhInvocationOptions, ScriptBackend,
    try_execute_lua_invocation, try_execute_qjs_invocation, try_execute_rh_invocation,
};
use crate::script_protocol::{ScriptBudgets, ScriptOperation};

// ---------------------------------------------------------------------
// §2.2 — shared types
// ---------------------------------------------------------------------

/// Shared invocation options across all three engines. Replaces the
/// previously-duplicated `RhInvocationOptions`/`LuaInvocationOptions`/
/// `QjsInvocationOptions` (those three still exist in `script_backend.rs`
/// unchanged this phase — the per-engine adapters below convert into them).
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
    fn check(&self, source: &str, options: &ScriptInvocationOptions) -> Result<(), ScriptEngineError>;

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

/// rh engine adapter. Delegates to `try_execute_rh_invocation` — does not
/// re-derive native-pack resolution, host binding, or output-budget logic.
pub struct RhEngineBackend;

impl ScriptEngineBackend for RhEngineBackend {
    fn backend_id(&self) -> ScriptBackend {
        ScriptBackend::Rh
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        &["rh", "rhai"]
    }

    fn check(&self, source: &str, options: &ScriptInvocationOptions) -> Result<(), ScriptEngineError> {
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

/// Lua engine adapter. Delegates to `try_execute_lua_invocation`.
pub struct LuaEngineBackend;

impl ScriptEngineBackend for LuaEngineBackend {
    fn backend_id(&self) -> ScriptBackend {
        ScriptBackend::Lua
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        &["lua"]
    }

    fn check(&self, source: &str, options: &ScriptInvocationOptions) -> Result<(), ScriptEngineError> {
        let lua_options = LuaInvocationOptions {
            project_root: options.project_root.clone(),
            arguments: options.arguments.clone(),
            budgets: options.budgets.clone(),
        };
        match try_execute_lua_invocation(ScriptOperation::Check, source, lua_options, None) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(not_enabled_error(self.backend_id())),
            Err(error) => Err(error),
        }
    }

    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError> {
        let lua_options = LuaInvocationOptions {
            project_root: options.project_root.clone(),
            arguments: options.arguments.clone(),
            budgets: options.budgets.clone(),
        };
        // script_lua_host::LuaFleetBridgeFn is already `Arc<dyn Fn(&str, &str)
        // -> Result<String, String> + Send + Sync>` — type-identical to
        // ScriptFleetBridgeFn, so it passes through unchanged.
        match try_execute_lua_invocation(ScriptOperation::Eval, source, lua_options, fleet_bridge) {
            Ok(Some(result)) => Ok(ScriptInvocationResult {
                stdout: result.stdout,
                value: result.value.map(Value::from),
            }),
            Ok(None) => Err(not_enabled_error(self.backend_id())),
            Err(error) => Err(error),
        }
    }
}

/// qjs engine adapter. Delegates to `try_execute_qjs_invocation`.
pub struct QjsEngineBackend;

impl ScriptEngineBackend for QjsEngineBackend {
    fn backend_id(&self) -> ScriptBackend {
        ScriptBackend::Qjs
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        &["js", "mjs"]
    }

    fn check(&self, source: &str, options: &ScriptInvocationOptions) -> Result<(), ScriptEngineError> {
        let qjs_options = QjsInvocationOptions {
            project_root: options.project_root.clone(),
            arguments: options.arguments.clone(),
            budgets: options.budgets.clone(),
        };
        match try_execute_qjs_invocation(ScriptOperation::Check, source, qjs_options, None) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(not_enabled_error(self.backend_id())),
            Err(error) => Err(error),
        }
    }

    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError> {
        let qjs_options = QjsInvocationOptions {
            project_root: options.project_root.clone(),
            arguments: options.arguments.clone(),
            budgets: options.budgets.clone(),
        };
        // script_qjs_host::QjsFleetBridgeFn is already `Arc<dyn Fn(&str, &str)
        // -> Result<String, String> + Send + Sync>` — type-identical to
        // ScriptFleetBridgeFn, so it passes through unchanged.
        match try_execute_qjs_invocation(ScriptOperation::Eval, source, qjs_options, fleet_bridge) {
            Ok(Some(result)) => Ok(ScriptInvocationResult {
                stdout: result.stdout,
                value: result.value,
            }),
            Ok(None) => Err(not_enabled_error(self.backend_id())),
            Err(error) => Err(error),
        }
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
    Lua(LuaEngineBackend),
    Qjs(QjsEngineBackend),
}

impl ScriptEngine {
    pub fn all() -> [ScriptEngine; 3] {
        [
            Self::Rh(RhEngineBackend),
            Self::Lua(LuaEngineBackend),
            Self::Qjs(QjsEngineBackend),
        ]
    }

    /// Construct the engine variant corresponding to `id`.
    pub fn for_backend(id: ScriptBackend) -> Self {
        match id {
            ScriptBackend::Rh => Self::Rh(RhEngineBackend),
            ScriptBackend::Lua => Self::Lua(LuaEngineBackend),
            ScriptBackend::Qjs => Self::Qjs(QjsEngineBackend),
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
            Self::Lua(backend) => backend.backend_id(),
            Self::Qjs(backend) => backend.backend_id(),
        }
    }

    fn entry_extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Rh(backend) => backend.entry_extensions(),
            Self::Lua(backend) => backend.entry_extensions(),
            Self::Qjs(backend) => backend.entry_extensions(),
        }
    }

    fn check(&self, source: &str, options: &ScriptInvocationOptions) -> Result<(), ScriptEngineError> {
        match self {
            Self::Rh(backend) => backend.check(source, options),
            Self::Lua(backend) => backend.check(source, options),
            Self::Qjs(backend) => backend.check(source, options),
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
            Self::Lua(backend) => backend.execute(source, options, fleet_bridge),
            Self::Qjs(backend) => backend.execute(source, options, fleet_bridge),
        }
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_backend::{
        LuaInvocationOptions as LuaOpts, QjsInvocationOptions as QjsOpts,
        RhInvocationOptions as RhOpts, try_execute_lua_invocation, try_execute_qjs_invocation,
        try_execute_rh_invocation,
    };

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
        assert!(!LuaEngineBackend.enabled());
        assert!(!QjsEngineBackend.enabled());
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

        let _env = EnvGuard::set("lua");
        assert!(!RhEngineBackend.enabled());
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
    fn lua_engine_execute_matches_direct_call() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("lua");
        let engine = LuaEngineBackend;
        let options = ScriptInvocationOptions::default();

        let via_trait = engine
            .execute(LUA_VALID_SOURCE, &options, None)
            .expect("trait execute should succeed");

        let direct = try_execute_lua_invocation(
            ScriptOperation::Eval,
            LUA_VALID_SOURCE,
            LuaOpts::default(),
            None,
        )
        .expect("direct call should not error")
        .expect("lua backend should be enabled");

        assert_eq!(via_trait.stdout, direct.stdout);
        assert_eq!(via_trait.value, direct.value.map(Value::from));
    }

    #[test]
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
    fn qjs_engine_execute_matches_direct_call() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let _env = EnvGuard::set("qjs");
        let engine = QjsEngineBackend;
        let options = ScriptInvocationOptions::default();

        let via_trait = engine
            .execute(QJS_VALID_SOURCE, &options, None)
            .expect("trait execute should succeed");

        let direct = try_execute_qjs_invocation(
            ScriptOperation::Eval,
            QJS_VALID_SOURCE,
            QjsOpts::default(),
            None,
        )
        .expect("direct call should not error")
        .expect("qjs backend should be enabled");

        assert_eq!(via_trait.stdout, direct.stdout);
        assert_eq!(via_trait.value, direct.value);
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

    // ---- ScriptEngine enum (static dispatch) ----

    #[test]
    fn script_engine_for_backend_and_engine_for_agree() {
        for id in [ScriptBackend::Rh, ScriptBackend::Lua, ScriptBackend::Qjs] {
            assert_eq!(ScriptEngine::for_backend(id).backend_id(), id);
            assert_eq!(engine_for(id).backend_id(), id);
        }
    }

    #[test]
    fn script_engine_all_covers_every_backend_id() {
        let ids: Vec<ScriptBackend> = ScriptEngine::all().iter().map(|e| e.backend_id()).collect();
        assert_eq!(ids, vec![ScriptBackend::Rh, ScriptBackend::Lua, ScriptBackend::Qjs]);
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
