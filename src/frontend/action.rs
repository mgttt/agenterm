//! Canonical product action identities shared by toolbar and shortcut surfaces.
//!
//! Win32 control IDs, winit events, and HTML elements remain adapter details.

pub const NEW_TAB: &str = "new-tab";
pub const TOGGLE_TABS: &str = "toggle-tabs";
pub const OPEN_CONTROL_CENTER: &str = "open-control-center";
pub const OPEN_SETTINGS: &str = "open-settings";
pub const TOGGLE_LOCALE: &str = "toggle-locale";
pub const FONT_DECREASE: &str = "font-decrease";
pub const FONT_INCREASE: &str = "font-increase";

/// Canonical left-to-right toolbar order. Every adapter `ORDER` must match.
pub const TOOLBAR_ACTION_ORDER: [&str; 7] = [
    TOGGLE_TABS,
    NEW_TAB,
    OPEN_CONTROL_CENTER,
    OPEN_SETTINGS,
    TOGGLE_LOCALE,
    FONT_DECREASE,
    FONT_INCREASE,
];

/// Reject adapter-local or stale identities before product dispatch.
pub fn is_toolbar_action_id(action_id: &str) -> bool {
    TOOLBAR_ACTION_ORDER.contains(&action_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_action_order_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for action_id in TOOLBAR_ACTION_ORDER {
            assert!(seen.insert(action_id), "duplicate {action_id}");
        }
        assert_eq!(TOOLBAR_ACTION_ORDER.len(), 7);
    }

    #[test]
    fn toolbar_action_ids_are_recognized() {
        assert!(is_toolbar_action_id(NEW_TAB));
        assert!(!is_toolbar_action_id("not-an-action"));
    }
}
