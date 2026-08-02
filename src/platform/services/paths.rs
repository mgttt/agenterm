//! AgenTerm naming composed over reusable host filesystem conventions.

use std::{ffi::OsString, path::PathBuf};

pub(crate) fn terminal_default_font_size() -> u16 {
    crate::platform::terminal_default_font_size()
}

pub(crate) fn script_worker_executable_names() -> Vec<String> {
    let native = crate::platform::filesystem::executable_name(
        crate::platform::script_worker_default_executable_name(),
    );
    if native.ends_with(".exe") {
        vec![native]
    } else {
        vec![native, "agenterm-rhai.exe".to_owned()]
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
        .unwrap_or_else(|| {
            crate::platform::settings_root_path().join("settings.json")
        })
}

pub(crate) fn instance_registry_dir(override_path: Option<OsString>) -> PathBuf {
    override_path
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::platform::instance_registry_directory_root().join("instances")
        })
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
