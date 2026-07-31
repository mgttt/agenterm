//! Host filesystem conventions exposed through one platform facade.
//!
//! Product modules consume these resolved paths and do not branch on an OS.

use std::{env, ffi::OsString, path::PathBuf};

pub(crate) fn terminal_default_font_size() -> u16 {
    if cfg!(target_os = "macos") { 14 } else { 12 }
}

/// Candidate sidecar names for the local Script worker, ordered by the host
/// platform's native executable convention.
pub(crate) fn script_worker_executable_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["agenterm-script.exe"]
    }
    #[cfg(not(windows))]
    {
        &["agenterm-script", "agenterm-script.exe"]
    }
}

pub(crate) fn settings_path(override_path: Option<OsString>) -> PathBuf {
    #[cfg(windows)]
    {
        windows_settings_path(override_path, env::var_os("LOCALAPPDATA"))
    }
    #[cfg(unix)]
    {
        unix_settings_path(
            override_path,
            env::var_os("XDG_CONFIG_HOME"),
            env::var_os("HOME"),
            env::temp_dir(),
        )
    }
}

pub(crate) fn instance_registry_dir(override_path: Option<OsString>) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join("AgenTerm")
            .join("instances")
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join(".local")
            .join("share")
            .join("agenterm")
            .join("instances")
    }
}

#[cfg(windows)]
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

#[cfg(unix)]
pub(crate) fn unix_settings_path(
    override_path: Option<OsString>,
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
    temp_dir: PathBuf,
) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    let config_home = xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".config"))
        })
        .unwrap_or_else(|| {
            if temp_dir.is_absolute() {
                temp_dir
            } else {
                PathBuf::from("/tmp")
            }
        });
    config_home.join("agenterm").join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_instance_registry_path_has_priority() {
        let path = instance_registry_dir(Some(OsString::from("isolated-instances")));
        assert_eq!(path, PathBuf::from("isolated-instances"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_settings_convention_preserves_override_and_local_app_data() {
        assert_eq!(
            windows_settings_path(Some(r"D:\isolated\settings.json".into()), None),
            PathBuf::from(r"D:\isolated\settings.json")
        );
        assert_eq!(
            windows_settings_path(Some("".into()), Some(r"D:\profile".into())),
            PathBuf::from(r"D:\profile\AgenTerm\settings.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_settings_convention_prefers_absolute_xdg_then_home() {
        assert_eq!(
            unix_settings_path(
                None,
                Some("relative-config".into()),
                Some("/home/example".into()),
                PathBuf::from("/var/tmp"),
            ),
            PathBuf::from("/home/example/.config/agenterm/settings.json")
        );
    }
}
