//! AgenTerm naming composed over reusable host filesystem conventions.

use std::{ffi::OsString, path::PathBuf};

use crate::platform::contract::ipc::{LogicalInstance, ServerScopeId};

fn host_directories() -> agenterm_platform::filesystem::HostDirectories {
    agenterm_platform::filesystem::host_directories().unwrap_or_else(|_| {
        let fallback = std::env::temp_dir();
        agenterm_platform::filesystem::HostDirectories {
            config: fallback.clone(),
            local_data: fallback,
        }
    })
}

pub(crate) fn terminal_default_font_size() -> u16 {
    crate::platform::terminal_default_font_size()
}

pub(crate) fn script_worker_executable_names() -> Vec<String> {
    let native = agenterm_platform::filesystem::executable_name(
        crate::platform::script_worker_default_executable_name(),
    );
    if native.ends_with(".exe") {
        vec![native]
    } else {
        vec![native, "agenterm-rhai.exe".to_owned()]
    }
}

pub(crate) fn control_center_executable_name() -> String {
    agenterm_platform::filesystem::executable_name("agenterm-cc")
}

pub(crate) fn default_workspace_path() -> PathBuf {
    if crate::platform::is_windows_host() {
        return crate::platform::local_data_root_for_product_directory(&host_directories())
            .join("workspace.json");
    }
    let instance = std::env::var("AGENTERM_INSTANCE")
        .ok()
        .and_then(|value| value.parse::<LogicalInstance>().ok())
        .unwrap_or_default();
    ServerScopeId::current(&instance)
        .map(|scope| crate::platform::ipc::default_workspace_path(&scope))
        .unwrap_or_else(|_| {
            crate::platform::local_data_root_for_product_directory(&host_directories())
                .join("workspaces")
                .join("main.json")
        })
}

pub(crate) fn settings_path(override_path: Option<OsString>) -> PathBuf {
    override_path
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let directories = host_directories();
            let root = if crate::platform::is_windows_host() {
                crate::platform::local_data_root_for_product_directory(&directories)
            } else {
                crate::platform::config_root_for_product_directory(&directories)
            };
            root.join("settings.json")
        })
}

pub(crate) fn instance_registry_dir(override_path: Option<OsString>) -> PathBuf {
    override_path
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            host_directories()
                .local_data
                .join(crate::platform::product_directory_name())
                .join("instances")
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
