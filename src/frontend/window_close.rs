//! Shared window-close confirmation state, choices, and snapshot policy.

use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowCloseChoice {
    KeepServerRunning,
    StopServerAndExit,
    Cancel,
}

impl WindowCloseChoice {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::KeepServerRunning => "keep-server-running",
            Self::StopServerAndExit => "stop-server-and-exit",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WindowCloseDialog {
    open: bool,
}

impl WindowCloseDialog {
    pub(crate) const fn new() -> Self {
        Self { open: false }
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn open(&mut self) {
        self.open = true;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    pub(crate) fn snapshot_modal(&self) -> serde_json::Value {
        json!({
            "kind": "confirm-window-close",
            "default_action": WindowCloseChoice::KeepServerRunning.as_str(),
            "actions": [
                WindowCloseChoice::KeepServerRunning.as_str(),
                WindowCloseChoice::StopServerAndExit.as_str(),
                WindowCloseChoice::Cancel.as_str(),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_window_close_dialog_is_closed() {
        let dialog = WindowCloseDialog::new();
        assert!(!dialog.is_open());
    }

    #[test]
    fn open_and_close_round_trip() {
        let mut dialog = WindowCloseDialog::new();
        dialog.open();
        assert!(dialog.is_open());
        dialog.close();
        assert!(!dialog.is_open());
    }

    #[test]
    fn snapshot_exposes_shared_choices() {
        let mut dialog = WindowCloseDialog::new();
        dialog.open();
        let snapshot = dialog.snapshot_modal();
        assert_eq!(snapshot["kind"], "confirm-window-close");
        assert_eq!(snapshot["default_action"], "keep-server-running");
        assert_eq!(snapshot["actions"][2], "cancel");
    }
}
