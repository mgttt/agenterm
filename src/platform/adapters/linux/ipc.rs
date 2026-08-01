//! AgenTerm endpoint and workspace policy for Unix transports.

use std::{env, path::PathBuf};

use crate::platform::contract::ipc::{IpcEndpoint, ServerScopeId};

pub(crate) fn default_native_endpoint(scope: &ServerScopeId) -> IpcEndpoint {
    IpcEndpoint::UnixSocket(
        agenterm_platform::ipc::native_runtime_directory()
            .join("agenterm")
            .join(format!("{}.sock", scope.as_str()))
            .to_string_lossy()
            .into_owned(),
    )
}

pub(crate) fn default_workspace_path(scope: &ServerScopeId) -> PathBuf {
    super::unix_data_root_from(
        env::var_os("XDG_DATA_HOME"),
        env::var_os("HOME"),
        env::temp_dir(),
    )
    .join("workspaces")
    .join(format!("{}.json", scope.as_str()))
}

pub(crate) fn default_workspace_path_for(scope: &ServerScopeId, _is_main: bool) -> PathBuf {
    default_workspace_path(scope)
}
