//! Typed operating-system facade for local IPC identity, endpoint placement,
//! persistence roots, and native byte streams.
//!
//! Product modules depend only on this file. Target selection and native API
//! use are confined to this platform boundary and its adapter.

use std::{io, path::PathBuf};

use crate::platform::contract::{
    ipc::{IpcEndpoint, ServerScopeId},
    ipc_transport::{IpcTransportError, unsupported},
};

/// Security-context identity bytes used only to derive an opaque server scope.
/// This type deliberately has no display, debug, or serialization surface.
pub(crate) use agenterm_platform::ipc::{NativeListener, NativeStream, TrustedUserIdentity};

pub(crate) fn trusted_user_identity() -> io::Result<TrustedUserIdentity> {
    agenterm_platform::ipc::trusted_user_identity()
}

pub(crate) fn default_native_endpoint(scope: &ServerScopeId) -> IpcEndpoint {
    match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => {
            IpcEndpoint::NamedPipe(format!(r"\\.\pipe\agenterm-{}", scope.as_str()))
        }
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
            IpcEndpoint::UnixSocket(
                agenterm_platform::ipc::native_runtime_directory()
                    .join("agenterm")
                    .join(format!("{}.sock", scope.as_str()))
                    .to_string_lossy()
                    .into_owned(),
            )
        }
        _ => IpcEndpoint::UnixSocket(
            std::env::temp_dir()
                .join("agenterm")
                .join(format!("{}.sock", scope.as_str()))
                .to_string_lossy()
                .into_owned(),
        ),
    }
}

pub(crate) fn default_workspace_path(scope: &ServerScopeId) -> PathBuf {
    match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => windows_scoped_workspace_root()
            .join("workspaces")
            .join(format!("{}.json", scope.as_str())),
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
            unix_data_root_from(
                std::env::var_os("XDG_DATA_HOME"),
                std::env::var_os("HOME"),
                std::env::temp_dir(),
            )
            .join("workspaces")
            .join(format!("{}.json", scope.as_str()))
        }
        _ => std::env::temp_dir()
            .join("agenterm")
            .join("workspaces")
            .join(format!("{}.json", scope.as_str())),
    }
}

pub(crate) fn default_workspace_path_for(scope: &ServerScopeId, is_main: bool) -> PathBuf {
    if is_main
        && matches!(
            agenterm_platform::platform_kind(),
            agenterm_platform::PlatformKind::Windows
        )
    {
        windows_main_workspace_root().join("workspace.json")
    } else {
        default_workspace_path(scope)
    }
}

fn windows_scoped_workspace_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("AgenTerm")
}

fn windows_main_workspace_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("AgenTerm")
}

/// Pure Unix persistence-root policy kept callable on every host so settings
/// and migration logic never needs an operating-system conditional.
pub(crate) fn unix_data_root_from(
    xdg_data_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    temp_dir: PathBuf,
) -> PathBuf {
    let data_home = xdg_data_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".local").join("share"))
        })
        .unwrap_or_else(|| {
            if temp_dir.is_absolute() {
                temp_dir
            } else {
                PathBuf::from("/tmp")
            }
        });
    data_home.join("agenterm")
}

pub(crate) fn native_transport_name() -> &'static str {
    agenterm_platform::ipc::native_transport_name()
}

pub(crate) fn unsupported_native_endpoint(
    endpoint: &IpcEndpoint,
    message: &'static str,
) -> IpcTransportError {
    unsupported(endpoint, message)
}
