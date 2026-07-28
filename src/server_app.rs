use std::{
    collections::HashSet,
    env,
    sync::{Arc, mpsc::Receiver},
    thread,
    time::{Duration, SystemTime},
};

use anyhow::{Context as _, Result};

use crate::{
    control_dispatch::{ControlHost, dispatch_shared_command, resolve_target_position},
    event_journal::{EventJournal, EventKind},
    instances::{InstanceRegistration, register_instance},
    ipc_transport::{IpcEnvelope, start_ipc_server},
    operations::validate_operation_args,
    protocol::{IpcRequest, IpcResponse},
    pty::TerminalSize,
    terminal_observation::TerminalProcessState,
    terminal_runtime::{TerminalLaunch, TerminalTab},
    wake_signal::WakeSignal,
    workspace::{SavedTab, SavedWorkspace, load_workspace, save_workspace, workspace_path},
};

const INITIAL_ROWS: u16 = 30;
const INITIAL_COLUMNS: u16 = 100;
const IPC_REQUESTS_PER_TICK: usize = 16;
const SERVER_TICK: Duration = Duration::from_millis(5);

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
        let mut state = Self {
            tabs: Vec::new(),
            collapsed_tabs: restored.collapsed_ids.into_iter().collect(),
            active: restored.active_id,
            next_id,
            session_name,
            started_at: SystemTime::now(),
            event_journal: EventJournal::new(),
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

    fn execute_request(&mut self, request: IpcRequest) -> IpcResponse {
        if let Err(error) = validate_operation_args(&request.args) {
            return IpcResponse::typed_failure(
                error,
                "operation_invalid_arguments",
                "validation",
                false,
            );
        }
        if let Some(response) = dispatch_shared_command(self, &request.args) {
            return response;
        }
        match request.args.first().map(String::as_str) {
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
        for tab in &mut self.tabs {
            let before = tab.observation();
            let cwd_before = tab.cwd.clone();
            let proxy_before = tab.proxy.facts();
            tab.poll();
            let after = tab.observation();
            match before.delta_to(&after) {
                Ok(delta) => {
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
        Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "projection": "headless_server",
                "server_pid": std::process::id(),
                "event_position": position,
                "active_tab_id": self.active.map(|id| format!("@{id}")),
                "tabs": self.tabs.iter().map(|tab| serde_json::json!({
                    "id": format!("@{}", tab.id),
                    "index": tab.index,
                    "parent_id": tab.parent_id.map(|id| format!("@{id}")),
                    "name": tab.title,
                    "note": tab.note,
                    "active": self.active == Some(tab.id),
                    "pid": tab.process_id,
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
    use super::configure_server_launch;

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
}
