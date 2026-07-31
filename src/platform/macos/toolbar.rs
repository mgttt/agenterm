//! macOS native toolbar hits mapped to stable product action identities.
//!
//! Labels remain shared product state. This adapter deliberately does not
//! duplicate them; rendering and `ui-snapshot` must consume the same resolved
//! label table after the primary-owned contract is wired.

#![cfg(target_os = "macos")]

use crate::platform::action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MacosToolbarHit {
    ToggleTabs,
    NewTab,
    ControlCenter,
    Settings,
    ToggleLocale,
    FontDecrease,
    FontIncrease,
}

impl MacosToolbarHit {
    /// Product order: two left controls, the centered Control Center entry,
    /// then the right-anchored settings/locale/font group.
    pub(crate) const ORDER: [Self; 7] = [
        Self::ToggleTabs,
        Self::NewTab,
        Self::ControlCenter,
        Self::Settings,
        Self::ToggleLocale,
        Self::FontDecrease,
        Self::FontIncrease,
    ];

    pub(crate) const fn action_id(self) -> &'static str {
        match self {
            Self::ToggleTabs => action::TOGGLE_TABS,
            Self::NewTab => action::NEW_TAB,
            Self::ControlCenter => action::OPEN_CONTROL_CENTER,
            Self::Settings => action::OPEN_SETTINGS,
            Self::ToggleLocale => action::TOGGLE_LOCALE,
            Self::FontDecrease => action::FONT_DECREASE,
            Self::FontIncrease => action::FONT_INCREASE,
        }
    }

    pub(crate) fn from_action_id(action_id: &str) -> Option<Self> {
        Self::ORDER
            .into_iter()
            .find(|hit| hit.action_id() == action_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_hits_map_to_stable_action_ids() {
        let action_ids = MacosToolbarHit::ORDER.map(MacosToolbarHit::action_id);
        assert_eq!(
            action_ids,
            [
                "toggle-tabs",
                "new-tab",
                "open-control-center",
                "open-settings",
                "toggle-locale",
                "font-decrease",
                "font-increase",
            ]
        );
    }

    #[test]
    fn adapter_does_not_translate_product_labels() {
        assert_eq!(MacosToolbarHit::ToggleLocale.action_id(), "toggle-locale");
        assert_eq!(MacosToolbarHit::FontIncrease.action_id(), "font-increase");
    }

    #[test]
    fn stable_action_ids_round_trip() {
        for hit in MacosToolbarHit::ORDER {
            assert_eq!(MacosToolbarHit::from_action_id(hit.action_id()), Some(hit));
        }
        assert_eq!(MacosToolbarHit::from_action_id("unknown"), None);
    }
}
