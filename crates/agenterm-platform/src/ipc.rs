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

pub use selected::ipc::NativeStreamExt;

pub struct NativeStream(pub(crate) selected::ipc::NativeStream);

impl NativeStream {
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
    /// Parses an OS-native local transport without linking TCP authority or
    /// IP-address parsing into consumers that cannot use network transport.
    pub fn from_native_address(address: &str) -> Result<Self, ParseIpcEndpointError> {
        if let Some(path) = address.strip_prefix("unix:") {
            nonempty_endpoint_value(path)?;
            return PathBuf::from(path)
                .is_absolute()
                .then(|| Self::UnixSocket(path.to_owned()))
                .ok_or(ParseIpcEndpointError);
        }
        if let Some(name) = address.strip_prefix("pipe:") {
            return nonempty_endpoint_value(name).map(|()| Self::NamedPipe(name.to_owned()));
        }
        Err(ParseIpcEndpointError)
    }

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

    #[test]
    fn native_address_parser_excludes_tcp_before_generic_network_parsing() {
        assert_eq!(
            IpcEndpoint::from_native_address("pipe:agenterm-test"),
            Ok(IpcEndpoint::NamedPipe("agenterm-test".to_owned()))
        );
        assert!(IpcEndpoint::from_native_address("unix:relative.sock").is_err());
        assert!(IpcEndpoint::from_native_address("tcp:127.0.0.1:42").is_err());
    }
}
