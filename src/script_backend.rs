//! Script execution backend selection.
//!
//! Pack execution defaults to the rh AOT backend. Legacy `AGENTERM_SCRIPT_BACKEND=rhai`
//! and `.rhai` entry paths are retired and normalize to `rh`.

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
            _ => Self::Rh,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rh => "rh",
            Self::Lua => "lua",
            Self::Qjs => "qjs",
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

pub fn lua_backend_enabled() -> bool {
    matches!(ScriptBackend::from_env(), ScriptBackend::Lua)
}

pub fn qjs_backend_enabled() -> bool {
    matches!(ScriptBackend::from_env(), ScriptBackend::Qjs)
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

#[derive(Clone, Debug, Default)]
pub struct LuaInvocationOptions {
    pub project_root: Option<PathBuf>,
    pub arguments: Option<Value>,
    pub budgets: Option<ScriptBudgets>,
}

pub struct LuaInvocationResult {
    pub stdout: String,
    pub value: Option<i64>,
}

/// Try to execute a Lua invocation. Returns `Ok(None)` if the Lua backend is not enabled.
pub fn try_execute_lua_invocation(
    operation: ScriptOperation,
    source: &str,
    options: LuaInvocationOptions,
    fleet_bridge: Option<crate::script_lua_host::LuaFleetBridgeFn>,
) -> Result<Option<LuaInvocationResult>, String> {
    if !lua_backend_enabled() {
        return Ok(None);
    }

    let engine = agenterm_lua::LuaEngine::new().map_err(|e| e.to_string())?;

    match operation {
        ScriptOperation::Api => Ok(None),
        ScriptOperation::Check => {
            engine.check(source).map_err(|e| e.to_string())?;
            Ok(Some(LuaInvocationResult {
                stdout: String::new(),
                value: None,
            }))
        }
        ScriptOperation::Run | ScriptOperation::Eval => {
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
            let args = options.arguments.clone();
            if let Some(ref arguments) = args {
                let args_for_len = arguments.clone();
                let args_for_arg = arguments.clone();
                host.args_len = Some(Arc::new(move || {
                    args_for_len
                        .as_array()
                        .map(|a| a.len() as i64)
                        .unwrap_or(0)
                }));
                host.arg = Some(Arc::new(move |index: i64| {
                    args_for_arg
                        .as_array()
                        .and_then(|a| a.get(index as usize))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_owned())
                        .ok_or_else(|| format!("argument {index} is unavailable"))
                }));
            }

            let result = engine
                .eval(source, &host)
                .map_err(|e| format!("lua_eval: {e}"))?;

            Ok(Some(LuaInvocationResult {
                stdout: result.stdout,
                value: Some(result.value),
            }))
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct QjsInvocationOptions {
    pub project_root: Option<PathBuf>,
    pub arguments: Option<Value>,
    pub budgets: Option<ScriptBudgets>,
}

pub struct QjsInvocationResult {
    pub stdout: String,
    pub value: Option<Value>,
}

/// Try to execute a qjs invocation. Returns `Ok(None)` if the qjs backend is not enabled.
///
/// Structurally mirrors `try_execute_lua_invocation` — same "not enabled ->
/// None", same fleet-bridge/args wiring shape — because qjs, like lua, is
/// an interpreted engine with no AOT/native-codegen step (unlike rh's
/// `try_execute_rh_invocation`, which resolves/loads a compiled native
/// pack). `value` is `Option<serde_json::Value>` rather than lua's
/// `Option<i64>` because `agenterm_qjs::eval_entry_with_host` already
/// produces a typed JSON value (via `JSON.stringify`) — a strict superset,
/// not a divergence: any lua-shaped i64 result is also representable here.
pub fn try_execute_qjs_invocation(
    operation: ScriptOperation,
    source: &str,
    options: QjsInvocationOptions,
    fleet_bridge: Option<crate::script_qjs_host::QjsFleetBridgeFn>,
) -> Result<Option<QjsInvocationResult>, String> {
    if !qjs_backend_enabled() {
        return Ok(None);
    }

    match operation {
        ScriptOperation::Api => Ok(None),
        ScriptOperation::Check => {
            agenterm_qjs::check(source, "invocation.js").map_err(|e| e.to_string())?;
            Ok(Some(QjsInvocationResult {
                stdout: String::new(),
                value: None,
            }))
        }
        ScriptOperation::Run | ScriptOperation::Eval => {
            let mut host = agenterm_qjs::QjsHostFunctions::default();

            // Wire fleet bridge
            if let Some(bridge) = fleet_bridge {
                host.fleet_call = Some(Arc::new(move |op_id: &str, params: &str| -> Result<String, String> {
                    bridge(op_id, params)
                }));
            }

            // Wire args_len / arg from options.arguments
            let args = options.arguments.clone();
            if let Some(ref arguments) = args {
                let args_for_len = arguments.clone();
                let args_for_arg = arguments.clone();
                host.args_len = Some(Arc::new(move || {
                    args_for_len
                        .as_array()
                        .map(|a| a.len() as i64)
                        .unwrap_or(0)
                }));
                host.arg = Some(Arc::new(move |index: i64| {
                    args_for_arg
                        .as_array()
                        .and_then(|a| a.get(index as usize))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_owned())
                        .ok_or_else(|| format!("argument {index} is unavailable"))
                }));
            }

            let result = agenterm_qjs::eval_entry_with_host(source, "invocation.js", &host)
                .map_err(|e| format!("qjs_eval: {e}"))?;

            Ok(Some(QjsInvocationResult {
                stdout: result.stdout,
                value: result.value,
            }))
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
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "lua");
        }
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Lua);
        assert!(super::lua_backend_enabled());

        // Also test check path
        let result = super::try_execute_lua_invocation(
            ScriptOperation::Check,
            "return 0",
            super::LuaInvocationOptions::default(),
            None,
        )
        .expect("probe")
        .expect("lua result");
        assert!(result.value.is_none());

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
    fn try_execute_lua_invocation_eval() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "lua");
        }
        let result = super::try_execute_lua_invocation(
            ScriptOperation::Eval,
            "return 42",
            super::LuaInvocationOptions::default(),
            None,
        )
        .expect("probe")
        .expect("lua result");
        assert_eq!(result.value, Some(42));
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
    fn try_execute_lua_invocation_check() {
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "lua");
        }
        let result = super::try_execute_lua_invocation(
            ScriptOperation::Check,
            "return 0",
            super::LuaInvocationOptions::default(),
            None,
        )
        .expect("probe")
        .expect("lua result");
        assert!(result.value.is_none());
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
    fn lua_backend_not_enabled_without_env() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
        }
        let result = super::try_execute_lua_invocation(
            ScriptOperation::Eval,
            "return 42",
            super::LuaInvocationOptions::default(),
            None,
        )
        .expect("probe");
        assert!(result.is_none());
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
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "qjs");
        }
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Qjs);
        assert!(super::qjs_backend_enabled());

        let result = super::try_execute_qjs_invocation(
            ScriptOperation::Check,
            "function entry() { return 0; }",
            super::QjsInvocationOptions::default(),
            None,
        )
        .expect("probe")
        .expect("qjs result");
        assert!(result.value.is_none());

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
    fn try_execute_qjs_invocation_eval() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "qjs");
        }
        let result = super::try_execute_qjs_invocation(
            ScriptOperation::Eval,
            "function entry() { return 42; }",
            super::QjsInvocationOptions::default(),
            None,
        )
        .expect("probe")
        .expect("qjs result");
        assert_eq!(result.value, Some(serde_json::json!(42)));
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
    fn try_execute_qjs_invocation_check() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "qjs");
        }
        let result = super::try_execute_qjs_invocation(
            ScriptOperation::Check,
            "function entry() { return 0; }",
            super::QjsInvocationOptions::default(),
            None,
        )
        .expect("probe")
        .expect("qjs result");
        assert!(result.value.is_none());
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
    fn qjs_backend_not_enabled_without_env() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
        }
        let result = super::try_execute_qjs_invocation(
            ScriptOperation::Eval,
            "function entry() { return 42; }",
            super::QjsInvocationOptions::default(),
            None,
        )
        .expect("probe");
        assert!(result.is_none());
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
    fn try_execute_qjs_invocation_wires_fleet_bridge_and_args() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", "qjs");
        }
        let bridge: crate::script_qjs_host::QjsFleetBridgeFn =
            std::sync::Arc::new(|op_id: &str, params: &str| {
                if op_id == "protocol.info" && params == "{}" {
                    Ok("{\"version\":1}".to_owned())
                } else {
                    Err(format!("unknown_op: {op_id}"))
                }
            });
        let mut options = super::QjsInvocationOptions::default();
        options.arguments = Some(serde_json::json!(["first", "second"]));
        let result = super::try_execute_qjs_invocation(
            ScriptOperation::Eval,
            "function entry() { \
                 const info = __host.fleet_call('protocol.info', '{}'); \
                 return __host.args_len() + ':' + __host.arg(0) + ':' + info; \
             }",
            options,
            Some(bridge),
        )
        .expect("probe")
        .expect("qjs result");
        assert_eq!(
            result.value,
            Some(serde_json::json!("2:first:{\"version\":1}"))
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
}
