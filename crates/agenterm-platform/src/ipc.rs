//! Transport-qualified local IPC endpoint types.

use std::{
    fmt,
    io::{self, Read, Write},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

pub use crate::contract::ipc_transport::{
    IpcTransportError, IpcTransportErrorCode, TransportResult,
};
use crate::selected;

pub struct TrustedUserIdentity {
    pub kind: &'static str,
    pub bytes: Vec<u8>,
}

pub fn trusted_user_identity() -> io::Result<TrustedUserIdentity> {
    selected::ipc::trusted_user_identity()
}

pub fn native_runtime_directory() -> PathBuf {
    selected::ipc::native_runtime_directory()
}

pub fn native_transport_name() -> &'static str {
    selected::ipc::NATIVE_TRANSPORT_NAME
}

pub struct NativeListener(selected::ipc::NativeListener);

impl NativeListener {
    pub fn bind(endpoint: &IpcEndpoint) -> TransportResult<Self> {
        selected::ipc::NativeListener::bind(endpoint).map(Self)
    }

    pub fn accept(&mut self, timeout: Duration) -> TransportResult<NativeStream> {
        self.0.accept(timeout).map(NativeStream)
    }
}

pub struct NativeStream(selected::ipc::NativeStream);

#[cfg(windows)]
impl std::os::windows::io::AsRawHandle for NativeStream {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.0.as_raw_handle()
    }
}

#[cfg(windows)]
impl std::os::windows::io::AsHandle for NativeStream {
    fn as_handle(&self) -> std::os::windows::io::BorrowedHandle<'_> {
        self.0.as_handle()
    }
}

#[cfg(unix)]
impl std::os::fd::AsRawFd for NativeStream {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.0.as_raw_fd()
    }
}

#[cfg(unix)]
impl std::os::fd::AsFd for NativeStream {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl NativeStream {
    #[cfg(windows)]
    pub fn from_owned_handle(handle: std::os::windows::io::OwnedHandle, timeout: Duration) -> Self {
        Self(selected::ipc::NativeStream::from_owned_handle(
            handle, timeout,
        ))
    }

    #[cfg(windows)]
    pub fn into_owned_handle(self) -> std::os::windows::io::OwnedHandle {
        self.0.into_owned_handle()
    }

    #[cfg(unix)]
    pub fn from_owned_fd(
        descriptor: std::os::fd::OwnedFd,
        endpoint: &IpcEndpoint,
        timeout: Duration,
    ) -> TransportResult<Self> {
        selected::ipc::NativeStream::from_owned_fd(descriptor, endpoint, timeout).map(Self)
    }

    #[cfg(unix)]
    pub fn into_owned_fd(self) -> std::os::fd::OwnedFd {
        self.0.into_owned_fd()
    }

    pub fn connect(endpoint: &IpcEndpoint, timeout: Duration) -> TransportResult<Self> {
        selected::ipc::NativeStream::connect(endpoint, timeout).map(Self)
    }

    pub fn set_io_timeout(&mut self, timeout: Duration) -> TransportResult<()> {
        self.0.set_io_timeout(timeout)
    }
}

impl Read for NativeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for NativeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
#[non_exhaustive]
pub enum IpcEndpoint {
    UnixSocket(String),
    NamedPipe(String),
    Tcp { host: String, port: u16 },
}

impl IpcEndpoint {
    pub fn from_legacy_address(address: &str) -> Result<Self, ParseIpcEndpointError> {
        parse_tcp_authority(address).map(|(host, port)| Self::Tcp { host, port })
    }

    pub fn legacy_address(&self) -> Option<String> {
        match self {
            Self::Tcp { host, port } => Some(format_tcp_authority(host, *port)),
            Self::UnixSocket(_) | Self::NamedPipe(_) => None,
        }
    }

    pub fn unix_socket_path(&self) -> Option<PathBuf> {
        match self {
            Self::UnixSocket(path) => Some(PathBuf::from(path)),
            Self::NamedPipe(_) | Self::Tcp { .. } => None,
        }
    }

    pub const fn transport_name(&self) -> &'static str {
        match self {
            Self::UnixSocket(_) => "unix",
            Self::NamedPipe(_) => "named_pipe",
            Self::Tcp { .. } => "tcp",
        }
    }

    pub fn validate_local(&self) -> Result<(), ParseIpcEndpointError> {
        match self {
            Self::Tcp { host, port } => format_tcp_authority(host, *port)
                .parse::<std::net::SocketAddr>()
                .ok()
                .filter(|address| address.ip().is_loopback())
                .map(|_| ())
                .ok_or(ParseIpcEndpointError),
            Self::UnixSocket(path) => {
                nonempty_endpoint_value(path)?;
                PathBuf::from(path)
                    .is_absolute()
                    .then_some(())
                    .ok_or(ParseIpcEndpointError)
            }
            Self::NamedPipe(name) => nonempty_endpoint_value(name),
        }
    }
}

impl fmt::Display for IpcEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnixSocket(path) => write!(formatter, "unix:{path}"),
            Self::NamedPipe(name) => write!(formatter, "pipe:{name}"),
            Self::Tcp { host, port } => {
                write!(formatter, "tcp:{}", format_tcp_authority(host, *port))
            }
        }
    }
}

impl From<IpcEndpoint> for String {
    fn from(endpoint: IpcEndpoint) -> Self {
        endpoint.to_string()
    }
}

impl TryFrom<String> for IpcEndpoint {
    type Error = ParseIpcEndpointError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseIpcEndpointError;

impl fmt::Display for ParseIpcEndpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IPC endpoint must be unix:<path>, pipe:<name>, or tcp:<host>:<port>")
    }
}

impl std::error::Error for ParseIpcEndpointError {}

impl FromStr for IpcEndpoint {
    type Err = ParseIpcEndpointError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(path) = value.strip_prefix("unix:") {
            return nonempty_endpoint_value(path).map(|()| Self::UnixSocket(path.to_owned()));
        }
        if let Some(name) = value.strip_prefix("pipe:") {
            return nonempty_endpoint_value(name).map(|()| Self::NamedPipe(name.to_owned()));
        }
        if let Some(authority) = value.strip_prefix("tcp:") {
            return parse_tcp_authority(authority).map(|(host, port)| Self::Tcp { host, port });
        }
        Err(ParseIpcEndpointError)
    }
}

fn nonempty_endpoint_value(value: &str) -> Result<(), ParseIpcEndpointError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        Err(ParseIpcEndpointError)
    } else {
        Ok(())
    }
}

fn parse_tcp_authority(authority: &str) -> Result<(String, u16), ParseIpcEndpointError> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once("]:").ok_or(ParseIpcEndpointError)?
    } else {
        authority.rsplit_once(':').ok_or(ParseIpcEndpointError)?
    };
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        return Err(ParseIpcEndpointError);
    }
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or(ParseIpcEndpointError)?;
    Ok((host.to_owned(), port))
}

fn format_tcp_authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_round_trip_with_transport_identity() {
        for value in ["unix:/tmp/example.sock", "pipe:example", "tcp:[::1]:9042"] {
            let endpoint = value.parse::<IpcEndpoint>().expect("endpoint");
            assert_eq!(endpoint.to_string(), value);
        }
    }

    #[test]
    fn local_validation_rejects_remote_tcp_and_relative_unix_paths() {
        assert!(
            "tcp:192.0.2.1:42"
                .parse::<IpcEndpoint>()
                .unwrap()
                .validate_local()
                .is_err()
        );
        assert!(
            "unix:relative.sock"
                .parse::<IpcEndpoint>()
                .unwrap()
                .validate_local()
                .is_err()
        );
    }
}
