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
    crate::platform::ipc_default_native_endpoint(scope)
}

pub(crate) fn default_workspace_path(scope: &ServerScopeId) -> PathBuf {
    crate::platform::ipc_default_workspace_path(scope)
}

pub(crate) fn default_workspace_path_for(scope: &ServerScopeId, is_main: bool) -> PathBuf {
    crate::platform::ipc_default_workspace_path_for(scope, is_main)
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
