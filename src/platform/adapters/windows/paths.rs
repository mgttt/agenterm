//! Windows filesystem and executable-name conventions.

use std::{env, ffi::OsString, path::PathBuf};

pub(crate) const fn terminal_default_font_size() -> u16 {
    12
}

pub(crate) fn script_worker_executable_names() -> &'static [&'static str] {
    &["agenterm-script.exe"]
}

pub(crate) fn control_center_executable_name() -> &'static str {
    "agenterm-cc.exe"
}

pub(crate) fn default_workspace_path() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("AgenTerm")
        .join("workspace.json")
}

pub(crate) fn settings_path(override_path: Option<OsString>) -> PathBuf {
    windows_settings_path(override_path, env::var_os("LOCALAPPDATA"))
}

pub(crate) fn instance_registry_dir(override_path: Option<OsString>) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("AgenTerm")
        .join("instances")
}

pub(crate) fn windows_settings_path(
    override_path: Option<OsString>,
    local_app_data: Option<OsString>,
) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    local_app_data
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("AgenTerm")
        .join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_convention_preserves_override_and_local_app_data() {
        assert_eq!(
            windows_settings_path(Some(r"D:\\isolated\\settings.json".into()), None),
            PathBuf::from(r"D:\\isolated\\settings.json")
        );
        assert_eq!(
            windows_settings_path(Some("".into()), Some(r"D:\\profile".into())),
            PathBuf::from(r"D:\\profile\\AgenTerm\\settings.json")
        );
    }
}
