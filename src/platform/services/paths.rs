//! Host filesystem conventions routed through the selected platform adapter.

use std::{ffi::OsString, path::PathBuf};

use crate::platform::selected;

pub(crate) fn terminal_default_font_size() -> u16 {
    selected::paths::terminal_default_font_size()
}

/// Candidate sidecar names for the local Script worker, ordered by the host
/// platform's native executable convention.
pub(crate) fn script_worker_executable_names() -> &'static [&'static str] {
    selected::paths::script_worker_executable_names()
}

/// Native sidecar name for the Control Center shell, without exposing an
/// executable-extension convention to product callers.
pub(crate) fn control_center_executable_name() -> &'static str {
    selected::paths::control_center_executable_name()
}

/// Resolve the default persisted workspace path for the current logical
/// instance without exposing host conventions to product persistence.
pub(crate) fn default_workspace_path() -> PathBuf {
    selected::paths::default_workspace_path()
}

pub(crate) fn settings_path(override_path: Option<OsString>) -> PathBuf {
    selected::paths::settings_path(override_path)
}

pub(crate) fn instance_registry_dir(override_path: Option<OsString>) -> PathBuf {
    selected::paths::instance_registry_dir(override_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_instance_registry_path_has_priority() {
        let path = instance_registry_dir(Some(OsString::from("isolated-instances")));
        assert_eq!(path, PathBuf::from("isolated-instances"));
    }
}
