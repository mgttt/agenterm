//! Product path semantics and host directory naming policy.
//!
//! These tables compose product-level names and default locations over the
//! OS-neutral host-directory mechanism from `agenterm-platform`.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum AtomicPathSemantics {
    VerbatimLongPath,
    CanonicalSafe,
}

#[allow(dead_code)]
pub(crate) fn atomic_path_semantics() -> AtomicPathSemantics {
    if matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    ) {
        AtomicPathSemantics::VerbatimLongPath
    } else {
        AtomicPathSemantics::CanonicalSafe
    }
}

pub(crate) fn product_directory_name() -> &'static str {
    if crate::platform::is_windows_host() {
        "AgenTerm"
    } else {
        "agenterm"
    }
}

pub(crate) const fn script_worker_default_executable_name() -> &'static str {
    "agenterm-rh"
}

pub(crate) fn terminal_default_font_size() -> u16 {
    if crate::platform::is_macos_host() {
        14
    } else {
        12
    }
}

pub(crate) fn default_audit_path() -> PathBuf {
    if crate::platform::is_windows_host() {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(product_directory_name())
            .join("script-audit.jsonl")
    } else if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(path)
            .join(product_directory_name())
            .join("script-audit.jsonl")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(product_directory_name())
            .join("script-audit.jsonl")
    } else {
        std::env::temp_dir().join("agenterm-rhai-audit.jsonl")
    }
}

fn host_directories() -> agenterm_platform::filesystem::HostDirectories {
    agenterm_platform::filesystem::host_directories().unwrap_or_else(|_| {
        let fallback = std::env::temp_dir();
        agenterm_platform::filesystem::HostDirectories {
            config: fallback.clone(),
            local_data: fallback,
        }
    })
}

pub(crate) fn default_workspace_path() -> PathBuf {
    if crate::platform::is_windows_host() {
        local_data_root_for_product_directory(&host_directories()).join("workspace.json")
    } else {
        let scope = std::env::var("AGENTERM_INSTANCE")
            .ok()
            .and_then(|value| {
                value
                    .parse::<crate::platform::contract::ipc::LogicalInstance>()
                    .ok()
            })
            .and_then(|instance| {
                crate::platform::contract::ipc::ServerScopeId::current(&instance).ok()
            });
        scope
            .map(|scope_id| crate::platform::ipc::default_workspace_path(&scope_id))
            .unwrap_or_else(|| {
                local_data_root_for_product_directory(&host_directories())
                    .join("workspaces")
                    .join("main.json")
            })
    }
}

pub(crate) fn settings_root_path() -> PathBuf {
    let directories = host_directories();
    if crate::platform::is_windows_host() {
        local_data_root_for_product_directory(&directories)
    } else {
        config_root_for_product_directory(&directories)
    }
}

pub(crate) fn workspace_instance_scope() -> Option<crate::platform::contract::ipc::ServerScopeId> {
    if !crate::platform::is_unix_host() {
        return None;
    }
    std::env::var("AGENTERM_INSTANCE")
        .ok()
        .and_then(|value| {
            value
                .parse::<crate::platform::contract::ipc::LogicalInstance>()
                .ok()
        })
        .and_then(|instance| crate::platform::contract::ipc::ServerScopeId::current(&instance).ok())
}

pub(crate) fn instance_registry_directory_root() -> PathBuf {
    host_directories().local_data.join(product_directory_name())
}

pub(crate) fn ipc_default_workspace_path(
    scope: &crate::platform::contract::ipc::ServerScopeId,
) -> PathBuf {
    if crate::platform::is_windows_host() {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(product_directory_name())
            .join("workspaces")
            .join(format!("{}.json", scope.as_str()))
    } else {
        crate::platform::services::ipc::unix_data_root_from(
            std::env::var_os("XDG_DATA_HOME"),
            std::env::var_os("HOME"),
            std::env::temp_dir(),
        )
        .join("workspaces")
        .join(format!("{}.json", scope.as_str()))
    }
}

pub(crate) fn ipc_default_workspace_path_for(
    scope: &crate::platform::contract::ipc::ServerScopeId,
    is_main: bool,
) -> PathBuf {
    if is_main && crate::platform::is_windows_host() {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(product_directory_name())
            .join("workspace.json")
    } else {
        ipc_default_workspace_path(scope)
    }
}

pub(crate) fn config_root_for_product_directory(
    directories: &agenterm_platform::filesystem::HostDirectories,
) -> PathBuf {
    directories.config.join(product_directory_name())
}

pub(crate) fn local_data_root_for_product_directory(
    directories: &agenterm_platform::filesystem::HostDirectories,
) -> PathBuf {
    directories.local_data.join(product_directory_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_semantics_matches_windows_host() {
        assert_eq!(
            atomic_path_semantics(),
            if matches!(
                agenterm_platform::platform_kind(),
                agenterm_platform::PlatformKind::Windows
            ) {
                AtomicPathSemantics::VerbatimLongPath
            } else {
                AtomicPathSemantics::CanonicalSafe
            }
        );
    }

    #[test]
    fn product_names_are_stable() {
        assert_eq!(script_worker_default_executable_name(), "agenterm-rh");
        assert!(!product_directory_name().is_empty());
    }
}
