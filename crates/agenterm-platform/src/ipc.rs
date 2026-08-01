//! Transport-qualified local IPC endpoint types.

use std::{fmt, path::PathBuf, str::FromStr};

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
