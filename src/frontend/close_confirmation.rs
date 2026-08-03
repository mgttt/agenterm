//! Shared live-tab close confirmation state and snapshot policy.

use serde_json::json;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CloseConfirmation {
    open: bool,
    target: Option<String>,
}

impl CloseConfirmation {
    pub(crate) const fn new() -> Self {
        Self {
            open: false,
            target: None,
        }
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub(crate) fn open(&mut self, target: String) {
        self.open = true;
        self.target = Some(target);
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.target = None;
    }

    pub(crate) fn snapshot_modal(&self) -> serde_json::Value {
        json!({
            "kind": "confirm-close-live",
            "window_id": self.target.as_deref().unwrap_or(""),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_close_confirmation_is_closed() {
        let confirmation = CloseConfirmation::new();
        assert!(!confirmation.is_open());
        assert_eq!(confirmation.target(), None);
    }

    #[test]
    fn open_exposes_target() {
        let mut confirmation = CloseConfirmation::new();
        confirmation.open("@7".to_owned());
        assert!(confirmation.is_open());
        assert_eq!(confirmation.target(), Some("@7"));
    }

    #[test]
    fn close_clears_target() {
        let mut confirmation = CloseConfirmation::new();
        confirmation.open("@7".to_owned());
        confirmation.close();
        assert!(!confirmation.is_open());
        assert_eq!(confirmation.target(), None);
    }

    #[test]
    fn snapshot_exposes_window_id_without_drafts() {
        let mut confirmation = CloseConfirmation::new();
        confirmation.open("@7".to_owned());
        let snapshot = confirmation.snapshot_modal();
        assert_eq!(snapshot["kind"], "confirm-close-live");
        assert_eq!(snapshot["window_id"], "@7");
    }
}
