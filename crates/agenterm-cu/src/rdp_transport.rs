//! RDP transport placeholder for the `rdp` target tier (PRD_02_30 cut 3.46).
//!
//! Host `agenterm-cu --rdp <host[:port]>` accepts the endpoint syntax and
//! selects `TargetRef::Rdp`, then every authorized command fails closed with
//! `error.code = "rdp_unavailable"` before any socket, TLS/CredSSP/NLA,
//! credential lookup, desktop attachment, screenshot, coordinate fallback,
//! or silent SSH/VNC/`current` reuse.
//!
//! Default TCP port 3389 is syntax-only. This module never connects.
//! `tree --window HANDLE` is the reserved first observe argv for a later
//! Windows agent that owns real session + UIA-over-RDP evidence. Until that
//! cut lands, no RDP capability is claimed.

use crate::{
    auth::Authorization,
    command::Command as CuCommand,
    reply::{CuError, CuReply},
};

/// Default RDP port when `--rdp host` omits `:port`. Reserved syntax only —
/// the placeholder never dials it.
pub const DEFAULT_RDP_PORT: u16 = 3389;

/// Opaque RDP endpoint. Holds host/port for diagnostics and later transport
/// work; cut 3.46 performs no I/O against it.
#[derive(Clone, Debug)]
pub struct RdpEndpoint {
    pub host: String,
    pub port: u16,
}

impl RdpEndpoint {
    /// Build from CLI `--rdp host[:port]`. Empty destination and non-numeric
    /// ports are `invalid_input` (parse failed before transport selection).
    /// A well-formed endpoint is accepted without contacting the host.
    pub fn from_parts(destination: String) -> Result<Self, CuError> {
        let destination = destination.trim().to_owned();
        if destination.is_empty() {
            return Err(CuError::new(
                "invalid_input",
                "rdp target requires a non-empty --rdp <host[:port]> destination",
            ));
        }
        let (host, port_from_dest) = split_host_port(&destination)?;
        if host.is_empty() {
            return Err(CuError::new(
                "invalid_input",
                "rdp target host must be non-empty",
            ));
        }
        let port = port_from_dest.unwrap_or(DEFAULT_RDP_PORT);
        Ok(Self { host, port })
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Fail-closed RDP entry point. Never opens a socket, never rewrites the
/// command to another target, never spawns a worker.
pub fn run_session(
    endpoint: &RdpEndpoint,
    _command: &CuCommand,
    _auth: &Authorization,
) -> Result<CuReply, CuError> {
    Err(CuError::new(
        "rdp_unavailable",
        format!(
            "RDP transport is reserved but not implemented for {}",
            endpoint.address()
        ),
    ))
}

fn split_host_port(raw: &str) -> Result<(String, Option<u16>), CuError> {
    if let Some((host, port_raw)) = raw.rsplit_once(':')
        && !host.is_empty()
        && !host.contains(']')
        && port_raw.chars().all(|c| c.is_ascii_digit())
        && !port_raw.is_empty()
    {
        let port: u16 = port_raw.parse().map_err(|_| {
            CuError::new(
                "invalid_input",
                format!("rdp port in {raw:?} is not a valid TCP port"),
            )
        })?;
        return Ok((host.to_owned(), Some(port)));
    }
    // A trailing `:` with a non-numeric suffix is a malformed port, not a
    // bare hostname (hostnames may contain colons only via bracketed IPv6,
    // which this first cut does not accept).
    if let Some((host, port_raw)) = raw.rsplit_once(':')
        && !host.is_empty()
        && !port_raw.is_empty()
        && !port_raw.chars().all(|c| c.is_ascii_digit())
    {
        return Err(CuError::new(
            "invalid_input",
            format!("rdp port in {raw:?} is not a valid TCP port"),
        ));
    }
    Ok((raw.to_owned(), None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth::Authorization, command::Command, target::TargetRef};
    use std::net::TcpListener;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread;
    use std::time::Duration;

    #[test]
    fn from_parts_accepts_host_and_optional_port() {
        let ep = RdpEndpoint::from_parts("WINDOWS_HOST:3389".into()).expect("parse");
        assert_eq!(ep.host, "WINDOWS_HOST");
        assert_eq!(ep.port, 3389);
        assert_eq!(ep.address(), "WINDOWS_HOST:3389");

        let ep = RdpEndpoint::from_parts("windows.example.test".into()).expect("default port");
        assert_eq!(ep.host, "windows.example.test");
        assert_eq!(ep.port, DEFAULT_RDP_PORT);
    }

    #[test]
    fn empty_and_malformed_ports_are_invalid_input() {
        let err = RdpEndpoint::from_parts(String::new()).expect_err("empty");
        assert_eq!(err.code, "invalid_input");
        let err = RdpEndpoint::from_parts("host:notaport".into()).expect_err("bad port");
        assert_eq!(err.code, "invalid_input");
        let err = RdpEndpoint::from_parts("host:99999".into()).expect_err("overflow port");
        assert_eq!(err.code, "invalid_input");
    }

    #[test]
    fn run_session_is_rdp_unavailable_without_connecting() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind sentinel");
        listener
            .set_nonblocking(true)
            .expect("nonblocking sentinel");
        let addr = listener.local_addr().expect("local addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_bg = Arc::clone(&hits);
        let sentinel = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_millis(200);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok(_) => {
                        hits_bg.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        let endpoint = RdpEndpoint {
            host: addr.ip().to_string(),
            port: addr.port(),
        };
        let command = Command::Tree {
            target: TargetRef::Rdp,
            window: Some(0x1000),
        };
        let auth = Authorization::from_cli_and_env(Some("observe"));
        let err = run_session(&endpoint, &command, &auth).expect_err("placeholder");
        assert_eq!(err.code, "rdp_unavailable");
        assert!(
            err.message.contains(&endpoint.address()),
            "message should name the non-secret endpoint: {}",
            err.message
        );

        sentinel.join().expect("sentinel join");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "RDP placeholder must not open a TCP connection to the endpoint"
        );
    }
}
