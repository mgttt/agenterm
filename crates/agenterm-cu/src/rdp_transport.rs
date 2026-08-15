//! RDP transport placeholder for the `rdp` target tier (PRD_02_30 cut 3.46/3.47).
//!
//! Host `agenterm-cu --rdp <host[:port]>` accepts the endpoint syntax and
//! selects `TargetRef::Rdp`. Cut 3.47: the observe verb `capabilities`
//! returns a static declaration that transport is placeholder/unavailable
//! and `tree` is unsupported — with **zero** socket I/O. Every other
//! authorized command still fails closed with `error.code =
//! "rdp_unavailable"` before any socket, TLS/CredSSP/NLA, credential
//! lookup, desktop attachment, screenshot, coordinate fallback, or silent
//! SSH/VNC/`current` reuse.
//!
//! Default TCP port 3389 is syntax-only. This module never connects.
//! `tree --window HANDLE` remains the reserved first *live* observe argv
//! for a later Windows agent that owns real session + UIA-over-RDP
//! evidence. Until that cut lands, no RDP live capability is claimed.

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

/// Static per-target capabilities declaration for the RDP placeholder.
/// Performs no DNS, TCP, TLS, CredSSP, UIA, accessibility, screenshot, or
/// coordinate work. Optional `endpoint` is diagnostic only (never dialed).
pub fn capabilities_declaration(endpoint: Option<&RdpEndpoint>) -> serde_json::Value {
    let mut transport = serde_json::json!({
        "status": "placeholder",
        "available": false,
        "reason": "rdp_unavailable",
    });
    if let Some(endpoint) = endpoint
        && let Some(obj) = transport.as_object_mut()
    {
        obj.insert(
            "endpoint".into(),
            serde_json::Value::String(endpoint.address()),
        );
    }
    serde_json::json!({
        "target": "rdp",
        "transport": transport,
        "verbs": {
            "capabilities": { "status": "available" },
            "tree": { "status": "unsupported", "reason": "rdp_unavailable" },
        },
    })
}

/// RDP entry point. `capabilities` is the only successful path (static
/// declaration, no I/O). Every other verb fails closed without a socket,
/// rewrite, or worker.
pub fn run_session(
    endpoint: &RdpEndpoint,
    command: &CuCommand,
    _auth: &Authorization,
) -> Result<CuReply, CuError> {
    if matches!(command, CuCommand::Capabilities { .. }) {
        return Ok(CuReply::ok(
            command,
            capabilities_declaration(Some(endpoint)),
        ));
    }
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

    fn spawn_sentinel(listener: TcpListener) -> (Arc<AtomicUsize>, thread::JoinHandle<()>) {
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
        (hits, sentinel)
    }

    #[test]
    fn run_session_tree_is_rdp_unavailable_without_connecting() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind sentinel");
        listener
            .set_nonblocking(true)
            .expect("nonblocking sentinel");
        let addr = listener.local_addr().expect("local addr");
        let (hits, sentinel) = spawn_sentinel(listener);

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

    #[test]
    fn capabilities_declaration_is_placeholder_and_tree_unsupported() {
        let endpoint = RdpEndpoint {
            host: "WINDOWS_HOST".into(),
            port: 3389,
        };
        let data = capabilities_declaration(Some(&endpoint));
        assert_eq!(data["target"], "rdp");
        assert_eq!(data["transport"]["status"], "placeholder");
        assert_eq!(data["transport"]["available"], false);
        assert_eq!(data["transport"]["reason"], "rdp_unavailable");
        assert_eq!(data["transport"]["endpoint"], "WINDOWS_HOST:3389");
        assert_eq!(data["verbs"]["capabilities"]["status"], "available");
        assert_eq!(data["verbs"]["tree"]["status"], "unsupported");
        assert_eq!(data["verbs"]["tree"]["reason"], "rdp_unavailable");
        // Live RDP / UIA / screenshots must not be declared available.
        assert!(data["verbs"].get("screenshot").is_none());
        assert!(data.pointer("/verbs/tree/available").is_none());
    }

    #[test]
    fn run_session_capabilities_succeeds_without_connecting() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind sentinel");
        listener
            .set_nonblocking(true)
            .expect("nonblocking sentinel");
        let addr = listener.local_addr().expect("local addr");
        let (hits, sentinel) = spawn_sentinel(listener);

        let endpoint = RdpEndpoint {
            host: addr.ip().to_string(),
            port: addr.port(),
        };
        let command = Command::Capabilities {
            target: TargetRef::Rdp,
        };
        let auth = Authorization::from_cli_and_env(Some("observe"));
        let reply = run_session(&endpoint, &command, &auth).expect("capabilities ok");
        assert!(reply.ok);
        assert_eq!(reply.target, "rdp");
        assert_eq!(reply.command, "capabilities");
        let data = reply.data.expect("data");
        assert_eq!(data["target"], "rdp");
        assert_eq!(data["transport"]["reason"], "rdp_unavailable");
        assert_eq!(data["verbs"]["tree"]["status"], "unsupported");

        sentinel.join().expect("sentinel join");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "RDP capabilities must not open a TCP connection to the endpoint"
        );
    }
}
