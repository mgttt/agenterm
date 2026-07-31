//! OS-neutral typed failures shared by every local IPC transport adapter.

use std::{fmt, io};

use super::ipc::IpcEndpoint;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IpcTransportErrorCode {
    UnsupportedEndpoint,
    InvalidEndpoint,
    EndpointInUse,
    #[allow(dead_code)]
    UnsafeEndpoint,
    ConnectTimeout,
    AcceptTimeout,
    Io,
}

#[derive(Debug)]
pub(crate) struct IpcTransportError {
    pub(crate) code: IpcTransportErrorCode,
    pub(crate) endpoint: String,
    source: io::Error,
}

impl IpcTransportError {
    pub(crate) fn new(
        code: IpcTransportErrorCode,
        endpoint: impl Into<String>,
        source: impl Into<io::Error>,
    ) -> Self {
        Self {
            code,
            endpoint: endpoint.into(),
            source: source.into(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn io_kind(&self) -> io::ErrorKind {
        self.source.kind()
    }
}

impl fmt::Display for IpcTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "IPC transport {:?} for {}: {}",
            self.code, self.endpoint, self.source
        )
    }
}

impl std::error::Error for IpcTransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

pub(crate) type TransportResult<T> = std::result::Result<T, IpcTransportError>;

pub(crate) fn transport_io(endpoint: &IpcEndpoint, error: io::Error) -> IpcTransportError {
    IpcTransportError::new(IpcTransportErrorCode::Io, endpoint.to_string(), error)
}

pub(crate) fn map_bind_error(endpoint: &IpcEndpoint, error: io::Error) -> IpcTransportError {
    let code = if error.kind() == io::ErrorKind::AddrInUse {
        IpcTransportErrorCode::EndpointInUse
    } else {
        IpcTransportErrorCode::Io
    };
    IpcTransportError::new(code, endpoint.to_string(), error)
}

pub(crate) fn timeout_error(
    code: IpcTransportErrorCode,
    endpoint: &IpcEndpoint,
) -> IpcTransportError {
    IpcTransportError::new(
        code,
        endpoint.to_string(),
        io::Error::new(io::ErrorKind::TimedOut, "bounded IPC operation timed out"),
    )
}

pub(crate) fn unsupported(endpoint: &IpcEndpoint, message: &str) -> IpcTransportError {
    IpcTransportError::new(
        IpcTransportErrorCode::UnsupportedEndpoint,
        endpoint.to_string(),
        io::Error::new(io::ErrorKind::Unsupported, message),
    )
}
