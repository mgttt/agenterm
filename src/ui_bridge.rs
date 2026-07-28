use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::upgrade_identity::UpgradeIdentity;

pub const UI_BRIDGE_SCHEMA_VERSION: u32 = 7;
pub const UI_BRIDGE_PROTOCOL_VERSION: u32 = 1;
pub const UI_HELLO_SCHEMA_VERSION: u32 = 1;
pub const UI_BOOTSTRAP_SCHEMA_VERSION: u32 = 2;
pub const UI_SCREEN_SCHEMA_VERSION: u32 = 2;
pub const UI_DELTA_SCHEMA_VERSION: u32 = 2;
pub const UI_LEASE_SCHEMA_VERSION: u32 = 2;
pub const UI_INTERACTION_SCHEMA_VERSION: u32 = 1;
pub const UI_BOOTSTRAP_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const UI_BOOTSTRAP_MAX_TABS: usize = 1024;
pub const UI_SCREEN_MAX_ROWS: u32 = 512;
pub const UI_SCREEN_MAX_COLUMNS: u32 = 512;
pub const UI_SCREEN_MAX_RUNS: usize = 256 * 1024;
pub const UI_SCREEN_MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const UI_TAB_TITLE_MAX_BYTES: usize = 4096;
pub const UI_TAB_NOTE_MAX_BYTES: usize = 64 * 1024;
pub const UI_DELTA_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const UI_DELTA_MAX_EVENTS: usize = 64;
pub const UI_CLIENT_ID_MAX_BYTES: usize = 128;
pub const UI_BUILD_IDENTITY_MAX_BYTES: usize = 2048;
pub const UI_INPUT_MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiOwnershipMode {
    CombinedGuiServer,
    SplitServerClient,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiProtocolRange {
    pub minimum: u32,
    pub maximum: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiCompatibility {
    Compatible,
    ClientTooOld,
    ClientTooNew,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UiBridgeFacts {
    pub schema_version: u32,
    pub protocol_version: u32,
    pub compatible_client_range: UiProtocolRange,
    pub ownership_mode: UiOwnershipMode,
    pub replaceable_ui: bool,
    pub server_executable: &'static str,
    pub target_server_executable: &'static str,
    pub bootstrap_snapshot: bool,
    pub ordered_deltas: bool,
    pub interactive_lease: bool,
    pub reconnect: bool,
    pub rollback_proven: bool,
    pub contract_schemas: UiContractSchemas,
    pub hard_limits: UiContractLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UiContractSchemas {
    pub hello: u32,
    pub bootstrap: u32,
    pub screen: u32,
    pub delta: u32,
    pub lease: u32,
    pub interaction: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UiContractLimits {
    pub bootstrap_bytes: usize,
    pub tabs: usize,
    pub screen_rows: u32,
    pub screen_columns: u32,
    pub screen_runs: usize,
    pub screen_text_bytes: usize,
    pub delta_bytes: usize,
    pub delta_events: usize,
    pub input_bytes: usize,
}

pub const fn current_facts() -> UiBridgeFacts {
    UiBridgeFacts {
        schema_version: UI_BRIDGE_SCHEMA_VERSION,
        protocol_version: UI_BRIDGE_PROTOCOL_VERSION,
        compatible_client_range: UiProtocolRange {
            minimum: UI_BRIDGE_PROTOCOL_VERSION,
            maximum: UI_BRIDGE_PROTOCOL_VERSION,
        },
        ownership_mode: UiOwnershipMode::CombinedGuiServer,
        replaceable_ui: false,
        server_executable: "agenterm.exe",
        target_server_executable: "agenterm-server.exe",
        bootstrap_snapshot: true,
        ordered_deltas: true,
        interactive_lease: false,
        reconnect: false,
        rollback_proven: false,
        contract_schemas: UiContractSchemas {
            hello: UI_HELLO_SCHEMA_VERSION,
            bootstrap: UI_BOOTSTRAP_SCHEMA_VERSION,
            screen: UI_SCREEN_SCHEMA_VERSION,
            delta: UI_DELTA_SCHEMA_VERSION,
            lease: UI_LEASE_SCHEMA_VERSION,
            interaction: UI_INTERACTION_SCHEMA_VERSION,
        },
        hard_limits: UiContractLimits {
            bootstrap_bytes: UI_BOOTSTRAP_MAX_BYTES,
            tabs: UI_BOOTSTRAP_MAX_TABS,
            screen_rows: UI_SCREEN_MAX_ROWS,
            screen_columns: UI_SCREEN_MAX_COLUMNS,
            screen_runs: UI_SCREEN_MAX_RUNS,
            screen_text_bytes: UI_SCREEN_MAX_TEXT_BYTES,
            delta_bytes: UI_DELTA_MAX_BYTES,
            delta_events: UI_DELTA_MAX_EVENTS,
            input_bytes: UI_INPUT_MAX_BYTES,
        },
    }
}

pub const fn headless_server_facts() -> UiBridgeFacts {
    let mut facts = current_facts();
    facts.ownership_mode = UiOwnershipMode::SplitServerClient;
    facts.replaceable_ui = true;
    facts.server_executable = "agenterm-server.exe";
    facts.interactive_lease = true;
    facts.reconnect = true;
    facts.rollback_proven = true;
    facts
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiHelloRequest {
    pub schema_version: u32,
    pub client_id: String,
    pub protocol_range: UiProtocolRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_build: Option<UpgradeIdentity>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiHelloResponse {
    pub schema_version: u32,
    pub accepted: bool,
    pub compatibility: UiCompatibility,
    pub client_id: String,
    pub protocol_version: u32,
    pub server_pid: u32,
    pub position: UiEventPosition,
    pub bootstrap_schema_version: u32,
    pub delta_schema_version: u32,
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_build: Option<UpgradeIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_build: Option<UpgradeIdentity>,
}

impl UiHelloRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UI_HELLO_SCHEMA_VERSION {
            return Err("ui_hello_schema_unsupported".to_owned());
        }
        if self.client_id.is_empty()
            || self.client_id.len() > UI_CLIENT_ID_MAX_BYTES
            || self.client_id.chars().any(char::is_control)
        {
            return Err("ui_hello_client_id_invalid".to_owned());
        }
        if self.protocol_range.minimum == 0
            || self.protocol_range.maximum < self.protocol_range.minimum
        {
            return Err("ui_hello_protocol_range_invalid".to_owned());
        }
        if self.client_build.as_ref().is_some_and(|identity| {
            serde_json::to_vec(identity)
                .map_or(true, |encoded| encoded.len() > UI_BUILD_IDENTITY_MAX_BYTES)
        }) {
            return Err("ui_hello_client_build_invalid".to_owned());
        }
        Ok(())
    }
}

impl UiHelloResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UI_HELLO_SCHEMA_VERSION {
            return Err("ui_hello_schema_unsupported".to_owned());
        }
        UiHelloRequest {
            schema_version: self.schema_version,
            client_id: self.client_id.clone(),
            protocol_range: UiProtocolRange {
                minimum: self.protocol_version,
                maximum: self.protocol_version,
            },
            client_build: self.client_build.clone(),
        }
        .validate()?;
        if self.position.server_epoch.is_empty()
            || self.position.server_epoch.len() > 128
            || self.bootstrap_schema_version != UI_BOOTSTRAP_SCHEMA_VERSION
            || self.delta_schema_version != UI_DELTA_SCHEMA_VERSION
            || self.accepted != (self.compatibility == UiCompatibility::Compatible)
        {
            return Err("ui_hello_response_invalid".to_owned());
        }
        let mut capabilities = HashSet::with_capacity(self.capabilities.len());
        if self.capabilities.is_empty()
            || self
                .capabilities
                .iter()
                .any(|capability| !capabilities.insert(capability.as_str()))
        {
            return Err("ui_hello_capabilities_invalid".to_owned());
        }
        if self.server_build.as_ref().is_some_and(|identity| {
            serde_json::to_vec(identity)
                .map_or(true, |encoded| encoded.len() > UI_BUILD_IDENTITY_MAX_BYTES)
        }) {
            return Err("ui_hello_build_identity_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiEventPosition {
    pub server_epoch: String,
    pub sequence: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiCursorSnapshot {
    pub row: u32,
    pub column: u32,
    pub visible: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiCellStyle {
    pub foreground: UiColor,
    pub background: UiColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiColor {
    Default,
    Indexed { index: u8 },
    Rgb { red: u8, green: u8, blue: u8 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiCellRun {
    pub row: u32,
    pub column: u32,
    pub columns: u32,
    pub text: String,
    pub style: UiCellStyle,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiScreenSnapshot {
    pub schema_version: u32,
    pub tab_id: String,
    pub generation: u64,
    pub rows: u32,
    pub columns: u32,
    pub scrollback_offset: usize,
    pub max_scrollback: usize,
    pub cursor: UiCursorSnapshot,
    pub runs: Vec<UiCellRun>,
    pub complete: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiTabBootstrap {
    pub id: String,
    pub index: u32,
    pub parent_id: Option<String>,
    /// Additive v2 field. Older compatible servers omit it and therefore
    /// expose every tree row as expanded.
    #[serde(default)]
    pub collapsed: bool,
    pub title: String,
    pub note: String,
    pub process_id: Option<u32>,
    pub dead: bool,
    pub exit_code: Option<u32>,
    pub composer: UiComposerSnapshot,
    pub working_context: UiWorkingContextSnapshot,
    pub screen: UiScreenSnapshot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiComposerSnapshot {
    pub text: Option<String>,
    pub sensitive: bool,
    pub byte_length: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiWorkingContextSnapshot {
    pub cwd: Option<String>,
    pub cwd_confirmed: bool,
    pub cwd_source: String,
    pub cwd_request_pending: bool,
    pub proxy_configured: bool,
    pub proxy_source: String,
    pub proxy_application_state: String,
    pub proxy_request_pending: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiBootstrapSnapshot {
    pub schema_version: u32,
    pub server_pid: u32,
    pub server_epoch: String,
    pub position: UiEventPosition,
    pub workspace_revision: Option<u64>,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<UiTabBootstrap>,
    pub complete: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct UiDeltaEvent {
    pub sequence: u64,
    pub kind: String,
    pub tab_id: Option<String>,
    pub request_id: Option<String>,
    pub operation_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct UiDeltaBatch {
    pub schema_version: u32,
    pub server_epoch: String,
    pub after_sequence: u64,
    pub through_sequence: u64,
    pub current_sequence: u64,
    pub events: Vec<UiDeltaEvent>,
    pub tab_updates: Vec<UiTabBootstrap>,
    pub closed_tab_ids: Vec<String>,
    pub active_tab_id: Option<String>,
    pub complete: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiLeaseGrant {
    pub schema_version: u32,
    pub lease_id: String,
    pub client_id: String,
    pub client_pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_build: Option<UpgradeIdentity>,
    pub server_pid: u32,
    pub position: UiEventPosition,
    pub expires_unix_ms: u64,
    pub ttl_ms: u64,
    pub observed_sequence: u64,
}

impl UiLeaseGrant {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UI_LEASE_SCHEMA_VERSION {
            return Err("ui_lease_schema_unsupported".to_owned());
        }
        if self.lease_id.is_empty()
            || self.lease_id.len() > UI_CLIENT_ID_MAX_BYTES
            || self.lease_id.chars().any(char::is_control)
        {
            return Err("ui_lease_id_invalid".to_owned());
        }
        UiHelloRequest {
            schema_version: UI_HELLO_SCHEMA_VERSION,
            client_id: self.client_id.clone(),
            protocol_range: UiProtocolRange {
                minimum: UI_BRIDGE_PROTOCOL_VERSION,
                maximum: UI_BRIDGE_PROTOCOL_VERSION,
            },
            client_build: self.client_build.clone(),
        }
        .validate()?;
        if self.client_pid == 0
            || self.server_pid == 0
            || self.position.server_epoch.is_empty()
            || self.ttl_ms == 0
            || self.expires_unix_ms < self.ttl_ms
            || self.observed_sequence > self.position.sequence
        {
            return Err("ui_lease_grant_invalid".to_owned());
        }
        Ok(())
    }
}

impl UiDeltaBatch {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UI_DELTA_SCHEMA_VERSION {
            return Err("ui_delta_schema_unsupported".to_owned());
        }
        if self.server_epoch.is_empty()
            || self.server_epoch.len() > 128
            || self.through_sequence < self.after_sequence
            || self.current_sequence < self.through_sequence
        {
            return Err("ui_delta_position_invalid".to_owned());
        }
        if self.events.len() > UI_DELTA_MAX_EVENTS {
            return Err("ui_delta_events_limit".to_owned());
        }
        if self.complete == self.truncated {
            return Err("ui_delta_completeness_invalid".to_owned());
        }
        if self.complete != (self.through_sequence == self.current_sequence) {
            return Err("ui_delta_completeness_invalid".to_owned());
        }
        let mut previous = self.after_sequence;
        for event in &self.events {
            if event.sequence <= previous || event.sequence > self.through_sequence {
                return Err("ui_delta_order_invalid".to_owned());
            }
            if event
                .tab_id
                .as_deref()
                .is_some_and(|id| !valid_stable_tab_id(id))
            {
                return Err("ui_delta_event_tab_id_invalid".to_owned());
            }
            previous = event.sequence;
        }
        if previous != self.through_sequence {
            return Err("ui_delta_through_sequence_invalid".to_owned());
        }
        let mut update_ids = HashSet::with_capacity(self.tab_updates.len());
        for tab in &self.tab_updates {
            if !valid_stable_tab_id(&tab.id)
                || tab.screen.tab_id != tab.id
                || !update_ids.insert(tab.id.as_str())
            {
                return Err("ui_delta_tab_update_duplicate".to_owned());
            }
            tab.screen.validate()?;
        }
        let mut closed_ids = HashSet::with_capacity(self.closed_tab_ids.len());
        for id in &self.closed_tab_ids {
            if !valid_stable_tab_id(id)
                || !closed_ids.insert(id.as_str())
                || update_ids.contains(id.as_str())
            {
                return Err("ui_delta_closed_tab_invalid".to_owned());
            }
        }
        if self
            .active_tab_id
            .as_deref()
            .is_some_and(|id| !valid_stable_tab_id(id))
        {
            return Err("ui_delta_active_tab_invalid".to_owned());
        }
        let encoded =
            serde_json::to_vec(self).map_err(|_| "ui_delta_serialization_failed".to_owned())?;
        if encoded.len() > UI_DELTA_MAX_BYTES {
            return Err("ui_delta_bytes_limit".to_owned());
        }
        Ok(())
    }
}

impl UiScreenSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UI_SCREEN_SCHEMA_VERSION {
            return Err("ui_screen_schema_unsupported".to_owned());
        }
        if !valid_stable_tab_id(&self.tab_id) {
            return Err("ui_screen_tab_id_invalid".to_owned());
        }
        if self.rows == 0
            || self.columns == 0
            || self.rows > UI_SCREEN_MAX_ROWS
            || self.columns > UI_SCREEN_MAX_COLUMNS
        {
            return Err("ui_screen_dimensions_limit".to_owned());
        }
        if self.runs.len() > UI_SCREEN_MAX_RUNS {
            return Err("ui_screen_runs_limit".to_owned());
        }
        if self.complete && self.truncated {
            return Err("ui_screen_completeness_invalid".to_owned());
        }
        if self.scrollback_offset > self.max_scrollback {
            return Err("ui_screen_scrollback_bounds".to_owned());
        }
        if self.cursor.row >= self.rows || self.cursor.column >= self.columns {
            return Err("ui_screen_cursor_bounds".to_owned());
        }
        let mut text_bytes = 0usize;
        for run in &self.runs {
            if run.columns == 0
                || run.row >= self.rows
                || run.column >= self.columns
                || run
                    .column
                    .checked_add(run.columns)
                    .is_none_or(|end| end > self.columns)
            {
                return Err("ui_screen_run_bounds".to_owned());
            }
            text_bytes = text_bytes
                .checked_add(run.text.len())
                .ok_or_else(|| "ui_screen_text_limit".to_owned())?;
            if text_bytes > UI_SCREEN_MAX_TEXT_BYTES {
                return Err("ui_screen_text_limit".to_owned());
            }
        }
        Ok(())
    }
}

impl UiBootstrapSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UI_BOOTSTRAP_SCHEMA_VERSION {
            return Err("ui_bootstrap_schema_unsupported".to_owned());
        }
        if self.server_pid == 0
            || self.server_epoch.is_empty()
            || self.server_epoch.len() > 128
            || self.position.server_epoch != self.server_epoch
        {
            return Err("ui_bootstrap_server_identity_invalid".to_owned());
        }
        if self.tabs.len() > UI_BOOTSTRAP_MAX_TABS {
            return Err("ui_bootstrap_tabs_limit".to_owned());
        }
        if self.complete && self.truncated {
            return Err("ui_bootstrap_completeness_invalid".to_owned());
        }
        let mut ids = HashSet::with_capacity(self.tabs.len());
        let mut indices = HashSet::with_capacity(self.tabs.len());
        for tab in &self.tabs {
            if !valid_stable_tab_id(&tab.id)
                || !ids.insert(tab.id.as_str())
                || !indices.insert(tab.index)
            {
                return Err("ui_bootstrap_tab_id_invalid".to_owned());
            }
            if tab.title.len() > UI_TAB_TITLE_MAX_BYTES || tab.note.len() > UI_TAB_NOTE_MAX_BYTES {
                return Err("ui_bootstrap_tab_metadata_limit".to_owned());
            }
            if tab.composer.sensitive == tab.composer.text.is_some()
                || tab
                    .composer
                    .text
                    .as_ref()
                    .is_some_and(|text| text.len() != tab.composer.byte_length)
            {
                return Err("ui_bootstrap_composer_invalid".to_owned());
            }
            if tab.screen.tab_id != tab.id {
                return Err("ui_bootstrap_screen_identity_mismatch".to_owned());
            }
            tab.screen.validate()?;
        }
        for tab in &self.tabs {
            if tab
                .parent_id
                .as_deref()
                .is_some_and(|parent| parent == tab.id || !ids.contains(parent))
            {
                return Err("ui_bootstrap_parent_invalid".to_owned());
            }
        }
        if self
            .active_tab_id
            .as_deref()
            .is_some_and(|active| !ids.contains(active))
        {
            return Err("ui_bootstrap_active_tab_invalid".to_owned());
        }
        let encoded =
            serde_json::to_vec(self).map_err(|_| "ui_bootstrap_serialization_failed".to_owned())?;
        if encoded.len() > UI_BOOTSTRAP_MAX_BYTES {
            return Err("ui_bootstrap_bytes_limit".to_owned());
        }
        Ok(())
    }
}

fn valid_stable_tab_id(value: &str) -> bool {
    value.strip_prefix('@').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

pub const fn negotiate(client: UiProtocolRange, server_version: u32) -> UiCompatibility {
    if server_version < client.minimum {
        UiCompatibility::ClientTooNew
    } else if server_version > client.maximum {
        UiCompatibility::ClientTooOld
    } else {
        UiCompatibility::Compatible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_screen(tab_id: &str) -> UiScreenSnapshot {
        UiScreenSnapshot {
            schema_version: UI_SCREEN_SCHEMA_VERSION,
            tab_id: tab_id.to_owned(),
            generation: 7,
            rows: 24,
            columns: 80,
            scrollback_offset: 0,
            max_scrollback: 0,
            cursor: UiCursorSnapshot {
                row: 3,
                column: 4,
                visible: true,
            },
            runs: vec![UiCellRun {
                row: 0,
                column: 0,
                columns: 5,
                text: "hello".to_owned(),
                style: UiCellStyle {
                    foreground: UiColor::Rgb {
                        red: 255,
                        green: 255,
                        blue: 255,
                    },
                    background: UiColor::Default,
                    bold: false,
                    italic: false,
                    underline: false,
                    inverse: false,
                },
            }],
            complete: true,
            truncated: false,
        }
    }

    fn valid_bootstrap() -> UiBootstrapSnapshot {
        UiBootstrapSnapshot {
            schema_version: UI_BOOTSTRAP_SCHEMA_VERSION,
            server_pid: 42,
            server_epoch: "epoch-1".to_owned(),
            position: UiEventPosition {
                server_epoch: "epoch-1".to_owned(),
                sequence: 9,
            },
            workspace_revision: Some(3),
            active_tab_id: Some("@1".to_owned()),
            tabs: vec![
                UiTabBootstrap {
                    id: "@1".to_owned(),
                    index: 0,
                    parent_id: None,
                    collapsed: false,
                    title: "root".to_owned(),
                    note: String::new(),
                    process_id: Some(100),
                    dead: false,
                    exit_code: None,
                    composer: UiComposerSnapshot {
                        text: Some(String::new()),
                        sensitive: false,
                        byte_length: 0,
                    },
                    working_context: UiWorkingContextSnapshot {
                        cwd: Some("C:\\work".to_owned()),
                        cwd_confirmed: true,
                        cwd_source: "launch".to_owned(),
                        cwd_request_pending: false,
                        proxy_configured: false,
                        proxy_source: "off".to_owned(),
                        proxy_application_state: "off".to_owned(),
                        proxy_request_pending: false,
                    },
                    screen: valid_screen("@1"),
                },
                UiTabBootstrap {
                    id: "@2".to_owned(),
                    index: 1,
                    parent_id: Some("@1".to_owned()),
                    collapsed: false,
                    title: "child".to_owned(),
                    note: "ready".to_owned(),
                    process_id: None,
                    dead: true,
                    exit_code: Some(0),
                    composer: UiComposerSnapshot {
                        text: None,
                        sensitive: true,
                        byte_length: 12,
                    },
                    working_context: UiWorkingContextSnapshot {
                        cwd: None,
                        cwd_confirmed: false,
                        cwd_source: "unknown".to_owned(),
                        cwd_request_pending: false,
                        proxy_configured: true,
                        proxy_source: "launch".to_owned(),
                        proxy_application_state: "launch_applied".to_owned(),
                        proxy_request_pending: false,
                    },
                    screen: valid_screen("@2"),
                },
            ],
            complete: true,
            truncated: false,
        }
    }

    fn test_build(profile: &str) -> UpgradeIdentity {
        UpgradeIdentity {
            protocol_version: Some(UI_BRIDGE_PROTOCOL_VERSION),
            version: Some("0.1.9".to_owned()),
            git_commit: Some("a".repeat(40)),
            profile: Some(profile.to_owned()),
            cargo_lock_sha256: Some("b".repeat(64)),
            artifact_manifest_sha256: Some("c".repeat(64)),
        }
    }

    #[test]
    fn compatibility_is_explicit_in_both_directions() {
        let client = UiProtocolRange {
            minimum: 2,
            maximum: 4,
        };
        assert_eq!(negotiate(client, 1), UiCompatibility::ClientTooNew);
        assert_eq!(negotiate(client, 2), UiCompatibility::Compatible);
        assert_eq!(negotiate(client, 4), UiCompatibility::Compatible);
        assert_eq!(negotiate(client, 5), UiCompatibility::ClientTooOld);
    }

    #[test]
    fn current_facts_do_not_claim_the_planned_split() {
        let facts = current_facts();
        assert_eq!(facts.schema_version, 7);
        assert_eq!(facts.ownership_mode, UiOwnershipMode::CombinedGuiServer);
        assert!(!facts.replaceable_ui);
        assert!(!facts.interactive_lease);
        assert!(!facts.reconnect);
        assert!(facts.bootstrap_snapshot);
        assert!(facts.ordered_deltas);
        assert_eq!(facts.server_executable, "agenterm.exe");
        assert_eq!(facts.target_server_executable, "agenterm-server.exe");
        assert_eq!(facts.contract_schemas.hello, UI_HELLO_SCHEMA_VERSION);
        assert_eq!(
            facts.contract_schemas.bootstrap,
            UI_BOOTSTRAP_SCHEMA_VERSION
        );
        assert_eq!(facts.contract_schemas.screen, UI_SCREEN_SCHEMA_VERSION);
        assert_eq!(facts.contract_schemas.delta, UI_DELTA_SCHEMA_VERSION);
        assert_eq!(facts.contract_schemas.lease, UI_LEASE_SCHEMA_VERSION);
        assert_eq!(
            facts.contract_schemas.interaction,
            UI_INTERACTION_SCHEMA_VERSION
        );
        assert_eq!(facts.hard_limits.tabs, UI_BOOTSTRAP_MAX_TABS);
        assert_eq!(facts.hard_limits.bootstrap_bytes, UI_BOOTSTRAP_MAX_BYTES);
        assert_eq!(facts.hard_limits.delta_bytes, UI_DELTA_MAX_BYTES);
        assert_eq!(facts.hard_limits.input_bytes, UI_INPUT_MAX_BYTES);
        assert_eq!(facts.hard_limits.delta_events, UI_DELTA_MAX_EVENTS);
    }

    #[test]
    fn headless_server_facts_publish_the_proven_replaceable_client_contract() {
        let facts = headless_server_facts();
        assert_eq!(facts.ownership_mode, UiOwnershipMode::SplitServerClient);
        assert_eq!(facts.server_executable, "agenterm-server.exe");
        assert!(facts.replaceable_ui);
        assert!(facts.interactive_lease);
        assert!(facts.reconnect);
        assert!(facts.rollback_proven);
        assert!(facts.bootstrap_snapshot);
        assert!(facts.ordered_deltas);
    }

    #[test]
    fn interactive_lease_grant_has_bounded_owner_and_causal_identity() {
        let grant = UiLeaseGrant {
            schema_version: UI_LEASE_SCHEMA_VERSION,
            lease_id: "ui-2a-3e8-1".to_owned(),
            client_id: "agenterm-gui:42".to_owned(),
            client_pid: 42,
            client_build: None,
            server_pid: 7,
            position: UiEventPosition {
                server_epoch: "epoch-1".to_owned(),
                sequence: 9,
            },
            expires_unix_ms: 6_000,
            ttl_ms: 5_000,
            observed_sequence: 8,
        };
        grant.validate().unwrap();
        let mut invalid = grant;
        invalid.lease_id = "bad\nlease".to_owned();
        assert_eq!(invalid.validate().unwrap_err(), "ui_lease_id_invalid");
    }

    #[test]
    fn hello_contract_validates_identity_compatibility_and_capabilities() {
        let request = UiHelloRequest {
            schema_version: UI_HELLO_SCHEMA_VERSION,
            client_id: "renderer-smoke".to_owned(),
            protocol_range: UiProtocolRange {
                minimum: 1,
                maximum: 1,
            },
            client_build: None,
        };
        request.validate().unwrap();
        let response = UiHelloResponse {
            schema_version: UI_HELLO_SCHEMA_VERSION,
            accepted: true,
            compatibility: UiCompatibility::Compatible,
            client_id: request.client_id.clone(),
            protocol_version: UI_BRIDGE_PROTOCOL_VERSION,
            server_pid: 42,
            position: UiEventPosition {
                server_epoch: "epoch-1".to_owned(),
                sequence: 9,
            },
            bootstrap_schema_version: UI_BOOTSTRAP_SCHEMA_VERSION,
            delta_schema_version: UI_DELTA_SCHEMA_VERSION,
            capabilities: vec![
                "bootstrap_snapshot".to_owned(),
                "ordered_delta_poll".to_owned(),
            ],
            client_build: None,
            server_build: None,
        };
        response.validate().unwrap();

        let mut invalid = response;
        invalid.accepted = false;
        assert_eq!(invalid.validate().unwrap_err(), "ui_hello_response_invalid");
        let mut invalid_request = request;
        invalid_request.client_id = "bad\nclient".to_owned();
        assert_eq!(
            invalid_request.validate().unwrap_err(),
            "ui_hello_client_id_invalid"
        );
    }

    #[test]
    fn build_identity_is_additive_bounded_and_prior_hello_json_remains_compatible() {
        let prior_request: UiHelloRequest = serde_json::from_value(serde_json::json!({
            "schema_version": UI_HELLO_SCHEMA_VERSION,
            "client_id": "prior-renderer",
            "protocol_range": {"minimum": 1, "maximum": 1}
        }))
        .unwrap();
        assert!(prior_request.client_build.is_none());
        prior_request.validate().unwrap();

        let mut response = UiHelloResponse {
            schema_version: UI_HELLO_SCHEMA_VERSION,
            accepted: true,
            compatibility: UiCompatibility::Compatible,
            client_id: "renderer-next".to_owned(),
            protocol_version: UI_BRIDGE_PROTOCOL_VERSION,
            server_pid: 42,
            position: UiEventPosition {
                server_epoch: "epoch-1".to_owned(),
                sequence: 9,
            },
            bootstrap_schema_version: UI_BOOTSTRAP_SCHEMA_VERSION,
            delta_schema_version: UI_DELTA_SCHEMA_VERSION,
            capabilities: vec!["bootstrap_snapshot".to_owned()],
            client_build: Some(test_build("dev")),
            server_build: Some(test_build("release-fast")),
        };
        response.validate().unwrap();

        response.server_build.as_mut().unwrap().version =
            Some("x".repeat(UI_BUILD_IDENTITY_MAX_BYTES));
        assert_eq!(
            response.validate().unwrap_err(),
            "ui_hello_build_identity_invalid"
        );
    }

    #[test]
    fn renderer_neutral_bootstrap_contract_accepts_bounded_tree_and_screen() {
        let snapshot = valid_bootstrap();
        snapshot.validate().unwrap();
        let encoded = serde_json::to_vec(&snapshot).unwrap();
        assert!(encoded.len() < UI_BOOTSTRAP_MAX_BYTES);
    }

    #[test]
    fn additive_collapsed_fact_accepts_a_prior_compatible_server() {
        let mut encoded = serde_json::to_value(valid_bootstrap()).unwrap();
        for tab in encoded["tabs"].as_array_mut().unwrap() {
            tab.as_object_mut().unwrap().remove("collapsed");
        }
        let decoded: UiBootstrapSnapshot = serde_json::from_value(encoded).unwrap();
        decoded.validate().unwrap();
        assert!(decoded.tabs.iter().all(|tab| !tab.collapsed));
    }

    #[test]
    fn renderer_neutral_bootstrap_contract_rejects_identity_and_screen_drift() {
        let mut duplicate = valid_bootstrap();
        duplicate.tabs[1].id = "@1".to_owned();
        duplicate.tabs[1].screen.tab_id = "@1".to_owned();
        assert_eq!(
            duplicate.validate().unwrap_err(),
            "ui_bootstrap_tab_id_invalid"
        );

        let mut epoch_mismatch = valid_bootstrap();
        epoch_mismatch.position.server_epoch = "epoch-2".to_owned();
        assert_eq!(
            epoch_mismatch.validate().unwrap_err(),
            "ui_bootstrap_server_identity_invalid"
        );

        let mut run_overflow = valid_bootstrap();
        run_overflow.tabs[0].screen.runs[0].column = 79;
        run_overflow.tabs[0].screen.runs[0].columns = 2;
        assert_eq!(run_overflow.validate().unwrap_err(), "ui_screen_run_bounds");
    }

    #[test]
    fn screen_history_position_is_bounded_by_the_published_maximum() {
        let mut screen = valid_screen("@1");
        screen.max_scrollback = 12;
        screen.scrollback_offset = 12;
        assert!(screen.validate().is_ok());
        screen.scrollback_offset = 13;
        assert_eq!(
            screen.validate(),
            Err("ui_screen_scrollback_bounds".to_owned())
        );
    }

    #[test]
    fn ordered_delta_contract_accepts_post_state_and_rejects_sequence_drift() {
        let bootstrap = valid_bootstrap();
        let batch = UiDeltaBatch {
            schema_version: UI_DELTA_SCHEMA_VERSION,
            server_epoch: "epoch-1".to_owned(),
            after_sequence: 7,
            through_sequence: 9,
            current_sequence: 9,
            events: vec![
                UiDeltaEvent {
                    sequence: 8,
                    kind: "tab.note".to_owned(),
                    tab_id: Some("@1".to_owned()),
                    request_id: Some("request-1".to_owned()),
                    operation_id: Some("tabs.set-note".to_owned()),
                    payload: serde_json::json!({"note": "ready"}),
                },
                UiDeltaEvent {
                    sequence: 9,
                    kind: "tab.active".to_owned(),
                    tab_id: Some("@2".to_owned()),
                    request_id: None,
                    operation_id: None,
                    payload: serde_json::json!({}),
                },
            ],
            tab_updates: bootstrap.tabs,
            closed_tab_ids: Vec::new(),
            active_tab_id: Some("@2".to_owned()),
            complete: true,
            truncated: false,
        };
        batch.validate().unwrap();

        let mut bad_through = batch.clone();
        bad_through.through_sequence = 8;
        bad_through.complete = false;
        bad_through.truncated = true;
        assert_eq!(
            bad_through.validate().unwrap_err(),
            "ui_delta_order_invalid"
        );
        let mut bad_completeness = batch;
        bad_completeness.current_sequence = 10;
        assert_eq!(
            bad_completeness.validate().unwrap_err(),
            "ui_delta_completeness_invalid"
        );
    }
}
