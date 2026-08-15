//! OpenSSH transport for the `ssh` target tier (PRD_02_30).
//!
//! Host `agenterm-cu --ssh <dest>` rewrites the abstract command to
//! `target=current` and runs a remote `agenterm-cu exec --json -` worker over
//! `ssh` stdio. No new verbs. Observe and actuate grants both forward; the
//! remote worker runs the same AT-SPI / UIA / AX path via its libagenterm.
//! Loopback `sshd` against a second `agenterm-con` is the first evidence path
//! for both read (`tree` / `wait` / `get-text` / `get-selection` /
//! `get-caret` / `get-extents`) and write (`send-text` / `paste` / `copy` /
//! `send-keys` / `select` / `set-caret` / `click` / `scroll` / `focus`).
//! Cut 3.19 locks the clipboard write: host `paste --text` plants the seed
//! on the remote Command field; host `get-text` equals that seed. Cut 3.20
//! locks clipboard publish: seed already on Command (or planted over ssh
//! paste/send-text), host `copy` publishes remote GetText onto the remote
//! session CLIPBOARD, then host `paste` (no `--text`) + `get-text` equals
//! that seed. Cut 3.21 locks key delivery: host `send-keys` types plain keys
//! into the remote focused Command field, then host `wait` + `get-text`
//! equals those keys. Cut 3.22 locks text selection: host `send-text` plants
//! a seed on remote Command (`--` ends flags; not `--text`), host
//! `select --start N --end M` runs remote AT-SPI `Text.SetSelection`
//! (`via=set-selection`), then host independent `get-selection` returns that
//! range (`via=get-selection`; start/end equal the selected slice of the
//! seed). Cut 3.23 locks caret placement: host `send-text` plants a seed on
//! remote Command, host `set-caret --offset N` runs remote AT-SPI
//! `Text.SetCaretOffset` (`via=set-caret-offset`), then host independent
//! `get-caret` returns that offset (`via=get-caret-offset`) and host
//! `get-text` still equals the seed. Cut 3.24 locks named Action click: host
//! `send-text` plants a seed on remote Command, host `click --name SEND`
//! runs remote AT-SPI Action `DoAction` (`addressing=accessibility-tree`),
//! then host independent `get-text --name Command` returns empty (composer
//! cleared on submit). Cut 3.25 locks named scroll: host
//! `scroll --name OffscreenField` runs remote AT-SPI
//! `Component.ScrollTo(TopEdge)` (`via=scroll-to`), then host independent
//! `get-extents` before/after proves nonzero `|Δy|` or `|Δx|` (snapshot
//! `node.bounds` do not count). Cut 3.26 locks named focus: host
//! `focus --name Command` (or `SEND`) runs remote AT-SPI Action `focus` /
//! `Component::grab_focus` (`addressing=accessibility-tree`), then host
//! independent `tree` shows that node `focused` and/or host
//! `get-text --window H` (no `--name`) reads the focused Text node. Cut
//! 3.27 locks structured tree observe: host `tree --window H` returns the
//! remote AT-SPI flattened control tree (`addressing=accessibility-tree`)
//! and the unique named Session children `Command`, `SEND`, and
//! `OffscreenField` each appear once among showing nodes. Cut 3.28 locks
//! get-caret as its own observe path: host `send-text` plants a seed on
//! remote Command (caret ends at seed length), host independent
//! `get-caret --name Command` returns that offset as an int
//! (`via=get-caret-offset`; native AT-SPI `CaretOffset` / `GetCaretOffset`).
//! Cut 3.29 locks get-extents as its own observe path: host
//! `get-extents --name OffscreenField` returns screen extents whose
//! `x` / `y` / `width` / `height` are ints (`via=get-extents`; native
//! AT-SPI `Component.GetExtents(Screen)`). Snapshot `node.bounds` do not
//! count. Cut 3.30 locks get-selection as its own observe path: host
//! `send-text` plants a seed on remote Command (`--` ends flags; not
//! `--text`), host `select --start N --end M` runs remote AT-SPI
//! `Text.SetSelection`, then host independent `get-selection --name
//! Command` returns that range (`via=get-selection`; start/end equal the
//! selected slice of the seed, or the seed when the range is the whole
//! field). Native AT-SPI `GetNSelections` + `GetSelection`. Never
//! screenshot / `--coords` / mouse-drag / XTest.
//!
//! This is not D-Bus port-forwarding and not a second control protocol. Auth
//! failure, missing destination, and remote non-JSON failures are typed.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use crate::{
    auth::Authorization,
    command::Command as CuCommand,
    reply::{CuError, CuReply},
};

/// Remote OpenSSH endpoint for one `ssh` target session.
#[derive(Clone, Debug)]
pub struct SshEndpoint {
    /// `user@host` or bare host accepted by OpenSSH.
    pub destination: String,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    /// Absolute path of `agenterm-cu` on the remote side (loopback may reuse
    /// the host binary path).
    pub remote_cu: PathBuf,
    /// `KEY=VAL` pairs applied by remote `env` before the worker.
    pub remote_env: Vec<(String, String)>,
    pub connect_timeout_secs: u64,
    /// When true, skip host-key prompts (`StrictHostKeyChecking=no`).
    pub insecure_host_key: bool,
    pub known_hosts_file: Option<PathBuf>,
}

impl SshEndpoint {
    /// Build from CLI flags plus env defaults. `destination` is required.
    pub fn from_parts(
        destination: String,
        port: Option<u16>,
        identity_file: Option<PathBuf>,
        remote_cu: Option<PathBuf>,
        extra_env: Vec<(String, String)>,
    ) -> Result<Self, CuError> {
        if destination.trim().is_empty() {
            return Err(CuError::new(
                "invalid_input",
                "ssh target requires a non-empty --ssh <user@host> destination",
            ));
        }
        let port = port.or_else(|| {
            std::env::var("AGENTERM_CU_SSH_PORT")
                .ok()
                .and_then(|raw| raw.parse().ok())
        });
        let identity_file = identity_file
            .or_else(|| std::env::var_os("AGENTERM_CU_SSH_IDENTITY").map(PathBuf::from));
        let remote_cu = remote_cu
            .or_else(|| std::env::var_os("AGENTERM_CU_SSH_CU").map(PathBuf::from))
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_else(|| PathBuf::from("agenterm-cu"));
        let mut remote_env = default_remote_env();
        if let Ok(raw) = std::env::var("AGENTERM_CU_SSH_ENV") {
            for part in raw.split(',') {
                if let Some(pair) = parse_env_pair(part) {
                    upsert_env(&mut remote_env, pair.0, pair.1);
                }
            }
        }
        for (key, value) in extra_env {
            upsert_env(&mut remote_env, key, value);
        }
        let connect_timeout_secs = std::env::var("AGENTERM_CU_SSH_CONNECT_TIMEOUT")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(15);
        let insecure_host_key = matches!(
            std::env::var("AGENTERM_CU_SSH_STRICT_HOSTKEY")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "0" | "false" | "no" | "off"
        );
        let known_hosts_file = std::env::var_os("AGENTERM_CU_SSH_KNOWN_HOSTS").map(PathBuf::from);
        Ok(Self {
            destination,
            port,
            identity_file,
            remote_cu,
            remote_env,
            connect_timeout_secs,
            insecure_host_key,
            known_hosts_file,
        })
    }

    /// argv[1..] after `ssh` for unit tests and diagnostics (no secrets).
    pub fn ssh_prefix_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            format!("ConnectTimeout={}", self.connect_timeout_secs),
        ];
        if self.insecure_host_key {
            args.push("-o".into());
            args.push("StrictHostKeyChecking=no".into());
            args.push("-o".into());
            args.push("UserKnownHostsFile=/dev/null".into());
        } else if let Some(path) = &self.known_hosts_file {
            args.push("-o".into());
            args.push("StrictHostKeyChecking=accept-new".into());
            args.push("-o".into());
            args.push(format!("UserKnownHostsFile={}", path.display()));
        }
        if let Some(port) = self.port {
            args.push("-p".into());
            args.push(port.to_string());
        }
        if let Some(identity) = &self.identity_file {
            args.push("-i".into());
            args.push(identity.display().to_string());
            args.push("-o".into());
            args.push("IdentitiesOnly=yes".into());
        }
        args.push(self.destination.clone());
        args
    }
}

/// Run `command` on the remote `agenterm-cu --target current` worker.
pub fn run_remote(
    endpoint: &SshEndpoint,
    command: &CuCommand,
    auth: &Authorization,
) -> Result<CuReply, CuError> {
    let remote_command = rewrite_command_target_current(command)?;
    let payload = serde_json::to_string(&remote_command).map_err(|error| {
        CuError::new(
            "serialize",
            format!("ssh transport could not serialize command: {error}"),
        )
    })?;
    let grant = auth.grant_cli_arg();
    if grant.is_empty() {
        return Err(CuError::new(
            "refused",
            "ssh transport requires at least one grant on the host command",
        ));
    }

    let mut remote_argv: Vec<String> = Vec::new();
    remote_argv.push("env".into());
    for (key, value) in &endpoint.remote_env {
        // OpenSSH joins the remote argv with spaces and runs it through the
        // remote shell; keep values free of whitespace so shell splitting is
        // stable. Callers that need spaces should export them on the remote.
        if key.is_empty() || key.contains('=') || key.contains(|c: char| c.is_whitespace()) {
            return Err(CuError::new(
                "invalid_input",
                format!("ssh remote env key is invalid: {key:?}"),
            ));
        }
        if value.contains(|c: char| c.is_whitespace()) {
            return Err(CuError::new(
                "invalid_input",
                format!(
                    "ssh remote env value for {key} must not contain whitespace (got {value:?})"
                ),
            ));
        }
        remote_argv.push(format!("{key}={value}"));
    }
    // `exec` must lead the remote argv: the shell parser only special-cases it
    // as the first token (global flags after `exec` are handled by dispatch_json).
    remote_argv.push(endpoint.remote_cu.display().to_string());
    remote_argv.push("exec".into());
    remote_argv.push("--grant".into());
    remote_argv.push(grant);
    remote_argv.push("--json".into());
    remote_argv.push("-".into());

    let mut ssh = Command::new("ssh");
    for arg in endpoint.ssh_prefix_args() {
        ssh.arg(arg);
    }
    for arg in remote_argv {
        ssh.arg(arg);
    }
    ssh.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = ssh.spawn().map_err(|error| {
        CuError::new(
            "ssh_unavailable",
            format!("could not spawn ssh for {}: {error}", endpoint.destination),
        )
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).map_err(|error| {
            CuError::new(
                "ssh_transport_failed",
                format!("could not write command JSON to ssh stdin: {error}"),
            )
        })?;
        // Drop stdin so the remote sees EOF after the JSON payload.
        drop(stdin);
    }

    let output = child.wait_with_output().map_err(|error| {
        CuError::new(
            "ssh_transport_failed",
            format!("ssh to {} failed: {error}", endpoint.destination),
        )
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let json_line = last_json_object_line(stdout.as_ref()).ok_or_else(|| {
        CuError::new(
            "ssh_transport_failed",
            format!(
                "remote agenterm-cu produced no JSON reply (exit={}): stderr={}",
                output.status.code().unwrap_or(-1),
                trim_for_error(&stderr)
            ),
        )
    })?;

    let mut reply: CuReply = serde_json::from_str(json_line).map_err(|error| {
        CuError::new(
            "ssh_transport_failed",
            format!(
                "remote agenterm-cu reply is not valid CuReply JSON: {error}; line={}",
                trim_for_error(json_line)
            ),
        )
    })?;
    // Host identity of this command is the ssh tier even when the remote
    // worker answered as target=current. Capabilities also restore
    // data.target so callers do not see the worker's "current" leak.
    restore_public_target(&mut reply, "ssh");
    if !output.status.success() && reply.ok {
        // Worker printed ok:true but process exit was non-zero — surface as
        // transport failure so callers do not treat it as success.
        return Err(CuError::new(
            "ssh_transport_failed",
            format!(
                "remote agenterm-cu exit {} with ok:true; stderr={}",
                output.status.code().unwrap_or(-1),
                trim_for_error(&stderr)
            ),
        ));
    }
    Ok(reply)
}

/// Public reply target is always the ssh tier. For `capabilities`, also
/// restore `data.target` and attach transport facts owned by this tier
/// without overwriting remote mechanism status from the worker.
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
    // Public tier owns transport. Preserve the worker's in-process transport
    // under worker_transport so mechanism facts stay inspectable.
    if let Some(prev_transport) = data.remove("transport") {
        data.entry("worker_transport".to_owned())
            .or_insert(prev_transport);
    }
    data.insert(
        "transport".to_owned(),
        serde_json::json!({
            "status": "available",
            "available": true,
            "kind": "openssh_exec",
        }),
    );
    // Do not invent live RDP or unproven Mac AX claims on the ssh tier.
    if let Some(gaps) = data.get_mut("gaps").and_then(|v| v.as_object_mut()) {
        gaps.entry("rdp_live".to_owned()).or_insert_with(|| {
            serde_json::Value::String(
                "rdp tier is placeholder; never declared available on ssh".into(),
            )
        });
        gaps.entry("macos_ax_live".to_owned()).or_insert_with(|| {
            serde_json::Value::String(
                "macOS AX live evidence is a separate cut; not claimed by ssh".into(),
            )
        });
    }
}

fn rewrite_command_target_current(command: &CuCommand) -> Result<CuCommand, CuError> {
    let mut value = serde_json::to_value(command).map_err(|error| {
        CuError::new(
            "serialize",
            format!("ssh transport could not re-encode command: {error}"),
        )
    })?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("target".into(), serde_json::Value::String("current".into()));
    }
    serde_json::from_value(value).map_err(|error| {
        CuError::new(
            "serialize",
            format!("ssh transport could not rebuild current command: {error}"),
        )
    })
}

fn default_remote_env() -> Vec<(String, String)> {
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

/// Deadline helper kept for callers that want a wall-clock bound around a
/// remote wait; OpenSSH itself has no per-command deadline beyond connect.
#[allow(dead_code)]
pub fn connect_deadline(endpoint: &SshEndpoint) -> Duration {
    Duration::from_secs(endpoint.connect_timeout_secs.saturating_add(5))
}

/// Resolve a remote binary path for diagnostics.
#[allow(dead_code)]
pub fn remote_cu_exists(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{command::WaitCondition, target::TargetRef};

    #[test]
    fn pointer_move_survives_ssh_target_rewrite() {
        let command = CuCommand::PointerMove {
            target: TargetRef::Ssh,
            x: -17,
            y: 2048,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert!(matches!(
            remote,
            CuCommand::PointerMove {
                target: TargetRef::Current,
                x: -17,
                y: 2048
            }
        ));
    }

    #[test]
    fn pointer_position_survives_ssh_target_rewrite() {
        let command = CuCommand::PointerPosition {
            target: TargetRef::Ssh,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert!(matches!(
            remote,
            CuCommand::PointerPosition {
                target: TargetRef::Current
            }
        ));
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
        restore_public_target(&mut reply, "ssh");
        assert_eq!(reply.target, "ssh");
        let data = reply.data.as_ref().expect("data");
        assert_eq!(data["target"], "ssh");
        assert_eq!(data["worker_target"], "current");
        assert_eq!(data["transport"]["available"], true);
        assert_eq!(data["transport"]["kind"], "openssh_exec");
        assert_eq!(data["transport"]["status"], "available");
        // Worker in_process transport must not leak as the public tier status.
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
        restore_public_target(&mut reply, "ssh");
        let data = reply.data.as_ref().expect("data");
        assert_eq!(data["target"], "ssh");
        assert_eq!(data["transport"]["kind"], "openssh_exec");
        assert_eq!(data["worker_transport"]["status"], "in_process");
    }

    #[test]
    fn rewrites_target_to_current_for_remote_worker() {
        let command = CuCommand::GetText {
            target: TargetRef::Ssh,
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
            target: TargetRef::Ssh,
        };
        let remote = rewrite_command_target_current(&command).expect("rewrite");
        assert_eq!(remote.verb(), "clipboard-read");
        assert_eq!(remote.target(), TargetRef::Current);
        assert!(matches!(remote, CuCommand::ClipboardRead { .. }));
    }

    #[test]
    fn wait_contains_survives_target_rewrite() {
        let command = CuCommand::Wait {
            target: TargetRef::Ssh,
            timeout_ms: 3_000,
            condition: WaitCondition::NodeTextContains {
                substring: "SEED".into(),
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
    fn send_text_write_survives_target_rewrite() {
        // 3.18: first ssh WRITE path reuses the same OpenSSH exec rewrite as
        // observe; the remote worker still runs target=current send-text.
        let command = CuCommand::SendText {
            target: TargetRef::Ssh,
            text: "318SSHSEED".into(),
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
                assert_eq!(text, "318SSHSEED");
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn paste_write_survives_target_rewrite() {
        // 3.19: first ssh paste path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current paste with optional --text seed.
        // Seed travels in the JSON command over ssh stdin, not local clipboard.
        let command = CuCommand::Paste {
            target: TargetRef::Ssh,
            text: Some("319SSHPASTE".into()),
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
                assert_eq!(text.as_deref(), Some("319SSHPASTE"));
                assert_eq!(window, Some(42));
                assert_eq!(name.as_deref(), Some("Command"));
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn copy_publish_survives_target_rewrite() {
        // 3.20: first ssh copy path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current copy (GetText → remote CLIPBOARD).
        // Circuit: seed on Command → ssh copy → ssh paste (no --text) →
        // ssh get-text equals seed. Clipboard is the remote session's.
        let command = CuCommand::Copy {
            target: TargetRef::Ssh,
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
        // 3.21: first ssh send-keys path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current send-keys. Circuit: focus remote
        // Command, host send-keys --window H -- KEYS (no --name; plain
        // typeable text uses focused EditableText fallback when Device/key
        // is absent on con Command), then host wait + get-text equals KEYS.
        // Keys travel in the JSON command over ssh stdin (`--` ends flags;
        // leftover argv joined with `+`). No focused field typed-fails on
        // the remote worker the same as local current.
        let command = CuCommand::SendKeys {
            target: TargetRef::Ssh,
            keys: "321SSHKEYS".into(),
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
                assert_eq!(keys, "321SSHKEYS");
                assert_eq!(window, Some(42));
                assert!(name.is_none());
                assert!(role.is_none());
            }
            other => panic!("unexpected command {other:?}"),
        }
    }

    #[test]
    fn select_range_survives_target_rewrite() {
        // 3.22: first ssh select path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current select. Circuit: host send-text
        // plants SEED on remote Command (`--` ends flags; not --text), host
        // select --window H --name Command --start 0 --end LEN runs remote
        // AT-SPI Text.SetSelection (via=set-selection), then host independent
        // get-selection returns that range (via=get-selection; start/end
        // equal the selected slice). Never screenshot / --coords / mouse-drag.
        // Missing Text typed-fails a11y_selection_unavailable on the remote
        // worker the same as local current.
        let command = CuCommand::Select {
            target: TargetRef::Ssh,
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
        // 3.30: first ssh get-selection as its own observe path reuses the
        // same OpenSSH exec rewrite; remote worker runs target=current
        // get-selection. Circuit: host send-text plants SEED on remote
        // Command (`--` ends flags; not --text), host select --window H
        // --name Command --start N --end M runs remote AT-SPI
        // Text.SetSelection, then host independent get-selection --window H
        // --name Command returns that range (via=get-selection; start/end
        // equal the selected slice of the seed, or the seed when the range
        // is the whole field). Native AT-SPI GetNSelections + GetSelection.
        // Never screenshot / --coords / mouse-drag / XTest. Missing Text
        // typed-fails a11y_selection_unavailable on the remote worker the
        // same as local current. No new verb; observe grant only. select
        // (3.22) remains a separate write path that may use get-selection
        // as readback.
        let command = CuCommand::GetSelection {
            target: TargetRef::Ssh,
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
        // 3.23: first ssh set-caret path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current set-caret. Circuit: host send-text
        // plants SEED on remote Command (`--` ends flags; not --text), host
        // set-caret --window H --name Command --offset 3 runs remote AT-SPI
        // Text.SetCaretOffset (via=set-caret-offset), then host independent
        // get-caret returns offset 3 (via=get-caret-offset) and get-text still
        // equals the seed. Never screenshot / --coords / mouse-drag. Missing
        // Text typed-fails a11y_caret_unavailable on the remote worker the
        // same as local current; SetCaretOffset false is a11y_caret_no_effect.
        let command = CuCommand::SetCaret {
            target: TargetRef::Ssh,
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
        // 3.28: first ssh get-caret as its own observe path reuses the same
        // OpenSSH exec rewrite; remote worker runs target=current get-caret.
        // Circuit: host send-text plants SEED on remote Command (`--` ends
        // flags; not --text; caret ends at seed length), host independent
        // get-caret --window H --name Command returns that offset as an int
        // (via=get-caret-offset; native AT-SPI CaretOffset / GetCaretOffset).
        // Never screenshot / --coords / mouse-drag. Missing Text typed-fails
        // a11y_caret_unavailable on the remote worker the same as local
        // current. No new verb; observe grant only. set-caret (3.23) remains
        // a separate write path that may use get-caret as readback.
        let command = CuCommand::GetCaret {
            target: TargetRef::Ssh,
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
        // 3.24: first ssh click path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current click. Circuit: host send-text
        // plants SEED on remote Command (`--` ends flags; not --text), host
        // click --window H --name SEND runs remote AT-SPI Action DoAction
        // (addressing=accessibility-tree; never --coords / XTest /
        // screenshot), then host independent get-text --name Command returns
        // empty (composer cleared on SEND submit). Missing / ambiguous name
        // typed-fails a11y_node_not_found / a11y_node_ambiguous on the remote
        // worker the same as local current.
        let command = CuCommand::Click {
            target: TargetRef::Ssh,
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
        // 3.25: first ssh scroll path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current scroll. Circuit: host
        // get-extents --window H --name OffscreenField records before,
        // host scroll --window H --name OffscreenField runs remote AT-SPI
        // Component.ScrollTo(TopEdge) (via=scroll-to; never --coords /
        // XTest / screenshot / Action scroll*), then host independent
        // get-extents after proves nonzero |Δy| or |Δx| (snapshot
        // node.bounds do not count). Missing / false / UnknownMethod
        // typed-fails a11y_scroll_unavailable on the remote worker the same
        // as local current; ScrollTo true with no later independent geometry
        // change is a11y_scroll_no_effect (CEO gate, not this rewrite test).
        let command = CuCommand::Scroll {
            target: TargetRef::Ssh,
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
        // 3.29: first ssh get-extents as its own observe path reuses the same
        // OpenSSH exec rewrite; remote worker runs target=current get-extents.
        // Circuit: host get-extents --window H --name OffscreenField returns
        // screen extents whose x/y/width/height are ints (via=get-extents;
        // native AT-SPI Component.GetExtents(Screen)). Snapshot node.bounds
        // do not count. Never screenshot / --coords / mouse-drag / XTest.
        // Missing / empty extents typed-fails a11y_extents_unavailable on the
        // remote worker the same as local current. No new verb; observe grant
        // only. scroll (3.25) remains a separate write path that may use
        // get-extents as independent before/after geometry proof.
        let command = CuCommand::GetExtents {
            target: TargetRef::Ssh,
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
        // 3.26: first ssh focus path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current focus. Circuit: host
        // focus --window H --name Command (or SEND) runs remote AT-SPI
        // Action focus / Component::grab_focus (addressing=accessibility-tree;
        // never --coords / XTest / screenshot), then host independent tree
        // shows that node focused and/or host get-text --window H (no
        // --name) reads the focused Text node. Missing / ambiguous name
        // typed-fails a11y_node_not_found / a11y_node_ambiguous on the remote
        // worker the same as local current.
        let command = CuCommand::Focus {
            target: TargetRef::Ssh,
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
        // 3.27: first ssh tree path reuses the same OpenSSH exec rewrite;
        // remote worker runs target=current tree. Circuit: host
        // tree --window H on a second agenterm-con returns the remote AT-SPI
        // flattened control tree (addressing=accessibility-tree; never
        // screenshot / --coords / XTest). Independent proof is the returned
        // nodes list: unique named Session children Command, SEND, and
        // OffscreenField each appear once among showing nodes. No new verb;
        // observe grant only.
        let command = CuCommand::Tree {
            target: TargetRef::Ssh,
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
    fn ssh_prefix_includes_port_and_identity() {
        let endpoint = SshEndpoint {
            destination: "user@127.0.0.1".into(),
            port: Some(2222),
            identity_file: Some(PathBuf::from("/tmp/id_ed25519")),
            remote_cu: PathBuf::from("/tmp/agenterm-cu"),
            remote_env: vec![],
            connect_timeout_secs: 10,
            insecure_host_key: true,
            known_hosts_file: None,
        };
        let args = endpoint.ssh_prefix_args();
        assert!(args.iter().any(|a| a == "BatchMode=yes"));
        assert!(args.windows(2).any(|w| w[0] == "-p" && w[1] == "2222"));
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-i" && w[1] == "/tmp/id_ed25519")
        );
        assert_eq!(args.last().map(String::as_str), Some("user@127.0.0.1"));
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
}
