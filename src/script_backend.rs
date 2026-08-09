//! Script execution backend selection.
//!
//! Pack execution defaults to the rh AOT backend. Legacy `AGENTERM_SCRIPT_BACKEND=rhai`
//! and `.rhai` entry paths are retired and normalize to `rh`.
//!
//! Trait-M4 (`plan/design-script-engine-trait.md` §4) folded the lua and
//! qjs engine-specific invocation logic into `script_engine.rs`'s
//! `LuaEngineBackend`/`QjsEngineBackend`; this module kept only
//! `try_execute_rh_invocation` (and its `RhInvocationOptions`/
//! `RhInvocationResult` types) because `crates/agenterm-rh/src/main.rs`
//! (the `agenterm-rh` bin target of this root package, per `Cargo.toml`)
//! calls it directly and depends on its typed `agenterm_rh::RhError`
//! return — see `script_engine.rs`'s module doc for the full rationale.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::Value;

use crate::script_protocol::{ScriptBudgets, ScriptOperation};
use crate::script_rh_run::RhRunContext;

/// Active script backend for pack execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptBackend {
    Rh,
    Lua,
    Qjs,
    Sql,
}

impl ScriptBackend {
    pub fn from_env() -> Self {
        match std::env::var("AGENTERM_SCRIPT_BACKEND")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("rhai") => Self::Rh,
            Some("lua") => Self::Lua,
            Some("qjs") => Self::Qjs,
            Some("sql") => Self::Sql,
            _ => Self::Rh,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rh => "rh",
            Self::Lua => "lua",
            Self::Qjs => "qjs",
            Self::Sql => "sql",
        }
    }

    /// Select backend from task entry file extension.
    pub fn from_entry_path(path: &str) -> Self {
        if path.ends_with(".lua") {
            return Self::Lua;
        }
        if path.ends_with(".js") || path.ends_with(".mjs") {
            return Self::Qjs;
        }
        if path.ends_with(".sql") {
            return Self::Sql;
        }
        if path.ends_with(".rh") {
            return Self::Rh;
        }
        if path.ends_with(".rhai") {
            return Self::Rh;
        }
        Self::Rh
    }
}

pub fn rh_backend_enabled() -> bool {
    matches!(ScriptBackend::from_env(), ScriptBackend::Rh)
}

#[derive(Clone, Debug, Default)]
pub struct RhInvocationOptions {
    pub project_root: Option<PathBuf>,
    pub arguments: Option<Value>,
    pub budgets: Option<ScriptBudgets>,
}

pub struct RhInvocationResult {
    pub stdout: String,
    pub value: Option<serde_json::Value>,
}

pub fn try_execute_rh_invocation(
    operation: ScriptOperation,
    source: &str,
    options: RhInvocationOptions,
    fleet_bridge: Option<crate::script_rh_host::FleetBridgeFn>,
) -> Result<Option<RhInvocationResult>, agenterm_rh::RhError> {
    if !rh_backend_enabled() {
        return Ok(None);
    }

    let output_limit = options.budgets.as_ref().map_or_else(
        || ScriptBudgets::default().output_bytes,
        |budgets| budgets.output_bytes,
    );
    let output_capture = Arc::new(crate::script_rh_run::RhOutputCapture::new(output_limit));
    let run_context = RhRunContext {
        project_root: options.project_root.clone(),
        arguments: options.arguments.clone(),
        budgets: options.budgets.clone(),
        output_capture: Some(Arc::clone(&output_capture)),
    };

    match operation {
        // Unreachable in practice: `script_worker.rs::execute_inner` short-circuits
        // `ScriptOperation::Api` before ever calling into this backend dispatch.
        // Kept only so this match stays exhaustive over `ScriptOperation`.
        ScriptOperation::Api => Ok(None),
        ScriptOperation::Check => {
            if !source.is_empty() {
                rh_check_with_project_validation(source, &options)?;
            } else if crate::script_rh_pack::cached_rh_pack().is_none() {
                return Err(agenterm_rh::RhError::Compile(
                    "AGENTERM_SCRIPT_BACKEND=rh requires AGENTERM_RH_PACK or non-empty source"
                        .into(),
                ));
            }
            Ok(Some(RhInvocationResult {
                stdout: String::new(),
                value: None,
            }))
        }
        ScriptOperation::Run | ScriptOperation::Eval => {
            let (pack, native_path) =
                resolve_rh_pack(source, options.project_root.as_deref())?;
            let entry_result = crate::script_rh_host::call_pack_entry_with_host_result(
                &native_path,
                fleet_bridge,
                run_context,
            )?;
            let entry_value = entry_result.entry_value;
            let mut stdout = output_capture.finish()?;
            for line in &pack.cc_lines {
                if stdout.len().saturating_add(line.len()).saturating_add(1) > output_limit {
                    return Err(agenterm_rh::RhError::Compile(
                        "rh invocation output exceeds its byte budget".into(),
                    ));
                }
                stdout.push_str(line);
                stdout.push('\n');
            }
            Ok(Some(RhInvocationResult {
                stdout,
                value: match entry_result.host_value {
                    Some(crate::script_rh_host::RhHostEntryValue::Unit) => None,
                    Some(crate::script_rh_host::RhHostEntryValue::Value(value)) => Some(value),
                    None => json_value_from_entry(entry_value),
                },
            }))
        }
    }
}

fn json_value_from_entry(entry_value: i64) -> Option<serde_json::Value> {
    Some(serde_json::Value::from(entry_value))
}

fn resolve_rh_pack(
    source: &str,
    project_root: Option<&std::path::Path>,
) -> Result<(crate::script_rh_pack::LoadedRhPack, std::path::PathBuf), agenterm_rh::RhError> {
    if let Some(pack) = crate::script_rh_pack::cached_rh_pack() {
        let native = crate::script_rh_pack::cached_native_path()
            .ok_or_else(|| {
                agenterm_rh::RhError::Compile("AGENTERM_RH_PACK native path is unavailable".into())
            })?
            .to_path_buf();
        return Ok((pack.clone(), native));
    }
    if !source.is_empty() {
        let pack =
            crate::script_rh_cache::loaded_pack_for_source_with_project(source, project_root)?;
        let native =
            crate::script_rh_cache::native_path_for_source_with_project(source, project_root)?;
        return Ok((pack, native));
    }
    Err(agenterm_rh::RhError::Compile(
        "AGENTERM_SCRIPT_BACKEND=rh requires AGENTERM_RH_PACK or non-empty rh source".into(),
    ))
}

pub fn rh_check(source: &str) -> Result<(), agenterm_rh::RhError> {
    agenterm_rh::check(source)
}

fn rh_check_with_project_validation(
    source: &str,
    options: &RhInvocationOptions,
) -> Result<(), agenterm_rh::RhError> {
    agenterm_rh::check_with_project_validation(source, options.project_root.as_deref())
}

pub fn rh_transpile(source: &str) -> Result<String, agenterm_rh::RhError> {
    agenterm_rh::transpile(source)
}

pub fn rh_compile(
    source: &str,
    output: &Path,
) -> Result<agenterm_rh::CompileOutput, agenterm_rh::RhError> {
    agenterm_rh::compile_native(source, output)
}

pub fn rh_run_smoke(native: &Path) -> Result<i64, agenterm_rh::RhError> {
    agenterm_rh::load_and_call_entry(native)
}

pub fn rh_load_pack(
    path: &Path,
) -> Result<crate::script_rh_pack::LoadedRhPack, agenterm_rh::RhError> {
    crate::script_rh_pack::load_rh_pack(path)
}

/// dynacore invocation result — deliberately its own small type (not
/// `crate::script_engine::ScriptInvocationResult`): dynacore packs do not go
/// through the `ScriptEngineBackend` trait (see
/// `try_execute_dynacore_pack_invocation`'s doc for why), so there is no
/// reason to share that trait's result shape.
pub struct DynacoreInvocationResult {
    pub stdout: String,
    pub value: Option<serde_json::Value>,
}

/// Runs a cached dynacore pack (`crate::script_dynacore_pack::cached_dynacore_pack`,
/// configured via `AGENTERM_DYNACORE_PACK_STORE`/`AGENTERM_DYNACORE_PACK_HASH`)
/// if one is configured. Mirrors `try_execute_rh_invocation`'s cached-native-
/// pack shape (`resolve_rh_pack`'s `cached_rh_pack()` branch): `Ok(None)`
/// means "no pack configured, fall through to the rh/lua/qjs/sql engines",
/// `Ok(Some(_))` is a completed run, `Err` covers a verification failure, a
/// step-limit abort, or a `Run`/`Eval` asked for without a fleet bridge.
///
/// dynacore packs are binary content-addressed artifacts, not human-authored
/// text source — this deliberately does NOT go through the
/// `ScriptEngineBackend` trait's `check(source)`/`execute(source)` shape
/// (`script_engine.rs`'s trait doc: "does not cover... pack/qualify CLI
/// verbs (engine-specific pack shapes, see design §3 non-goals)"; rh's own
/// native pack — `script_rh_pack.rs` — takes the exact same "own path, not
/// the trait" stance for the same reason). `script_worker.rs::execute_inner`
/// calls this directly, before dispatching to any `ScriptEngineBackend`.
pub fn try_execute_dynacore_pack_invocation(
    operation: ScriptOperation,
    fleet_bridge: Option<crate::script_dynacore_host::DynacoreFleetBridgeFn>,
) -> Result<Option<DynacoreInvocationResult>, String> {
    let Some(pack) = crate::script_dynacore_pack::cached_dynacore_pack() else {
        return Ok(None);
    };
    // Re-verifying here (cheap — see verify.rs's own doc: "produce-time, no
    // execution, one pass") is what actually produces the `VerifiedModule`
    // this call needs: `VerifiedModule<'a>` borrows the `Module` it
    // verifies, so the OnceLock-cached pack can only hold the owned
    // `Module` (load-time verification in `load_dynacore_pack` already
    // proved it well-formed once; this is not a new, unproven gate).
    let verified = crate::script_dynacore_host::verify_pack(&pack.module)
        .map_err(|fault| format!("dynacore pack failed verification: {fault:?}"))?;

    match operation {
        // Mirrors try_execute_rh_invocation's own Api arm: unreachable in
        // practice (execute_inner short-circuits ScriptOperation::Api before
        // any backend dispatch), kept only so this match stays exhaustive.
        ScriptOperation::Api => Ok(None),
        ScriptOperation::Check => Ok(Some(DynacoreInvocationResult {
            stdout: String::new(),
            value: None,
        })),
        ScriptOperation::Run | ScriptOperation::Eval => {
            let Some(bridge) = fleet_bridge else {
                return Err(
                    "dynacore pack invocation requires a fleet bridge (broker) but none was supplied"
                        .to_owned(),
                );
            };
            let outcome = crate::script_dynacore_host::run_pack(&verified, &bridge);
            match outcome.termination {
                agenterm_dynacore::eval_core::Termination::Exited(value) => {
                    Ok(Some(DynacoreInvocationResult {
                        stdout: String::new(),
                        value: Some(serde_json::Value::from(value)),
                    }))
                }
                agenterm_dynacore::eval_core::Termination::StepLimitExceeded => Err(
                    "dynacore pack exceeded its step limit before finishing".to_owned(),
                ),
            }
        }
    }
}

/// nativecore invocation result — same "own small type, not
/// `ScriptEngineBackend`'s result shape" reasoning as `DynacoreInvocationResult`
/// (see that type's doc); the two are not unified because nativecore packs
/// have no fleet-call surface to report through.
pub struct NativecoreInvocationResult {
    pub stdout: String,
    pub value: Option<serde_json::Value>,
}

/// Runs a cached nativecore pack
/// (`crate::script_nativecore_pack::cached_nativecore_pack`, configured via
/// `AGENTERM_NATIVECORE_PACK_STORE`/`AGENTERM_NATIVECORE_PACK_HASH`) if one is
/// configured. Mirrors `try_execute_dynacore_pack_invocation`'s shape, minus
/// a fleet bridge parameter: nativecore intents call `seam.rs::do_intent`
/// directly onto real Win32 APIs (`plan/design-dynacore-native-core.md` §7.1
/// — "没有 bridge 要穿过去"), so there is no broker to plumb through and no
/// `Run`/`Eval`-without-a-bridge error case to report. `Ok(None)` means "no
/// pack configured, fall through to the dynacore/rh/lua/qjs/sql chain",
/// `Ok(Some(_))` is a completed run, `Err` covers a verification failure or a
/// step-limit abort.
///
/// nativecore packs are binary content-addressed artifacts, not
/// human-authored text source — this deliberately does NOT go through the
/// `ScriptEngineBackend` trait, for the same reason
/// `try_execute_dynacore_pack_invocation` does not (see that function's
/// doc). `script_worker.rs::execute_inner` calls this directly, before
/// dispatching to any `ScriptEngineBackend`.
pub fn try_execute_nativecore_pack_invocation(
    operation: ScriptOperation,
) -> Result<Option<NativecoreInvocationResult>, String> {
    let Some(pack) = crate::script_nativecore_pack::cached_nativecore_pack() else {
        return Ok(None);
    };
    // Re-verifying here (cheap — see agenterm-nativecore's verify.rs doc:
    // "produce-time, no execution, one pass") is what actually produces the
    // `VerifiedModule` this call needs: `VerifiedModule<'a>` borrows the
    // `Module` it verifies, so the OnceLock-cached pack can only hold the
    // owned `Module` (load-time verification in `load_nativecore_pack`
    // already proved it well-formed once; this is not a new, unproven gate).
    let verified = agenterm_nativecore::verify::verify(&pack.module)
        .map_err(|fault| format!("nativecore pack failed verification: {fault:?}"))?;

    match operation {
        // Mirrors try_execute_dynacore_pack_invocation's own Api arm:
        // unreachable in practice (execute_inner short-circuits
        // ScriptOperation::Api before any backend dispatch), kept only so
        // this match stays exhaustive.
        ScriptOperation::Api => Ok(None),
        ScriptOperation::Check => Ok(Some(NativecoreInvocationResult {
            stdout: String::new(),
            value: None,
        })),
        ScriptOperation::Run | ScriptOperation::Eval => {
            let outcome = agenterm_nativecore::eval_core::run(&verified);
            match outcome.termination {
                agenterm_nativecore::eval_core::Termination::Exited(value) => {
                    Ok(Some(NativecoreInvocationResult {
                        stdout: String::new(),
                        value: Some(serde_json::Value::from(value)),
                    }))
                }
                agenterm_nativecore::eval_core::Termination::StepLimitExceeded => Err(
                    "nativecore pack exceeded its step limit before finishing".to_owned(),
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RhInvocationOptions, ScriptBackend, rh_backend_enabled, try_execute_rh_invocation,
    };
    use crate::script_protocol::ScriptOperation;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn default_backend_is_rh() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
        }
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Rh);
        assert!(rh_backend_enabled());
        assert!(
            try_execute_rh_invocation(
                ScriptOperation::Check,
                "fn entry() { 1 }",
                RhInvocationOptions::default(),
                None,
            )
            .expect("probe")
            .is_some()
        );
        match prior {
            Some(value) => unsafe {
                std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
            },
            None => unsafe {
                std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
            },
        }
    }

    #[test]
    fn lua_backend_from_env() {
        // Trait-M4: was mixed with a try_execute_lua_invocation check-path
        // probe and a lua_backend_enabled() assertion; both are now covered
        // in script_engine.rs (LuaEngineBackend::enabled /
        // lua_engine_check_valid_and_broken_source). This test stays
        // ScriptBackend-enum-routing-only.
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "lua");
        }
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Lua);

        match prior {
            Some(value) => unsafe {
                std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
            },
            None => unsafe {
                std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
            },
        }
    }

    #[test]
    fn lua_backend_from_entry_path() {
        assert_eq!(
            ScriptBackend::from_entry_path("scripts/lua/test.lua"),
            ScriptBackend::Lua
        );
        assert_eq!(
            ScriptBackend::from_entry_path("test.rh"),
            ScriptBackend::Rh
        );
        assert_eq!(
            ScriptBackend::from_entry_path("test.rhai"),
            ScriptBackend::Rh
        );
    }

    #[test]
    fn lua_backend_as_str() {
        assert_eq!(ScriptBackend::Lua.as_str(), "lua");
        assert_eq!(ScriptBackend::Rh.as_str(), "rh");
    }

    #[test]
    fn retired_rhai_backend_env_defaults_to_rh() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "rhai");
        }
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Rh);
        assert!(rh_backend_enabled());
        match prior {
            Some(value) => unsafe {
                std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
            },
            None => unsafe {
                std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
            },
        }
    }

    #[test]
    fn qjs_backend_from_env() {
        // Trait-M4: was mixed with a try_execute_qjs_invocation check-path
        // probe and a qjs_backend_enabled() assertion; both are now covered
        // in script_engine.rs (QjsEngineBackend::enabled /
        // qjs_engine_check_valid_and_broken_source). This test stays
        // ScriptBackend-enum-routing-only.
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "qjs");
        }
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Qjs);

        match prior {
            Some(value) => unsafe {
                std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
            },
            None => unsafe {
                std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
            },
        }
    }

    #[test]
    fn qjs_backend_from_entry_path() {
        assert_eq!(
            ScriptBackend::from_entry_path("scripts/qjs/test.js"),
            ScriptBackend::Qjs
        );
        assert_eq!(
            ScriptBackend::from_entry_path("scripts/qjs/test.mjs"),
            ScriptBackend::Qjs
        );
        assert_eq!(
            ScriptBackend::from_entry_path("test.lua"),
            ScriptBackend::Lua
        );
    }

    #[test]
    fn qjs_backend_as_str() {
        assert_eq!(ScriptBackend::Qjs.as_str(), "qjs");
    }

    #[test]
    fn sql_backend_from_env() {
        // Mirrors qjs_backend_from_env: ScriptBackend-enum-routing-only, no
        // enabled()/check-path probe here (sql has no such probe yet — its
        // check/execute story lives in script_engine.rs's SqlEngineBackend
        // tests, once that exists).
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "sql");
        }
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Sql);

        match prior {
            Some(value) => unsafe {
                std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
            },
            None => unsafe {
                std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
            },
        }
    }

    #[test]
    fn sql_backend_from_entry_path() {
        assert_eq!(
            ScriptBackend::from_entry_path("scripts/sql/test.sql"),
            ScriptBackend::Sql
        );
        assert_eq!(
            ScriptBackend::from_entry_path("test.js"),
            ScriptBackend::Qjs
        );
    }

    #[test]
    fn sql_backend_as_str() {
        assert_eq!(ScriptBackend::Sql.as_str(), "sql");
    }
}
