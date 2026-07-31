//! Win32 toolbar hits mapped to stable product action identities.

#![cfg(target_os = "windows")]

use crate::platform::action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowsToolbarHit {
    ToggleTabs,
    NewTab,
    ControlCenter,
    Settings,
    ToggleLocale,
    FontDecrease,
    FontIncrease,
}

impl WindowsToolbarHit {
    #[cfg_attr(not(test), allow(dead_code))]
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_order_and_actions_match_the_shared_geometry() {
        assert_eq!(
            WindowsToolbarHit::ORDER.map(WindowsToolbarHit::action_id),
            [
                action::TOGGLE_TABS,
                action::NEW_TAB,
                action::OPEN_CONTROL_CENTER,
                action::OPEN_SETTINGS,
                action::TOGGLE_LOCALE,
                action::FONT_DECREASE,
                action::FONT_INCREASE,
            ]
        );
    }
}
