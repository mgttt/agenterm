//! AgenTerm endpoint and workspace policy for the Windows transport.

use std::{env, path::PathBuf};

use crate::platform::contract::ipc::{IpcEndpoint, ServerScopeId};

pub(crate) fn default_native_endpoint(scope: &ServerScopeId) -> IpcEndpoint {
    IpcEndpoint::NamedPipe(format!(r"\\.\pipe\agenterm-{}", scope.as_str()))
}

pub(crate) fn default_workspace_path(scope: &ServerScopeId) -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("AgenTerm")
        .join("workspaces")
        .join(format!("{}.json", scope.as_str()))
}

pub(crate) fn default_workspace_path_for(scope: &ServerScopeId, is_main: bool) -> PathBuf {
    if is_main {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("AgenTerm")
            .join("workspace.json")
    } else {
        default_workspace_path(scope)
    }
}
