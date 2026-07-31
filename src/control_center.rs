//! Isolated Control Center process shell.
//!
//! This module intentionally owns no terminal, PTY, workspace, server, or
//! workflow authority.  It is a replaceable native projection host.

use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u32 = 1;
const REGISTRY_SCHEMA_VERSION: u32 = 2;
const PUBLIC_UI_ACTION: &str = "open-control-center";
const TYPED_OPERATION: &str = "control-center.open";
const HELP: &str = "\
AgenTerm Control Center

Usage:
  agenterm-cc [open] [--no-activate] [--server-endpoint ENDPOINT]
  agenterm-cc status [--json]
  agenterm-cc close [--json]
  agenterm-cc snapshot [--json] [--server-endpoint ENDPOINT]
  agenterm-cc capabilities [--json]
  agenterm-cc --help
  agenterm-cc --version

The Control Center is an isolated projection process. It never owns terminal,
PTY, workspace, server, or workflow state.
";

#[derive(Clone, Debug, PartialEq, Eq)]
enum EntryCommand {
    Open {
        no_activate: bool,
        context: Option<ServerContext>,
    },
    Help,
    Version,
    Capabilities {
        json: bool,
    },
    Status {
        json: bool,
    },
    Close {
        json: bool,
    },
    Snapshot {
        json: bool,
        context: Option<ServerContext>,
    },
}

#[derive(Debug, Serialize)]
struct CapabilityDocument {
    schema_version: u32,
    executable: &'static str,
    role: &'static str,
    public_ui_action: &'static str,
    typed_operation: &'static str,
    renderer: &'static str,
    webview_host: crate::webview_host::WebViewHostFacts,
    owns_terminal_authority: bool,
    process_reuse: bool,
    no_activate: bool,
    views: [&'static str; 4],
}

#[derive(Debug, Serialize)]
pub(crate) struct SnapshotDocument {
    schema_version: u32,
    executable: &'static str,
    process_role: &'static str,
    renderer: &'static str,
    webview_host: crate::webview_host::WebViewHostFacts,
    connected_server: Option<ConnectedServer>,
    server_state: &'static str,
    server_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_detail: Option<String>,
    views: [ViewSnapshot; 4],
}

#[derive(Debug, Serialize)]
struct ViewSnapshot {
    id: &'static str,
    label: &'static str,
    state: &'static str,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ServerContext {
    endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    logical_instance: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConnectedServer {
    endpoint: String,
    logical_instance: Option<String>,
    pid: u64,
    epoch: String,
    sequence: u64,
    version: Option<String>,
    build: Value,
    active_tab_id: Option<String>,
    tabs: Vec<TabSummary>,
    components: ComponentAvailability,
}

#[derive(Debug, Serialize)]
struct TabSummary {
    id: String,
    index: u64,
    title: String,
    note: String,
    process_id: Option<u64>,
    dead: bool,
}

#[derive(Debug, Serialize)]
struct ComponentAvailability {
    server: &'static str,
    workflows: &'static str,
    extensions: &'static str,
    info_hub: &'static str,
}

#[derive(Debug, Serialize)]
struct StatusDocument {
    schema_version: u32,
    executable: &'static str,
    state: &'static str,
    pid: Option<u32>,
    context: Option<ServerContext>,
}

#[derive(Debug, Serialize)]
struct CloseDocument {
    schema_version: u32,
    executable: &'static str,
    state: &'static str,
    pid: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RegistryRecord {
    schema_version: u32,
    pid: u32,
    process_start_identity: String,
}

struct RegistryOwner {
    path: PathBuf,
    pid: u32,
    process_start_identity: String,
}

impl RegistryOwner {
    fn publish_native_window(&self, native_window: i64) -> Result<()> {
        write_private_atomic(
            &native_window_path(&self.path),
            native_window.to_string().as_bytes(),
        )
    }
}

impl Drop for RegistryOwner {
    fn drop(&mut self) {
        let belongs_to_us = read_registry(&self.path).is_some_and(|record| {
            record.pid == self.pid
                && record.process_start_identity == self.process_start_identity
                && registry_process_matches(&record)
        });
        if belongs_to_us {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(native_window_path(&self.path));
            let _ = fs::remove_file(focus_request_path(&self.path));
            let _ = fs::remove_file(context_path(&self.path));
            let _ = fs::remove_file(close_request_path(&self.path));
        }
    }
}

enum RegistryClaim {
    Owner(RegistryOwner),
    Existing(RegistryRecord),
}

struct ShellProjection {
    registry_file: PathBuf,
    context_file: PathBuf,
    context_bytes: Option<Vec<u8>>,
    refresh_file: PathBuf,
    refresh_bytes: Option<Vec<u8>>,
    snapshot: SnapshotDocument,
}

impl ShellProjection {
    fn new(registry: &Path) -> Self {
        let mut projection = Self {
            registry_file: registry.to_owned(),
            context_file: context_path(registry),
            context_bytes: None,
            refresh_file: focus_request_path(registry),
            refresh_bytes: None,
            snapshot: disconnected_snapshot(),
        };
        projection.refresh(true);
        projection
    }

    fn refresh(&mut self, force: bool) -> bool {
        let Ok(bytes) = read_regular_file(&self.context_file) else {
            // A context update uses replace-by-rename. Preserve the last known
            // projection if polling observes the tiny replacement gap.
            return false;
        };
        if !force && self.context_bytes.as_deref() == Some(bytes.as_slice()) {
            return false;
        }
        let context = serde_json::from_slice::<ServerContext>(&bytes)
            .ok()
            .filter(|value| validate_context_value("server endpoint", &value.endpoint).is_ok());
        self.snapshot = snapshot_for_context(context);
        self.context_bytes = Some(bytes);
        true
    }

    fn poll(&mut self) -> bool {
        let refresh = read_regular_file(&self.refresh_file).ok();
        let forced = refresh.is_some() && refresh != self.refresh_bytes;
        if forced {
            self.refresh_bytes = refresh;
        }
        self.refresh(forced)
    }

    fn close_requested(&self) -> bool {
        let Some(owner) = read_registry(&self.registry_file) else {
            return true;
        };
        read_registry(&close_request_path(&self.registry_file)).is_some_and(|request| {
            request.pid == owner.pid
                && request.process_start_identity == owner.process_start_identity
                && registry_process_matches(&owner)
        })
    }

    fn title(&self) -> String {
        let suffix = match (
            self.snapshot.server_state,
            self.snapshot.connected_server.as_ref(),
        ) {
            ("connected", Some(server)) => {
                format!("{} · {} tabs", server.endpoint, server.tabs.len())
            }
            _ => self
                .snapshot
                .server_reason
                .as_deref()
                .unwrap_or("no_server_context")
                .to_owned(),
        };
        format!("AgenTerm Control Center — Cockpit · {suffix}")
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            "AgenTerm Control Center".to_owned(),
            format!(
                "Cockpit     {} ({})",
                self.snapshot.views[0].state, self.snapshot.views[0].reason
            ),
        ];
        if let Some(server) = self.snapshot.connected_server.as_ref() {
            lines.push(format!(
                "Server      {} · PID {} · sequence {}",
                server.endpoint, server.pid, server.sequence
            ));
            lines.push(format!(
                "Fleet       {} tabs · active {}",
                server.tabs.len(),
                server.active_tab_id.as_deref().unwrap_or("none")
            ));
        }
        lines.extend(
            self.snapshot.views[1..]
                .iter()
                .map(|view| format!("{:<12}{} ({})", view.label, view.state, view.reason)),
        );
        lines
    }
}

/// Start or reuse the isolated Control Center without blocking the GUI thread.
pub(crate) fn open_control_center(no_activate: bool, server_endpoint: &str) -> Result<()> {
    validate_context_value("server endpoint", server_endpoint)?;
    let executable = control_center_executable()?;
    let mut command = Command::new(&executable);
    command
        .arg("open")
        .arg("--server-endpoint")
        .arg(server_endpoint);
    if let Ok(instance) = env::var("AGENTERM_INSTANCE")
        && !instance.trim().is_empty()
    {
        validate_context_value("logical instance", &instance)?;
        command.arg("--logical-instance").arg(instance);
    }
    if no_activate {
        command.arg("--no-activate");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .with_context(|| format!("failed to launch {}", executable.display()))?;
    Ok(())
}

/// Local CLI surface. It never starts a server and always emits one JSON document.
pub(crate) fn run_control_center_cli(args: &[String], endpoint: &str) -> i32 {
    let result = match parse_control_center_cli(args, endpoint) {
        Ok(ControlCenterCli::Open { no_activate }) => open_control_center(no_activate, endpoint)
            .map(|()| {
                serde_json::json!({
                    "schema_version": SCHEMA_VERSION,
                    "operation": TYPED_OPERATION,
                    "state": "launch_requested",
                    "server_endpoint": endpoint,
                    "no_activate": no_activate,
                })
            }),
        Ok(ControlCenterCli::Status) => {
            Ok(serde_json::to_value(status_document()).unwrap_or_default())
        }
        Ok(ControlCenterCli::Close) => {
            Ok(serde_json::to_value(close_control_center()).unwrap_or_default())
        }
        Ok(ControlCenterCli::Snapshot) => {
            let context = ServerContext {
                endpoint: endpoint.to_owned(),
                logical_instance: env::var("AGENTERM_INSTANCE")
                    .ok()
                    .filter(|value| !value.trim().is_empty()),
            };
            Ok(serde_json::to_value(snapshot_for_context(Some(context))).unwrap_or_default())
        }
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    match result {
        Ok(document) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&document).unwrap_or_default()
            );
            0
        }
        Err(error) => {
            eprintln!("control_center_unavailable: {error:#}");
            1
        }
    }
}

enum ControlCenterCli {
    Open { no_activate: bool },
    Status,
    Close,
    Snapshot,
}

fn parse_control_center_cli(args: &[String], endpoint: &str) -> Result<ControlCenterCli, String> {
    validate_context_value("server endpoint", endpoint).map_err(|error| error.to_string())?;
    let Some(subcommand) = args.get(1).map(String::as_str) else {
        return Err(
            "control-center requires open, status, snapshot, or close\nUsage: agenterm-cli control-center open|status|snapshot|close [--no-activate]".to_owned(),
        );
    };
    match (subcommand, &args[2..]) {
        ("open", []) => Ok(ControlCenterCli::Open {
            no_activate: crate::client::no_activate_from_environment(),
        }),
        ("open", [flag]) if flag == "--no-activate" => {
            Ok(ControlCenterCli::Open { no_activate: true })
        }
        ("status", []) => Ok(ControlCenterCli::Status),
        ("close", []) => Ok(ControlCenterCli::Close),
        ("snapshot", []) => Ok(ControlCenterCli::Snapshot),
        _ => Err(
            "invalid control-center arguments\nUsage: agenterm-cli control-center open|status|snapshot|close [--no-activate]"
                .to_owned(),
        ),
    }
}

/// Public binary boundary; informational commands have no registry or GUI side effects.
pub fn run_control_center_entry_with_args(args: impl IntoIterator<Item = OsString>) -> i32 {
    let arguments = args.into_iter().collect::<Vec<_>>();
    let command = match parse_entry(&arguments) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("agenterm-cc: {error}\n\n{HELP}");
            return 2;
        }
    };

    match run_entry(command) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("agenterm-cc: {error:#}");
            1
        }
    }
}

fn parse_entry(args: &[OsString]) -> std::result::Result<EntryCommand, String> {
    let values = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>();
    if let Some(option) = values.iter().find(|value| {
        value.starts_with('-')
            && !matches!(
                value.as_ref(),
                "--no-activate"
                    | "--json"
                    | "--server-endpoint"
                    | "--logical-instance"
                    | "--help"
                    | "-h"
                    | "--version"
                    | "-V"
            )
    }) {
        return Err(format!("unknown option: {option}"));
    }
    let explicit_no_activate = values.iter().any(|value| value == "--no-activate");
    let no_activate = explicit_no_activate || crate::client::no_activate_from_environment();
    let json = values.iter().any(|value| value == "--json");
    let mut positional = Vec::new();
    let mut endpoint = None;
    let mut logical_instance = None;
    let mut position = 0;
    while position < values.len() {
        match values[position].as_ref() {
            "--server-endpoint" | "--logical-instance" => {
                let option = values[position].as_ref();
                let Some(value) = values.get(position + 1) else {
                    return Err(format!("{option} requires a value"));
                };
                validate_context_value(option.trim_start_matches('-'), value)
                    .map_err(|error| error.to_string())?;
                if option == "--server-endpoint" {
                    endpoint = Some(value.to_string());
                } else {
                    logical_instance = Some(value.to_string());
                }
                position += 2;
            }
            value if !value.starts_with('-') => {
                positional.push(value);
                position += 1;
            }
            _ => position += 1,
        }
    }
    let context = endpoint.map(|endpoint| ServerContext {
        endpoint,
        logical_instance,
    });

    let help = values
        .iter()
        .any(|value| value == "--help" || value == "-h");
    let version = values
        .iter()
        .any(|value| value == "--version" || value == "-V");
    if (help || version)
        && (!positional.is_empty()
            || explicit_no_activate
            || json
            || (help && version)
            || values.len() != 1)
    {
        return Err("help and version flags must be used alone".to_owned());
    }
    if help {
        return Ok(EntryCommand::Help);
    }
    if version {
        return Ok(EntryCommand::Version);
    }
    match positional.as_slice() {
        [] | ["open"] if !json => Ok(EntryCommand::Open {
            no_activate,
            context,
        }),
        ["capabilities"] if !explicit_no_activate && context.is_none() => {
            Ok(EntryCommand::Capabilities { json })
        }
        ["status"] if !explicit_no_activate && context.is_none() => {
            Ok(EntryCommand::Status { json })
        }
        ["close"] if !explicit_no_activate && context.is_none() => Ok(EntryCommand::Close { json }),
        ["snapshot"] if !explicit_no_activate => Ok(EntryCommand::Snapshot { json, context }),
        [] | ["open"] => Err("--json is valid only for capabilities or snapshot".to_owned()),
        ["capabilities"] | ["status"] | ["snapshot"] | ["close"] => {
            Err("--no-activate is valid only for open".to_owned())
        }
        [other, ..] => Err(format!("unknown command: {other}")),
    }
}

fn run_entry(command: EntryCommand) -> Result<()> {
    match command {
        EntryCommand::Help => print!("{HELP}"),
        EntryCommand::Version => println!("agenterm-cc {}", env!("CARGO_PKG_VERSION")),
        EntryCommand::Capabilities { json } => {
            let document = capabilities();
            if json {
                println!("{}", serde_json::to_string_pretty(&document)?);
            } else {
                println!(
                    "Control Center: native isolated projection\n\
                     Public UI action: {PUBLIC_UI_ACTION}\n\
                     Typed operation: {TYPED_OPERATION}\n\
                     Views: Cockpit, Workflows, Extensions, InfoHub"
                );
            }
        }
        EntryCommand::Status { json } => {
            let document = status_document();
            if json {
                println!("{}", serde_json::to_string_pretty(&document)?);
            } else {
                println!(
                    "Control Center: {}{}",
                    document.state,
                    document
                        .pid
                        .map(|pid| format!(" (PID {pid})"))
                        .unwrap_or_default()
                );
            }
        }
        EntryCommand::Close { json } => {
            let document = close_control_center();
            if json {
                println!("{}", serde_json::to_string_pretty(&document)?);
            } else {
                println!("Control Center: {}", document.state);
            }
        }
        EntryCommand::Snapshot { json, context } => {
            let document = snapshot_for_context(context.or_else(read_persisted_context));
            if json {
                println!("{}", serde_json::to_string_pretty(&document)?);
            } else {
                println!("Control Center server state: {}", document.server_state);
                for view in document.views {
                    println!("{}: {} ({})", view.label, view.state, view.reason);
                }
            }
        }
        EntryCommand::Open {
            no_activate,
            context,
        } => run_shell(no_activate, context)?,
    }
    Ok(())
}

fn capabilities() -> CapabilityDocument {
    CapabilityDocument {
        schema_version: SCHEMA_VERSION,
        executable: "agenterm-cc",
        role: "isolated_projection",
        public_ui_action: PUBLIC_UI_ACTION,
        typed_operation: TYPED_OPERATION,
        renderer: "native",
        webview_host: crate::webview_host::probe(),
        owns_terminal_authority: false,
        process_reuse: true,
        no_activate: true,
        views: ["cockpit", "workflows", "extensions", "info_hub"],
    }
}

fn disconnected_snapshot() -> SnapshotDocument {
    SnapshotDocument {
        schema_version: SCHEMA_VERSION,
        executable: "agenterm-cc",
        process_role: "isolated_projection",
        renderer: "native",
        webview_host: crate::webview_host::probe(),
        connected_server: None,
        server_state: "disconnected",
        server_reason: Some("no_server_context".to_owned()),
        server_detail: None,
        views: [
            unavailable("cockpit", "Cockpit", "no_server_context"),
            unavailable("workflows", "Workflows", "workflow_runtime_not_connected"),
            unavailable(
                "extensions",
                "Extensions",
                "extension_catalog_not_connected",
            ),
            unavailable("info_hub", "InfoHub", "info_sources_not_connected"),
        ],
    }
}

fn unavailable(id: &'static str, label: &'static str, reason: &'static str) -> ViewSnapshot {
    ViewSnapshot {
        id,
        label,
        state: "unavailable",
        reason: reason.to_owned(),
        data: None,
    }
}

fn available(id: &'static str, label: &'static str, data: Value) -> ViewSnapshot {
    ViewSnapshot {
        id,
        label,
        state: "available",
        reason: "connected".to_owned(),
        data: Some(data),
    }
}

fn control_center_executable() -> Result<PathBuf> {
    let current = env::current_exe().context("current executable is unavailable")?;
    let mut executable = current.with_file_name("agenterm-cc");
    if cfg!(windows) {
        executable.set_extension("exe");
    }
    Ok(executable)
}

fn registry_path() -> PathBuf {
    if let Some(path) = env::var_os("AGENTERM_CC_REGISTRY_PATH").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    let settings_domain = env::var_os("AGENTERM_SETTINGS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_settings_path);
    let digest = Sha256::digest(settings_domain.to_string_lossy().as_bytes());
    let domain = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    settings_domain
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("control-center")
        .join(format!("{domain}.json"))
}

fn context_path(path: &Path) -> PathBuf {
    path.with_extension("context.json")
}

fn validate_context_value(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        anyhow::bail!("{label} must be non-empty, bounded, and contain no control characters");
    }
    Ok(())
}

fn write_context(path: &Path, context: &ServerContext) -> Result<()> {
    validate_context_value("server endpoint", &context.endpoint)?;
    if let Some(instance) = &context.logical_instance {
        validate_context_value("logical instance", instance)?;
    }
    write_private_atomic(&context_path(path), &serde_json::to_vec_pretty(context)?)
}

fn read_persisted_context() -> Option<ServerContext> {
    read_regular_file(&context_path(&registry_path()))
        .ok()
        .and_then(|content| serde_json::from_slice::<ServerContext>(&content).ok())
        .filter(|context| validate_context_value("server endpoint", &context.endpoint).is_ok())
}

fn read_regular_file(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control center state path is not a regular file",
        ));
    }
    fs::read(path)
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        anyhow::bail!("control_center_state_path_not_regular");
    }
    let parent = path
        .parent()
        .context("control center state path has no parent")?;
    fs::create_dir_all(parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut temporary = None;
    for attempt in 0..8_u8 {
        let candidate = parent.join(format!(
            ".agenterm-cc-{}-{nonce}-{attempt}.tmp",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                temporary = Some(candidate);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let temporary = temporary.context("control_center_temporary_file_collision")?;
    let result = replace_file(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

fn status_document() -> StatusDocument {
    let record = read_registry(&registry_path()).filter(registry_process_matches);
    StatusDocument {
        schema_version: SCHEMA_VERSION,
        executable: "agenterm-cc",
        state: if record.is_some() {
            "running"
        } else {
            "not_running"
        },
        pid: record.as_ref().map(|record| record.pid),
        context: record.and_then(|_| read_persisted_context()),
    }
}

fn close_control_center() -> CloseDocument {
    let path = registry_path();
    let Some(record) = read_registry(&path) else {
        if registry_is_fresh(&path) {
            return CloseDocument {
                schema_version: SCHEMA_VERSION,
                executable: "agenterm-cc",
                state: "starting",
                pid: None,
            };
        }
        recover_stale_registry(&path);
        return CloseDocument {
            schema_version: SCHEMA_VERSION,
            executable: "agenterm-cc",
            state: "not_running",
            pid: None,
        };
    };
    if !registry_process_matches(&record) {
        recover_stale_registry(&path);
        return CloseDocument {
            schema_version: SCHEMA_VERSION,
            executable: "agenterm-cc",
            state: "stale_recovered",
            pid: Some(record.pid),
        };
    }
    if write_private_atomic(
        &close_request_path(&path),
        &serde_json::to_vec(&record).unwrap_or_default(),
    )
    .is_err()
    {
        return CloseDocument {
            schema_version: SCHEMA_VERSION,
            executable: "agenterm-cc",
            state: "close_request_failed",
            pid: Some(record.pid),
        };
    }

    let deadline = std::time::Instant::now() + Duration::from_millis(750);
    while std::time::Instant::now() < deadline {
        if !read_registry(&path).is_some_and(|current| {
            current.pid == record.pid
                && current.process_start_identity == record.process_start_identity
                && registry_process_matches(&current)
        }) {
            return CloseDocument {
                schema_version: SCHEMA_VERSION,
                executable: "agenterm-cc",
                state: "closed",
                pid: Some(record.pid),
            };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    CloseDocument {
        schema_version: SCHEMA_VERSION,
        executable: "agenterm-cc",
        state: "close_requested",
        pid: Some(record.pid),
    }
}

fn recover_stale_registry(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(native_window_path(path));
    let _ = fs::remove_file(focus_request_path(path));
    let _ = fs::remove_file(context_path(path));
    let _ = fs::remove_file(close_request_path(path));
}

fn snapshot_for_context(context: Option<ServerContext>) -> SnapshotDocument {
    let Some(context) = context else {
        return disconnected_snapshot();
    };
    match query_server(&context) {
        Ok(server) => {
            let cockpit = serde_json::to_value(&server).unwrap_or_default();
            SnapshotDocument {
                schema_version: SCHEMA_VERSION,
                executable: "agenterm-cc",
                process_role: "isolated_projection",
                renderer: "native",
                webview_host: crate::webview_host::probe(),
                connected_server: Some(server),
                server_state: "connected",
                server_reason: None,
                server_detail: None,
                views: [
                    available("cockpit", "Cockpit", cockpit),
                    unavailable("workflows", "Workflows", "workflow_runtime_unavailable"),
                    unavailable("extensions", "Extensions", "extension_catalog_unavailable"),
                    unavailable("info_hub", "InfoHub", "info_sources_unavailable"),
                ],
            }
        }
        Err(error) => {
            let mut snapshot = disconnected_snapshot();
            snapshot.server_state = "unavailable";
            let detail = format!("{error:#}");
            let reason = server_failure_reason(&detail);
            snapshot.server_reason = Some(reason.to_owned());
            snapshot.server_detail = Some(detail);
            snapshot.views[0].reason = reason.to_owned();
            snapshot
        }
    }
}

fn server_failure_reason(detail: &str) -> &'static str {
    if detail.contains("incompatible")
        || detail.contains("omitted")
        || detail.contains("authority PID changed")
    {
        "server_incompatible"
    } else if detail.contains("rejected") {
        "server_rejected"
    } else {
        "server_unreachable"
    }
}

fn query_server(context: &ServerContext) -> Result<ConnectedServer> {
    let timeout = Duration::from_millis(750);
    let bootstrap = crate::client::send_ipc_request_to_timeout(
        &context.endpoint,
        vec!["ui-bootstrap".to_owned()],
        timeout,
    )
    .context("server bootstrap unavailable")?;
    if !bootstrap.ok {
        anyhow::bail!(
            "server bootstrap rejected: {}",
            if bootstrap.error.is_empty() {
                "unknown error"
            } else {
                &bootstrap.error
            }
        );
    }
    let bootstrap: Value =
        serde_json::from_str(&bootstrap.output).context("server bootstrap is incompatible")?;
    let protocol = crate::client::send_ipc_request_to_timeout(
        &context.endpoint,
        vec!["protocol-info".to_owned(), "--running".to_owned()],
        timeout,
    )
    .context("server protocol facts unavailable")?;
    if !protocol.ok {
        anyhow::bail!("server protocol facts rejected: {}", protocol.error);
    }
    let protocol: Value =
        serde_json::from_str(&protocol.output).context("server protocol facts are incompatible")?;
    let pid = bootstrap["server_pid"]
        .as_u64()
        .context("server bootstrap omitted PID")?;
    if protocol["pid"].as_u64() != Some(pid) {
        anyhow::bail!("server_restart: authority PID changed during snapshot");
    }
    let epoch = bootstrap["server_epoch"]
        .as_str()
        .filter(|epoch| !epoch.is_empty())
        .context("server bootstrap omitted epoch")?
        .to_owned();
    let sequence = bootstrap["position"]["sequence"]
        .as_u64()
        .context("server bootstrap omitted sequence")?;
    let tabs = bootstrap["tabs"]
        .as_array()
        .context("server bootstrap omitted tabs")?
        .iter()
        .map(|tab| TabSummary {
            id: tab["id"].as_str().unwrap_or_default().to_owned(),
            index: tab["index"].as_u64().unwrap_or_default(),
            title: tab["title"].as_str().unwrap_or_default().to_owned(),
            note: tab["note"].as_str().unwrap_or_default().to_owned(),
            process_id: tab["process_id"].as_u64(),
            dead: tab["dead"].as_bool().unwrap_or(false),
        })
        .collect();
    Ok(ConnectedServer {
        endpoint: context.endpoint.clone(),
        logical_instance: context.logical_instance.clone(),
        pid,
        epoch,
        sequence,
        version: protocol["agenterm_version"].as_str().map(str::to_owned),
        build: protocol["build_identity"].clone(),
        active_tab_id: bootstrap["active_tab_id"].as_str().map(str::to_owned),
        tabs,
        components: ComponentAvailability {
            server: "available",
            workflows: "unavailable",
            extensions: "unavailable",
            info_hub: "unavailable",
        },
    })
}

fn default_settings_path() -> PathBuf {
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join("AgenTerm")
            .join("settings.json")
    }
    #[cfg(unix)]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(env::temp_dir)
            .join("agenterm")
            .join("settings.json")
    }
}

#[cfg(windows)]
fn process_start_identity(pid: u32) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut creation: FILETIME = unsafe { std::mem::zeroed() };
    let mut exit: FILETIME = unsafe { std::mem::zeroed() };
    let mut kernel: FILETIME = unsafe { std::mem::zeroed() };
    let mut user: FILETIME = unsafe { std::mem::zeroed() };
    let queried =
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } != 0;
    unsafe { CloseHandle(process) };
    queried.then(|| {
        let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        format!("windows-filetime:{ticks}")
    })
}

#[cfg(unix)]
fn process_start_identity(pid: u32) -> Option<String> {
    // Linux exposes an unambiguous kernel start tick in field 22.  Keep the
    // full command name grouping intact because it may contain spaces.
    if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat"))
        && let Some(after_name) = stat.rsplit_once(") ").map(|(_, fields)| fields)
        && let Some(start_ticks) = after_name.split_whitespace().nth(19)
    {
        return Some(format!("proc-start-ticks:{start_ticks}"));
    }

    // macOS and other supported Unix hosts have `ps`; elapsed seconds changes
    // over time, so use the stable textual process start timestamp.
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let started = String::from_utf8(output.stdout).ok()?;
    let started = started.trim();
    (!started.is_empty()).then(|| format!("ps-lstart:{started}"))
}

fn registry_process_matches(record: &RegistryRecord) -> bool {
    !record.process_start_identity.is_empty()
        && process_start_identity(record.pid).as_deref()
            == Some(record.process_start_identity.as_str())
}

fn claim_registry(path: &Path) -> Result<RegistryClaim> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        #[cfg(unix)]
        if parent
            .file_name()
            .is_some_and(|name| name == "control-center")
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
    }
    for _ in 0..2 {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(mut file) => {
                let _ = fs::remove_file(native_window_path(path));
                let _ = fs::remove_file(focus_request_path(path));
                let _ = fs::remove_file(close_request_path(path));
                let process_start_identity = process_start_identity(std::process::id())
                    .context("control_center_process_start_identity_unavailable")?;
                let record = RegistryRecord {
                    schema_version: REGISTRY_SCHEMA_VERSION,
                    pid: std::process::id(),
                    process_start_identity,
                };
                serde_json::to_writer(&mut file, &record)?;
                file.write_all(b"\n")?;
                return Ok(RegistryClaim::Owner(RegistryOwner {
                    path: path.to_owned(),
                    pid: record.pid,
                    process_start_identity: record.process_start_identity,
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if let Some(record) = read_registry(path) {
                    if registry_process_matches(&record) {
                        return Ok(RegistryClaim::Existing(record));
                    }
                } else if registry_is_fresh(path)
                    && fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0)
                {
                    // Another process may still be publishing its create-new
                    // record. Treat a fresh unreadable file as claimed instead
                    // of deleting a live owner's lock.
                    return Ok(RegistryClaim::Existing(RegistryRecord {
                        schema_version: REGISTRY_SCHEMA_VERSION,
                        pid: 0,
                        process_start_identity: String::new(),
                    }));
                }
                recover_stale_registry(path);
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("control_center_registry_race")
}

fn registry_is_fresh(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age < std::time::Duration::from_secs(2))
}

fn read_registry(path: &Path) -> Option<RegistryRecord> {
    read_regular_file(path)
        .ok()
        .and_then(|content| serde_json::from_slice(&content).ok())
        .filter(|record: &RegistryRecord| record.schema_version == REGISTRY_SCHEMA_VERSION)
}

fn native_window_path(path: &Path) -> PathBuf {
    path.with_extension("window")
}

fn focus_request_path(path: &Path) -> PathBuf {
    path.with_extension("focus")
}

fn close_request_path(path: &Path) -> PathBuf {
    path.with_extension("close")
}

fn request_projection_refresh(registry_path: &Path, no_activate: bool) {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mode = if no_activate {
        "no-activate"
    } else {
        "activate"
    };
    let _ = write_private_atomic(
        &focus_request_path(registry_path),
        format!("{mode}:{}:{nonce}\n", std::process::id()).as_bytes(),
    );
}

fn run_shell(no_activate: bool, context: Option<ServerContext>) -> Result<()> {
    let path = registry_path();
    match claim_registry(&path)? {
        RegistryClaim::Existing(record) => {
            if let Some(context) = &context {
                write_context(&path, context)?;
            }
            focus_existing(&record, &path, no_activate);
            Ok(())
        }
        RegistryClaim::Owner(owner) => {
            if let Some(context) = &context {
                write_context(&path, context)?;
            }
            platform_shell(owner, no_activate)
        }
    }
}

#[cfg(windows)]
fn focus_existing(_record: &RegistryRecord, registry_path: &Path, no_activate: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsWindow, SW_RESTORE, SW_SHOWNOACTIVATE, SetForegroundWindow, ShowWindowAsync,
    };
    let native_window = read_regular_file(&native_window_path(registry_path))
        .ok()
        .and_then(|value| String::from_utf8(value).ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default();
    request_projection_refresh(registry_path, no_activate);
    let window = native_window as isize as *mut std::ffi::c_void;
    if window.is_null() || unsafe { IsWindow(window) } == 0 {
        return;
    }
    unsafe {
        ShowWindowAsync(
            window,
            if no_activate {
                SW_SHOWNOACTIVATE
            } else {
                SW_RESTORE
            },
        );
        if !no_activate {
            SetForegroundWindow(window);
        }
    }
}

#[cfg(unix)]
fn focus_existing(_record: &RegistryRecord, registry_path: &Path, no_activate: bool) {
    request_projection_refresh(registry_path, no_activate);
}

#[cfg(windows)]
fn platform_shell(owner: RegistryOwner, no_activate: bool) -> Result<()> {
    windows_shell(owner, no_activate)
}

#[cfg(unix)]
fn platform_shell(owner: RegistryOwner, no_activate: bool) -> Result<()> {
    unix_shell(owner, no_activate)
}

#[cfg(windows)]
static WINDOWS_PROJECTION: std::sync::OnceLock<std::sync::Mutex<ShellProjection>> =
    std::sync::OnceLock::new();

#[cfg(windows)]
fn windows_shell(owner: RegistryOwner, no_activate: bool) -> Result<()> {
    use std::{mem, ptr};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, EndPaint, GetStockObject, InvalidateRect, PAINTSTRUCT, TextOutW,
            WHITE_BRUSH,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
            DispatchMessageW, GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostMessageW,
            PostQuitMessage, RegisterClassW, SW_SHOW, SW_SHOWNOACTIVATE, SetTimer, SetWindowTextW,
            ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW,
            WS_OVERLAPPEDWINDOW,
        },
    };

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_PAINT => {
                let mut paint: PAINTSTRUCT = unsafe { mem::zeroed() };
                let device = unsafe { BeginPaint(window, &mut paint) };
                let lines = WINDOWS_PROJECTION
                    .get()
                    .and_then(|projection| projection.lock().ok())
                    .map(|projection| projection.lines())
                    .unwrap_or_else(|| {
                        vec!["AgenTerm Control Center · state unavailable".to_owned()]
                    });
                for (index, line) in lines.iter().enumerate() {
                    let wide = line.encode_utf16().collect::<Vec<_>>();
                    unsafe {
                        TextOutW(
                            device,
                            24,
                            24 + i32::try_from(index).unwrap_or(0) * 28,
                            wide.as_ptr(),
                            i32::try_from(wide.len()).unwrap_or(0),
                        );
                    }
                }
                unsafe { EndPaint(window, &paint) };
                0
            }
            WM_TIMER => {
                if let Some(projection) = WINDOWS_PROJECTION.get()
                    && let Ok(mut projection) = projection.lock()
                {
                    if projection.close_requested() {
                        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
                    } else if projection.poll() {
                        let title = wide_null(&projection.title());
                        unsafe {
                            SetWindowTextW(window, title.as_ptr());
                            InvalidateRect(window, ptr::null(), 1);
                        }
                    }
                }
                0
            }
            WM_DESTROY => {
                unsafe {
                    KillTimer(window, 1);
                    PostQuitMessage(0);
                }
                0
            }
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }

    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let class = wide_null("AgenTermControlCenterWindow");
    let projection = ShellProjection::new(&owner.path);
    let title = wide_null(&projection.title());
    WINDOWS_PROJECTION
        .set(std::sync::Mutex::new(projection))
        .map_err(|_| anyhow::anyhow!("control_center_projection_already_initialized"))?;
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        hbrBackground: unsafe { GetStockObject(WHITE_BRUSH) } as _,
        lpszClassName: class.as_ptr(),
        ..unsafe { mem::zeroed() }
    };
    unsafe { RegisterClassW(&window_class) };
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            480,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        )
    };
    if window.is_null() {
        anyhow::bail!("control_center_window_create_failed");
    }
    owner.publish_native_window(window as isize as i64)?;
    unsafe {
        SetTimer(window, 1, 200, None);
        ShowWindow(
            window,
            if no_activate {
                SW_SHOWNOACTIVATE
            } else {
                SW_SHOW
            },
        );
    }
    let mut message: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    drop(owner);
    Ok(())
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(unix)]
fn unix_shell(owner: RegistryOwner, no_activate: bool) -> Result<()> {
    use std::{num::NonZeroU32, rc::Rc};

    use softbuffer::{Context, Surface};
    use winit::{
        application::ApplicationHandler,
        dpi::LogicalSize,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        window::{UserAttentionType, Window, WindowAttributes, WindowId},
    };

    struct App {
        no_activate: bool,
        context: Context<winit::event_loop::OwnedDisplayHandle>,
        window: Option<Rc<Window>>,
        surface: Option<Surface<winit::event_loop::OwnedDisplayHandle, Rc<winit::window::Window>>>,
        focus_request: PathBuf,
        last_focus_request: Option<String>,
        projection: ShellProjection,
        _owner: RegistryOwner,
    }

    impl App {
        fn request_redraw(&self) {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }

        fn redraw(&mut self) {
            let Some(window) = self.window.as_ref() else {
                return;
            };
            let physical_size = window.inner_size();
            let (Some(width), Some(height)) = (
                NonZeroU32::new(physical_size.width),
                NonZeroU32::new(physical_size.height),
            ) else {
                return;
            };
            let Some(surface) = self.surface.as_mut() else {
                return;
            };
            if surface.resize(width, height).is_err() {
                return;
            }
            let Ok(mut buffer) = surface.buffer_mut() else {
                return;
            };
            let buffer_width = buffer.width().get();
            let buffer_height = buffer.height().get();
            render_unix_shell(
                &mut buffer,
                buffer_width,
                buffer_height,
                &self.projection.lines(),
            );
            let _ = buffer.present();
        }
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let base = WindowAttributes::default()
                .with_title(self.projection.title())
                .with_inner_size(LogicalSize::new(760, 480));
            #[cfg(target_os = "linux")]
            let attributes = crate::platform::linux::activation::configure_window_attributes(
                base,
                self.no_activate,
            );
            #[cfg(not(target_os = "linux"))]
            let attributes = base.with_active(!self.no_activate);
            match event_loop.create_window(attributes) {
                Ok(window) => {
                    let window = Rc::new(window);
                    match Surface::new(&self.context, Rc::clone(&window)) {
                        Ok(surface) => {
                            self.surface = Some(surface);
                            self.window = Some(window);
                            self.request_redraw();
                        }
                        Err(error) => {
                            eprintln!("agenterm-cc: {error}");
                            event_loop.exit();
                        }
                    }
                }
                Err(error) => {
                    eprintln!("agenterm-cc: {error}");
                    event_loop.exit();
                }
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    self.request_redraw();
                }
                WindowEvent::RedrawRequested => self.redraw(),
                _ => {}
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.projection.close_requested() {
                event_loop.exit();
                return;
            }
            if self.projection.poll()
                && let Some(window) = self.window.as_ref()
            {
                window.set_title(&self.projection.title());
                window.request_redraw();
            }
            let request = read_regular_file(&self.focus_request)
                .ok()
                .and_then(|value| String::from_utf8(value).ok());
            if request.is_some() && request != self.last_focus_request {
                self.last_focus_request = request;
                if let Some(window) = self.window.as_ref()
                    && !self
                        .last_focus_request
                        .as_deref()
                        .is_some_and(|request| request.starts_with("no-activate:"))
                {
                    window.set_minimized(false);
                    window.request_user_attention(Some(UserAttentionType::Informational));
                    window.focus_window();
                }
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(200),
            ));
        }
    }

    let event_loop = EventLoop::new()?;
    let context = Context::new(event_loop.owned_display_handle())
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let projection = ShellProjection::new(&owner.path);
    let mut app = App {
        no_activate,
        context,
        window: None,
        surface: None,
        focus_request: focus_request_path(&owner.path),
        last_focus_request: None,
        projection,
        _owner: owner,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(unix)]
fn render_unix_shell(pixels: &mut [u32], width: u32, height: u32, lines: &[String]) {
    const BACKGROUND: u32 = 0x00F4_F6F8;
    const HEADER: u32 = 0x001B_2533;
    const TITLE: u32 = 0x00F8_FAFC;
    const BODY: u32 = 0x0020_2937;
    const DIVIDER: u32 = 0x00D9_DFE7;

    pixels.fill(BACKGROUND);
    fill_unix_rect(pixels, width, height, 0, 0, width, 64, HEADER);
    fill_unix_rect(pixels, width, height, 0, 64, width, 1, DIVIDER);

    let body_scale = if width >= 640 { 2 } else { 1 };
    let title_scale = if width >= 420 { 2 } else { 1 };
    if let Some(title) = lines.first() {
        draw_unix_text(
            pixels,
            width,
            height,
            24,
            if title_scale == 2 { 24 } else { 28 },
            title_scale,
            TITLE,
            title,
        );
    }
    let line_height = crate::unix_app::font::GLYPH_HEIGHT * body_scale + 12;
    for (index, line) in lines.iter().skip(1).enumerate() {
        let y = 88_u32.saturating_add(
            u32::try_from(index)
                .unwrap_or(u32::MAX)
                .saturating_mul(line_height),
        );
        if y >= height {
            break;
        }
        draw_unix_text(pixels, width, height, 24, y, body_scale, BODY, line);
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn fill_unix_rect(
    pixels: &mut [u32],
    stride: u32,
    height: u32,
    x: u32,
    y: u32,
    width: u32,
    rect_height: u32,
    color: u32,
) {
    let right = x.saturating_add(width).min(stride);
    let bottom = y.saturating_add(rect_height).min(height);
    for row in y.min(height)..bottom {
        let start = (row.saturating_mul(stride).saturating_add(x.min(stride))) as usize;
        let end = (row.saturating_mul(stride).saturating_add(right)) as usize;
        if let Some(slice) = pixels.get_mut(start..end) {
            slice.fill(color);
        }
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn draw_unix_text(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    color: u32,
    text: &str,
) {
    let advance = (crate::unix_app::font::GLYPH_WIDTH + 1).saturating_mul(scale);
    let mut cursor_x = x;
    for character in text.chars() {
        if cursor_x >= width {
            break;
        }
        draw_unix_glyph(
            pixels,
            width,
            height,
            cursor_x,
            y,
            scale,
            color,
            unix_display_character(character),
        );
        cursor_x = cursor_x.saturating_add(advance);
    }
}

#[cfg(unix)]
fn unix_display_character(character: char) -> char {
    match character {
        '—' | '–' => '-',
        '·' | '•' => '*',
        character if character.is_ascii() && !character.is_ascii_control() => character,
        _ => '?',
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn draw_unix_glyph(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    color: u32,
    character: char,
) {
    let Some(rows) = crate::unix_app::font::glyph_rows(character)
        .or_else(|| crate::unix_app::font::glyph_rows('?'))
    else {
        return;
    };
    for (row, bits) in rows.iter().copied().enumerate() {
        for column in 0..crate::unix_app::font::GLYPH_WIDTH {
            if !crate::unix_app::font::row_contains_pixel(bits, column) {
                continue;
            }
            fill_unix_rect(
                pixels,
                width,
                height,
                x.saturating_add(column.saturating_mul(scale)),
                y.saturating_add(u32::try_from(row).unwrap_or(u32::MAX).saturating_mul(scale)),
                scale,
                scale,
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn unix_shell_renderer_paints_projection_text_and_respects_small_surfaces() {
        const WIDTH: u32 = 760;
        const HEIGHT: u32 = 200;
        let mut pixels = vec![0; (WIDTH * HEIGHT) as usize];
        render_unix_shell(
            &mut pixels,
            WIDTH,
            HEIGHT,
            &[
                "AgenTerm Control Center".to_owned(),
                "Cockpit     available (read_only)".to_owned(),
                "Workflows   unavailable (not_implemented)".to_owned(),
            ],
        );
        assert!(pixels.contains(&0x001B_2533), "header background");
        assert!(pixels.contains(&0x00F8_FAFC), "header text");
        assert!(pixels.contains(&0x0020_2937), "projection body text");

        let mut tiny = vec![0; 7 * 5];
        render_unix_shell(&mut tiny, 7, 5, &["AgenTerm Control Center".to_owned()]);
        assert_eq!(tiny.len(), 35);
        assert!(tiny.iter().all(|pixel| *pixel == 0x001B_2533));
    }

    #[cfg(unix)]
    #[test]
    fn unix_shell_renderer_uses_visible_ascii_fallbacks_for_projection_punctuation() {
        assert_eq!(unix_display_character('—'), '-');
        assert_eq!(unix_display_character('·'), '*');
        assert_eq!(unix_display_character('中'), '?');
        assert_eq!(unix_display_character('a'), 'a');
    }

    #[test]
    fn default_command_opens_and_accepts_no_activate() {
        assert_eq!(
            parse_entry(&[OsString::from("--no-activate")]).unwrap(),
            EntryCommand::Open {
                no_activate: true,
                context: None,
            }
        );
    }

    #[test]
    fn informational_commands_do_not_map_to_open() {
        assert_eq!(
            parse_entry(&[OsString::from("capabilities"), OsString::from("--json")]).unwrap(),
            EntryCommand::Capabilities { json: true }
        );
        assert_eq!(
            parse_entry(&[OsString::from("snapshot"), OsString::from("--json")]).unwrap(),
            EntryCommand::Snapshot {
                json: true,
                context: None,
            }
        );
    }

    #[test]
    fn unknown_or_misplaced_options_fail_without_opening() {
        assert!(parse_entry(&[OsString::from("--unknown")]).is_err());
        assert!(parse_entry(&[OsString::from("--json")]).is_err());
        assert!(
            parse_entry(&[
                OsString::from("capabilities"),
                OsString::from("--no-activate")
            ])
            .is_err()
        );
        assert!(parse_entry(&[OsString::from("--help"), OsString::from("--json")]).is_err());
    }

    #[test]
    fn capability_contract_distinguishes_ui_action_and_typed_operation() {
        let capability = capabilities();
        assert_eq!(capability.public_ui_action, "open-control-center");
        assert_eq!(capability.typed_operation, "control-center.open");
        assert!(!capability.owns_terminal_authority);
    }

    #[test]
    fn disconnected_views_are_truthfully_unavailable() {
        let snapshot = disconnected_snapshot();
        assert!(snapshot.connected_server.is_none());
        assert!(
            snapshot
                .views
                .iter()
                .all(|view| view.state == "unavailable")
        );
    }

    #[test]
    fn live_registry_claim_reuses_the_existing_process() {
        let path = env::temp_dir().join(format!(
            "agenterm-cc-registry-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time after epoch")
                .as_nanos()
        ));
        let owner = match claim_registry(&path).expect("first registry claim") {
            RegistryClaim::Owner(owner) => owner,
            RegistryClaim::Existing(_) => panic!("isolated registry unexpectedly existed"),
        };
        match claim_registry(&path).expect("second registry claim") {
            RegistryClaim::Existing(record) => {
                assert_eq!(record.pid, std::process::id());
                assert_eq!(
                    Some(record.process_start_identity.as_str()),
                    process_start_identity(std::process::id()).as_deref()
                );
            }
            RegistryClaim::Owner(_) => panic!("duplicate registry owner"),
        }
        drop(owner);
        assert!(!path.exists());
    }

    #[test]
    fn live_foreign_pid_identity_is_recovered_without_reusing_it() {
        let path = env::temp_dir().join(format!(
            "agenterm-cc-foreign-identity-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time after epoch")
                .as_nanos()
        ));
        let foreign = RegistryRecord {
            schema_version: REGISTRY_SCHEMA_VERSION,
            pid: std::process::id(),
            process_start_identity: "foreign-start-identity".to_owned(),
        };
        fs::write(&path, serde_json::to_vec(&foreign).expect("registry JSON"))
            .expect("write foreign registry");
        let owner = match claim_registry(&path).expect("recover foreign registry") {
            RegistryClaim::Owner(owner) => owner,
            RegistryClaim::Existing(_) => panic!("foreign identity must not be reused"),
        };
        assert_ne!(owner.process_start_identity, foreign.process_start_identity);
        drop(owner);
    }

    #[test]
    fn corrupt_registry_is_recovered_without_signaling_a_process() {
        let path = env::temp_dir().join(format!(
            "agenterm-cc-corrupt-registry-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time after epoch")
                .as_nanos()
        ));
        fs::write(&path, b"{corrupt").expect("write corrupt registry");
        let owner = match claim_registry(&path).expect("recover corrupt registry") {
            RegistryClaim::Owner(owner) => owner,
            RegistryClaim::Existing(_) => panic!("corrupt registry must not be reused"),
        };
        drop(owner);
    }

    #[test]
    fn unavailable_snapshot_has_typed_reason_and_diagnostic_detail() {
        let snapshot = snapshot_for_context(Some(ServerContext {
            endpoint: "127.0.0.1:9".to_owned(),
            logical_instance: Some("main".to_owned()),
        }));
        assert_eq!(snapshot.server_state, "unavailable");
        assert_eq!(
            snapshot.server_reason.as_deref(),
            Some("server_unreachable")
        );
        assert!(snapshot.server_detail.is_some());
        assert_eq!(snapshot.views[0].reason, "server_unreachable");
        assert_eq!(snapshot.views[1].state, "unavailable");
        assert_eq!(snapshot.views[2].state, "unavailable");
        assert_eq!(snapshot.views[3].state, "unavailable");
    }

    #[test]
    fn close_request_is_bound_to_the_exact_registry_owner() {
        let path = env::temp_dir().join(format!(
            "agenterm-cc-close-owner-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time after epoch")
                .as_nanos()
        ));
        let owner = match claim_registry(&path).expect("claim registry") {
            RegistryClaim::Owner(owner) => owner,
            RegistryClaim::Existing(_) => panic!("isolated registry unexpectedly existed"),
        };
        let projection = ShellProjection::new(&path);
        let foreign = RegistryRecord {
            schema_version: REGISTRY_SCHEMA_VERSION,
            pid: owner.pid,
            process_start_identity: "foreign-start-identity".to_owned(),
        };
        write_private_atomic(
            &close_request_path(&path),
            &serde_json::to_vec(&foreign).expect("foreign close JSON"),
        )
        .expect("write foreign close");
        assert!(!projection.close_requested());

        let exact = RegistryRecord {
            schema_version: REGISTRY_SCHEMA_VERSION,
            pid: owner.pid,
            process_start_identity: owner.process_start_identity.clone(),
        };
        write_private_atomic(
            &close_request_path(&path),
            &serde_json::to_vec(&exact).expect("exact close JSON"),
        )
        .expect("write exact close");
        assert!(projection.close_requested());
        drop(owner);
    }
}
