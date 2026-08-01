//! Isolated Control Center process shell.
//!
//! This module intentionally owns no terminal, PTY, workspace, server, or
//! workflow authority.  It is a replaceable native projection host.

use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::ipc_endpoint::{EndpointSelectorArgs, IpcEndpoint, resolve_ipc_endpoint};

const SCHEMA_VERSION: u32 = 1;
const REGISTRY_SCHEMA_VERSION: u32 = 2;
const REGISTRY_INCOMPATIBLE_LIVE: &str = "control_center_registry_incompatible_live";
const REGISTRY_UNPARSEABLE: &str = "control_center_registry_unparseable";
const PROJECTION_IPC_TIMEOUT: Duration = Duration::from_millis(750);
const PROJECTION_RETRY_MIN: Duration = Duration::from_millis(50);
const PROJECTION_RETRY_MAX: Duration = Duration::from_secs(1);
const PUBLIC_UI_ACTION: &str = "open-control-center";
const TYPED_OPERATION: &str = "control-center.open";
const HELP: &str = "\
AgenTerm Control Center

Usage:
  agenterm-cc [open] [--no-activate] [--instance NAME | --endpoint ENDPOINT]
  agenterm-cc status [--json]
  agenterm-cc close [--json]
  agenterm-cc snapshot [--json] [--instance NAME | --endpoint ENDPOINT]
  agenterm-cc screenshot --output PATH [--json]
  agenterm-cc capabilities [--json]
  agenterm-cc --help
  agenterm-cc --version

ENDPOINT is transport-qualified: unix:<path>, pipe:<name>, or tcp:<host>:<port>.
The legacy --server-endpoint and --logical-instance spellings remain migration
aliases. Endpoint and instance selectors are mutually exclusive.

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
    Screenshot {
        json: bool,
        output: PathBuf,
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
    screenshot: &'static str,
    views: [&'static str; 4],
}

#[derive(Debug, Serialize)]
struct ScreenshotDocument {
    schema_version: u32,
    executable: &'static str,
    state: &'static str,
    renderer: &'static str,
    owner_pid: u32,
    output: String,
    width: u32,
    height: u32,
    bytes: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rendered_snapshot: Option<RendererSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RendererSnapshot {
    schema_version: u32,
    owner_pid: u32,
    renderer: String,
    selected_view: String,
    server_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logical_instance: Option<String>,
    window_title: String,
    physical_width: u32,
    physical_height: u32,
    scale_factor: f64,
}

#[derive(Debug, Deserialize, Serialize)]
struct ScreenshotRequest {
    schema_version: u32,
    owner_pid: u32,
    process_start_identity: String,
    request_id: String,
    output: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
struct RendererCaptureResult {
    schema_version: u32,
    owner_pid: u32,
    process_start_identity: String,
    request_id: String,
    output: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<RendererSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
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
    tab_counts: TabCounts,
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
struct TabCounts {
    total: u64,
    running: u64,
    dead: u64,
}

impl TabCounts {
    fn from_tabs(tabs: &[TabSummary]) -> Self {
        let total = u64::try_from(tabs.len()).unwrap_or(u64::MAX);
        let dead = u64::try_from(tabs.iter().filter(|tab| tab.dead).count()).unwrap_or(u64::MAX);
        Self {
            total,
            running: total.saturating_sub(dead),
            dead,
        }
    }
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
            let _ = fs::remove_file(screenshot_request_path(&self.path));
            let _ = fs::remove_file(screenshot_result_path(&self.path));
        }
    }
}

enum RegistryClaim {
    Owner(RegistryOwner),
    Existing(RegistryRecord),
}

enum RegistryInspection {
    Missing,
    Publishing,
    Compatible(RegistryRecord),
    Incompatible(RegistryRecord),
    Unparseable,
}

struct ShellProjection {
    registry_file: PathBuf,
    context_file: PathBuf,
    context_bytes: Option<Vec<u8>>,
    refresh_file: PathBuf,
    refresh_bytes: Option<Vec<u8>>,
    generation: u64,
    mailbox: Arc<ProjectionMailbox>,
    worker_failure_applied: bool,
    snapshot: SnapshotDocument,
}

struct ProjectionMailbox {
    state: Mutex<ProjectionMailboxState>,
    wake: Condvar,
}

#[derive(Default)]
struct ProjectionMailboxState {
    request: Option<ProjectionRequest>,
    update: Option<ProjectionUpdate>,
    stop: bool,
    worker_stopped: bool,
}

struct ProjectionRequest {
    generation: u64,
    context: Option<ServerContext>,
}

struct ProjectionUpdate {
    generation: u64,
    snapshot: SnapshotDocument,
}

enum ProjectionWake {
    Context(ProjectionRequest),
    Deadline,
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectionProbeDecision {
    Quiet,
    Refresh,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectionPublishResult {
    Published,
    Superseded,
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProjectionBackoff {
    delay: Duration,
}

impl ProjectionBackoff {
    const fn new() -> Self {
        Self {
            delay: PROJECTION_RETRY_MIN,
        }
    }

    fn reset(&mut self) {
        self.delay = PROJECTION_RETRY_MIN;
    }

    fn advance(&mut self) {
        self.delay = self.delay.saturating_mul(2).min(PROJECTION_RETRY_MAX);
    }
}

impl ShellProjection {
    fn new(registry: &Path) -> Self {
        let mailbox = Arc::new(ProjectionMailbox {
            state: Mutex::new(ProjectionMailboxState::default()),
            wake: Condvar::new(),
        });
        spawn_projection_worker(Arc::clone(&mailbox));
        let mut projection = Self {
            registry_file: registry.to_owned(),
            context_file: context_path(registry),
            context_bytes: None,
            refresh_file: focus_request_path(registry),
            refresh_bytes: None,
            generation: 0,
            mailbox,
            worker_failure_applied: false,
            snapshot: disconnected_snapshot(),
        };
        projection.request_refresh(true);
        projection
    }

    fn request_refresh(&mut self, force: bool) {
        let bytes = match read_regular_file(&self.context_file) {
            Ok(bytes) => bytes,
            Err(_) if force && self.context_bytes.is_none() => {
                self.submit_context(None);
                return;
            }
            Err(_) => {
                // A context update uses replace-by-rename. Preserve the last known
                // projection if polling observes the tiny replacement gap.
                return;
            }
        };
        if !force && self.context_bytes.as_deref() == Some(bytes.as_slice()) {
            return;
        }
        let context = serde_json::from_slice::<ServerContext>(&bytes)
            .ok()
            .filter(|value| validate_context_value("server endpoint", &value.endpoint).is_ok());
        self.context_bytes = Some(bytes);
        self.submit_context(context);
    }

    fn submit_context(&mut self, context: Option<ServerContext>) {
        self.generation = self.generation.saturating_add(1);
        let mut state = lock_projection_mailbox(&self.mailbox);
        state.request = Some(ProjectionRequest {
            generation: self.generation,
            context,
        });
        self.mailbox.wake.notify_one();
    }

    fn poll(&mut self) -> bool {
        let refresh = read_regular_file(&self.refresh_file).ok();
        let forced = refresh.is_some() && refresh != self.refresh_bytes;
        if forced {
            self.refresh_bytes = refresh;
        }
        self.request_refresh(forced);

        let (update, worker_stopped) = {
            let mut state = lock_projection_mailbox(&self.mailbox);
            (state.update.take(), state.worker_stopped)
        };
        let mut changed = false;
        if let Some(update) = update
            && update.generation == self.generation
        {
            self.snapshot = update.snapshot;
            changed = true;
        }
        if worker_stopped && !self.worker_failure_applied {
            self.snapshot = projection_worker_unavailable_snapshot();
            self.worker_failure_applied = true;
            changed = true;
        }
        changed
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
            lines.extend(connected_cockpit_lines(server));
        }
        lines.extend(
            self.snapshot.views[1..]
                .iter()
                .map(|view| format!("{:<12}{} ({})", view.label, view.state, view.reason)),
        );
        lines
    }
}

fn connected_cockpit_lines(server: &ConnectedServer) -> Vec<String> {
    let authority = server.logical_instance.as_deref().unwrap_or("explicit");
    let version = server.version.as_deref().unwrap_or("unknown");
    let active = server
        .active_tab_id
        .as_deref()
        .and_then(|id| {
            server
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| format!("{id} · {}", tab.title))
        })
        .unwrap_or_else(|| "none".to_owned());
    vec![
        format!("Server      {authority} · PID {} · v{version}", server.pid),
        format!("Build       {}", build_identity_summary(&server.build)),
        format!(
            "Event       epoch {} · sequence {}",
            compact_identity(&server.epoch),
            server.sequence
        ),
        format!(
            "Fleet       {} tabs · {} running · {} dead",
            server.tab_counts.total, server.tab_counts.running, server.tab_counts.dead
        ),
        format!("Active      {active}"),
        format!(
            "Components  server {} · workflows {} · extensions {} · info {}",
            server.components.server,
            server.components.workflows,
            server.components.extensions,
            server.components.info_hub
        ),
    ]
}

fn build_identity_summary(build: &Value) -> String {
    let commit = build["git_commit"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(compact_identity)
        .unwrap_or_else(|| "unknown".to_owned());
    let profile = build["profile"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let cleanliness = build["git_dirty"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    format!("{commit} · {profile} · {cleanliness}")
}

fn compact_identity(identity: &str) -> String {
    const DISPLAY_CHARS: usize = 12;
    let mut chars = identity.chars();
    let prefix = chars.by_ref().take(DISPLAY_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

impl Drop for ShellProjection {
    fn drop(&mut self) {
        let mut state = lock_projection_mailbox(&self.mailbox);
        state.stop = true;
        state.request = None;
        self.mailbox.wake.notify_one();
    }
}

fn lock_projection_mailbox(
    mailbox: &ProjectionMailbox,
) -> std::sync::MutexGuard<'_, ProjectionMailboxState> {
    mailbox
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn spawn_projection_worker(mailbox: Arc<ProjectionMailbox>) {
    let worker_mailbox = Arc::clone(&mailbox);
    let spawned = std::thread::Builder::new()
        .name("agenterm-cc-projection".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                projection_worker_loop(Arc::clone(&worker_mailbox));
            }));
            let mut state = lock_projection_mailbox(&worker_mailbox);
            if !state.stop || result.is_err() {
                state.worker_stopped = true;
            }
            worker_mailbox.wake.notify_all();
        });
    if spawned.is_err() {
        let mut state = lock_projection_mailbox(&mailbox);
        state.worker_stopped = true;
    }
}

fn projection_worker_loop(mailbox: Arc<ProjectionMailbox>) {
    let mut context = None;
    let mut generation = 0;
    let mut position: Option<(String, u64)> = None;
    let mut next_delay = None;
    let mut backoff = ProjectionBackoff::new();

    loop {
        match wait_for_projection_wake(&mailbox, next_delay) {
            ProjectionWake::Stop => return,
            ProjectionWake::Context(request) => {
                generation = request.generation;
                context = request.context;
                backoff.reset();
            }
            ProjectionWake::Deadline => {
                if let (Some(context), Some((epoch, sequence))) = (&context, &position)
                    && probe_projection_events(context, epoch, *sequence)
                        == ProjectionProbeDecision::Quiet
                {
                    backoff.advance();
                    next_delay = Some(backoff.delay);
                    continue;
                }
            }
        }

        let snapshot = snapshot_for_context(context.clone());
        let next_position = snapshot
            .connected_server
            .as_ref()
            .map(|server| (server.epoch.clone(), server.sequence));
        match publish_projection_update(&mailbox, generation, snapshot) {
            ProjectionPublishResult::Stop => return,
            ProjectionPublishResult::Superseded => {
                next_delay = None;
                continue;
            }
            ProjectionPublishResult::Published => {}
        }
        position = next_position;
        if position.is_some() {
            backoff.reset();
        } else {
            backoff.advance();
        }
        next_delay = Some(backoff.delay);
    }
}

fn wait_for_projection_wake(
    mailbox: &ProjectionMailbox,
    timeout: Option<Duration>,
) -> ProjectionWake {
    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let mut state = lock_projection_mailbox(mailbox);
    loop {
        if state.stop {
            return ProjectionWake::Stop;
        }
        if let Some(request) = state.request.take() {
            return ProjectionWake::Context(request);
        }
        let Some(deadline) = deadline else {
            state = mailbox
                .wake
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            continue;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return ProjectionWake::Deadline;
        }
        let (next_state, _) = mailbox
            .wake
            .wait_timeout(state, remaining)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state = next_state;
    }
}

fn publish_projection_update(
    mailbox: &ProjectionMailbox,
    generation: u64,
    snapshot: SnapshotDocument,
) -> ProjectionPublishResult {
    let mut state = lock_projection_mailbox(mailbox);
    if state.stop {
        return ProjectionPublishResult::Stop;
    }
    if state
        .request
        .as_ref()
        .is_some_and(|request| request.generation > generation)
    {
        return ProjectionPublishResult::Superseded;
    }
    state.update = Some(ProjectionUpdate {
        generation,
        snapshot,
    });
    ProjectionPublishResult::Published
}

fn probe_projection_events(
    context: &ServerContext,
    epoch: &str,
    after: u64,
) -> ProjectionProbeDecision {
    let response = crate::client::send_ipc_request_to_timeout(
        &context.endpoint,
        vec![
            "read-events".to_owned(),
            "--epoch".to_owned(),
            epoch.to_owned(),
            "--after".to_owned(),
            after.to_string(),
            "--limit".to_owned(),
            "1".to_owned(),
        ],
        PROJECTION_IPC_TIMEOUT,
    );
    match response {
        Ok(response) => classify_projection_event_response(&response, epoch, after),
        Err(_) => ProjectionProbeDecision::Refresh,
    }
}

fn classify_projection_event_response(
    response: &crate::protocol::IpcResponse,
    epoch: &str,
    after: u64,
) -> ProjectionProbeDecision {
    if !response.ok {
        return ProjectionProbeDecision::Refresh;
    }
    let Ok(batch) = serde_json::from_str::<Value>(&response.output) else {
        return ProjectionProbeDecision::Refresh;
    };
    let same_epoch = batch["position"]["epoch"].as_str() == Some(epoch);
    let sequence = batch["position"]["sequence"].as_u64();
    let events_are_empty = batch["events"].as_array().is_some_and(Vec::is_empty);
    if same_epoch && sequence == Some(after) && events_are_empty {
        ProjectionProbeDecision::Quiet
    } else {
        ProjectionProbeDecision::Refresh
    }
}

fn projection_worker_unavailable_snapshot() -> SnapshotDocument {
    let mut snapshot = disconnected_snapshot();
    snapshot.server_state = "unavailable";
    snapshot.server_reason = Some("projection_worker_unavailable".to_owned());
    snapshot.server_detail = Some("background projection worker stopped".to_owned());
    snapshot.views[0].reason = "projection_worker_unavailable".to_owned();
    snapshot
}

/// Start or reuse the isolated Control Center without blocking the GUI thread.
pub(crate) fn open_control_center(no_activate: bool, server_endpoint: &str) -> Result<()> {
    let executable = control_center_executable()?;
    let mut command = Command::new(&executable);
    let instance = env::var("AGENTERM_INSTANCE")
        .ok()
        .filter(|instance| !instance.trim().is_empty());
    command.args(control_center_launch_arguments(
        no_activate,
        server_endpoint,
        instance.as_deref(),
    )?);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .with_context(|| format!("failed to launch {}", executable.display()))?;
    Ok(())
}

fn control_center_launch_arguments(
    no_activate: bool,
    server_endpoint: &str,
    logical_instance: Option<&str>,
) -> Result<Vec<OsString>> {
    let server_endpoint = canonical_endpoint(server_endpoint)?;
    let mut arguments = vec![
        OsString::from("open"),
        OsString::from("--server-endpoint"),
        OsString::from(server_endpoint.to_string()),
    ];
    if let Some(instance) = logical_instance {
        validate_context_value("logical instance", instance)?;
        arguments.push(OsString::from("--logical-instance"));
        arguments.push(OsString::from(instance));
    }
    if no_activate {
        arguments.push(OsString::from("--no-activate"));
    }
    Ok(arguments)
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
                    | "--endpoint"
                    | "--instance"
                    | "--server-endpoint"
                    | "--logical-instance"
                    | "--output"
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
    let mut selectors = EndpointSelectorArgs::default();
    let mut migration_endpoint = None;
    let mut migration_instance = None;
    let mut screenshot_output = None;
    let mut position = 0;
    while position < values.len() {
        match values[position].as_ref() {
            "--endpoint" | "--instance" | "--server-endpoint" | "--logical-instance"
            | "--output" => {
                let option = values[position].as_ref();
                let Some(value) = values.get(position + 1) else {
                    return Err(format!("{option} requires a value"));
                };
                if option == "--output" {
                    if value.is_empty() {
                        return Err("--output requires a non-empty path".to_owned());
                    }
                    if screenshot_output
                        .replace(PathBuf::from(value.as_ref()))
                        .is_some()
                    {
                        return Err("--output may be specified only once".to_owned());
                    }
                    position += 2;
                    continue;
                }
                validate_context_value(option.trim_start_matches('-'), value)
                    .map_err(|error| error.to_string())?;
                match option {
                    "--endpoint" => {
                        if selectors.endpoint.replace(value.to_string()).is_some() {
                            return Err(
                                "endpoint_selector_conflict: an endpoint selector may be specified only once"
                                    .to_owned(),
                            );
                        }
                    }
                    "--instance" => {
                        if selectors.instance.replace(value.to_string()).is_some() {
                            return Err(
                                "endpoint_selector_conflict: an instance selector may be specified only once"
                                    .to_owned(),
                            );
                        }
                    }
                    "--server-endpoint" => {
                        let value = canonical_endpoint(value)
                            .map_err(|error| format!("invalid {option}: {error:#}"))?
                            .to_string();
                        if migration_endpoint.replace(value).is_some() {
                            return Err(
                                "endpoint_selector_conflict: --server-endpoint may be specified only once"
                                    .to_owned(),
                            );
                        }
                    }
                    "--logical-instance" => {
                        if migration_instance.replace(value.to_string()).is_some() {
                            return Err(
                                "endpoint_selector_conflict: --logical-instance may be specified only once"
                                    .to_owned(),
                            );
                        }
                    }
                    _ => unreachable!("selector option was matched above"),
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
    let has_canonical_selector = selectors.endpoint.is_some() || selectors.instance.is_some();
    let has_migration_selector = migration_endpoint.is_some() || migration_instance.is_some();
    if has_canonical_selector && has_migration_selector {
        return Err(
            "endpoint_selector_conflict: canonical --endpoint/--instance selectors cannot be mixed with migration aliases"
                .to_owned(),
        );
    }
    let context = if let Some(endpoint) = migration_endpoint {
        Some(ServerContext {
            endpoint,
            logical_instance: migration_instance,
        })
    } else if let Some(instance) = migration_instance {
        resolve_selector_context(EndpointSelectorArgs {
            instance: Some(instance),
            ..EndpointSelectorArgs::default()
        })?
    } else if has_canonical_selector {
        resolve_selector_context(selectors)?
    } else {
        None
    };

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
    if screenshot_output.is_some() && positional.as_slice() != ["screenshot"] {
        return Err("--output is valid only for screenshot".to_owned());
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
        ["screenshot"]
            if !explicit_no_activate && context.is_none() && screenshot_output.is_some() =>
        {
            Ok(EntryCommand::Screenshot {
                json,
                output: screenshot_output.expect("guarded above"),
            })
        }
        ["screenshot"] if screenshot_output.is_none() => {
            Err("screenshot requires --output PATH".to_owned())
        }
        ["screenshot"] if context.is_some() => {
            Err("screenshot targets the exact live Control Center registry owner; endpoint selectors are not valid".to_owned())
        }
        ["screenshot"] => Err("--no-activate is valid only for open".to_owned()),
        [] | ["open"] => {
            Err("--json is valid only for capabilities, snapshot, or screenshot".to_owned())
        }
        ["capabilities"] | ["status"] | ["snapshot"] | ["close"] => {
            Err("--no-activate is valid only for open".to_owned())
        }
        [other, ..] => Err(format!("unknown command: {other}")),
    }
}

fn resolve_selector_context(
    selectors: EndpointSelectorArgs,
) -> std::result::Result<Option<ServerContext>, String> {
    resolve_ipc_endpoint(&selectors)
        .map(|resolved| {
            Some(ServerContext {
                endpoint: resolved.endpoint.to_string(),
                logical_instance: Some(resolved.logical_instance.canonical_name()),
            })
        })
        .map_err(|error| format!("endpoint_selector_error: {error}"))
}

fn canonical_endpoint(value: &str) -> Result<IpcEndpoint> {
    let endpoint = value
        .parse::<IpcEndpoint>()
        .or_else(|_| IpcEndpoint::from_legacy_address(value))
        .map_err(anyhow::Error::new)
        .context("server endpoint must be unix:<path>, pipe:<name>, tcp:<host>:<port>, or a legacy loopback HOST:PORT")?;
    endpoint
        .validate_local()
        .map_err(anyhow::Error::new)
        .context("server endpoint must identify a local IPC transport")?;
    Ok(endpoint)
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
        EntryCommand::Screenshot { json, output } => {
            let document = capture_control_center_screenshot(&output)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&document)?);
            } else {
                println!("{}", document.output);
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
        screenshot: crate::platform::control_center::screenshot_capability(),
        views: ["cockpit", "workflows", "extensions", "info_hub"],
    }
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        anyhow::bail!("control_center_screenshot_invalid_png");
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("four-byte PNG width"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("four-byte PNG height"));
    if width == 0 || height == 0 {
        anyhow::bail!("control_center_screenshot_invalid_dimensions");
    }
    Ok((width, height))
}

fn capture_control_center_screenshot(output: &Path) -> Result<ScreenshotDocument> {
    match crate::platform::control_center::screenshot_strategy() {
        crate::platform::control_center::ScreenshotStrategy::DirectNativeWindow => {
            capture_direct_native_screenshot(output)
        }
        crate::platform::control_center::ScreenshotStrategy::RendererRequest => {
            capture_renderer_requested_screenshot(output)
        }
        crate::platform::control_center::ScreenshotStrategy::Unsupported => anyhow::bail!(
            "control_center_screenshot_unsupported: native Control Center screenshot capture is unavailable on this platform"
        ),
    }
}

fn capture_direct_native_screenshot(output: &Path) -> Result<ScreenshotDocument> {
    let registry = registry_path();
    let owner = read_registry(&registry)
        .filter(registry_process_matches)
        .context("control_center_screenshot_not_running")?;
    let native_window = read_regular_file(&native_window_path(&registry))
        .context("control_center_screenshot_window_unavailable")?;
    let native_window = String::from_utf8(native_window)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .context("control_center_screenshot_window_invalid")?;
    let output = if output.is_absolute() {
        output.to_owned()
    } else {
        env::current_dir()
            .context("control_center_screenshot_current_directory_unavailable")?
            .join(output)
    };
    crate::platform::control_center::capture_native_window_png(native_window, &output)
        .map_err(|error| anyhow::anyhow!("control_center_screenshot_capture_failed: {error}"))?;

    let still_exact_owner = read_registry(&registry).is_some_and(|current| {
        current.pid == owner.pid
            && current.process_start_identity == owner.process_start_identity
            && registry_process_matches(&current)
    });
    if !still_exact_owner {
        let _ = fs::remove_file(&output);
        anyhow::bail!("control_center_screenshot_owner_changed");
    }
    let bytes = fs::read(&output).context("control_center_screenshot_readback_failed")?;
    let (width, height) = png_dimensions(&bytes)?;
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(ScreenshotDocument {
        schema_version: SCHEMA_VERSION,
        executable: "agenterm-cc",
        state: "captured",
        renderer: "native",
        owner_pid: owner.pid,
        output: output.to_string_lossy().into_owned(),
        width,
        height,
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        sha256,
        rendered_snapshot: None,
    })
}

fn capture_renderer_requested_screenshot(output: &Path) -> Result<ScreenshotDocument> {
    let registry = registry_path();
    let owner = read_registry(&registry)
        .filter(registry_process_matches)
        .context("control_center_screenshot_not_running")?;
    let output = if output.is_absolute() {
        output.to_owned()
    } else {
        env::current_dir()
            .context("control_center_screenshot_current_directory_unavailable")?
            .join(output)
    };
    if fs::symlink_metadata(&output).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        anyhow::bail!("control_center_screenshot_output_symlink");
    }
    let request_id = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    );
    let request = ScreenshotRequest {
        schema_version: SCHEMA_VERSION,
        owner_pid: owner.pid,
        process_start_identity: owner.process_start_identity.clone(),
        request_id: request_id.clone(),
        output: output.clone(),
    };
    let request_path = screenshot_request_path(&registry);
    let result_path = screenshot_result_path(&registry);
    let _ = fs::remove_file(&result_path);
    write_private_atomic(&request_path, &serde_json::to_vec_pretty(&request)?)?;

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let result = loop {
        if !read_registry(&registry).is_some_and(|current| {
            current.pid == owner.pid
                && current.process_start_identity == owner.process_start_identity
                && registry_process_matches(&current)
        }) {
            let _ = fs::remove_file(&request_path);
            let _ = fs::remove_file(&output);
            anyhow::bail!("control_center_screenshot_owner_changed");
        }
        if let Ok(bytes) = read_regular_file(&result_path)
            && let Ok(result) = serde_json::from_slice::<RendererCaptureResult>(&bytes)
            && result.owner_pid == owner.pid
            && result.process_start_identity == owner.process_start_identity
            && result.request_id == request_id
        {
            break result;
        }
        if std::time::Instant::now() >= deadline {
            let _ = fs::remove_file(&request_path);
            let _ = fs::remove_file(&output);
            anyhow::bail!("control_center_screenshot_renderer_timeout");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = fs::remove_file(&request_path);
    let _ = fs::remove_file(&result_path);
    if let Some(error) = result.error {
        let _ = fs::remove_file(&output);
        anyhow::bail!("control_center_screenshot_capture_failed: {error}");
    }
    if result.output != output {
        let _ = fs::remove_file(&output);
        anyhow::bail!("control_center_screenshot_result_mismatch");
    }
    let snapshot = result
        .snapshot
        .context("control_center_screenshot_snapshot_missing")?;
    let bytes = fs::read(&output).context("control_center_screenshot_readback_failed")?;
    let (width, height) = png_dimensions(&bytes)?;
    if width != snapshot.physical_width || height != snapshot.physical_height {
        let _ = fs::remove_file(&output);
        anyhow::bail!("control_center_screenshot_dimensions_mismatch");
    }
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(ScreenshotDocument {
        schema_version: SCHEMA_VERSION,
        executable: "agenterm-cc",
        state: "captured",
        renderer: "native",
        owner_pid: owner.pid,
        output: output.to_string_lossy().into_owned(),
        width,
        height,
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        sha256,
        rendered_snapshot: Some(snapshot),
    })
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
    Ok(current.with_file_name(crate::platform::paths::control_center_executable_name()))
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
    read_context(&registry_path())
}

fn read_context(registry: &Path) -> Option<ServerContext> {
    read_regular_file(&context_path(registry))
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
        let options = crate::platform::control_center::private_create_new_options();
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
    let result = crate::platform::control_center::replace_file(&temporary, path)
        .map_err(anyhow::Error::from);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn status_document() -> StatusDocument {
    status_document_at(&registry_path())
}

fn status_document_at(path: &Path) -> StatusDocument {
    let (state, record) = match inspect_registry(path) {
        RegistryInspection::Compatible(record) if registry_process_matches(&record) => {
            ("running", Some(record))
        }
        RegistryInspection::Incompatible(record) if registry_process_matches(&record) => {
            ("registry_incompatible", Some(record))
        }
        RegistryInspection::Publishing => ("starting", None),
        RegistryInspection::Unparseable => ("registry_unparseable", None),
        RegistryInspection::Missing
        | RegistryInspection::Compatible(_)
        | RegistryInspection::Incompatible(_) => ("not_running", None),
    };
    let pid = record.as_ref().map(|record| record.pid);
    let context = if state == "running" {
        read_context(path)
    } else {
        None
    };
    StatusDocument {
        schema_version: SCHEMA_VERSION,
        executable: "agenterm-cc",
        state,
        pid,
        context,
    }
}

fn close_control_center() -> CloseDocument {
    let path = registry_path();
    close_control_center_at(&path)
}

fn close_control_center_at(path: &Path) -> CloseDocument {
    let record = match inspect_registry(path) {
        RegistryInspection::Compatible(record) => record,
        RegistryInspection::Incompatible(record) if registry_process_matches(&record) => {
            return CloseDocument {
                schema_version: SCHEMA_VERSION,
                executable: "agenterm-cc",
                state: "registry_incompatible",
                pid: Some(record.pid),
            };
        }
        RegistryInspection::Incompatible(record) => {
            recover_stale_registry(path);
            return CloseDocument {
                schema_version: SCHEMA_VERSION,
                executable: "agenterm-cc",
                state: "stale_recovered",
                pid: Some(record.pid),
            };
        }
        RegistryInspection::Publishing => {
            return CloseDocument {
                schema_version: SCHEMA_VERSION,
                executable: "agenterm-cc",
                state: "starting",
                pid: None,
            };
        }
        RegistryInspection::Unparseable => {
            return CloseDocument {
                schema_version: SCHEMA_VERSION,
                executable: "agenterm-cc",
                state: "registry_unparseable",
                pid: None,
            };
        }
        RegistryInspection::Missing => {
            recover_stale_registry(path);
            return CloseDocument {
                schema_version: SCHEMA_VERSION,
                executable: "agenterm-cc",
                state: "not_running",
                pid: None,
            };
        }
    };
    if !registry_process_matches(&record) {
        recover_stale_registry(path);
        return CloseDocument {
            schema_version: SCHEMA_VERSION,
            executable: "agenterm-cc",
            state: "stale_recovered",
            pid: Some(record.pid),
        };
    }
    if write_private_atomic(
        &close_request_path(path),
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
        if !read_registry(path).is_some_and(|current| {
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
    let _ = fs::remove_file(screenshot_request_path(path));
    let _ = fs::remove_file(screenshot_result_path(path));
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
        || detail.contains("invalid response")
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
    let tabs: Vec<TabSummary> = bootstrap["tabs"]
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
    let tab_counts = TabCounts::from_tabs(&tabs);
    Ok(ConnectedServer {
        endpoint: context.endpoint.clone(),
        logical_instance: context.logical_instance.clone(),
        pid,
        epoch,
        sequence,
        version: protocol["agenterm_version"].as_str().map(str::to_owned),
        build: protocol["build_identity"].clone(),
        active_tab_id: bootstrap["active_tab_id"].as_str().map(str::to_owned),
        tab_counts,
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
    crate::platform::paths::settings_path(None)
}

fn process_start_identity(pid: u32) -> Option<String> {
    crate::platform::process::start_identity(pid).ok()
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
        if parent
            .file_name()
            .is_some_and(|name| name == "control-center")
        {
            crate::platform::control_center::protect_state_directory(parent)?;
        }
    }
    for _ in 0..2 {
        let options = crate::platform::control_center::private_create_new_options();
        match options.open(path) {
            Ok(mut file) => {
                let _ = fs::remove_file(native_window_path(path));
                let _ = fs::remove_file(focus_request_path(path));
                let _ = fs::remove_file(close_request_path(path));
                let _ = fs::remove_file(screenshot_request_path(path));
                let _ = fs::remove_file(screenshot_result_path(path));
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
                match inspect_registry(path) {
                    RegistryInspection::Compatible(record) => {
                        if registry_process_matches(&record) {
                            return Ok(RegistryClaim::Existing(record));
                        }
                        recover_stale_registry(path);
                    }
                    RegistryInspection::Incompatible(record) => {
                        if registry_process_matches(&record) {
                            anyhow::bail!(
                                "{REGISTRY_INCOMPATIBLE_LIVE}: schema_version={} owner_pid={}",
                                record.schema_version,
                                record.pid
                            );
                        }
                        recover_stale_registry(path);
                    }
                    RegistryInspection::Publishing => {
                        // Another process is still publishing its create-new
                        // record. Reuse the claim without launching a second
                        // process; the focus request is safe to retry.
                        return Ok(RegistryClaim::Existing(RegistryRecord {
                            schema_version: REGISTRY_SCHEMA_VERSION,
                            pid: 0,
                            process_start_identity: String::new(),
                        }));
                    }
                    RegistryInspection::Unparseable => {
                        anyhow::bail!(
                            "{REGISTRY_UNPARSEABLE}: refusing to replace an owner whose identity cannot be verified"
                        );
                    }
                    RegistryInspection::Missing => {}
                }
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
    match inspect_registry(path) {
        RegistryInspection::Compatible(record) => Some(record),
        RegistryInspection::Missing
        | RegistryInspection::Publishing
        | RegistryInspection::Incompatible(_)
        | RegistryInspection::Unparseable => None,
    }
}

fn inspect_registry(path: &Path) -> RegistryInspection {
    let content = match read_regular_file(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return RegistryInspection::Missing;
        }
        Err(_) => return RegistryInspection::Unparseable,
    };
    if content.is_empty() && registry_is_fresh(path) {
        return RegistryInspection::Publishing;
    }
    match serde_json::from_slice::<RegistryRecord>(&content) {
        Ok(record) if record.schema_version == REGISTRY_SCHEMA_VERSION => {
            RegistryInspection::Compatible(record)
        }
        Ok(record) => RegistryInspection::Incompatible(record),
        Err(_) => RegistryInspection::Unparseable,
    }
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

fn screenshot_request_path(path: &Path) -> PathBuf {
    path.with_extension("screenshot-request.json")
}

fn screenshot_result_path(path: &Path) -> PathBuf {
    path.with_extension("screenshot-result.json")
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

fn focus_existing(_record: &RegistryRecord, registry_path: &Path, no_activate: bool) {
    let native_window = read_regular_file(&native_window_path(registry_path))
        .ok()
        .and_then(|value| String::from_utf8(value).ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default();
    request_projection_refresh(registry_path, no_activate);
    crate::platform::control_center::focus_existing_window(native_window, no_activate);
}

struct ProductShellHost {
    owner: RegistryOwner,
    projection: ShellProjection,
    focus_request: PathBuf,
    last_focus_request: Option<String>,
    screenshot_request: PathBuf,
    screenshot_result: PathBuf,
    last_screenshot_request: Option<String>,
}

impl ProductShellHost {
    fn new(owner: RegistryOwner) -> Self {
        Self {
            projection: ShellProjection::new(&owner.path),
            focus_request: focus_request_path(&owner.path),
            last_focus_request: None,
            screenshot_request: screenshot_request_path(&owner.path),
            screenshot_result: screenshot_result_path(&owner.path),
            last_screenshot_request: None,
            owner,
        }
    }
}

impl crate::platform::services::control_center_shell::ControlCenterShellHost for ProductShellHost {
    fn title(&self) -> String {
        self.projection.title()
    }
    fn lines(&self) -> Vec<String> {
        self.projection.lines()
    }
    fn poll(&mut self) -> bool {
        self.projection.poll()
    }
    fn close_requested(&self) -> bool {
        self.projection.close_requested()
    }

    fn publish_native_window(
        &mut self,
        raw_handle: i64,
    ) -> crate::platform::services::control_center_shell::ControlCenterShellResult<()> {
        self.owner
            .publish_native_window(raw_handle)
            .map_err(|error| {
                crate::platform::services::control_center_shell::ControlCenterShellError::failed(
                    "control_center_native_window_publish_failed",
                    error,
                )
            })
    }

    fn take_focus_request(
        &mut self,
    ) -> Option<crate::platform::services::control_center_shell::ControlCenterFocusRequest> {
        let request = read_regular_file(&self.focus_request)
            .ok()
            .and_then(|value| String::from_utf8(value).ok());
        if request.is_none() || request == self.last_focus_request {
            return None;
        }
        self.last_focus_request = request;
        if self
            .last_focus_request
            .as_deref()
            .is_some_and(|request| request.starts_with("no-activate:"))
        {
            Some(crate::platform::services::control_center_shell::ControlCenterFocusRequest::NoActivate)
        } else {
            Some(crate::platform::services::control_center_shell::ControlCenterFocusRequest::Activate)
        }
    }

    fn capture_requested_screenshot(
        &mut self,
        frame: Option<crate::platform::services::control_center_shell::ControlCenterFrame<'_>>,
    ) -> crate::platform::services::control_center_shell::ControlCenterShellResult<()> {
        let Ok(bytes) = read_regular_file(&self.screenshot_request) else {
            return Ok(());
        };
        let Ok(request) = serde_json::from_slice::<ScreenshotRequest>(&bytes) else {
            return Ok(());
        };
        let Some(frame) = frame.filter(|frame| !frame.pixels.is_empty()) else {
            return Ok(());
        };
        if self.last_screenshot_request.as_deref() == Some(request.request_id.as_str())
            || request.schema_version != SCHEMA_VERSION
            || request.owner_pid != self.owner.pid
            || request.process_start_identity != self.owner.process_start_identity
        {
            return Ok(());
        }
        self.last_screenshot_request = Some(request.request_id.clone());

        let server = self.projection.snapshot.connected_server.as_ref();
        let snapshot = RendererSnapshot {
            schema_version: SCHEMA_VERSION,
            owner_pid: self.owner.pid,
            renderer: "native".to_owned(),
            selected_view: "cockpit".to_owned(),
            server_state: self.projection.snapshot.server_state.to_owned(),
            server_reason: self.projection.snapshot.server_reason.clone(),
            server_endpoint: server.map(|server| server.endpoint.clone()),
            logical_instance: server.and_then(|server| server.logical_instance.clone()),
            window_title: self.projection.title(),
            physical_width: frame.width,
            physical_height: frame.height,
            scale_factor: frame.scale_factor,
        };
        let error = crate::platform::services::ui_screenshot::write_xrgb_png(
            crate::platform::contract::ui_screenshot::XrgbFrame {
                path: &request.output,
                width: frame.width,
                height: frame.height,
                pixels: frame.pixels,
                clip: None,
            },
        )
        .err()
        .map(|error| error.message());
        let result = RendererCaptureResult {
            schema_version: SCHEMA_VERSION,
            owner_pid: self.owner.pid,
            process_start_identity: self.owner.process_start_identity.clone(),
            request_id: request.request_id,
            output: request.output,
            snapshot: error.is_none().then_some(snapshot),
            error,
        };
        write_private_atomic(
            &self.screenshot_result,
            &serde_json::to_vec_pretty(&result).unwrap_or_default(),
        )
        .map_err(|error| {
            crate::platform::services::control_center_shell::ControlCenterShellError::failed(
                "control_center_screenshot_result_write_failed",
                error,
            )
        })
    }
}

fn platform_shell(owner: RegistryOwner, no_activate: bool) -> Result<()> {
    crate::platform::services::control_center_shell::run_native_shell(
        Box::new(ProductShellHost::new(owner)),
        no_activate,
    )
    .map_err(anyhow::Error::new)
}
#[cfg(test)]
mod tests {
    use super::*;

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
    fn canonical_instance_selector_resolves_a_typed_server_context() {
        let command = parse_entry(&[
            OsString::from("snapshot"),
            OsString::from("--instance"),
            OsString::from("dev"),
            OsString::from("--json"),
        ])
        .expect("resolve dev instance");
        let EntryCommand::Snapshot {
            context: Some(context),
            ..
        } = command
        else {
            panic!("instance selector must produce a snapshot server context");
        };
        assert_eq!(context.logical_instance.as_deref(), Some("dev"));
        assert!(
            context.endpoint.starts_with("pipe:")
                || context.endpoint.starts_with("unix:")
                || context.endpoint.starts_with("tcp:"),
            "resolved endpoint must retain its typed transport: {}",
            context.endpoint
        );
    }

    #[test]
    fn endpoint_selectors_reject_conflicts_and_duplicates_before_opening() {
        let conflict = parse_entry(&[
            OsString::from("snapshot"),
            OsString::from("--endpoint"),
            OsString::from("tcp:127.0.0.1:42001"),
            OsString::from("--instance"),
            OsString::from("dev"),
        ])
        .expect_err("endpoint and instance are mutually exclusive");
        assert!(conflict.contains("endpoint_selector_error"));
        assert!(conflict.contains("ConflictingCliSelectors"));

        let duplicate = parse_entry(&[
            OsString::from("snapshot"),
            OsString::from("--instance"),
            OsString::from("main"),
            OsString::from("--logical-instance"),
            OsString::from("dev"),
        ])
        .expect_err("canonical and migration spellings are one selector");
        assert!(duplicate.contains("endpoint_selector_conflict"));
    }

    #[test]
    fn migration_endpoint_alias_normalizes_legacy_loopback_addresses() {
        let command = parse_entry(&[
            OsString::from("snapshot"),
            OsString::from("--server-endpoint"),
            OsString::from("127.0.0.1:42002"),
            OsString::from("--logical-instance"),
            OsString::from("dev"),
        ])
        .expect("resolve migration endpoint alias");
        let EntryCommand::Snapshot {
            context: Some(context),
            ..
        } = command
        else {
            panic!("endpoint alias must produce a snapshot server context");
        };
        assert_eq!(context.endpoint, "tcp:127.0.0.1:42002");
        assert_eq!(context.logical_instance.as_deref(), Some("dev"));
    }

    #[test]
    fn toolbar_launch_preserves_exact_endpoint_and_inherited_dev_context() {
        let arguments = control_center_launch_arguments(true, "127.0.0.1:42004", Some("dev"))
            .expect("build toolbar launch arguments");
        assert_eq!(
            arguments,
            [
                "open",
                "--server-endpoint",
                "tcp:127.0.0.1:42004",
                "--logical-instance",
                "dev",
                "--no-activate",
            ]
            .map(OsString::from)
        );
        let parsed = parse_entry(&arguments).expect("parse toolbar launch arguments");
        let EntryCommand::Open {
            no_activate: true,
            context: Some(context),
        } = parsed
        else {
            panic!("toolbar launch must remain a no-activate connected context");
        };
        assert_eq!(context.endpoint, "tcp:127.0.0.1:42004");
        assert_eq!(context.logical_instance.as_deref(), Some("dev"));
    }

    #[test]
    fn canonical_and_migration_selector_groups_cannot_be_mixed() {
        let error = parse_entry(&[
            OsString::from("snapshot"),
            OsString::from("--endpoint"),
            OsString::from("tcp:127.0.0.1:42003"),
            OsString::from("--logical-instance"),
            OsString::from("dev"),
        ])
        .expect_err("public endpoint and migration context must not mix");
        assert!(error.contains("endpoint_selector_conflict"));
        assert!(error.contains("cannot be mixed"));
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
    fn screenshot_requires_one_output_and_rejects_authority_selectors() {
        let command = parse_entry(&[
            OsString::from("screenshot"),
            OsString::from("--output"),
            OsString::from("cockpit.png"),
            OsString::from("--json"),
        ])
        .expect("parse screenshot command");
        assert_eq!(
            command,
            EntryCommand::Screenshot {
                json: true,
                output: PathBuf::from("cockpit.png"),
            }
        );
        assert!(
            parse_entry(&[OsString::from("screenshot")])
                .expect_err("missing output must fail")
                .contains("requires --output")
        );
        assert!(
            parse_entry(&[
                OsString::from("screenshot"),
                OsString::from("--output"),
                OsString::from("cockpit.png"),
                OsString::from("--instance"),
                OsString::from("main"),
            ])
            .expect_err("screenshot must target the registry owner")
            .contains("exact live Control Center registry owner")
        );
        assert!(
            parse_entry(&[
                OsString::from("status"),
                OsString::from("--output"),
                OsString::from("ignored.png"),
            ])
            .expect_err("output is screenshot-only")
            .contains("valid only for screenshot")
        );
    }

    #[test]
    fn screenshot_png_header_dimensions_are_strict() {
        let mut header = Vec::from(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".as_slice());
        header.extend_from_slice(&760_u32.to_be_bytes());
        header.extend_from_slice(&480_u32.to_be_bytes());
        assert_eq!(png_dimensions(&header).unwrap(), (760, 480));
        assert!(png_dimensions(b"not a png").is_err());
        header[16..20].copy_from_slice(&0_u32.to_be_bytes());
        assert!(png_dimensions(&header).is_err());
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
    fn cockpit_lines_project_causal_tab_health_and_component_facts() {
        let tabs = vec![
            TabSummary {
                id: "@1".to_owned(),
                index: 0,
                title: "build".to_owned(),
                note: String::new(),
                process_id: Some(41),
                dead: false,
            },
            TabSummary {
                id: "@2".to_owned(),
                index: 1,
                title: "finished".to_owned(),
                note: String::new(),
                process_id: Some(42),
                dead: true,
            },
        ];
        let server = ConnectedServer {
            endpoint: "pipe:test".to_owned(),
            logical_instance: Some("dev".to_owned()),
            pid: 123,
            epoch: "epoch-0123456789abcdef".to_owned(),
            sequence: 77,
            version: Some("0.1.12".to_owned()),
            build: serde_json::json!({
                "git_commit": "abcdef0123456789",
                "profile": "dev",
                "git_dirty": "clean"
            }),
            active_tab_id: Some("@1".to_owned()),
            tab_counts: TabCounts::from_tabs(&tabs),
            tabs,
            components: ComponentAvailability {
                server: "available",
                workflows: "unavailable",
                extensions: "unavailable",
                info_hub: "unavailable",
            },
        };

        let lines = connected_cockpit_lines(&server);
        assert_eq!(lines.len(), 6);
        assert!(lines[0].contains("dev · PID 123 · v0.1.12"));
        assert!(lines[1].contains("abcdef012345… · dev · clean"));
        assert!(lines[2].contains("epoch epoch-012345… · sequence 77"));
        assert!(lines[3].contains("2 tabs · 1 running · 1 dead"));
        assert!(lines[4].contains("@1 · build"));
        assert!(lines[5].contains("server available"));
        assert!(lines[5].contains("workflows unavailable"));
    }

    #[test]
    fn projection_backoff_is_bounded_and_resets_after_causal_progress() {
        let mut backoff = ProjectionBackoff::new();
        assert_eq!(backoff.delay, PROJECTION_RETRY_MIN);
        for _ in 0..32 {
            backoff.advance();
        }
        assert_eq!(backoff.delay, PROJECTION_RETRY_MAX);
        backoff.reset();
        assert_eq!(backoff.delay, PROJECTION_RETRY_MIN);
    }

    #[test]
    fn projection_event_probe_refreshes_on_change_restart_gap_or_invalid_data() {
        let quiet = crate::protocol::IpcResponse::success(
            serde_json::json!({
                "position": {"epoch": "epoch-a", "sequence": 7},
                "events": [],
            })
            .to_string(),
        );
        assert_eq!(
            classify_projection_event_response(&quiet, "epoch-a", 7),
            ProjectionProbeDecision::Quiet
        );

        let changed = crate::protocol::IpcResponse::success(
            serde_json::json!({
                "position": {"epoch": "epoch-a", "sequence": 8},
                "events": [{"sequence": 8, "kind": "tab.created"}],
            })
            .to_string(),
        );
        assert_eq!(
            classify_projection_event_response(&changed, "epoch-a", 7),
            ProjectionProbeDecision::Refresh
        );

        for code in ["server_restart", "journal_gap"] {
            let failure = crate::protocol::IpcResponse::typed_failure(
                format!("{{\"code\":\"{code}\"}}"),
                code,
                "precondition",
                false,
            );
            assert_eq!(
                classify_projection_event_response(&failure, "epoch-a", 7),
                ProjectionProbeDecision::Refresh
            );
        }
        assert_eq!(
            classify_projection_event_response(
                &crate::protocol::IpcResponse::success("not-json"),
                "epoch-a",
                7
            ),
            ProjectionProbeDecision::Refresh
        );
    }

    #[test]
    fn projection_mailbox_rejects_late_generation_and_keeps_one_latest_update() {
        let mailbox = ProjectionMailbox {
            state: Mutex::new(ProjectionMailboxState {
                request: Some(ProjectionRequest {
                    generation: 2,
                    context: None,
                }),
                ..ProjectionMailboxState::default()
            }),
            wake: Condvar::new(),
        };
        assert_eq!(
            publish_projection_update(&mailbox, 1, disconnected_snapshot()),
            ProjectionPublishResult::Superseded
        );
        assert!(lock_projection_mailbox(&mailbox).update.is_none());

        {
            let mut state = lock_projection_mailbox(&mailbox);
            state.request = None;
        }
        assert_eq!(
            publish_projection_update(&mailbox, 2, disconnected_snapshot()),
            ProjectionPublishResult::Published
        );
        assert_eq!(
            publish_projection_update(&mailbox, 2, projection_worker_unavailable_snapshot()),
            ProjectionPublishResult::Published
        );
        let update = lock_projection_mailbox(&mailbox)
            .update
            .take()
            .expect("latest projection update");
        assert_eq!(update.generation, 2);
        assert_eq!(
            update.snapshot.server_reason.as_deref(),
            Some("projection_worker_unavailable")
        );
    }

    #[test]
    fn stopped_projection_worker_downgrades_live_state_to_typed_unavailable() {
        let snapshot = projection_worker_unavailable_snapshot();
        assert_eq!(snapshot.server_state, "unavailable");
        assert!(snapshot.connected_server.is_none());
        assert_eq!(
            snapshot.server_reason.as_deref(),
            Some("projection_worker_unavailable")
        );
        assert_eq!(snapshot.views[0].state, "unavailable");
        assert_eq!(snapshot.views[0].reason, "projection_worker_unavailable");
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
    fn unparseable_registry_fails_closed_without_replacing_or_deleting_it() {
        let path = env::temp_dir().join(format!(
            "agenterm-cc-corrupt-registry-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time after epoch")
                .as_nanos()
        ));
        fs::write(&path, b"{corrupt").expect("write corrupt registry");
        let error = match claim_registry(&path) {
            Err(error) => error,
            Ok(_) => panic!("unparseable registry must fail closed"),
        };
        assert!(error.to_string().contains(REGISTRY_UNPARSEABLE));
        assert_eq!(fs::read(&path).expect("preserved registry"), b"{corrupt");
        assert_eq!(status_document_at(&path).state, "registry_unparseable");
        assert_eq!(close_control_center_at(&path).state, "registry_unparseable");
        assert_eq!(fs::read(&path).expect("registry after close"), b"{corrupt");
        fs::remove_file(&path).expect("remove test registry");
    }

    #[test]
    fn live_incompatible_registry_fails_closed_and_close_preserves_it() {
        let path = env::temp_dir().join(format!(
            "agenterm-cc-incompatible-registry-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time after epoch")
                .as_nanos()
        ));
        let incompatible = RegistryRecord {
            schema_version: REGISTRY_SCHEMA_VERSION + 1,
            pid: std::process::id(),
            process_start_identity: process_start_identity(std::process::id())
                .expect("current process identity"),
        };
        let bytes = serde_json::to_vec(&incompatible).expect("registry JSON");
        fs::write(&path, &bytes).expect("write incompatible registry");

        let error = match claim_registry(&path) {
            Err(error) => error,
            Ok(_) => panic!("live incompatible registry must fail closed"),
        };
        assert!(error.to_string().contains(REGISTRY_INCOMPATIBLE_LIVE));
        assert_eq!(fs::read(&path).expect("preserved registry"), bytes);

        assert_eq!(status_document_at(&path).state, "registry_incompatible");
        let close = close_control_center_at(&path);
        assert_eq!(close.state, "registry_incompatible");
        assert_eq!(fs::read(&path).expect("registry after close"), bytes);
        fs::remove_file(&path).expect("remove test registry");
    }

    #[test]
    fn stale_incompatible_registry_is_recovered() {
        let path = env::temp_dir().join(format!(
            "agenterm-cc-stale-incompatible-registry-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time after epoch")
                .as_nanos()
        ));
        let incompatible = RegistryRecord {
            schema_version: REGISTRY_SCHEMA_VERSION + 1,
            pid: std::process::id(),
            process_start_identity: "foreign-start-identity".to_owned(),
        };
        fs::write(
            &path,
            serde_json::to_vec(&incompatible).expect("registry JSON"),
        )
        .expect("write incompatible registry");
        let owner = match claim_registry(&path).expect("recover stale incompatible registry") {
            RegistryClaim::Owner(owner) => owner,
            RegistryClaim::Existing(_) => panic!("stale incompatible registry was reused"),
        };
        assert_ne!(
            owner.process_start_identity,
            incompatible.process_start_identity
        );
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
    fn malformed_sibling_protocol_is_incompatible_not_unreachable() {
        assert_eq!(
            server_failure_reason(
                "server bootstrap unavailable: invalid response from AgenTerm server"
            ),
            "server_incompatible"
        );
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
