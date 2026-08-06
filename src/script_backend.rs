//! Script execution backend selection.
//!
//! Pack execution defaults to the rh AOT backend. Set `AGENTERM_SCRIPT_BACKEND=rhai`
//! to force the legacy Rhai interpreter path.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::script_protocol::{ScriptBudgets, ScriptOperation};
use crate::script_rh_run::RhRunContext;

/// Active script backend for pack execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptBackend {
    Rhai,
    Rh,
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
            Some("rhai") => Self::Rhai,
            _ => Self::Rh,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rhai => "rhai",
            Self::Rh => "rh",
        }
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

    let run_context = RhRunContext {
        project_root: options.project_root.clone(),
        arguments: options.arguments.clone(),
        budgets: options.budgets.clone(),
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
            let (pack, native_path) = resolve_rh_pack(source)?;
            let entry_value = crate::script_rh_host::call_pack_entry_with_host(
                &native_path,
                fleet_bridge,
                run_context,
            )?;
            let mut stdout = String::new();
            for line in &pack.cc_lines {
                stdout.push_str(line);
                stdout.push('\n');
            }
            Ok(Some(RhInvocationResult {
                stdout,
                value: json_value_from_entry(entry_value),
            }))
        }
    }
}

fn json_value_from_entry(entry_value: i64) -> Option<serde_json::Value> {
    Some(serde_json::Value::from(entry_value))
}

fn resolve_rh_pack(
    source: &str,
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
        let pack = crate::script_rh_cache::loaded_pack_for_source(source)?;
        let native = crate::script_rh_cache::native_path_for_source(source)?;
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
    rh_check(source)?;
    if let Some(project_root) = options.project_root.as_deref() {
        let mut engine = rhai::Engine::new();
        let budgets = options.budgets.clone().unwrap_or_default();
        crate::script_runtime::configure_engine(&mut engine, &budgets);
        let module_sources = crate::script_project::validate_project_imports(
            &engine,
            project_root,
            source,
        )
        .map_err(|error| agenterm_rh::RhError::Compile(error.to_string()))?;
        for module_source in module_sources {
            crate::script_api_validate::validate_available_apis(&module_source)
                .map_err(script_failure_to_rh_error)?;
        }
    }
    crate::script_api_validate::validate_available_apis(source).map_err(script_failure_to_rh_error)
}

fn script_failure_to_rh_error(failure: crate::script_protocol::ScriptFailure) -> agenterm_rh::RhError {
    agenterm_rh::RhError::Compile(format!("{}: {}", failure.code, failure.message))
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

#[cfg(test)]
mod tests {
    use super::{ScriptBackend, rh_backend_enabled, try_execute_rh_invocation, RhInvocationOptions};
    use crate::script_protocol::ScriptOperation;

    #[test]
    fn default_backend_is_rh() {
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
}
