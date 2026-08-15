//! RFB/VNC transport for the `vnc` target tier (PRD_02_30).
//!
//! Host `agenterm-cu --vnc <host[:port]>` proves the endpoint speaks RFB
//! (loopback `x11vnc` is the first evidence path), rewrites the abstract
//! command to `target=current`, and runs a local `agenterm-cu exec --json -`
//! worker against the desktop session that x11vnc shares (`DISPLAY` /
//! `AT_SPI_BUS` / related env). Same verbs; no new verb. Structured
//! observation still uses AT-SPI GetText on that session — never
//! screenshot / `--coords` / RFB framebuffer OCR.
//!
//! Cut 3.31 locked the first observe path. Cut 3.32 locked the first
//! actuate path via `send-text`. Cut 3.33 locked the first clipboard write
//! via `paste --text`. Cut 3.34 locked clipboard publish via `copy`.
//! Cut 3.35 locked key delivery via `send-keys`. Cut 3.36 locked text
//! selection via `select` / `get-selection`. Cut 3.37 locked caret
//! placement via `set-caret` / `get-caret`. Cut 3.38 locked named Action
//! click. Cut 3.39 locked named scroll. Cut 3.40 locked named focus.
//! Cut 3.41 locked structured tree observe. Cut 3.42 locked get-caret as
//! its own observe path. Cut 3.43 locked get-extents as its own observe
//! path. Cut 3.44 locks get-selection as its own observe path: host
//! `get-selection --window H --name Command` over `--vnc` returns the
//! session AT-SPI selection range (`via=get-selection`; native
//! `GetNSelections` + `GetSelection(0)`; `n == 1` and integer `start` /
//! `end` equal the known precondition range so
//! `seed[start:end] == expected`) from a second `agenterm-con` `Command`
//! field. Gate precondition (not this cut's verb): `Command` holds a
//! known ASCII seed and a known non-empty selection `START..END` (use
//! already-landed `send-text` + `select`). Never screenshot / `--coords`
//! / mouse-drag / RFB framebuffer OCR / cached setter reply. Gate-owned
//! dedicated loopback x11vnc; never steal
//! `unix:/tmp/run-box/agenterm-con.sock` or treat the resident `:2` x11vnc
//! as the only proof. Observe and actuate grants both forward.
//! `get-extents` (3.43), `get-caret` (3.42), `tree` (3.41), `focus`
//! (3.40), `scroll` (3.39), `click` (3.38), `set-caret` (3.37),
//! `select` (3.36), `send-keys` (3.35), `copy` (3.34), `paste --text`
//! (3.33), and `send-text` (3.32) over vnc remain valid.
//!
//! This is not a second control protocol and not D-Bus port-forwarding.
//! Connect / protocol / auth failures are typed. True off-box VNC without
//! a co-located session worker remains a later cut; first evidence is
//! loopback.

use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use crate::{
    auth::Authorization,
    command::Command as CuCommand,
    reply::{CuError, CuReply},
};

/// Default RFB port when `--vnc host` omits `:port` and no `--vnc-port` /
/// `AGENTERM_CU_VNC_PORT` is set.
pub const DEFAULT_RFB_PORT: u16 = 5900;

/// Remote-desktop endpoint reached by RFB, with session env for the local
/// `current` worker that owns structured observe/actuate.
#[derive(Clone, Debug)]
pub struct VncEndpoint {
    pub host: String,
    pub port: u16,
    /// Absolute path of `agenterm-cu` for the session worker (loopback
    /// reuses the host binary path).
    pub worker_cu: PathBuf,
    /// `KEY=VAL` pairs applied by `env` before the worker (DISPLAY, AT-SPI, …).
    pub session_env: Vec<(String, String)>,
    pub connect_timeout_secs: u64,
}

impl VncEndpoint {
    /// Build from CLI flags plus env defaults. `destination` is `host` or
    /// `host:port` (IPv4 / hostname; first cut does not parse bracketed IPv6).
    pub fn from_parts(
        destination: String,
        port_override: Option<u16>,
        worker_cu: Option<PathBuf>,
        extra_env: Vec<(String, String)>,
    ) -> Result<Self, CuError> {
        let destination = destination.trim().to_owned();
        if destination.is_empty() {
            return Err(CuError::new(
                "invalid_input",
                "vnc target requires a non-empty --vnc <host[:port]> destination",
            ));
        }
        let (host, port_from_dest) = split_host_port(&destination)?;
        if host.is_empty() {
            return Err(CuError::new(
                "invalid_input",
                "vnc target host must be non-empty",
            ));
        }
        let port = port_override
            .or(port_from_dest)
            .or_else(|| {
                std::env::var("AGENTERM_CU_VNC_PORT")
                    .ok()
                    .and_then(|raw| raw.parse().ok())
            })
            .unwrap_or(DEFAULT_RFB_PORT);
        let worker_cu = worker_cu
            .or_else(|| std::env::var_os("AGENTERM_CU_VNC_CU").map(PathBuf::from))
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_else(|| PathBuf::from("agenterm-cu"));
        let mut session_env = default_session_env();
        if let Ok(raw) = std::env::var("AGENTERM_CU_VNC_ENV") {
            for part in raw.split(',') {
                if let Some(pair) = parse_env_pair(part) {
                    upsert_env(&mut session_env, pair.0, pair.1);
                }
            }
        }
        for (key, value) in extra_env {
            upsert_env(&mut session_env, key, value);
        }
        let connect_timeout_secs = std::env::var("AGENTERM_CU_VNC_CONNECT_TIMEOUT")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(10);
        Ok(Self {
            host,
            port,
            worker_cu,
            session_env,
            connect_timeout_secs,
        })
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Prove RFB reachability, then run `command` on a local `target=current` worker
/// bound to the session env for the desktop behind that VNC.
pub fn run_session(
    endpoint: &VncEndpoint,
    command: &CuCommand,
    auth: &Authorization,
) -> Result<CuReply, CuError> {
    rfb_handshake(endpoint)?;
    let session_command = rewrite_command_target_current(command)?;
    let payload = serde_json::to_string(&session_command).map_err(|error| {
        CuError::new(
            "serialize",
            format!("vnc transport could not serialize command: {error}"),
        )
    })?;
    let grant = auth.grant_cli_arg();
    if grant.is_empty() {
        return Err(CuError::new(
            "refused",
            "vnc transport requires at least one grant on the host command",
        ));
    }

    let mut worker_argv: Vec<String> = Vec::new();
    worker_argv.push("env".into());
    for (key, value) in &endpoint.session_env {
        if key.is_empty() || key.contains('=') || key.contains(|c: char| c.is_whitespace()) {
            return Err(CuError::new(
                "invalid_input",
                format!("vnc session env key is invalid: {key:?}"),
            ));
        }
        if value.contains(|c: char| c.is_whitespace()) {
            return Err(CuError::new(
                "invalid_input",
                format!(
                    "vnc session env value for {key} must not contain whitespace (got {value:?})"
                ),
            ));
        }
        worker_argv.push(format!("{key}={value}"));
    }
    // `exec` must lead the worker argv after env so dispatch_json sees it.
    worker_argv.push(endpoint.worker_cu.display().to_string());
    worker_argv.push("exec".into());
    worker_argv.push("--grant".into());
    worker_argv.push(grant);
    worker_argv.push("--json".into());
    worker_argv.push("-".into());

    let mut child_cmd = Command::new(&worker_argv[0]);
    for arg in &worker_argv[1..] {
        child_cmd.arg(arg);
    }
    child_cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = child_cmd.spawn().map_err(|error| {
        CuError::new(
            "vnc_unavailable",
            format!(
                "could not spawn session worker for {}: {error}",
                endpoint.address()
            ),
        )
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).map_err(|error| {
            CuError::new(
                "vnc_transport_failed",
                format!("could not write command JSON to vnc worker stdin: {error}"),
            )
        })?;
        drop(stdin);
    }

    let output = child.wait_with_output().map_err(|error| {
        CuError::new(
            "vnc_transport_failed",
            format!(
                "vnc session worker for {} failed: {error}",
                endpoint.address()
            ),
        )
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json_line = last_json_object_line(stdout.as_ref()).ok_or_else(|| {
        CuError::new(
            "vnc_transport_failed",
            format!(
                "vnc session worker produced no JSON reply (exit={}): stderr={}",
                output.status.code().unwrap_or(-1),
                trim_for_error(&stderr)
            ),
        )
    })?;

    let mut reply: CuReply = serde_json::from_str(json_line).map_err(|error| {
        CuError::new(
            "vnc_transport_failed",
            format!(
                "vnc session worker reply is not valid CuReply JSON: {error}; line={}",
                trim_for_error(json_line)
            ),
        )
    })?;
    // Host identity of this command is the vnc tier even when the worker
    // answered as target=current. Capabilities also restore data.target so
    // callers do not see the worker's "current" leak.
    restore_public_target(&mut reply, "vnc");
    if !output.status.success() && reply.ok {
        return Err(CuError::new(
            "vnc_transport_failed",
            format!(
                "vnc session worker exit {} with ok:true; stderr={}",
                output.status.code().unwrap_or(-1),
                trim_for_error(&stderr)
            ),
        ));
    }
    Ok(reply)
}

/// Public reply target is always the vnc tier. For `capabilities`, also
/// restore `data.target` and attach transport facts owned by this tier
/// without overwriting session-worker mechanism status.
fn restore_public_target(reply: &mut CuReply, public: &str) {
    reply.target = public.into();
    if reply.command != "capabilities" {
        return;
    }
    let Some(data) = reply.data.as_mut().and_then(|v| v.as_object_mut()) else {
        return;
    };
    if let Some(prev) = data.get("target").cloned()
        && prev.as_str() != Some(public)
    {
        data.entry("worker_target".to_owned()).or_insert(prev);
    }
    data.insert(
        "target".to_owned(),
        serde_json::Value::String(public.to_owned()),
    );
    // Public tier owns transport. Preserve the session worker's in-process
    // transport under worker_transport so mechanism facts stay inspectable.
    if let Some(prev_transport) = data.remove("transport") {
        data.entry("worker_transport".to_owned())
            .or_insert(prev_transport);
    }
    data.insert(
        "transport".to_owned(),
        serde_json::json!({
            "status": "available",
            "available": true,
            "kind": "rfb_session_worker",
        }),
    );
    // Do not invent live RDP or unproven Mac AX claims on the vnc tier.
    if let Some(gaps) = data.get_mut("gaps").and_then(|v| v.as_object_mut()) {
        gaps.entry("rdp_live".to_owned()).or_insert_with(|| {
            serde_json::Value::String(
                "rdp tier is placeholder; never declared available on vnc".into(),
            )
        });
        gaps.entry("macos_ax_live".to_owned()).or_insert_with(|| {
            serde_json::Value::String(
                "macOS AX live evidence is a separate cut; not claimed by vnc".into(),
            )
        });
    }
}

/// TCP connect + minimal RFB version/security handshake (None only).
pub fn rfb_handshake(endpoint: &VncEndpoint) -> Result<(), CuError> {
    let addr = endpoint.address();
    let timeout = Duration::from_secs(endpoint.connect_timeout_secs.max(1));
    let mut addrs = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|error| {
            CuError::new(
                "invalid_input",
                format!("vnc address {addr} could not be resolved: {error}"),
            )
        })?;
    let socket_addr = addrs.next().ok_or_else(|| {
        CuError::new(
            "invalid_input",
            format!("vnc address {addr} resolved to no socket addresses"),
        )
    })?;
    let mut stream = TcpStream::connect_timeout(&socket_addr, timeout).map_err(|error| {
        CuError::new(
            "vnc_unavailable",
            format!("could not connect RFB to {addr}: {error}"),
        )
    })?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let mut version = [0u8; 12];
    stream.read_exact(&mut version).map_err(|error| {
        CuError::new(
            "vnc_transport_failed",
            format!("RFB server at {addr} did not send ProtocolVersion: {error}"),
        )
    })?;
    if !version.starts_with(b"RFB ") || version[11] != b'\n' {
        return Err(CuError::new(
            "vnc_transport_failed",
            format!(
                "endpoint {addr} is not RFB (got {:?})",
                String::from_utf8_lossy(&version)
            ),
        ));
    }
    // Prefer 3.8; accept whatever major.minor the server offered when parseable.
    let client_version = if &version[4..11] >= b"003.008" {
        *b"RFB 003.008\n"
    } else {
        version
    };
    stream.write_all(&client_version).map_err(|error| {
        CuError::new(
            "vnc_transport_failed",
            format!("could not send RFB ProtocolVersion to {addr}: {error}"),
        )
    })?;

    // Protocol 3.7+ security-types list; 3.3 sends a single u32 type.
    let major_minor = &version[4..11];
    if major_minor < b"003.007" {
        let mut sec = [0u8; 4];
        stream.read_exact(&mut sec).map_err(|error| {
            CuError::new(
                "vnc_transport_failed",
                format!("RFB 3.3 security type read failed at {addr}: {error}"),
            )
        })?;
        let sec_type = u32::from_be_bytes(sec);
        match sec_type {
            1 => {
                // None — 3.3 does not send SecurityResult for type None.
                return Ok(());
            }
            0 => {
                return Err(read_rfb_failure_reason(
                    &mut stream,
                    &addr,
                    "RFB connection failed",
                ));
            }
            2 => {
                return Err(CuError::new(
                    "vnc_auth_failed",
                    format!(
                        "RFB at {addr} requires VNC authentication; first cut supports -nopw / security type None only"
                    ),
                ));
            }
            other => {
                return Err(CuError::new(
                    "vnc_auth_failed",
                    format!("RFB at {addr} offered unsupported security type {other}"),
                ));
            }
        }
    }

    let mut count_buf = [0u8; 1];
    stream.read_exact(&mut count_buf).map_err(|error| {
        CuError::new(
            "vnc_transport_failed",
            format!("RFB security-types count read failed at {addr}: {error}"),
        )
    })?;
    let count = count_buf[0] as usize;
    if count == 0 {
        return Err(read_rfb_failure_reason(
            &mut stream,
            &addr,
            "RFB server rejected connection (zero security types)",
        ));
    }
    let mut types = vec![0u8; count];
    stream.read_exact(&mut types).map_err(|error| {
        CuError::new(
            "vnc_transport_failed",
            format!("RFB security-types list read failed at {addr}: {error}"),
        )
    })?;
    if !types.contains(&1) {
        if types.contains(&2) {
            return Err(CuError::new(
                "vnc_auth_failed",
                format!(
                    "RFB at {addr} requires VNC authentication; first cut supports -nopw / security type None only"
                ),
            ));
        }
        return Err(CuError::new(
            "vnc_auth_failed",
            format!("RFB at {addr} offered no None security type (got {types:?})"),
        ));
    }
    stream.write_all(&[1u8]).map_err(|error| {
        CuError::new(
            "vnc_transport_failed",
            format!("could not select RFB security type None at {addr}: {error}"),
        )
    })?;
    let mut result = [0u8; 4];
    stream.read_exact(&mut result).map_err(|error| {
        CuError::new(
            "vnc_transport_failed",
            format!("RFB SecurityResult read failed at {addr}: {error}"),
        )
    })?;
    if result != [0, 0, 0, 0] {
        return Err(CuError::new(
            "vnc_auth_failed",
            format!(
                "RFB SecurityResult failed at {addr}: {}",
                u32::from_be_bytes(result)
            ),
        ));
    }
    // Drop the stream without ClientInit — reachability + auth proof is enough.
    Ok(())
}

fn read_rfb_failure_reason(stream: &mut TcpStream, addr: &str, prefix: &str) -> CuError {
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).is_err() {
        return CuError::new(
            "vnc_transport_failed",
            format!("{prefix} at {addr} (no reason string)"),
        );
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    let len = len.min(400);
    let mut reason = vec![0u8; len];
    let _ = stream.read_exact(&mut reason);
    CuError::new(
        "vnc_transport_failed",
        format!(
            "{prefix} at {addr}: {}",
            trim_for_error(&String::from_utf8_lossy(&reason))
        ),
    )
}

fn rewrite_command_target_current(command: &CuCommand) -> Result<CuCommand, CuError> {
    let mut value = serde_json::to_value(command).map_err(|error| {
        CuError::new(
            "serialize",
            format!("vnc transport could not re-encode command: {error}"),
        )
    })?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("target".into(), serde_json::Value::String("current".into()));
    }
    serde_json::from_value(value).map_err(|error| {
        CuError::new(
            "serialize",
            format!("vnc transport could not rebuild current command: {error}"),
        )
    })
}

fn default_session_env() -> Vec<(String, String)> {
    const KEYS: &[&str] = &[
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "AT_SPI_BUS",
        "AT_SPI_BUS_ADDRESS",
        "LD_LIBRARY_PATH",
        "AGENTERM_ABI_LIB",
        "AGENTERM_CU_AUDIT_PATH",
        "HOME",
        "LANG",
        "LC_ALL",
    ];
    let mut out = Vec::new();
    for key in KEYS {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
            && !value.contains(|c: char| c.is_whitespace())
        {
            out.push(((*key).to_owned(), value));
        }
    }
    out
}

fn parse_env_pair(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (key, value) = raw.split_once('=')?;
    if key.is_empty() {
        return None;
    }
    Some((key.to_owned(), value.to_owned()))
}

fn upsert_env(env: &mut Vec<(String, String)>, key: String, value: String) {
    if let Some(slot) = env.iter_mut().find(|(k, _)| k == &key) {
        slot.1 = value;
    } else {
        env.push((key, value));
    }
}

fn last_json_object_line(stdout: &str) -> Option<&str> {
    stdout
        .lines()
        .map(str::trim)
        .rfind(|line| line.starts_with('{') && line.ends_with('}'))
}

fn trim_for_error(raw: &str) -> String {
    const MAX: usize = 400;
    let flat: String = raw
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    if flat.len() <= MAX {
        flat
    } else {
        format!("{}…", &flat[..MAX])
    }
}

/// Split `host` or `host:port`. First cut: last `:` separates port when the
/// suffix is a u16; otherwise the whole string is the host.
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
                format!("vnc port in {raw:?} is not a valid TCP port"),
            )
        })?;
        return Ok((host.to_owned(), Some(port)));
    }
    Ok((raw.to_owned(), None))
}

/// Resolve a worker binary path for diagnostics.
#[allow(dead_code)]
pub fn worker_cu_exists(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{command::WaitCondition, target::TargetRef};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn pointer_move_survives_vnc_target_rewrite() {
        let command = CuCommand::PointerMove {
            target: TargetRef::Vnc,
            x: 4096,
            y: -64,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert!(matches!(
            remote,
            CuCommand::PointerMove {
                target: TargetRef::Current,
                x: 4096,
                y: -64
            }
        ));
    }

    #[test]
    fn split_host_port_parses_inline_port() {
        let (host, port) = split_host_port("127.0.0.1:5931").expect("parse");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, Some(5931));
        let (host, port) = split_host_port("127.0.0.1").expect("parse");
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, None);
    }

    #[test]
    fn capabilities_restore_public_target_does_not_leak_current() {
        // Worker capabilities answer with data.target=current; host must
        // restore public tier identity for both reply.target and data.target.
        let mut reply = CuReply {
            ok: true,
            target: "current".into(),
            command: "capabilities".into(),
            data: Some(serde_json::json!({
                "target": "current",
                "mechanism": "libagenterm",
                "capabilities": { "tree": "Available" },
                "gaps": {},
            })),
            error: None,
        };
        restore_public_target(&mut reply, "vnc");
        assert_eq!(reply.target, "vnc");
        let data = reply.data.as_ref().expect("data");
        assert_eq!(data["target"], "vnc");
        assert_eq!(data["worker_target"], "current");
        assert_eq!(data["transport"]["available"], true);
        assert_eq!(data["transport"]["kind"], "rfb_session_worker");
        assert_eq!(data["transport"]["status"], "available");
        assert_ne!(data["transport"]["status"], "in_process");
        assert_eq!(data["capabilities"]["tree"], "Available");
        assert!(data["gaps"]["rdp_live"].as_str().is_some());
        assert!(data["gaps"]["macos_ax_live"].as_str().is_some());
    }

    #[test]
    fn capabilities_overwrites_worker_in_process_transport() {
        let mut reply = CuReply {
            ok: true,
            target: "current".into(),
            command: "capabilities".into(),
            data: Some(serde_json::json!({
                "target": "current",
                "transport": { "status": "in_process", "available": true },
                "gaps": {},
            })),
            error: None,
        };
        restore_public_target(&mut reply, "vnc");
        let data = reply.data.as_ref().expect("data");
        assert_eq!(data["target"], "vnc");
        assert_eq!(data["transport"]["kind"], "rfb_session_worker");
        assert_eq!(data["worker_transport"]["status"], "in_process");
    }

    #[test]
    fn rewrites_target_to_current_for_session_worker() {
        let command = CuCommand::GetText {
            target: TargetRef::Vnc,
            window: Some(42),
            name: Some("Command".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::GetText {
                window, name, role, ..
            } => {
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn clipboard_read_observe_survives_target_rewrite() {
        let command = CuCommand::ClipboardRead {
            target: TargetRef::Vnc,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "clipboard-read");
        assert_eq!(remote.target(), TargetRef::Current);
        assert!(matches!(remote, CuCommand::ClipboardRead { .. }));
    }

    #[test]
    fn wait_equals_survives_target_rewrite() {
        let command = CuCommand::Wait {
            target: TargetRef::Vnc,
            timeout_ms: 3_000,
            condition: WaitCondition::NodeTextEquals {
                expected: "SEED".into(),
                name: "Command".into(),
                role: None,
                window: Some(7),
            },
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "wait");
        assert_eq!(remote.target(), TargetRef::Current);
    }

    #[test]
    fn windows_observe_survives_target_rewrite() {
        let command = CuCommand::Windows {
            target: TargetRef::Vnc,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "windows");
        assert_eq!(remote.target(), TargetRef::Current);
    }

    #[test]
    fn send_text_write_survives_target_rewrite() {
        // 3.32: first vnc WRITE path reuses the same RFB + session-worker
        // rewrite as observe; the worker still runs target=current send-text.
        let command = CuCommand::SendText {
            target: TargetRef::Vnc,
            text: "332VNCSEED".into(),
            window: Some(42),
            name: Some("Command".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "send-text");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::SendText {
                text,
                window,
                name,
                role,
                ..
            } => {
                assert_eq!(text, "332VNCSEED");
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn paste_write_survives_target_rewrite() {
        // 3.33: first vnc paste path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current paste with optional
        // --text seed. Seed travels in the session command JSON, not host
        // clipboard and not a local --target current write.
        let command = CuCommand::Paste {
            target: TargetRef::Vnc,
            text: Some("333VNCPASTE".into()),
            window: Some(42),
            name: Some("Command".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "paste");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::Paste {
                text,
                window,
                name,
                role,
                ..
            } => {
                assert_eq!(text.as_deref(), Some("333VNCPASTE"));
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn copy_publish_survives_target_rewrite() {
        // 3.34: first vnc copy path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current copy (GetText →
        // session CLIPBOARD). Circuit: seed on Command → vnc copy → clear →
        // vnc paste (no --text) → vnc get-text equals seed. Clipboard is the
        // session's, never the host's and never RFB framebuffer OCR.
        let command = CuCommand::Copy {
            target: TargetRef::Vnc,
            window: Some(42),
            name: Some("Command".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "copy");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::Copy {
                window, name, role, ..
            } => {
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn send_keys_write_survives_target_rewrite() {
        // 3.35: first vnc send-keys path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current send-keys. Circuit:
        // focus remote Command, host send-keys --window H -- KEYS (no
        // --name; plain typeable text uses focused EditableText fallback
        // when Device/key is absent on con Command), then host get-text
        // equals KEYS. Keys travel in the session command JSON (`--` ends
        // flags). No focused field typed-fails on the session worker the
        // same as local current.
        let command = CuCommand::SendKeys {
            target: TargetRef::Vnc,
            keys: "335VNCKEYS".into(),
            window: Some(42),
            name: None,
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "send-keys");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::SendKeys {
                keys,
                window,
                name,
                role,
                ..
            } => {
                assert_eq!(keys, "335VNCKEYS");
                assert_eq!(window, Some(42));
                assert!(name.is_none());
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn select_range_survives_target_rewrite() {
        // 3.36: first vnc select path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current select. Circuit:
        // host send-text plants SEED on session Command (`--` ends flags;
        // not --text), host select --window H --name Command --start 0
        // --end LEN runs session AT-SPI Text.SetSelection
        // (via=set-selection), then host independent get-selection returns
        // that range (via=get-selection; start/end equal the selected
        // slice). Never screenshot / --coords / mouse-drag / RFB OCR.
        // Missing Text typed-fails a11y_selection_unavailable on the
        // session worker the same as local current.
        let command = CuCommand::Select {
            target: TargetRef::Vnc,
            start: 0,
            end: 11,
            window: Some(42),
            name: Some("Command".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "select");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::Select {
                start,
                end,
                window,
                name,
                role,
                ..
            } => {
                assert_eq!(start, 0);
                assert_eq!(end, 11);
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn get_selection_observe_survives_target_rewrite() {
        // 3.44: first vnc get-selection as its own observe path reuses the
        // same RFB + session-worker rewrite; the worker still runs
        // target=current get-selection. Circuit: gate precondition places a
        // known ASCII SEED on session Command with a known non-empty
        // selection START..END (seed/range setup is not this cut's verb —
        // send-text / select remain prior paths), host independent
        // get-selection --window H --name Command returns that range
        // (via=get-selection; native AT-SPI GetNSelections +
        // GetSelection(0); n == 1 and integer start/end equal the
        // precondition range so seed[start:end] == expected). Never
        // screenshot / --coords / mouse-drag / RFB framebuffer OCR /
        // cached setter reply. Missing Text typed-fails
        // a11y_selection_unavailable on the session worker the same as
        // local current. No new verb; observe grant only. select (3.36)
        // remains a separate write path that may use get-selection as
        // readback. Window/name/role must survive the rewrite so the
        // session worker scopes GetSelection to the node.
        let command = CuCommand::GetSelection {
            target: TargetRef::Vnc,
            window: Some(42),
            name: Some("Command".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "get-selection");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::GetSelection {
                window, name, role, ..
            } => {
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn set_caret_offset_survives_target_rewrite() {
        // 3.37: first vnc set-caret path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current set-caret. Circuit:
        // host send-text plants SEED on session Command (`--` ends flags;
        // not --text), host set-caret --window H --name Command --offset 3
        // runs session AT-SPI Text.SetCaretOffset (via=set-caret-offset),
        // then host independent get-caret returns offset 3
        // (via=get-caret-offset) and get-text still equals the seed. Never
        // screenshot / --coords / mouse-drag / RFB OCR. Missing Text
        // typed-fails a11y_caret_unavailable on the session worker the same
        // as local current; SetCaretOffset false is a11y_caret_no_effect.
        let command = CuCommand::SetCaret {
            target: TargetRef::Vnc,
            offset: 3,
            window: Some(42),
            name: Some("Command".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "set-caret");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::SetCaret {
                offset,
                window,
                name,
                role,
                ..
            } => {
                assert_eq!(offset, 3);
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn get_caret_observe_survives_target_rewrite() {
        // 3.42: first vnc get-caret as its own observe path reuses the same
        // RFB + session-worker rewrite; the worker still runs
        // target=current get-caret. Circuit: gate precondition places a
        // known ASCII SEED on session Command with caret at seed end
        // (seed/caret state is not this cut's verb — send-text / set-caret
        // remain prior paths), host independent get-caret --window H
        // --name Command returns that offset as an int
        // (via=get-caret-offset; native AT-SPI CaretOffset /
        // GetCaretOffset; offset == seed_len). Never screenshot /
        // --coords / RFB framebuffer OCR / inferred string length.
        // Missing Text typed-fails a11y_caret_unavailable on the session
        // worker the same as local current. No new verb; observe grant
        // only. set-caret (3.37) remains a separate write path that may
        // use get-caret as readback. Window/name/role must survive the
        // rewrite so the session worker scopes CaretOffset to the node.
        let command = CuCommand::GetCaret {
            target: TargetRef::Vnc,
            window: Some(42),
            name: Some("Command".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "get-caret");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::GetCaret {
                window, name, role, ..
            } => {
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn click_name_survives_target_rewrite() {
        // 3.38: first vnc click path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current click. Circuit:
        // host send-text plants SEED on session Command (`--` ends flags;
        // not --text), host click --window H --name SEND runs session
        // AT-SPI Action DoAction (addressing=accessibility-tree; never
        // --coords / RFB pointer / screenshot), then host independent
        // get-text --name Command returns empty (composer cleared on SEND
        // submit). Missing / ambiguous name typed-fails a11y_node_not_found
        // / a11y_node_ambiguous on the session worker the same as local
        // current. coords:None and selector must survive the rewrite.
        let command = CuCommand::Click {
            target: TargetRef::Vnc,
            window: Some(42),
            node: None,
            name: Some("SEND".into()),
            role: Some("button".into()),
            coords: None,
            degraded: false,
            clicks: 1,
            button: crate::command::PointerButton::Left,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "click");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::Click {
                window,
                node,
                name,
                role,
                coords,
                degraded,
                clicks,
                button,
                ..
            } => {
                assert_eq!(window, Some(42));
                assert!(node.is_none());
                assert_eq!(name.as_deref(), Some("SEND"));
                assert_eq!(role.as_deref(), Some("button"));
                assert!(coords.is_none());
                assert!(!degraded);
                assert_eq!(clicks, 1);
                assert_eq!(button, crate::command::PointerButton::Left);
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn scroll_name_survives_target_rewrite() {
        // 3.39: first vnc scroll path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current scroll. Circuit:
        // host get-extents --window H --name OffscreenField records before
        // extents, host scroll --window H --name OffscreenField runs
        // session AT-SPI Component.ScrollTo(TopEdge) (via=scroll-to; never
        // --coords / RFB pointer/wheel / screenshot / Action scroll* /
        // XTest), then host independent get-extents after proves nonzero
        // |Δy| or |Δx| (snapshot node.bounds do not count). Missing /
        // false / UnknownMethod typed-fails a11y_scroll_unavailable on the
        // session worker the same as local current; ScrollTo true with no
        // later independent geometry change is a11y_scroll_no_effect (CEO
        // gate, not this rewrite test). Selector must survive the rewrite.
        let command = CuCommand::Scroll {
            target: TargetRef::Vnc,
            window: Some(42),
            name: Some("OffscreenField".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "scroll");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::Scroll {
                window, name, role, ..
            } => {
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("OffscreenField"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn get_extents_observe_survives_target_rewrite() {
        // 3.43: first vnc get-extents as its own observe path reuses the same
        // RFB + session-worker rewrite; the worker still runs
        // target=current get-extents. Circuit: host get-extents --window H
        // --name OffscreenField returns screen extents whose
        // x/y/width/height are ints (via=get-extents; native AT-SPI
        // Component.GetExtents(Screen); width/height >= 0). Snapshot
        // node.bounds / copied matched.bounds do not count. Never
        // screenshot / --coords / RFB framebuffer OCR. Missing / empty
        // extents typed-fails a11y_extents_unavailable on the session
        // worker the same as local current. No new verb; observe grant
        // only. scroll (3.39) remains a separate write path that may use
        // get-extents as independent before/after geometry proof.
        // Window/name/role must survive the rewrite so the session worker
        // scopes GetExtents to the node.
        let command = CuCommand::GetExtents {
            target: TargetRef::Vnc,
            window: Some(42),
            name: Some("OffscreenField".into()),
            role: None,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "get-extents");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::GetExtents {
                window, name, role, ..
            } => {
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("OffscreenField"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn focus_name_survives_target_rewrite() {
        // 3.40: first vnc focus path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current focus. Circuit:
        // host focus --window H --name Command (or SEND) runs session
        // AT-SPI Action focus / Component::grab_focus
        // (addressing=accessibility-tree; never --coords / RFB pointer /
        // screenshot / XTest), then host independent tree shows that node
        // focused and host independent get-text --window H (no --name)
        // equals that Command text (focused Text node). Missing /
        // ambiguous name typed-fails a11y_node_not_found /
        // a11y_node_ambiguous on the session worker the same as local
        // current. Selector (window/name/role/node) must survive the
        // rewrite.
        let command = CuCommand::Focus {
            target: TargetRef::Vnc,
            window: Some(42),
            node: None,
            name: Some("Command".into()),
            role: Some("text".into()),
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "focus");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::Focus {
                window,
                node,
                name,
                role,
                ..
            } => {
                assert_eq!(window, Some(42));
                assert!(node.is_none());
                assert_eq!(name.as_deref(), Some("Command"));
                assert_eq!(role.as_deref(), Some("text"));
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn tree_window_survives_target_rewrite() {
        // 3.41: first vnc tree path reuses the same RFB + session-worker
        // rewrite; the worker still runs target=current tree. Circuit:
        // host tree --window H on a second agenterm-con returns the
        // session AT-SPI flattened control tree
        // (addressing=accessibility-tree; never screenshot / --coords /
        // RFB framebuffer OCR). Independent proof is the returned nodes
        // list: unique named Session children Command, SEND, and
        // OffscreenField each appear once among showing nodes. No new
        // verb; observe grant only. Window must survive the rewrite so
        // the session worker scopes the tree to the intended con.
        let command = CuCommand::Tree {
            target: TargetRef::Vnc,
            window: Some(42),
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "tree");
        assert_eq!(remote.target(), TargetRef::Current);
        match remote {
            CuCommand::Tree { window, .. } => {
                assert_eq!(window, Some(42));
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn rfb_handshake_accepts_nopw_security_none() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream.write_all(b"RFB 003.008\n").expect("version");
            let mut client_ver = [0u8; 12];
            stream.read_exact(&mut client_ver).expect("client ver");
            assert_eq!(&client_ver, b"RFB 003.008\n");
            // one security type: None
            stream.write_all(&[1u8, 1u8]).expect("types");
            let mut selected = [0u8; 1];
            stream.read_exact(&mut selected).expect("select");
            assert_eq!(selected, [1]);
            stream.write_all(&[0, 0, 0, 0]).expect("result");
        });
        let endpoint = VncEndpoint {
            host: "127.0.0.1".into(),
            port,
            worker_cu: PathBuf::from("agenterm-cu"),
            session_env: vec![],
            connect_timeout_secs: 5,
        };
        rfb_handshake(&endpoint).expect("handshake");
        server.join().expect("server");
    }

    #[test]
    fn rfb_handshake_types_vnc_auth_as_auth_failed() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream.write_all(b"RFB 003.008\n").expect("version");
            let mut client_ver = [0u8; 12];
            stream.read_exact(&mut client_ver).expect("client ver");
            stream.write_all(&[1u8, 2u8]).expect("types"); // only VncAuth
        });
        let endpoint = VncEndpoint {
            host: "127.0.0.1".into(),
            port,
            worker_cu: PathBuf::from("agenterm-cu"),
            session_env: vec![],
            connect_timeout_secs: 5,
        };
        let err = rfb_handshake(&endpoint).expect_err("auth");
        assert_eq!(err.code, "vnc_auth_failed");
        server.join().expect("server");
    }

    #[test]
    fn rfb_handshake_types_closed_port_unavailable() {
        // Bind and drop so the port is closed.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let endpoint = VncEndpoint {
            host: "127.0.0.1".into(),
            port,
            worker_cu: PathBuf::from("agenterm-cu"),
            session_env: vec![],
            connect_timeout_secs: 1,
        };
        let err = rfb_handshake(&endpoint).expect_err("closed");
        assert_eq!(err.code, "vnc_unavailable");
    }

    #[test]
    fn last_json_line_skips_noise() {
        let stdout =
            "warn: something\n{\"ok\":true,\"target\":\"current\",\"command\":\"get-text\"}\n";
        assert_eq!(
            last_json_object_line(stdout),
            Some("{\"ok\":true,\"target\":\"current\",\"command\":\"get-text\"}")
        );
    }

    #[test]
    fn from_parts_reads_inline_port() {
        let endpoint = VncEndpoint::from_parts("127.0.0.1:5931".into(), None, None, vec![])
            .expect("from_parts");
        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 5931);
        assert_eq!(endpoint.address(), "127.0.0.1:5931");
    }
}
