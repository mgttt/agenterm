//! Shared multi-instance picker dialog (S1 attach / S3 open-another).
//!
//! Rows reuse the same registration discovery as `server-list` / `list-instances`.
//! Stale rows are listed but cannot attach.

use crate::instances::{
    RegistrationOwnerState, discover_instances, instance_process_is_alive, registration_owner_state,
};
use crate::ipc_endpoint::IpcEndpoint;
use serde_json::json;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum InstancePickerMode {
    #[default]
    Attach,
    OpenAnother,
}

impl InstancePickerMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Attach => "attach",
            Self::OpenAnother => "open-another",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "attach" => Some(Self::Attach),
            "open-another" | "open" | "new-window" => Some(Self::OpenAnother),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstancePickerRow {
    pub instance: String,
    pub instance_label: String,
    pub pid: u32,
    pub endpoint: String,
    pub classification: String,
    pub can_attach: bool,
    pub tab_count: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct InstancePickerDialog {
    open: bool,
    mode: InstancePickerMode,
    rows: Vec<InstancePickerRow>,
    selected: usize,
    last_error: Option<String>,
}

impl InstancePickerDialog {
    pub(crate) const fn new() -> Self {
        Self {
            open: false,
            mode: InstancePickerMode::Attach,
            rows: Vec::new(),
            selected: 0,
            last_error: None,
        }
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) const fn mode(&self) -> InstancePickerMode {
        self.mode
    }

    pub(crate) fn rows(&self) -> &[InstancePickerRow] {
        &self.rows
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn selected_row(&self) -> Option<&InstancePickerRow> {
        self.rows.get(self.selected)
    }

    pub(crate) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub(crate) fn open_with_rows(
        &mut self,
        mode: InstancePickerMode,
        rows: Vec<InstancePickerRow>,
    ) {
        self.open = true;
        self.mode = mode;
        self.rows = rows;
        self.selected = self.rows.iter().position(|row| row.can_attach).unwrap_or(0);
        self.last_error = None;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.rows.clear();
        self.selected = 0;
        self.last_error = None;
    }

    pub(crate) fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
    }

    pub(crate) fn select_next(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.rows.len();
    }

    pub(crate) fn select_prev(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.rows.len() - 1
        } else {
            self.selected - 1
        };
    }

    pub(crate) fn select_by_instance(&mut self, name: &str) -> Result<&InstancePickerRow, String> {
        let needle = name.trim();
        let idx = self
            .rows
            .iter()
            .position(|row| {
                row.instance == needle
                    || row.instance_label == needle
                    || row.instance.strip_prefix("custom:") == Some(needle)
                    || row.instance_label.ends_with(&format!("_{needle}"))
            })
            .ok_or_else(|| format!("instance `{name}` is not in the picker list"))?;
        self.selected = idx;
        Ok(&self.rows[idx])
    }

    pub(crate) fn select_by_pid(&mut self, pid: u32) -> Result<&InstancePickerRow, String> {
        let idx = self
            .rows
            .iter()
            .position(|row| row.pid == pid)
            .ok_or_else(|| format!("pid {pid} is not in the picker list"))?;
        self.selected = idx;
        Ok(&self.rows[idx])
    }

    pub(crate) fn snapshot_modal(&self) -> serde_json::Value {
        json!({
            "kind": "instance-picker",
            "mode": self.mode.as_str(),
            "selected": self.selected,
            "error": self.last_error,
            "rows": self.rows.iter().map(|row| json!({
                "instance": row.instance,
                "instance_label": row.instance_label,
                "pid": row.pid,
                "endpoint": row.endpoint,
                "classification": row.classification,
                "can_attach": row.can_attach,
                "tab_count": row.tab_count,
            })).collect::<Vec<_>>(),
            "actions": ["confirm", "cancel", "next", "prev"],
        })
    }
}

/// Build picker rows from the same registration directory as `server-list`.
pub(crate) fn collect_instance_picker_rows() -> Result<Vec<InstancePickerRow>, String> {
    let instances = discover_instances().map_err(|error| error.to_string())?;
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".to_owned());
    let mut rows = Vec::new();
    for instance in instances {
        let Some(endpoint) = instance.record.resolved_endpoint() else {
            continue;
        };
        let logical = instance.record.resolved_logical_instance();
        let owner = registration_owner_state(&instance.record);
        // Enter/switch is allowed whenever the owner process is still alive.
        // Empty tab trees and flaky protocol probes must not block the strip;
        // attach itself fails typed if the endpoint is truly unreachable.
        let alive = instance_process_is_alive(instance.record.pid);
        let (classification, can_attach) = match owner {
            RegistrationOwnerState::ConfirmedLive { .. } if alive && protocol_live(&endpoint) => {
                ("live".to_owned(), true)
            }
            RegistrationOwnerState::ConfirmedLive { .. }
            | RegistrationOwnerState::OwnerUnknown { .. }
                if alive =>
            {
                ("live-unverified".to_owned(), true)
            }
            RegistrationOwnerState::ConfirmedLive { .. }
            | RegistrationOwnerState::OwnerUnknown { .. } => ("stale".to_owned(), false),
            RegistrationOwnerState::Dead { .. } | RegistrationOwnerState::PidReused { .. } => {
                ("stale".to_owned(), false)
            }
        };
        rows.push(InstancePickerRow {
            instance: logical.canonical_name(),
            instance_label: logical.display_label(&username),
            pid: instance.record.pid,
            endpoint: endpoint.to_string(),
            classification,
            can_attach,
            tab_count: 0,
        });
    }
    rows.sort_by(|left, right| {
        right
            .can_attach
            .cmp(&left.can_attach)
            .then_with(|| left.instance.cmp(&right.instance))
            .then_with(|| left.pid.cmp(&right.pid))
    });
    Ok(rows)
}

fn protocol_live(endpoint: &IpcEndpoint) -> bool {
    crate::instances::protocol_probe_ok_for_picker(endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_refuses_stale_confirm_selection_helpers() {
        let mut dialog = InstancePickerDialog::new();
        dialog.open_with_rows(
            InstancePickerMode::Attach,
            vec![
                InstancePickerRow {
                    instance: "work".to_owned(),
                    instance_label: "u_work".to_owned(),
                    pid: 2,
                    endpoint: "pipe:work".to_owned(),
                    classification: "live".to_owned(),
                    can_attach: true,
                    tab_count: 1,
                },
                InstancePickerRow {
                    instance: "main".to_owned(),
                    instance_label: "u_main".to_owned(),
                    pid: 1,
                    endpoint: "pipe:main".to_owned(),
                    classification: "stale".to_owned(),
                    can_attach: false,
                    tab_count: 0,
                },
            ],
        );
        assert!(dialog.is_open());
        assert_eq!(dialog.selected_row().unwrap().instance, "work");
        let stale = dialog.select_by_instance("main").unwrap();
        assert!(!stale.can_attach);
        assert_eq!(dialog.snapshot_modal()["kind"], "instance-picker");
        assert_eq!(dialog.snapshot_modal()["mode"], "attach");
    }

    #[test]
    fn mode_parse_accepts_aliases() {
        assert_eq!(
            InstancePickerMode::parse("open-another"),
            Some(InstancePickerMode::OpenAnother)
        );
        assert_eq!(
            InstancePickerMode::parse("attach"),
            Some(InstancePickerMode::Attach)
        );
    }
}
