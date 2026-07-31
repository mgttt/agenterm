//! Linux filesystem and executable-name conventions.

use std::{env, ffi::OsString, path::PathBuf};

use crate::platform::contract::ipc::{LogicalInstance, ServerScopeId};

pub(crate) const fn terminal_default_font_size() -> u16 {
    12
}

pub(crate) fn script_worker_executable_names() -> &'static [&'static str] {
    &["agenterm-script", "agenterm-script.exe"]
}

pub(crate) fn control_center_executable_name() -> &'static str {
    "agenterm-cc"
}

pub(crate) fn default_workspace_path() -> PathBuf {
    let instance = env::var("AGENTERM_INSTANCE")
        .ok()
        .and_then(|value| value.parse::<LogicalInstance>().ok())
        .unwrap_or_default();
    ServerScopeId::current(&instance)
        .map(|scope| crate::platform::ipc::default_workspace_path(&scope))
        .unwrap_or_else(|_| {
            crate::platform::ipc::unix_data_root_from(
                env::var_os("XDG_DATA_HOME"),
                env::var_os("HOME"),
                env::temp_dir(),
            )
            .join("workspaces")
            .join("main.json")
        })
}

pub(crate) fn settings_path(override_path: Option<OsString>) -> PathBuf {
    unix_settings_path(
        override_path,
        env::var_os("XDG_CONFIG_HOME"),
        env::var_os("HOME"),
        env::temp_dir(),
    )
}

pub(crate) fn instance_registry_dir(override_path: Option<OsString>) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join(".local")
        .join("share")
        .join("agenterm")
        .join("instances")
}

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
    fn settings_convention_prefers_absolute_xdg_then_home() {
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
