//! Product toolbar action mapping, independent of the native window toolkit.

use crate::platform::action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeToolbarHit {
    ToggleTabs,
    NewTab,
    ControlCenter,
    Settings,
    ToggleLocale,
    FontDecrease,
    FontIncrease,
}

impl NativeToolbarHit {
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
