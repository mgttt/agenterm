//! OS-neutral typed failures shared by every local IPC transport adapter.

use std::{fmt, io};

use crate::ipc::IpcEndpoint;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcTransportErrorCode {
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
pub struct IpcTransportError {
    pub code: IpcTransportErrorCode,
    pub endpoint: String,
    source: io::Error,
}

impl IpcTransportError {
    pub fn new(
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
    pub fn io_kind(&self) -> io::ErrorKind {
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

pub type TransportResult<T> = std::result::Result<T, IpcTransportError>;

pub fn transport_io(endpoint: &IpcEndpoint, error: io::Error) -> IpcTransportError {
    IpcTransportError::new(IpcTransportErrorCode::Io, endpoint.to_string(), error)
}

pub fn map_bind_error(endpoint: &IpcEndpoint, error: io::Error) -> IpcTransportError {
    let code = if error.kind() == io::ErrorKind::AddrInUse {
        IpcTransportErrorCode::EndpointInUse
    } else {
        IpcTransportErrorCode::Io
    };
    IpcTransportError::new(code, endpoint.to_string(), error)
}

pub fn timeout_error(code: IpcTransportErrorCode, endpoint: &IpcEndpoint) -> IpcTransportError {
    IpcTransportError::new(
        code,
        endpoint.to_string(),
        io::Error::new(io::ErrorKind::TimedOut, "bounded IPC operation timed out"),
    )
}

pub fn unsupported(endpoint: &IpcEndpoint, message: &str) -> IpcTransportError {
    IpcTransportError::new(
        IpcTransportErrorCode::UnsupportedEndpoint,
        endpoint.to_string(),
        io::Error::new(io::ErrorKind::Unsupported, message),
    )
}
