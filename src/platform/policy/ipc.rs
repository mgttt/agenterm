//! Product IPC endpoint naming policy shared by product modules.
//!
//! The native transport mechanism lives in agenterm-platform; this table
//! decides the AgenTerm-facing endpoint identity and placement.

use crate::platform::contract::ipc::{IpcEndpoint, ServerScopeId};

pub(crate) fn ipc_default_native_endpoint(scope: &ServerScopeId) -> IpcEndpoint {
    if crate::platform::policy::host::is_windows_host() {
        IpcEndpoint::NamedPipe(format!(r"\\.\pipe\agenterm-{}", scope.as_str()))
    } else {
        IpcEndpoint::UnixSocket(
            agenterm_platform::ipc::native_runtime_directory()
                .join("agenterm")
                .join(format!("{}.sock", scope.as_str()))
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::ipc_default_native_endpoint;
    use crate::platform::contract::ipc::{IpcEndpoint, LogicalInstance, ServerScopeId};
    use crate::platform::policy::host::is_windows_host;

    #[test]
    fn endpoint_kind_matches_runtime_host() {
        let scope = ServerScopeId::current(&LogicalInstance::Main).unwrap();
        let endpoint = ipc_default_native_endpoint(&scope);
        assert_eq!(
            matches!(endpoint, IpcEndpoint::NamedPipe(_)),
            is_windows_host()
        );
    }
}
