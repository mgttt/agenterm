//! AgenTerm naming composed over reusable host filesystem conventions.

use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScriptWorkerExecutableRole {
    Primary,
    CompatibilityFallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScriptWorkerExecutableCandidate {
    pub(crate) name: String,
    pub(crate) role: ScriptWorkerExecutableRole,
}

impl ScriptWorkerExecutableCandidate {
    pub(crate) fn is_compatibility_fallback(&self) -> bool {
        self.role == ScriptWorkerExecutableRole::CompatibilityFallback
    }
}

pub(crate) fn terminal_default_font_size() -> u16 {
    crate::platform::terminal_default_font_size()
}

pub(crate) fn script_worker_executable_names() -> Vec<ScriptWorkerExecutableCandidate> {
    let mut candidates = Vec::with_capacity(4);
    push_worker_names(
        &mut candidates,
        crate::platform::script_worker_default_executable_name(),
        ScriptWorkerExecutableRole::Primary,
    );
    push_worker_names(
        &mut candidates,
        crate::platform::policy::paths::script_worker_compatibility_executable_name(),
        ScriptWorkerExecutableRole::CompatibilityFallback,
    );
    candidates
}

fn push_worker_names(
    candidates: &mut Vec<ScriptWorkerExecutableCandidate>,
    base_name: &str,
    role: ScriptWorkerExecutableRole,
) {
    let native = crate::platform::filesystem::executable_name(base_name);
    candidates.push(ScriptWorkerExecutableCandidate {
        name: native.clone(),
        role: role.clone(),
    });
    if !native.ends_with(".exe") {
        candidates.push(ScriptWorkerExecutableCandidate {
            name: format!("{base_name}.exe"),
            role,
        });
    }
}

pub(crate) fn control_center_executable_name() -> String {
    crate::platform::filesystem::executable_name("agenterm-cc")
}

pub(crate) fn default_workspace_path() -> PathBuf {
    if let Some(scope) = crate::platform::workspace_instance_scope() {
        crate::platform::ipc::default_workspace_path(&scope)
    } else {
        crate::platform::default_workspace_path()
    }
}

pub(crate) fn settings_path(override_path: Option<OsString>) -> PathBuf {
    override_path
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::platform::settings_root_path().join("settings.json"))
}

pub(crate) fn instance_registry_dir(override_path: Option<OsString>) -> PathBuf {
    override_path
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::platform::instance_registry_directory_root().join("instances"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_worker_candidates_keep_primary_before_compatibility_fallback() {
        let candidates = script_worker_executable_names();
        let first_fallback = candidates
            .iter()
            .position(|candidate| {
                candidate.role == ScriptWorkerExecutableRole::CompatibilityFallback
            })
            .expect("compatibility fallback");

        assert!(first_fallback > 0);
        assert!(
            candidates[..first_fallback]
                .iter()
                .all(|candidate| candidate.role == ScriptWorkerExecutableRole::Primary)
        );
        assert!(
            candidates[first_fallback..].iter().all(
                |candidate| candidate.role == ScriptWorkerExecutableRole::CompatibilityFallback
            )
        );
        assert_eq!(
            candidates[0].name,
            crate::platform::filesystem::executable_name("agenterm-rh")
        );
        assert_eq!(
            candidates[first_fallback].name,
            crate::platform::filesystem::executable_name("agenterm-rhai")
        );
    }

    #[test]
    fn explicit_instance_registry_path_has_priority() {
        let path = instance_registry_dir(Some(OsString::from("isolated-instances")));
        assert_eq!(path, PathBuf::from("isolated-instances"));
    }
}
