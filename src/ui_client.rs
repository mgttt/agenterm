use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

use crate::{
    client::send_ipc_request,
    protocol::IpcResponse,
    ui_bridge::{
        UI_BRIDGE_PROTOCOL_VERSION, UI_CLIENT_STATE_SCHEMA_VERSION, UI_DELTA_MAX_EVENTS,
        UI_HELLO_SCHEMA_VERSION, UiBootstrapSnapshot, UiDeltaBatch, UiHelloRequest,
        UiHelloResponse, UiLeaseGrant, UiProtocolRange, UiTabBootstrap,
    },
    ui_command::UiClientCommand,
    upgrade_identity::UpgradeIdentity,
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const MAX_DELTA_ROUNDS: usize = 32;

pub(crate) struct UiClientModel {
    client_id: String,
    client_pid: u32,
    lease: UiLeaseGrant,
    snapshot: UiBootstrapSnapshot,
    supports_state_publication: bool,
    supports_client_commands: bool,
    last_heartbeat: Instant,
}

impl UiClientModel {
    pub(crate) fn connect(client_id: String) -> Result<Self> {
        let client_pid = std::process::id();
        let client_build = UpgradeIdentity::current(UI_BRIDGE_PROTOCOL_VERSION);
        let hello_request = UiHelloRequest {
            schema_version: UI_HELLO_SCHEMA_VERSION,
            client_id: client_id.clone(),
            protocol_range: UiProtocolRange {
                minimum: UI_BRIDGE_PROTOCOL_VERSION,
                maximum: UI_BRIDGE_PROTOCOL_VERSION,
            },
            client_build: Some(client_build.clone()),
        };
        hello_request.validate().map_err(anyhow::Error::msg)?;
        let client_build_json =
            serde_json::to_string(&client_build).context("could not encode UI build identity")?;
        let hello: UiHelloResponse = request_json(vec![
            "ui-hello".to_owned(),
            "--minimum".to_owned(),
            UI_BRIDGE_PROTOCOL_VERSION.to_string(),
            "--maximum".to_owned(),
            UI_BRIDGE_PROTOCOL_VERSION.to_string(),
            "--client-id".to_owned(),
            client_id.clone(),
            "--client-build-json".to_owned(),
            client_build_json.clone(),
        ])?;
        hello.validate().map_err(anyhow::Error::msg)?;
        if !hello.accepted
            || hello
                .client_build
                .as_ref()
                .is_some_and(|echoed| echoed != &client_build)
            || !hello
                .capabilities
                .iter()
                .any(|capability| capability == "interactive_lease")
            || !hello
                .capabilities
                .iter()
                .any(|capability| capability == "lease_gated_interaction")
        {
            anyhow::bail!("server does not expose the compatible interactive UI contract");
        }

        let lease: UiLeaseGrant = request_json(vec![
            "ui-lease".to_owned(),
            "attach".to_owned(),
            "--client-id".to_owned(),
            client_id.clone(),
            "--client-pid".to_owned(),
            client_pid.to_string(),
            "--client-build-json".to_owned(),
            client_build_json,
        ])?;
        lease.validate().map_err(anyhow::Error::msg)?;
        if lease.client_id != client_id
            || lease.client_pid != client_pid
            || lease
                .client_build
                .as_ref()
                .is_some_and(|echoed| echoed != &client_build)
            || lease.server_pid != hello.server_pid
            || lease.position.server_epoch != hello.position.server_epoch
        {
            anyhow::bail!("UI lease identity does not match the negotiated server");
        }

        let snapshot: UiBootstrapSnapshot = request_json(vec!["ui-bootstrap".to_owned()])?;
        snapshot.validate().map_err(anyhow::Error::msg)?;
        if snapshot.server_pid != hello.server_pid
            || snapshot.server_epoch != hello.position.server_epoch
            || snapshot.server_epoch != lease.position.server_epoch
        {
            anyhow::bail!("UI bootstrap identity does not match the negotiated server");
        }

        let mut model = Self {
            client_id,
            client_pid,
            lease,
            snapshot,
            supports_state_publication: hello
                .capabilities
                .iter()
                .any(|capability| capability == "lease_owned_client_state"),
            supports_client_commands: hello
                .capabilities
                .iter()
                .any(|capability| capability == "lease_owned_client_commands"),
            last_heartbeat: Instant::now(),
        };
        model.acknowledge_observed()?;
        Ok(model)
    }

    pub(crate) fn snapshot(&self) -> &UiBootstrapSnapshot {
        &self.snapshot
    }

    pub(crate) fn client_id(&self) -> &str {
        &self.client_id
    }

    pub(crate) fn server_pid(&self) -> u32 {
        self.snapshot.server_pid
    }

    pub(crate) fn heartbeat_if_due(&mut self) -> Result<bool> {
        if self.last_heartbeat.elapsed() < HEARTBEAT_INTERVAL {
            return Ok(false);
        }
        let renewed: UiLeaseGrant = request_json(vec![
            "ui-lease".to_owned(),
            "heartbeat".to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
        ])?;
        renewed.validate().map_err(anyhow::Error::msg)?;
        if renewed.lease_id != self.lease.lease_id
            || renewed.client_id != self.client_id
            || renewed.client_pid != self.client_pid
            || renewed.server_pid != self.snapshot.server_pid
            || renewed.position.server_epoch != self.snapshot.server_epoch
        {
            anyhow::bail!("renewed UI lease identity changed");
        }
        self.lease = renewed;
        self.last_heartbeat = Instant::now();
        Ok(true)
    }

    pub(crate) fn poll_deltas(&mut self) -> Result<bool> {
        let mut changed = false;
        for _ in 0..MAX_DELTA_ROUNDS {
            let batch: UiDeltaBatch = request_json(vec![
                "ui-deltas".to_owned(),
                "--epoch".to_owned(),
                self.snapshot.server_epoch.clone(),
                "--after".to_owned(),
                self.snapshot.position.sequence.to_string(),
                "--limit".to_owned(),
                UI_DELTA_MAX_EVENTS.to_string(),
            ])?;
            batch.validate().map_err(anyhow::Error::msg)?;
            if batch.server_epoch != self.snapshot.server_epoch
                || batch.after_sequence != self.snapshot.position.sequence
            {
                anyhow::bail!("UI delta does not continue the current bootstrap position");
            }
            changed |= apply_delta(&mut self.snapshot, &batch)?;
            self.acknowledge_observed()?;
            if batch.complete {
                return Ok(changed);
            }
        }
        anyhow::bail!("UI delta catch-up exceeded the bounded round limit")
    }

    pub(crate) fn select_tab(&mut self, tab_id: &str) -> Result<()> {
        self.interact("select", tab_id, Vec::new())
    }

    pub(crate) fn send_input(&mut self, tab_id: &str, bytes: &[u8]) -> Result<()> {
        let mut arguments = vec!["--hex".to_owned(), encode_hex(bytes)];
        self.interact("input", tab_id, std::mem::take(&mut arguments))
    }

    pub(crate) fn resize(&mut self, tab_id: &str, rows: u16, columns: u16) -> Result<()> {
        self.interact(
            "resize",
            tab_id,
            vec![
                "--rows".to_owned(),
                rows.to_string(),
                "--columns".to_owned(),
                columns.to_string(),
            ],
        )
    }

    pub(crate) fn run_control(&mut self, arguments: Vec<String>) -> Result<IpcResponse> {
        require_success(send_ipc_request(arguments)?)
    }

    pub(crate) fn publish_snapshot(&mut self, snapshot_json: &str) -> Result<()> {
        if !self.supports_state_publication {
            return Ok(());
        }
        let response: serde_json::Value = request_json(vec![
            "ui-client-state".to_owned(),
            "publish".to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
            "--snapshot-json".to_owned(),
            snapshot_json.to_owned(),
        ])?;
        if response["schema_version"].as_u64() != Some(u64::from(UI_CLIENT_STATE_SCHEMA_VERSION))
            || response["published"].as_bool() != Some(true)
            || response["client_pid"].as_u64() != Some(u64::from(self.client_pid))
            || response["position"]["server_epoch"].as_str()
                != Some(self.snapshot.server_epoch.as_str())
        {
            anyhow::bail!("UI client snapshot publication identity changed");
        }
        Ok(())
    }

    pub(crate) fn poll_client_command(&mut self) -> Result<Option<UiClientCommand>> {
        if !self.supports_client_commands {
            return Ok(None);
        }
        let response: serde_json::Value = request_json(vec![
            "ui-client-command".to_owned(),
            "poll".to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
        ])?;
        match response.get("command") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(command) => serde_json::from_value(command.clone())
                .context("invalid UI client command")
                .map(Some),
        }
    }

    pub(crate) fn apply_client_command(&mut self, command_id: &str) -> Result<IpcResponse> {
        require_success(send_ipc_request(vec![
            "ui-client-command".to_owned(),
            "apply".to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
            "--command-id".to_owned(),
            command_id.to_owned(),
        ])?)
    }

    pub(crate) fn invoke_client_action(&mut self, arguments: Vec<String>) -> Result<IpcResponse> {
        let args_json =
            serde_json::to_string(&arguments).context("could not encode UI client action")?;
        require_success(send_ipc_request(vec![
            "ui-client-command".to_owned(),
            "invoke".to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
            "--args-json".to_owned(),
            args_json,
        ])?)
    }

    pub(crate) fn complete_client_command(
        &mut self,
        command_id: &str,
        response: &IpcResponse,
        detach: bool,
        shutdown_after_result: bool,
    ) -> Result<()> {
        let response_json =
            serde_json::to_string(response).context("could not encode UI client response")?;
        let mut arguments = vec![
            "ui-client-command".to_owned(),
            "complete".to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
            "--command-id".to_owned(),
            command_id.to_owned(),
            "--response-json".to_owned(),
            response_json,
        ];
        if detach {
            arguments.push("--detach".to_owned());
        }
        if shutdown_after_result {
            arguments.push("--shutdown-after-result".to_owned());
        }
        let completion: serde_json::Value = request_json(arguments)?;
        if completion["completed"].as_bool() != Some(true)
            || completion["command_id"].as_str() != Some(command_id)
        {
            anyhow::bail!("UI client command completion identity changed");
        }
        Ok(())
    }

    fn interact(&mut self, action: &str, tab_id: &str, tail: Vec<String>) -> Result<()> {
        let mut arguments = vec![
            "ui-interact".to_owned(),
            action.to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
            "-t".to_owned(),
            tab_id.to_owned(),
        ];
        arguments.extend(tail);
        let response = require_success(send_ipc_request(arguments)?)?;
        let value: serde_json::Value =
            serde_json::from_str(&response.output).context("invalid UI interaction response")?;
        if value["action"].as_str() != Some(action)
            || value["tab_id"].as_str() != Some(tab_id)
            || value["position"]["server_epoch"].as_str()
                != Some(self.snapshot.server_epoch.as_str())
        {
            anyhow::bail!("UI interaction response identity changed");
        }
        Ok(())
    }

    pub(crate) fn detach(&mut self) -> Result<()> {
        require_success(send_ipc_request(vec![
            "ui-lease".to_owned(),
            "detach".to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
        ])?)?;
        Ok(())
    }

    fn acknowledge_observed(&mut self) -> Result<()> {
        let acknowledged: UiLeaseGrant = request_json(vec![
            "ui-lease".to_owned(),
            "acknowledge".to_owned(),
            "--lease-id".to_owned(),
            self.lease.lease_id.clone(),
            "--client-pid".to_owned(),
            self.client_pid.to_string(),
            "--sequence".to_owned(),
            self.snapshot.position.sequence.to_string(),
        ])?;
        acknowledged.validate().map_err(anyhow::Error::msg)?;
        if acknowledged.lease_id != self.lease.lease_id
            || acknowledged.client_pid != self.client_pid
            || acknowledged.server_pid != self.snapshot.server_pid
            || acknowledged.position.server_epoch != self.snapshot.server_epoch
            || acknowledged.observed_sequence != self.snapshot.position.sequence
        {
            anyhow::bail!("UI observation acknowledgement identity changed");
        }
        self.lease = acknowledged;
        self.last_heartbeat = Instant::now();
        Ok(())
    }
}

fn request_json<T: serde::de::DeserializeOwned>(arguments: Vec<String>) -> Result<T> {
    let response = require_success(send_ipc_request(arguments)?)?;
    serde_json::from_str(&response.output).context("invalid typed UI response")
}

fn require_success(response: IpcResponse) -> Result<IpcResponse> {
    if response.ok {
        Ok(response)
    } else {
        anyhow::bail!(
            "{} [{}:{}]",
            response.error,
            response.error_category,
            response.error_code
        )
    }
}

fn apply_delta(snapshot: &mut UiBootstrapSnapshot, batch: &UiDeltaBatch) -> Result<bool> {
    let before = serde_json::to_vec(snapshot)?;
    snapshot
        .tabs
        .retain(|tab| !batch.closed_tab_ids.contains(&tab.id));
    for update in &batch.tab_updates {
        if let Some(current) = snapshot.tabs.iter_mut().find(|tab| tab.id == update.id) {
            *current = update.clone();
        } else {
            snapshot.tabs.push(update.clone());
        }
    }
    snapshot.tabs.sort_by_key(|tab| tab.index);
    snapshot.active_tab_id.clone_from(&batch.active_tab_id);
    snapshot.position.sequence = batch.through_sequence;
    snapshot.complete = snapshot.tabs.iter().all(|tab| tab.screen.complete);
    snapshot.truncated = !snapshot.complete;
    snapshot.validate().map_err(anyhow::Error::msg)?;
    Ok(before != serde_json::to_vec(snapshot)?)
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(DIGITS[usize::from(byte >> 4)] as char);
        output.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    output
}

pub(crate) fn tab_by_id<'a>(
    snapshot: &'a UiBootstrapSnapshot,
    tab_id: &str,
) -> Option<&'a UiTabBootstrap> {
    snapshot.tabs.iter().find(|tab| tab.id == tab_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_bridge::{
        UI_BOOTSTRAP_SCHEMA_VERSION, UI_DELTA_SCHEMA_VERSION, UI_SCREEN_SCHEMA_VERSION,
        UiComposerSnapshot, UiCursorSnapshot, UiEventPosition, UiScreenSnapshot,
        UiWorkingContextSnapshot,
    };

    fn tab(id: &str, index: u32) -> UiTabBootstrap {
        UiTabBootstrap {
            id: id.to_owned(),
            index,
            parent_id: None,
            collapsed: false,
            title: id.to_owned(),
            note: String::new(),
            process_id: Some(index + 10),
            dead: false,
            exit_code: None,
            composer: UiComposerSnapshot {
                text: Some(String::new()),
                sensitive: false,
                byte_length: 0,
            },
            working_context: UiWorkingContextSnapshot {
                cwd: None,
                cwd_confirmed_path: None,
                cwd_confirmed: true,
                cwd_source: "unknown".to_owned(),
                cwd_request_pending: false,
                shell: "unknown".to_owned(),
                proxy_configured: false,
                proxy_source: "none".to_owned(),
                proxy_application_state: "not_configured".to_owned(),
                proxy_request_pending: false,
            },
            screen: UiScreenSnapshot {
                schema_version: UI_SCREEN_SCHEMA_VERSION,
                tab_id: id.to_owned(),
                generation: 1,
                terminal_title: id.to_owned(),
                rows: 24,
                columns: 80,
                scrollback_offset: 0,
                max_scrollback: 0,
                cursor: UiCursorSnapshot {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                runs: Vec::new(),
                complete: true,
                truncated: false,
            },
        }
    }

    #[test]
    fn applies_ordered_post_state_without_mutable_title_identity() {
        let mut snapshot = UiBootstrapSnapshot {
            schema_version: UI_BOOTSTRAP_SCHEMA_VERSION,
            server_pid: 7,
            server_epoch: "epoch".to_owned(),
            position: UiEventPosition {
                server_epoch: "epoch".to_owned(),
                sequence: 2,
            },
            workspace_revision: None,
            active_tab_id: Some("@1".to_owned()),
            tabs: vec![tab("@1", 0), tab("@2", 1)],
            complete: true,
            truncated: false,
        };
        let mut moved = tab("@3", 0);
        moved.title = "replacement".to_owned();
        let batch = UiDeltaBatch {
            schema_version: UI_DELTA_SCHEMA_VERSION,
            server_epoch: "epoch".to_owned(),
            after_sequence: 2,
            through_sequence: 4,
            current_sequence: 4,
            events: vec![
                crate::ui_bridge::UiDeltaEvent {
                    sequence: 3,
                    kind: "tab.closed".to_owned(),
                    tab_id: Some("@1".to_owned()),
                    request_id: None,
                    operation_id: None,
                    payload: serde_json::Value::Null,
                },
                crate::ui_bridge::UiDeltaEvent {
                    sequence: 4,
                    kind: "tab.created".to_owned(),
                    tab_id: Some("@3".to_owned()),
                    request_id: None,
                    operation_id: None,
                    payload: serde_json::Value::Null,
                },
            ],
            tab_updates: vec![moved],
            closed_tab_ids: vec!["@1".to_owned()],
            active_tab_id: Some("@3".to_owned()),
            complete: true,
            truncated: false,
        };
        assert!(apply_delta(&mut snapshot, &batch).unwrap());
        assert_eq!(
            snapshot
                .tabs
                .iter()
                .map(|tab| tab.id.as_str())
                .collect::<Vec<_>>(),
            ["@3", "@2"]
        );
        assert_eq!(snapshot.active_tab_id.as_deref(), Some("@3"));
        assert_eq!(snapshot.position.sequence, 4);
    }

    #[test]
    fn binary_input_encoding_is_exact_and_allocation_bounded() {
        assert_eq!(encode_hex(&[0, b'A', 0xff]), "0041ff");
    }
}
