//! Linux toolbar hits mapped to stable product action identities.
//!
//! Bridges to shared contract revision 1 [`crate::platform::action`].
//! Labels remain shared product/render state (`unix_app` / locale); this
//! adapter deliberately does not own a second label table so rendering and
//! `ui-snapshot` cannot diverge.

#![cfg(target_os = "linux")]

use crate::platform::action;

/// Native toolbar control hit on Linux (adapter-local detail).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinuxToolbarHit {
    NewTab,
    ToggleTabs,
    Settings,
    ToggleLocale,
    FontDecrease,
    FontIncrease,
}

impl LinuxToolbarHit {
    /// Visible left-to-right order matching the Unix workspace toolbar.
    pub(crate) const ORDER: [Self; 6] = [
        Self::NewTab,
        Self::ToggleTabs,
        Self::Settings,
        Self::ToggleLocale,
        Self::FontDecrease,
        Self::FontIncrease,
    ];

    /// Map a native hit to the stable product action identity.
    pub(crate) const fn action_id(self) -> &'static str {
        match self {
            Self::NewTab => action::NEW_TAB,
            Self::ToggleTabs => action::TOGGLE_TABS,
            Self::Settings => action::OPEN_SETTINGS,
            Self::ToggleLocale => action::TOGGLE_LOCALE,
            Self::FontDecrease => action::FONT_DECREASE,
            Self::FontIncrease => action::FONT_INCREASE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_hits_map_to_shared_action_ids() {
        let action_ids = LinuxToolbarHit::ORDER.map(LinuxToolbarHit::action_id);
        assert_eq!(
            action_ids,
            [
                action::NEW_TAB,
                action::TOGGLE_TABS,
                action::OPEN_SETTINGS,
                action::TOGGLE_LOCALE,
                action::FONT_DECREASE,
                action::FONT_INCREASE,
            ]
        );
    }

    #[test]
    fn action_ids_match_prd_string_literals() {
        assert_eq!(LinuxToolbarHit::ToggleTabs.action_id(), "toggle-tabs");
        assert_eq!(LinuxToolbarHit::ToggleLocale.action_id(), "toggle-locale");
        assert_eq!(LinuxToolbarHit::FontIncrease.action_id(), "font-increase");
        assert_eq!(LinuxToolbarHit::FontDecrease.action_id(), "font-decrease");
        assert_eq!(LinuxToolbarHit::NewTab.action_id(), "new-tab");
        assert_eq!(LinuxToolbarHit::Settings.action_id(), "open-settings");
    }
}
