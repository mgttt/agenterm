use std::{
    collections::HashSet,
    env,
    sync::{Arc, mpsc::Receiver},
    thread,
    time::{Duration, SystemTime},
};

use anyhow::{Context as _, Result};

use crate::{
    UpgradeIdentity,
    commands::{has_option, option_value},
    control_authority::{
        ControlAdmission, ControlAuthority, control_event_position, resolved_control_target,
        submission_wait,
    },
    control_dispatch::{ControlHost, dispatch_shared_command, resolve_target_position},
    event_journal::{EventJournal, EventKind},
    instances::{InstanceRegistration, instance_process_is_alive, register_instance},
    ipc_transport::{IpcEnvelope, start_ipc_server},
    operations::{
        UI_TABS_HIDE, UI_TABS_SET_WIDTH, UI_TABS_SHOW, UI_TABS_TOGGLE, validate_operation_args,
    },
    protocol::{IpcRequest, IpcResponse},
    pty::TerminalSize,
    terminal_observation::TerminalProcessState,
    terminal_runtime::{TerminalLaunch, TerminalTab},
    ui_bridge::{
        UI_BUILD_IDENTITY_MAX_BYTES, UI_CLIENT_STATE_MAX_BYTES, UI_CLIENT_STATE_SCHEMA_VERSION,
        UI_INTERACTION_SCHEMA_VERSION, UI_LEASE_SCHEMA_VERSION, UiEventPosition, UiLeaseGrant,
    },
    ui_command::{
        UI_CLIENT_COMMAND_MAX_ARGUMENTS, UI_CLIENT_COMMAND_MAX_BYTES,
        UI_CLIENT_COMMAND_SCHEMA_VERSION, UiClientCommandQueue, UiClientCommandResult,
    },
    ui_interaction::{UiInteraction, parse_ui_interaction},
    ui_lease::{UI_LEASE_TTL_MS, UiLeaseAuthority, UiLeaseError, UiLeaseRecord},
    wake_signal::WakeSignal,
    working_context::{CwdSource, cwd_command, validate_path},
    workspace::{SavedTab, SavedWorkspace, load_workspace, save_workspace, workspace_path},
};

const INITIAL_ROWS: u16 = 30;
const INITIAL_COLUMNS: u16 = 100;
const IPC_REQUESTS_PER_TICK: usize = 16;
const SERVER_TICK: Duration = Duration::from_millis(5);

fn validate_ui_client_snapshot(
    json: &str,
    client_pid: u32,
    server_pid: u32,
    server_epoch: &str,
    current_sequence: u64,
) -> Result<(), String> {
    let byte_len = json.len();
    if byte_len == 0 || byte_len > UI_CLIENT_STATE_MAX_BYTES {
        return Err(format!(
            "UI client snapshot must contain 1..={UI_CLIENT_STATE_MAX_BYTES} bytes"
        ));
    }
    let value = serde_json::from_str::<serde_json::Value>(json)
        .map_err(|error| format!("UI client snapshot is not valid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "UI client snapshot must be a JSON object".to_owned())?;
    if object
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(u64::from(UI_CLIENT_STATE_SCHEMA_VERSION))
    {
        return Err(format!(
            "UI client snapshot schema_version must be {UI_CLIENT_STATE_SCHEMA_VERSION}"
        ));
    }
    if object.get("projection").and_then(serde_json::Value::as_str) != Some("replaceable_ui_client")
    {
        return Err("UI client snapshot projection must be replaceable_ui_client".to_owned());
    }
    if object.get("client_pid").and_then(serde_json::Value::as_u64) != Some(u64::from(client_pid)) {
        return Err("UI client snapshot client_pid does not match the lease owner".to_owned());
    }
    if object.get("server_pid").and_then(serde_json::Value::as_u64) != Some(u64::from(server_pid)) {
        return Err("UI client snapshot server_pid does not match this server".to_owned());
    }
    let event_position = object
        .get("event_position")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "UI client snapshot requires event_position".to_owned())?;
    if event_position
        .get("epoch")
        .and_then(serde_json::Value::as_str)
        != Some(server_epoch)
    {
        return Err(
            "UI client snapshot event_position epoch does not match this server".to_owned(),
        );
    }
    let sequence = event_position
        .get("sequence")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "UI client snapshot event_position requires numeric sequence".to_owned())?;
    if sequence > current_sequence {
        return Err(format!(
            "UI client snapshot sequence {sequence} is ahead of server sequence {current_sequence}"
        ));
    }
    if !object.get("tabs").is_some_and(serde_json::Value::is_array) {
        return Err("UI client snapshot requires a tabs array".to_owned());
    }
    Ok(())
}

struct UiClientSnapshotRecord {
    lease_id: String,
    client_pid: u32,
    json: String,
}

pub fn run_server_entry() -> i32 {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if let Err(error) = configure_server_launch(&arguments) {
        eprintln!("AgenTerm server argument error: {error:#}");
        return 2;
    }
    match run_server() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("AgenTerm server failed: {error:#}");
            1
        }
    }
}

fn configure_server_launch(arguments: &[String]) -> Result<()> {
    let mut address = None;
    let mut position = 0;
    while position < arguments.len() {
        match arguments[position].as_str() {
            "--address" => {
                if address.is_some() {
                    anyhow::bail!("agenterm-server.exe --address may be specified only once");
                }
                let value = arguments
                    .get(position + 1)
                    .context("agenterm-server.exe --address requires HOST:PORT")?;
                crate::client::parse_loopback_ipc_address(value)?;
                address = Some(value.clone());
                position += 2;
            }
            argument => anyhow::bail!("unsupported AgenTerm server argument: {argument}"),
        }
    }
    crate::client::set_ipc_address_override(address);
    Ok(())
}

fn run_server() -> Result<()> {
    let mut server = ServerState::new()?;
    while !server.shutdown_requested {
        server.drain();
        thread::sleep(SERVER_TICK);
    }
    server.persist_workspace()?;
    Ok(())
}

struct ServerState {
    tabs: Vec<TerminalTab>,
    collapsed_tabs: HashSet<u64>,
    active: Option<u64>,
    next_id: u64,
    session_name: String,
    started_at: SystemTime,
    event_journal: EventJournal,
    control_authority: ControlAuthority,
    ui_lease: UiLeaseAuthority,
    ui_client_snapshot: Option<UiClientSnapshotRecord>,
    ui_client_commands: UiClientCommandQueue,
    shutdown_after_ui_result: Option<String>,
    wake_signal: Arc<WakeSignal>,
    ipc_receiver: Receiver<IpcEnvelope>,
    shutdown_requested: bool,
    _instance_registration: InstanceRegistration,
}

impl ServerState {
    fn new() -> Result<Self> {
        let wake_signal = Arc::new(WakeSignal::new());
        let ipc_receiver = start_ipc_server(0, Arc::clone(&wake_signal))?;
        let restored = load_workspace().unwrap_or_else(default_workspace);
        let session_name = if restored.session_name.is_empty() {
            "agenterm".to_owned()
        } else {
            restored.session_name
        };
        let next_id = restored
            .tabs
            .iter()
            .map(|tab| tab.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let instance_registration =
            register_instance(&crate::ipc_address(), &workspace_path(), &session_name)?;
        let event_journal = EventJournal::new();
        let command_identity = event_journal.position().epoch;
        let mut state = Self {
            tabs: Vec::new(),
            collapsed_tabs: restored.collapsed_ids.into_iter().collect(),
            active: restored.active_id,
            next_id,
            session_name,
            started_at: SystemTime::now(),
            event_journal,
            control_authority: ControlAuthority::default(),
            ui_lease: UiLeaseAuthority::default(),
            ui_client_snapshot: None,
            ui_client_commands: UiClientCommandQueue::new(command_identity),
            shutdown_after_ui_result: None,
            wake_signal,
            ipc_receiver,
            shutdown_requested: false,
            _instance_registration: instance_registration,
        };
        for saved in restored.tabs {
            state.restore_tab(saved)?;
        }
        if state.tabs.is_empty() {
            state
                .create_tab(None, Vec::new(), Vec::new(), true, None)
                .map_err(anyhow::Error::msg)?;
        } else if state
            .active
            .is_none_or(|id| !state.tabs.iter().any(|tab| tab.id == id))
        {
            state.active = state.tabs.first().map(|tab| tab.id);
        }
        Ok(state)
    }

    fn restore_tab(&mut self, saved: SavedTab) -> Result<()> {
        let id = saved.id;
        let index = saved.index;
        let mut tab = TerminalTab::spawn(TerminalLaunch {
            id,
            index,
            parent_id: saved.parent_id,
            title: (!saved.title.is_empty()).then_some(saved.title),
            command_line: saved.command_line,
            tab_environment: Vec::new(),
            session_name: self.session_name.clone(),
            window: 0,
            wake_signal: Arc::clone(&self.wake_signal),
            initial_size: TerminalSize {
                rows: INITIAL_ROWS,
                cols: INITIAL_COLUMNS,
            },
        })
        .with_context(|| format!("failed to restore terminal @{id}"))?;
        tab.note = saved.note;
        tab.composer = saved.composer;
        self.tabs.push(tab);
        self.tabs.sort_by_key(|tab| tab.index);
        self.event_journal.commit(
            EventKind::TabCreated,
            Some(id),
            serde_json::json!({
                "index": index,
                "restored": true,
                "selected": self.active == Some(id),
            }),
        );
        Ok(())
    }

    fn saved_workspace(&self) -> SavedWorkspace {
        SavedWorkspace {
            version: 1,
            session_name: self.session_name.clone(),
            active_id: self.active,
            collapsed_ids: self.collapsed_tabs.iter().copied().collect(),
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    id: tab.id,
                    index: tab.index,
                    parent_id: tab.parent_id,
                    title: tab.title.clone(),
                    note: tab.note.clone(),
                    composer: tab.composer.clone(),
                    command_line: tab.command_line.clone(),
                })
                .collect(),
        }
    }

    fn persist_workspace(&mut self) -> Result<()> {
        save_workspace(&self.saved_workspace())?;
        self.event_journal
            .commit(EventKind::WorkspaceSaved, None, serde_json::json!({}));
        Ok(())
    }

    fn reap_stale_ui_lease(&mut self, now_unix_ms: u64) {
        if let Some((record, reason)) = self
            .ui_lease
            .reap_stale(now_unix_ms, instance_process_is_alive)
        {
            self.ui_client_snapshot = None;
            self.ui_client_commands.clear_active();
            self.commit_ui_lease_event(&record, "detached", reason);
            self.commit_window_visibility(false, true, reason);
        }
    }

    fn commit_ui_lease_event(&mut self, record: &UiLeaseRecord, state: &str, reason: &str) {
        self.event_journal.commit(
            EventKind::UiLease,
            None,
            serde_json::json!({
                "state": state,
                "client_id": record.client_id,
                "client_pid": record.client_pid,
                "client_build": record.client_build,
                "reason": reason,
            }),
        );
    }

    fn commit_window_visibility(&mut self, visible: bool, detached: bool, reason: &str) {
        self.event_journal.commit(
            EventKind::WindowVisibility,
            None,
            serde_json::json!({
                "visible": visible,
                "detached": detached,
                "reason": reason,
            }),
        );
    }

    fn ui_lease_grant_json(&self, record: UiLeaseRecord) -> IpcResponse {
        let position = self.event_journal.position();
        let grant = UiLeaseGrant {
            schema_version: UI_LEASE_SCHEMA_VERSION,
            lease_id: record.lease_id,
            client_id: record.client_id,
            client_pid: record.client_pid,
            client_build: record.client_build,
            server_pid: std::process::id(),
            position: UiEventPosition {
                server_epoch: position.epoch,
                sequence: position.sequence,
            },
            expires_unix_ms: record.expires_unix_ms,
            ttl_ms: UI_LEASE_TTL_MS,
            observed_sequence: record.observed_sequence,
        };
        if let Err(error) = grant.validate() {
            return IpcResponse::typed_failure(error, "ui_lease_grant_invalid", "internal", false);
        }
        match serde_json::to_string_pretty(&grant) {
            Ok(json) => IpcResponse::success(json),
            Err(error) => IpcResponse::typed_failure(
                error.to_string(),
                "ui_lease_serialization_failed",
                "internal",
                false,
            ),
        }
    }

    fn ui_lease_failure(error: UiLeaseError) -> IpcResponse {
        IpcResponse::typed_failure(
            error.message(),
            error.code(),
            error.category(),
            error.retryable(),
        )
    }

    fn execute_ui_lease_command(&mut self, args: &[String]) -> IpcResponse {
        let action = args.get(1).map(String::as_str).unwrap_or_default();
        let now_unix_ms = crate::client::unix_time_ms();
        self.reap_stale_ui_lease(now_unix_ms);
        match action {
            "attach" => {
                let Some(client_id) = option_value(args, "--client-id") else {
                    return IpcResponse::typed_failure(
                        "ui-lease attach requires --client-id",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let Some(client_pid) =
                    option_value(args, "--client-pid").and_then(|value| value.parse::<u32>().ok())
                else {
                    return IpcResponse::typed_failure(
                        "ui-lease attach requires numeric --client-pid",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                if !instance_process_is_alive(client_pid) {
                    return Self::ui_lease_failure(UiLeaseError::InvalidClientPid);
                }
                let client_build = match option_value(args, "--client-build-json") {
                    Some(value) if value.len() > UI_BUILD_IDENTITY_MAX_BYTES => {
                        return IpcResponse::typed_failure(
                            "ui-lease attach client build identity exceeds its byte budget",
                            "ui_lease_client_build_invalid",
                            "validation",
                            false,
                        );
                    }
                    Some(value) => match serde_json::from_str::<UpgradeIdentity>(value) {
                        Ok(identity) => Some(identity),
                        Err(error) => {
                            return IpcResponse::typed_failure(
                                format!(
                                    "ui-lease attach client build identity is invalid: {error}"
                                ),
                                "ui_lease_client_build_invalid",
                                "validation",
                                false,
                            );
                        }
                    },
                    None => None,
                };
                match self
                    .ui_lease
                    .attach(client_id, client_pid, client_build, now_unix_ms)
                {
                    Ok((record, created)) => {
                        if created {
                            self.ui_client_snapshot = None;
                            self.ui_client_commands.clear_active();
                            self.commit_ui_lease_event(&record, "attached", "requested");
                            self.commit_window_visibility(true, false, "lease-attached");
                        }
                        self.ui_lease_grant_json(record)
                    }
                    Err(error) => Self::ui_lease_failure(error),
                }
            }
            "heartbeat" => {
                let Some(lease_id) = option_value(args, "--lease-id") else {
                    return IpcResponse::typed_failure(
                        "ui-lease heartbeat requires --lease-id",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let Some(client_pid) =
                    option_value(args, "--client-pid").and_then(|value| value.parse::<u32>().ok())
                else {
                    return IpcResponse::typed_failure(
                        "ui-lease heartbeat requires numeric --client-pid",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                match self.ui_lease.heartbeat(lease_id, client_pid, now_unix_ms) {
                    Ok(record) => self.ui_lease_grant_json(record),
                    Err(error) => Self::ui_lease_failure(error),
                }
            }
            "detach" => {
                let Some(lease_id) = option_value(args, "--lease-id") else {
                    return IpcResponse::typed_failure(
                        "ui-lease detach requires --lease-id",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let Some(client_pid) =
                    option_value(args, "--client-pid").and_then(|value| value.parse::<u32>().ok())
                else {
                    return IpcResponse::typed_failure(
                        "ui-lease detach requires numeric --client-pid",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                match self.ui_lease.detach(lease_id, client_pid) {
                    Ok(record) => {
                        self.ui_client_snapshot = None;
                        self.ui_client_commands.clear_active();
                        self.commit_ui_lease_event(&record, "detached", "requested");
                        self.commit_window_visibility(false, true, "detach");
                        let position = self.event_journal.position();
                        IpcResponse::success(
                            serde_json::json!({
                                "schema_version": UI_LEASE_SCHEMA_VERSION,
                                "detached": true,
                                "client_id": record.client_id,
                                "client_pid": record.client_pid,
                                "client_build": record.client_build,
                                "position": {
                                    "server_epoch": position.epoch,
                                    "sequence": position.sequence,
                                },
                            })
                            .to_string(),
                        )
                    }
                    Err(error) => Self::ui_lease_failure(error),
                }
            }
            "acknowledge" => {
                let Some(lease_id) = option_value(args, "--lease-id") else {
                    return IpcResponse::typed_failure(
                        "ui-lease acknowledge requires --lease-id",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let Some(client_pid) =
                    option_value(args, "--client-pid").and_then(|value| value.parse::<u32>().ok())
                else {
                    return IpcResponse::typed_failure(
                        "ui-lease acknowledge requires numeric --client-pid",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let Some(sequence) =
                    option_value(args, "--sequence").and_then(|value| value.parse::<u64>().ok())
                else {
                    return IpcResponse::typed_failure(
                        "ui-lease acknowledge requires numeric --sequence",
                        "ui_lease_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let current_sequence = self.event_journal.position().sequence;
                match self.ui_lease.acknowledge(
                    lease_id,
                    client_pid,
                    sequence,
                    current_sequence,
                    now_unix_ms,
                ) {
                    Ok(record) => self.ui_lease_grant_json(record),
                    Err(error) => Self::ui_lease_failure(error),
                }
            }
            "status" => {
                let position = self.event_journal.position();
                let active = self.ui_lease.active();
                IpcResponse::success(
                    serde_json::json!({
                        "schema_version": UI_LEASE_SCHEMA_VERSION,
                        "attached": active.is_some(),
                        "client_id": active.map(|record| record.client_id.as_str()),
                        "client_pid": active.map(|record| record.client_pid),
                        "client_build": active.and_then(|record| record.client_build.as_ref()),
                        "expires_unix_ms": active.map(|record| record.expires_unix_ms),
                        "observed_sequence": active.map(|record| record.observed_sequence),
                        "position": {
                            "server_epoch": position.epoch,
                            "sequence": position.sequence,
                        },
                    })
                    .to_string(),
                )
            }
            _ => IpcResponse::typed_failure(
                "ui-lease requires attach, heartbeat, acknowledge, detach, or status",
                "ui_lease_invalid_arguments",
                "validation",
                false,
            ),
        }
    }

    fn execute_ui_client_state_command(&mut self, args: &[String]) -> IpcResponse {
        if args.get(1).map(String::as_str) != Some("publish") {
            return IpcResponse::typed_failure(
                "ui-client-state requires publish",
                "ui_client_state_invalid_arguments",
                "validation",
                false,
            );
        }
        let Some(lease_id) = option_value(args, "--lease-id") else {
            return IpcResponse::typed_failure(
                "ui-client-state publish requires --lease-id",
                "ui_client_state_invalid_arguments",
                "validation",
                false,
            );
        };
        let Some(client_pid) =
            option_value(args, "--client-pid").and_then(|value| value.parse::<u32>().ok())
        else {
            return IpcResponse::typed_failure(
                "ui-client-state publish requires numeric --client-pid",
                "ui_client_state_invalid_arguments",
                "validation",
                false,
            );
        };
        if let Err(error) = self.ui_lease.verify_owner(lease_id, client_pid) {
            return Self::ui_lease_failure(error);
        }
        let Some(snapshot_json) = option_value(args, "--snapshot-json") else {
            return IpcResponse::typed_failure(
                "ui-client-state publish requires --snapshot-json",
                "ui_client_state_invalid_arguments",
                "validation",
                false,
            );
        };
        let position = self.event_journal.position();
        if let Err(error) = validate_ui_client_snapshot(
            snapshot_json,
            client_pid,
            std::process::id(),
            &position.epoch,
            position.sequence,
        ) {
            return IpcResponse::typed_failure(
                error,
                "ui_client_state_invalid",
                "validation",
                false,
            );
        }
        self.ui_client_snapshot = Some(UiClientSnapshotRecord {
            lease_id: lease_id.to_owned(),
            client_pid,
            json: snapshot_json.to_owned(),
        });
        IpcResponse::success(
            serde_json::json!({
                "schema_version": UI_CLIENT_STATE_SCHEMA_VERSION,
                "published": true,
                "client_pid": client_pid,
                "position": {
                    "server_epoch": position.epoch,
                    "sequence": position.sequence,
                },
            })
            .to_string(),
        )
    }

    fn execute_ui_client_command(&mut self, args: &[String]) -> IpcResponse {
        let action = args.get(1).map(String::as_str).unwrap_or_default();
        if action == "result" {
            let Some(command_id) = option_value(args, "--command-id") else {
                return IpcResponse::typed_failure(
                    "ui-client-command result requires --command-id",
                    "ui_client_command_invalid_arguments",
                    "validation",
                    false,
                );
            };
            let mut completed = false;
            let value = match self.ui_client_commands.result(command_id) {
                UiClientCommandResult::Pending => {
                    serde_json::json!({"state": "pending", "command_id": command_id})
                }
                UiClientCommandResult::InFlight => {
                    serde_json::json!({"state": "in_flight", "command_id": command_id})
                }
                UiClientCommandResult::Complete(response_json) => {
                    completed = true;
                    let response = serde_json::from_str::<serde_json::Value>(response_json)
                        .unwrap_or(serde_json::Value::Null);
                    serde_json::json!({
                        "state": "complete",
                        "command_id": command_id,
                        "response": response,
                    })
                }
                UiClientCommandResult::Unknown => {
                    return IpcResponse::typed_failure(
                        "UI client command is unknown or expired",
                        "ui_client_command_unknown",
                        "precondition",
                        false,
                    );
                }
            };
            if completed && self.shutdown_after_ui_result.as_deref() == Some(command_id) {
                self.shutdown_after_ui_result = None;
                self.shutdown_requested = true;
            }
            return IpcResponse::success(value.to_string());
        }

        let Some(lease_id) = option_value(args, "--lease-id") else {
            return IpcResponse::typed_failure(
                format!("ui-client-command {action} requires --lease-id"),
                "ui_client_command_invalid_arguments",
                "validation",
                false,
            );
        };
        let Some(client_pid) =
            option_value(args, "--client-pid").and_then(|value| value.parse::<u32>().ok())
        else {
            return IpcResponse::typed_failure(
                format!("ui-client-command {action} requires numeric --client-pid"),
                "ui_client_command_invalid_arguments",
                "validation",
                false,
            );
        };
        if let Err(error) = self.ui_lease.verify_owner(lease_id, client_pid) {
            return Self::ui_lease_failure(error);
        }

        match action {
            "poll" => IpcResponse::success(
                serde_json::json!({
                    "schema_version": UI_CLIENT_COMMAND_SCHEMA_VERSION,
                    "command": self.ui_client_commands.poll(),
                })
                .to_string(),
            ),
            "apply" => {
                let Some(command_id) = option_value(args, "--command-id") else {
                    return IpcResponse::typed_failure(
                        "ui-client-command apply requires --command-id",
                        "ui_client_command_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let Some(command) = self.ui_client_commands.in_flight(command_id).cloned() else {
                    return IpcResponse::typed_failure(
                        "UI client command is not in flight",
                        "ui_client_command_not_in_flight",
                        "precondition",
                        false,
                    );
                };
                dispatch_shared_command(self, &command.args).unwrap_or_else(|| {
                    IpcResponse::typed_failure(
                        "UI client command has no server-owned apply phase",
                        "ui_client_command_apply_unsupported",
                        "unsupported",
                        false,
                    )
                })
            }
            "invoke" => {
                let Some(args_json) = option_value(args, "--args-json") else {
                    return IpcResponse::typed_failure(
                        "ui-client-command invoke requires --args-json",
                        "ui_client_command_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                if args_json.len() > UI_CLIENT_COMMAND_MAX_BYTES {
                    return IpcResponse::typed_failure(
                        "ui-client-command invoke exceeds its byte budget",
                        "ui_client_command_invalid_arguments",
                        "validation",
                        false,
                    );
                }
                let invoked = match serde_json::from_str::<Vec<String>>(args_json) {
                    Ok(invoked)
                        if !invoked.is_empty()
                            && invoked.len() <= UI_CLIENT_COMMAND_MAX_ARGUMENTS
                            && invoked.first().is_some_and(|value| value == "ui-action") =>
                    {
                        invoked
                    }
                    _ => {
                        return IpcResponse::typed_failure(
                            "ui-client-command invoke requires bounded ui-action arguments",
                            "ui_client_command_invalid_arguments",
                            "validation",
                            false,
                        );
                    }
                };
                if let Err(error) = validate_operation_args(&invoked) {
                    return IpcResponse::typed_failure(
                        error,
                        "operation_invalid_arguments",
                        "validation",
                        false,
                    );
                }
                dispatch_shared_command(self, &invoked).unwrap_or_else(|| {
                    IpcResponse::typed_failure(
                        "UI client command has no server-owned invoke phase",
                        "ui_client_command_invoke_unsupported",
                        "unsupported",
                        false,
                    )
                })
            }
            "complete" => {
                let Some(command_id) = option_value(args, "--command-id") else {
                    return IpcResponse::typed_failure(
                        "ui-client-command complete requires --command-id",
                        "ui_client_command_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let Some(response_json) = option_value(args, "--response-json") else {
                    return IpcResponse::typed_failure(
                        "ui-client-command complete requires --response-json",
                        "ui_client_command_invalid_arguments",
                        "validation",
                        false,
                    );
                };
                let completed_command = self.ui_client_commands.in_flight(command_id).cloned();
                let mut response = match self
                    .ui_client_commands
                    .complete(command_id, response_json.to_owned())
                {
                    Ok(response) => response,
                    Err(error) => {
                        return IpcResponse::typed_failure(
                            error,
                            "ui_client_command_completion_invalid",
                            "validation",
                            false,
                        );
                    }
                };
                if response.ok
                    && let Some(command) = completed_command.as_ref()
                {
                    self.commit_client_ui_action_event(command, &response);
                }
                if has_option(args, "--detach") {
                    let record = match self.ui_lease.detach(lease_id, client_pid) {
                        Ok(record) => record,
                        Err(error) => return Self::ui_lease_failure(error),
                    };
                    self.ui_client_snapshot = None;
                    self.ui_client_commands.clear_active();
                    self.commit_ui_lease_event(&record, "detached", "requested");
                    self.commit_window_visibility(false, true, "detach");
                    let position = self.event_journal.position();
                    if response.ok
                        && let Ok(mut value) =
                            serde_json::from_str::<serde_json::Value>(&response.output)
                        && value["projection"].as_str() == Some("replaceable_ui_client")
                    {
                        value["event_position"]["epoch"] =
                            serde_json::Value::String(position.epoch);
                        value["event_position"]["sequence"] =
                            serde_json::Value::from(position.sequence);
                        response.output =
                            serde_json::to_string_pretty(&value).unwrap_or(response.output);
                    }
                    if let Err(error) = self
                        .ui_client_commands
                        .replace_completed(command_id, &response)
                    {
                        return IpcResponse::typed_failure(
                            error,
                            "ui_client_command_completion_invalid",
                            "internal",
                            false,
                        );
                    }
                }
                if has_option(args, "--shutdown-after-result") {
                    self.shutdown_after_ui_result = Some(command_id.to_owned());
                }
                if response.ok
                    && serde_json::from_str::<serde_json::Value>(&response.output)
                        .ok()
                        .and_then(|value| {
                            (value["projection"].as_str() == Some("replaceable_ui_client"))
                                .then_some(value)
                        })
                        .is_some()
                {
                    let position = self.event_journal.position();
                    if validate_ui_client_snapshot(
                        &response.output,
                        client_pid,
                        std::process::id(),
                        &position.epoch,
                        position.sequence,
                    )
                    .is_ok()
                    {
                        self.ui_client_snapshot = Some(UiClientSnapshotRecord {
                            lease_id: lease_id.to_owned(),
                            client_pid,
                            json: response.output.clone(),
                        });
                    }
                }
                IpcResponse::success(
                    serde_json::json!({
                        "schema_version": UI_CLIENT_COMMAND_SCHEMA_VERSION,
                        "completed": true,
                        "command_id": command_id,
                    })
                    .to_string(),
                )
            }
            _ => IpcResponse::typed_failure(
                "ui-client-command requires poll, apply, invoke, complete, or result",
                "ui_client_command_invalid_arguments",
                "validation",
                false,
            ),
        }
    }

    fn commit_client_ui_action_event(
        &mut self,
        command: &crate::ui_command::UiClientCommand,
        response: &IpcResponse,
    ) {
        let Some(action) = command.args.get(1).map(String::as_str) else {
            return;
        };
        let previous = self
            .ui_client_snapshot
            .as_ref()
            .and_then(|snapshot| serde_json::from_str::<serde_json::Value>(&snapshot.json).ok());
        let Ok(current) = serde_json::from_str::<serde_json::Value>(&response.output) else {
            return;
        };
        match action {
            "tabs-show" | "tabs-hide" | "tabs-toggle" | "toggle-tabs" => {
                let visible = current["layout"]["sidebar"]["visible"].as_bool();
                let previous_visible = previous
                    .as_ref()
                    .and_then(|value| value["layout"]["sidebar"]["visible"].as_bool());
                if visible.is_some() && visible != previous_visible {
                    let operation_id = match action {
                        "tabs-show" => UI_TABS_SHOW,
                        "tabs-hide" => UI_TABS_HIDE,
                        _ => UI_TABS_TOGGLE,
                    };
                    self.event_journal.commit(
                        EventKind::LayoutTabsVisibility,
                        None,
                        serde_json::json!({
                            "visible": visible,
                            "cause": "semantic",
                            "operation_id": operation_id,
                        }),
                    );
                }
            }
            "tabs-set-width" => {
                let width = current["layout"]["sidebar"]["configured_width"].as_u64();
                let previous_width = previous
                    .as_ref()
                    .and_then(|value| value["layout"]["sidebar"]["configured_width"].as_u64());
                if width.is_some() && width != previous_width {
                    self.event_journal.commit(
                        EventKind::LayoutTabsWidth,
                        None,
                        serde_json::json!({
                            "configured_width": width,
                            "effective_width":
                                current["layout"]["sidebar"]["effective_width"],
                            "cause": "semantic",
                            "operation_id": UI_TABS_SET_WIDTH,
                        }),
                    );
                }
            }
            _ => {}
        }
    }

    fn enqueue_ui_client_command(&mut self, args: &[String]) -> IpcResponse {
        self.reap_stale_ui_lease(crate::client::unix_time_ms());
        if self.ui_lease.active().is_none() {
            return IpcResponse::typed_failure(
                "no interactive GUI client is attached to this server",
                "ui_client_unavailable",
                "availability",
                true,
            );
        }
        let command_id = match self.ui_client_commands.enqueue(args.to_vec()) {
            Ok(command_id) => command_id,
            Err(error) => {
                return IpcResponse::typed_failure(
                    error,
                    "ui_client_command_queue_full",
                    "capacity",
                    true,
                );
            }
        };
        let position = self.event_journal.position();
        IpcResponse::success(
            serde_json::json!({
                "schema_version": UI_CLIENT_COMMAND_SCHEMA_VERSION,
                "relay": "ui_client",
                "queued": true,
                "command_id": command_id,
                "position": {
                    "server_epoch": position.epoch,
                    "sequence": position.sequence,
                },
            })
            .to_string(),
        )
    }

    fn execute_ui_interaction_command(&mut self, args: &[String]) -> IpcResponse {
        let interaction = match parse_ui_interaction(args) {
            Ok(interaction) => interaction,
            Err(error) => {
                return IpcResponse::typed_failure(
                    error,
                    "ui_interaction_invalid_arguments",
                    "validation",
                    false,
                );
            }
        };
        let now_unix_ms = crate::client::unix_time_ms();
        self.reap_stale_ui_lease(now_unix_ms);
        let (lease_id, client_pid) = interaction.lease_identity();
        let lease = match self.ui_lease.heartbeat(lease_id, client_pid, now_unix_ms) {
            Ok(lease) => lease,
            Err(error) => return Self::ui_lease_failure(error),
        };
        let tab_id = interaction.tab_id();
        let action = interaction.action();
        let Some(position) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return IpcResponse::typed_failure(
                format!("can't find UI interaction target: @{tab_id}"),
                "ui_interaction_target_not_found",
                "not_found",
                false,
            );
        };

        let (input_bytes, rows, columns) = match interaction {
            UiInteraction::Select { .. } => {
                if let Err(error) = self.select_tab_at(position) {
                    return IpcResponse::typed_failure(
                        error,
                        "ui_interaction_select_failed",
                        "precondition",
                        false,
                    );
                }
                (None, None, None)
            }
            UiInteraction::Input { bytes, .. } => {
                if self.active != Some(tab_id) {
                    return IpcResponse::typed_failure(
                        "UI input target is not the active tab",
                        "ui_interaction_target_not_active",
                        "conflict",
                        true,
                    );
                }
                if self.tabs[position].submission.is_pending() {
                    return IpcResponse::typed_failure(
                        "composer submission is pending; UI terminal input is paused",
                        "ui_interaction_submission_pending",
                        "conflict",
                        true,
                    );
                }
                let length = bytes.len();
                if !self.tabs[position].send(&bytes) {
                    return IpcResponse::typed_failure(
                        "terminal input was not accepted because the pane is no longer writable",
                        "terminal_not_writable",
                        "precondition",
                        false,
                    );
                }
                (Some(length), None, None)
            }
            UiInteraction::Resize { rows, columns, .. } => {
                self.tabs[position].resize(rows, columns);
                self.event_journal.commit(
                    EventKind::TerminalResized,
                    Some(tab_id),
                    serde_json::json!({
                        "rows": rows,
                        "columns": columns,
                        "source": "ui_lease",
                    }),
                );
                (None, Some(rows), Some(columns))
            }
        };
        let event_position = self.event_journal.position();
        IpcResponse::success(
            serde_json::json!({
                "schema_version": UI_INTERACTION_SCHEMA_VERSION,
                "action": action,
                "tab_id": format!("@{tab_id}"),
                "input_bytes": input_bytes,
                "rows": rows,
                "columns": columns,
                "lease_expires_unix_ms": lease.expires_unix_ms,
                "position": {
                    "server_epoch": event_position.epoch,
                    "sequence": event_position.sequence,
                },
            })
            .to_string(),
        )
    }

    fn execute_command(&mut self, args: &[String]) -> IpcResponse {
        if let Err(error) = validate_operation_args(args) {
            return IpcResponse::typed_failure(
                error,
                "operation_invalid_arguments",
                "validation",
                false,
            );
        }
        if args.first().is_some_and(|command| command == "ui-lease") {
            return self.execute_ui_lease_command(args);
        }
        if args
            .first()
            .is_some_and(|command| command == "ui-client-state")
        {
            return self.execute_ui_client_state_command(args);
        }
        if args
            .first()
            .is_some_and(|command| command == "ui-client-command")
        {
            return self.execute_ui_client_command(args);
        }
        if args.first().is_some_and(|command| {
            matches!(
                command.as_str(),
                "ui-action"
                    | "focus"
                    | "get-settings"
                    | "set-setting"
                    | "screenshot"
                    | "screenshot-pane"
                    | "screenshot-tab"
                    | "__focus"
                    | "__show-no-activate"
            )
        }) {
            return self.enqueue_ui_client_command(args);
        }
        if args.first().is_some_and(|command| command == "ui-interact") {
            return self.execute_ui_interaction_command(args);
        }
        if let Some(response) = dispatch_shared_command(self, args) {
            return response;
        }
        match args.first().map(String::as_str) {
            Some("save-workspace") => match self.persist_workspace() {
                Ok(()) => IpcResponse::success(workspace_path().display().to_string()),
                Err(error) => IpcResponse::typed_failure(
                    format!("{error:#}"),
                    "operation_persistence_failed",
                    "precondition",
                    false,
                ),
            },
            Some("shutdown") => {
                if let Err(error) = self.persist_workspace() {
                    return IpcResponse::typed_failure(
                        format!("{error:#}"),
                        "operation_persistence_failed",
                        "precondition",
                        false,
                    );
                }
                self.event_journal.commit(
                    EventKind::WorkspaceShutdown,
                    None,
                    serde_json::json!({"saved": true}),
                );
                self.shutdown_requested = true;
                IpcResponse::success("")
            }
            Some("__focus") | Some("__show-no-activate") => IpcResponse::typed_failure(
                "no interactive GUI client is attached to this server",
                "ui_client_unavailable",
                "availability",
                true,
            ),
            Some(command) => IpcResponse::typed_failure(
                format!("headless AgenTerm server does not implement `{command}`"),
                "server_command_unsupported",
                "unsupported",
                false,
            ),
            None => IpcResponse::failure("no command specified"),
        }
    }

    fn execute_request(&mut self, request: IpcRequest) -> IpcResponse {
        let IpcRequest { args, control } = request;
        let control =
            match self
                .control_authority
                .admit(control, &args, crate::client::unix_time_ms())
            {
                ControlAdmission::Uncontrolled => return self.execute_command(&args),
                ControlAdmission::Respond(response) => return *response,
                ControlAdmission::Execute(control) => control,
            };
        let before_position = control_event_position(self);
        let mut resolved = resolved_control_target(self, &args);
        let response = self.execute_command(&args);
        let after_position = control_event_position(self);
        if resolved.tab_id.is_none()
            && let Some(id) = response
                .output
                .trim()
                .strip_prefix('@')
                .and_then(|value| value.parse::<u64>().ok())
        {
            resolved.tab_id = Some(id);
        }
        let wait = submission_wait(self, &control, response.ok, &resolved, &after_position);
        self.control_authority.complete(
            control,
            response,
            resolved,
            before_position,
            after_position,
            wait,
        )
    }

    fn drain(&mut self) {
        self.wake_signal.begin_drain();
        self.poll_terminals();
        let envelopes = self
            .ipc_receiver
            .try_iter()
            .take(IPC_REQUESTS_PER_TICK)
            .collect::<Vec<_>>();
        let budget_exhausted = envelopes.len() == IPC_REQUESTS_PER_TICK;
        for envelope in envelopes {
            let response = self.execute_request(envelope.request);
            let _ = envelope.respond_to.send(response);
        }
        let _ = self.wake_signal.rearm_if(budget_exhausted);
    }

    fn poll_terminals(&mut self) {
        let mut events = Vec::new();
        let mut completed_submissions = Vec::new();
        for tab in &mut self.tabs {
            let before = tab.observation();
            let cwd_before = tab.cwd.clone();
            let proxy_before = tab.proxy.facts();
            tab.poll();
            let after = tab.observation();
            match before.delta_to(&after) {
                Ok(delta) => {
                    if delta.submission_finished {
                        completed_submissions.push((
                            tab.id,
                            after.submission_enter_written.unwrap_or(false),
                            after.finalized,
                        ));
                    }
                    if delta.output_advanced_by > 0 {
                        events.push((
                            EventKind::TerminalOutput,
                            tab.id,
                            serde_json::json!({
                                "output_bytes": after.output_bytes,
                                "advanced_by": delta.output_advanced_by,
                            }),
                        ));
                    }
                    if delta.process_state_changed || delta.lifecycle_changed {
                        let state = match after.process_state() {
                            TerminalProcessState::Running => "running",
                            TerminalProcessState::Exited { .. } => "dead",
                            TerminalProcessState::Error { .. } => "error",
                        };
                        events.push((
                            EventKind::TabState,
                            tab.id,
                            serde_json::json!({
                                "state": state,
                                "exit_code": after.exit_code,
                                "error": after.error,
                                "reader_closed": after.reader_closed,
                                "parser_drained": after.parser_drained,
                                "finalized": after.finalized,
                                "became_finalized": delta.became_finalized,
                            }),
                        ));
                    }
                }
                Err(error) => {
                    tab.error = Some(error.to_string());
                    events.push((
                        EventKind::TabState,
                        tab.id,
                        serde_json::json!({
                            "state": "error",
                            "error": tab.error,
                            "became_finalized": false,
                        }),
                    ));
                }
            }
            if tab.cwd != cwd_before {
                events.push((
                    EventKind::WorkingContextCwd,
                    tab.id,
                    serde_json::json!({
                        "path": tab.cwd.path(),
                        "source": tab.cwd.source().as_str(),
                        "pending": tab.cwd.pending(),
                    }),
                ));
            }
            let proxy_after = tab.proxy.facts();
            if proxy_after != proxy_before {
                events.push((
                    EventKind::WorkingContextProxyResolved,
                    tab.id,
                    serde_json::json!({
                        "configured": proxy_after.configured,
                        "source": proxy_after.source.as_str(),
                        "application_state": proxy_after.application_state.as_str(),
                        "request_pending": proxy_after.request_pending,
                    }),
                ));
            }
        }
        for (kind, tab_id, payload) in events {
            self.event_journal.commit(kind, Some(tab_id), payload);
        }
        for (tab_id, enter_written, terminal_finalized) in completed_submissions {
            if let Err(error) = self.control_authority.finish_submission(
                &mut self.event_journal,
                tab_id,
                enter_written,
                terminal_finalized,
            ) && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id)
            {
                tab.error = Some(format!("failed to finalize control receipt: {error}"));
            }
        }
    }

    fn close_tab(&mut self, id: u64) -> Result<bool, String> {
        let Some(position) = self.tabs.iter().position(|tab| tab.id == id) else {
            return Err(format!("can't find tab: @{id}"));
        };
        let parent_id = self.tabs[position].parent_id;
        let index = self.tabs[position].index;
        let exit_code = self.tabs[position].exited;
        let promoted_children = self
            .tabs
            .iter()
            .filter(|tab| tab.parent_id == Some(id))
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        for tab in &mut self.tabs {
            if tab.parent_id == Some(id) {
                tab.parent_id = parent_id;
            }
        }
        self.collapsed_tabs.remove(&id);
        let terminal_shutdown_complete = self.tabs[position].close_process();
        self.tabs.remove(position);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(position)
                .or_else(|| {
                    position
                        .checked_sub(1)
                        .and_then(|index| self.tabs.get(index))
                })
                .map(|tab| tab.id);
        }
        self.event_journal.commit(
            EventKind::TabClosed,
            Some(id),
            serde_json::json!({
                "index": index,
                "parent_id": parent_id,
                "exit_code": exit_code,
                "promoted_children": promoted_children,
                "active_id": self.active,
                "terminal_shutdown_complete": terminal_shutdown_complete,
            }),
        );
        Ok(terminal_shutdown_complete)
    }
}

impl ControlHost for ServerState {
    fn session_name(&self) -> &str {
        &self.session_name
    }

    fn started_at_unix_secs(&self) -> u64 {
        self.started_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default()
    }

    fn tabs(&self) -> &[TerminalTab] {
        &self.tabs
    }

    fn tabs_mut(&mut self) -> &mut Vec<TerminalTab> {
        &mut self.tabs
    }

    fn active_id(&self) -> Option<u64> {
        self.active
    }

    fn set_active_id(&mut self, id: Option<u64>) {
        self.active = id;
    }

    fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
    }

    fn ui_bridge_facts(&self) -> crate::ui_bridge::UiBridgeFacts {
        crate::ui_bridge::headless_server_facts()
    }

    fn set_session_name(&mut self, name: String) {
        self.session_name = name;
    }

    fn collapsed_tab_ids(&self) -> Vec<u64> {
        self.collapsed_tabs.iter().copied().collect()
    }

    fn toggle_tab_collapsed(&mut self, tab_id: u64) -> Result<(), String> {
        if !self.tabs.iter().any(|tab| tab.id == tab_id) {
            return Err(format!("can't find tab: @{tab_id}"));
        }
        if !self.tabs.iter().any(|tab| tab.parent_id == Some(tab_id)) {
            return Err("tab has no child nodes".to_owned());
        }
        let collapsed = if self.collapsed_tabs.remove(&tab_id) {
            false
        } else {
            self.collapsed_tabs.insert(tab_id);
            true
        };
        self.event_journal.commit(
            EventKind::LayoutTreeCollapse,
            Some(tab_id),
            serde_json::json!({ "collapsed": collapsed }),
        );
        Ok(())
    }

    fn prepare_cwd(&mut self, tab_id: u64, path: &str, mode: &str) -> Result<(), String> {
        validate_path(path).map_err(|error| format!("{error:#}"))?;
        let Some(position) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Err(format!("can't find tab: @{tab_id}"));
        };
        let command = cwd_command(self.tabs[position].shell_kind, path)
            .map_err(|error| format!("{error:#}"))?;
        let previous = self.tabs[position].composer.clone();
        let next = match mode {
            "empty-only" if !previous.is_empty() => {
                return Err(
                    "Composer already has a draft; explicitly choose append or replace".to_owned(),
                );
            }
            "empty-only" | "replace" => command,
            "append" if previous.is_empty() => command,
            "append" => format!("{previous}\r\n{command}"),
            _ => return Err(format!("unknown CWD composer mode: {mode}")),
        };
        self.tabs[position].composer = next;
        self.tabs[position]
            .cwd
            .request(path.to_owned())
            .map_err(|error| error.to_string())?;
        self.event_journal.commit(
            EventKind::WorkingContextCwdRequested,
            Some(tab_id),
            serde_json::json!({
                "path": path,
                "source": CwdSource::UserRequested.as_str(),
                "pending": true,
                "disposition": "prepared",
                "composer_mode": mode,
            }),
        );
        self.event_journal.commit(
            EventKind::ComposerDraft,
            Some(tab_id),
            serde_json::json!({
                "length": self.tabs[position].composer.chars().count(),
            }),
        );
        Ok(())
    }

    fn send_cwd_now(&mut self, tab_id: u64, path: &str) -> Result<(), String> {
        validate_path(path).map_err(|error| format!("{error:#}"))?;
        let Some(position) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Err(format!("can't find tab: @{tab_id}"));
        };
        let shell = self.tabs[position].shell_kind;
        let command = cwd_command(shell, path).map_err(|error| format!("{error:#}"))?;
        if !self.tabs[position].submit(&command) {
            return Err("terminal is unavailable or already has a pending submission".to_owned());
        }
        self.tabs[position]
            .cwd
            .request(path.to_owned())
            .map_err(|error| error.to_string())?;
        self.event_journal.commit(
            EventKind::WorkingContextCwdRequested,
            Some(tab_id),
            serde_json::json!({
                "path": path,
                "source": CwdSource::UserRequested.as_str(),
                "pending": true,
                "disposition": "sent",
                "shell": shell.as_str(),
            }),
        );
        Ok(())
    }

    fn create_tab(
        &mut self,
        title: Option<String>,
        command_line: Vec<String>,
        tab_environment: Vec<(String, String)>,
        select: bool,
        parent_id: Option<u64>,
    ) -> Result<u32, String> {
        if parent_id.is_some_and(|parent| !self.tabs.iter().any(|tab| tab.id == parent)) {
            return Err(format!(
                "can't find parent tab: @{}",
                parent_id.unwrap_or_default()
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let index = (0..)
            .find(|candidate| !self.tabs.iter().any(|tab| tab.index == *candidate))
            .unwrap_or(self.tabs.len() as u32);
        let (rows, cols) = self
            .active
            .and_then(|active| self.tabs.iter().find(|tab| tab.id == active))
            .or_else(|| self.tabs.first())
            .map(|tab| tab.last_size)
            .unwrap_or((INITIAL_ROWS, INITIAL_COLUMNS));
        let tab = TerminalTab::spawn(TerminalLaunch {
            id,
            index,
            parent_id,
            title,
            command_line,
            tab_environment,
            session_name: self.session_name.clone(),
            window: 0,
            wake_signal: Arc::clone(&self.wake_signal),
            initial_size: TerminalSize { rows, cols },
        })
        .map_err(|error| format!("{error:#}"))?;
        self.tabs.push(tab);
        self.tabs.sort_by_key(|tab| tab.index);
        if let Some(parent_id) = parent_id
            && self.collapsed_tabs.remove(&parent_id)
        {
            self.event_journal.commit(
                EventKind::LayoutTreeCollapse,
                Some(parent_id),
                serde_json::json!({ "collapsed": false }),
            );
        }
        self.event_journal.commit(
            EventKind::TabCreated,
            Some(id),
            serde_json::json!({
                "index": index,
                "parent_id": parent_id,
                "selected": select,
            }),
        );
        if select {
            self.active = Some(id);
            self.event_journal
                .commit(EventKind::TabSelected, Some(id), serde_json::json!({}));
        }
        Ok(index)
    }

    fn select_tab_at(&mut self, position: usize) -> Result<(), String> {
        let Some(tab) = self.tabs.get(position) else {
            return Err("can't find window".to_owned());
        };
        self.active = Some(tab.id);
        self.event_journal
            .commit(EventKind::TabSelected, Some(tab.id), serde_json::json!({}));
        Ok(())
    }

    fn close_tab_id(&mut self, id: u64) -> Result<bool, String> {
        self.close_tab(id)
    }

    fn resolve_parent_id(&self, target: &str) -> Result<Option<u64>, String> {
        if matches!(target, "root" | "none" | "-") {
            return Ok(None);
        }
        let Some(position) = resolve_target_position(&self.tabs, self.active, Some(target)) else {
            return Err(format!("can't find parent tab: {target}"));
        };
        Ok(Some(self.tabs[position].id))
    }

    fn event_journal(&self) -> &EventJournal {
        &self.event_journal
    }

    fn event_journal_mut(&mut self) -> &mut EventJournal {
        &mut self.event_journal
    }

    fn ui_snapshot_json(&mut self) -> Option<String> {
        let position = self.event_journal.position();
        if let Some(snapshot) = &self.ui_client_snapshot
            && self.ui_lease.active().is_some_and(|lease| {
                lease.lease_id == snapshot.lease_id && lease.client_pid == snapshot.client_pid
            })
            && serde_json::from_str::<serde_json::Value>(&snapshot.json)
                .ok()
                .is_some_and(|value| {
                    value["event_position"]["epoch"].as_str() == Some(position.epoch.as_str())
                        && value["event_position"]["sequence"].as_u64() == Some(position.sequence)
                })
        {
            return Some(snapshot.json.clone());
        }
        Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "projection": "headless_server",
                "server_pid": std::process::id(),
                "event_position": position,
                "active_tab_id": self.active.map(|id| format!("@{id}")),
                "window": {
                    "title": serde_json::Value::Null,
                    "visible": false,
                    "detached": true,
                    "minimized": false,
                    "state": "detached",
                },
                "layout": {
                    "composer": {
                        "visible": false,
                        "input_visible": false,
                        "send_visible": false,
                    },
                },
                "focus": {
                    "surface": serde_json::Value::Null,
                    "window_id": self.active.map(|id| format!("@{id}")),
                },
                "modal": serde_json::Value::Null,
                "tabs": self.tabs.iter().map(|tab| serde_json::json!({
                    "id": format!("@{}", tab.id),
                    "index": tab.index,
                    "parent_id": tab.parent_id.map(|id| format!("@{id}")),
                    "collapsed": self.collapsed_tabs.contains(&tab.id),
                    "name": tab.title,
                    "note": tab.note,
                    "active": self.active == Some(tab.id),
                    "pid": tab.process_id,
                    "state": if tab.exited.is_some() { "dead" } else { "running" },
                    "dead": tab.exited.is_some(),
                    "exit_code": tab.exited,
                    "rows": tab.last_size.0,
                    "cols": tab.last_size.1,
                })).collect::<Vec<_>>(),
            }))
            .unwrap_or_default(),
        )
    }
}

impl Drop for ServerState {
    fn drop(&mut self) {
        let _ = save_workspace(&self.saved_workspace());
    }
}

fn default_workspace() -> SavedWorkspace {
    SavedWorkspace {
        version: 1,
        session_name: "agenterm".to_owned(),
        active_id: Some(1),
        collapsed_ids: Vec::new(),
        tabs: vec![SavedTab {
            id: 1,
            index: 0,
            ..SavedTab::default()
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::{configure_server_launch, validate_ui_client_snapshot};

    #[test]
    fn server_arguments_are_internal_bounded_and_loopback_only() {
        assert!(configure_server_launch(&[]).is_ok());
        assert!(
            configure_server_launch(&["--address".to_owned(), "127.0.0.1:48815".to_owned()])
                .is_ok()
        );
        assert!(
            configure_server_launch(&["--address".to_owned(), "0.0.0.0:48815".to_owned()]).is_err()
        );
        assert!(configure_server_launch(&["--unknown".to_owned()]).is_err());
    }

    #[test]
    fn ui_client_snapshot_is_bounded_causal_and_owned() {
        let valid = serde_json::json!({
            "schema_version": 1,
            "projection": "replaceable_ui_client",
            "client_pid": 42,
            "server_pid": 7,
            "event_position": {
                "epoch": "epoch-1",
                "sequence": 8,
            },
            "tabs": [],
        })
        .to_string();
        assert!(validate_ui_client_snapshot(&valid, 42, 7, "epoch-1", 9).is_ok());

        let mismatched_owner = valid.replace("\"client_pid\":42", "\"client_pid\":43");
        assert!(
            validate_ui_client_snapshot(&mismatched_owner, 42, 7, "epoch-1", 9)
                .unwrap_err()
                .contains("lease owner")
        );
        let future = valid.replace("\"sequence\":8", "\"sequence\":10");
        assert!(
            validate_ui_client_snapshot(&future, 42, 7, "epoch-1", 9)
                .unwrap_err()
                .contains("ahead")
        );
    }

    #[test]
    fn ui_client_snapshot_rejects_wrong_shape_and_oversize_payloads() {
        assert!(validate_ui_client_snapshot("[]", 42, 7, "epoch-1", 9).is_err());
        assert!(
            validate_ui_client_snapshot(
                &"x".repeat(crate::ui_bridge::UI_CLIENT_STATE_MAX_BYTES + 1),
                42,
                7,
                "epoch-1",
                9,
            )
            .unwrap_err()
            .contains("bytes")
        );
        let missing_tabs = serde_json::json!({
            "schema_version": 1,
            "projection": "replaceable_ui_client",
            "client_pid": 42,
            "server_pid": 7,
            "event_position": {
                "epoch": "epoch-1",
                "sequence": 8,
            },
        })
        .to_string();
        assert!(
            validate_ui_client_snapshot(&missing_tabs, 42, 7, "epoch-1", 9)
                .unwrap_err()
                .contains("tabs")
        );
    }
}
