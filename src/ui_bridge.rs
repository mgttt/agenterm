use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const UI_BRIDGE_SCHEMA_VERSION: u32 = 2;
pub const UI_BRIDGE_PROTOCOL_VERSION: u32 = 1;
pub const UI_BOOTSTRAP_SCHEMA_VERSION: u32 = 1;
pub const UI_SCREEN_SCHEMA_VERSION: u32 = 1;
pub const UI_BOOTSTRAP_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const UI_BOOTSTRAP_MAX_TABS: usize = 1024;
pub const UI_SCREEN_MAX_ROWS: u32 = 512;
pub const UI_SCREEN_MAX_COLUMNS: u32 = 512;
pub const UI_SCREEN_MAX_RUNS: usize = 256 * 1024;
pub const UI_SCREEN_MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const UI_TAB_TITLE_MAX_BYTES: usize = 4096;
pub const UI_TAB_NOTE_MAX_BYTES: usize = 64 * 1024;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    pub reconnect: bool,
    pub rollback_proven: bool,
    pub contract_schemas: UiContractSchemas,
    pub hard_limits: UiContractLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UiContractSchemas {
    pub bootstrap: u32,
    pub screen: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UiContractLimits {
    pub bootstrap_bytes: usize,
    pub tabs: usize,
    pub screen_rows: u32,
    pub screen_columns: u32,
    pub screen_runs: usize,
    pub screen_text_bytes: usize,
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
        bootstrap_snapshot: false,
        ordered_deltas: false,
        reconnect: false,
        rollback_proven: false,
        contract_schemas: UiContractSchemas {
            bootstrap: UI_BOOTSTRAP_SCHEMA_VERSION,
            screen: UI_SCREEN_SCHEMA_VERSION,
        },
        hard_limits: UiContractLimits {
            bootstrap_bytes: UI_BOOTSTRAP_MAX_BYTES,
            tabs: UI_BOOTSTRAP_MAX_TABS,
            screen_rows: UI_SCREEN_MAX_ROWS,
            screen_columns: UI_SCREEN_MAX_COLUMNS,
            screen_runs: UI_SCREEN_MAX_RUNS,
            screen_text_bytes: UI_SCREEN_MAX_TEXT_BYTES,
        },
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
    pub foreground_rgb: u32,
    pub background_rgb: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
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
    pub cursor: UiCursorSnapshot,
    pub runs: Vec<UiCellRun>,
    pub complete: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiTabBootstrap {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub note: String,
    pub process_id: Option<u32>,
    pub dead: bool,
    pub exit_code: Option<u32>,
    pub screen: UiScreenSnapshot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiBootstrapSnapshot {
    pub schema_version: u32,
    pub server_pid: u32,
    pub server_epoch: String,
    pub position: UiEventPosition,
    pub workspace_revision: u64,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<UiTabBootstrap>,
    pub complete: bool,
    pub truncated: bool,
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
        for tab in &self.tabs {
            if !valid_stable_tab_id(&tab.id) || !ids.insert(tab.id.as_str()) {
                return Err("ui_bootstrap_tab_id_invalid".to_owned());
            }
            if tab.title.len() > UI_TAB_TITLE_MAX_BYTES || tab.note.len() > UI_TAB_NOTE_MAX_BYTES {
                return Err("ui_bootstrap_tab_metadata_limit".to_owned());
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
                    foreground_rgb: 0xffffff,
                    background_rgb: 0,
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
            workspace_revision: 3,
            active_tab_id: Some("@1".to_owned()),
            tabs: vec![
                UiTabBootstrap {
                    id: "@1".to_owned(),
                    parent_id: None,
                    title: "root".to_owned(),
                    note: String::new(),
                    process_id: Some(100),
                    dead: false,
                    exit_code: None,
                    screen: valid_screen("@1"),
                },
                UiTabBootstrap {
                    id: "@2".to_owned(),
                    parent_id: Some("@1".to_owned()),
                    title: "child".to_owned(),
                    note: "ready".to_owned(),
                    process_id: None,
                    dead: true,
                    exit_code: Some(0),
                    screen: valid_screen("@2"),
                },
            ],
            complete: true,
            truncated: false,
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
        assert_eq!(facts.schema_version, 2);
        assert_eq!(facts.ownership_mode, UiOwnershipMode::CombinedGuiServer);
        assert!(!facts.replaceable_ui);
        assert!(!facts.reconnect);
        assert_eq!(facts.server_executable, "agenterm.exe");
        assert_eq!(facts.target_server_executable, "agenterm-server.exe");
        assert_eq!(
            facts.contract_schemas.bootstrap,
            UI_BOOTSTRAP_SCHEMA_VERSION
        );
        assert_eq!(facts.contract_schemas.screen, UI_SCREEN_SCHEMA_VERSION);
        assert_eq!(facts.hard_limits.tabs, UI_BOOTSTRAP_MAX_TABS);
        assert_eq!(facts.hard_limits.bootstrap_bytes, UI_BOOTSTRAP_MAX_BYTES);
    }

    #[test]
    fn renderer_neutral_bootstrap_contract_accepts_bounded_tree_and_screen() {
        let snapshot = valid_bootstrap();
        snapshot.validate().unwrap();
        let encoded = serde_json::to_vec(&snapshot).unwrap();
        assert!(encoded.len() < UI_BOOTSTRAP_MAX_BYTES);
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
}
