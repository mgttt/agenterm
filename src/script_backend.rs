//! Script execution backend selection.
//!
//! Today every live invocation uses Rhai unless `AGENTERM_SCRIPT_BACKEND=rh`.
//! The parallel `rh` track (`crates/agenterm-rh`) validates pack subsets,
//! AOT-compiles to native libraries, and loads them with dlopen.

use std::path::Path;

use crate::script_protocol::ScriptOperation;

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
        {
            Some(value) if value == "rh" => Self::Rh,
            _ => Self::Rhai,
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

pub struct RhInvocationResult {
    pub stdout: String,
    pub value: Option<serde_json::Value>,
}

pub fn try_execute_rh_invocation(
    operation: ScriptOperation,
    source: &str,
    fleet_bridge: Option<crate::script_rh_host::FleetBridgeFn>,
) -> Result<Option<RhInvocationResult>, agenterm_rh::RhError> {
    if !rh_backend_enabled() {
        return Ok(None);
    }

    match operation {
        ScriptOperation::Api => Ok(None),
        ScriptOperation::Check => {
            if !source.is_empty() {
                rh_check(source)?;
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
            let entry_value =
                crate::script_rh_host::call_pack_entry_with_host(&native_path, fleet_bridge)?;
            let mut stdout = String::new();
            for line in &pack.cc_lines {
                stdout.push_str(line);
                stdout.push('\n');
            }
            Ok(Some(RhInvocationResult {
                stdout,
                value: Some(serde_json::Value::from(entry_value)),
            }))
        }
    }
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
    use super::{ScriptBackend, rh_backend_enabled, try_execute_rh_invocation};
    use crate::script_protocol::ScriptOperation;

    #[test]
    fn default_backend_is_rhai() {
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Rhai);
        assert!(!rh_backend_enabled());
        assert!(
            try_execute_rh_invocation(ScriptOperation::Check, "fn entry() { 1 }", None)
                .expect("probe")
                .is_none()
        );
    }
}
