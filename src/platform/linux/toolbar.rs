//! Linux toolbar label/action bridge for platform migration slice 1.
//!
//! Product-visible action identities stay platform-neutral (`operations` /
//! `ui-action` aliases). This module only records the Linux-side mapping from
//! native toolbar hits to those identities. Shared platform trait shapes land
//! in primary-owned `src/platform/mod.rs` — do not duplicate them here.

#![cfg(target_os = "linux")]

/// Stable `ui-action` / operation aliases used by the slice-1 toolbar surface.
///
/// These are product semantic identities (PRD shared-contract consumers), not
/// OS capability grants.
pub(crate) mod action_id {
    pub const NEW_TAB: &str = "new-tab";
    pub const TOGGLE_TABS: &str = "toggle-tabs";
    pub const OPEN_SETTINGS: &str = "open-settings";
    pub const TOGGLE_LOCALE: &str = "toggle-locale";
    pub const FONT_DECREASE: &str = "font-decrease";
    pub const FONT_INCREASE: &str = "font-increase";
}

/// Native toolbar control hit on Linux (adapter-local until shared enums land).
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
    /// Map a native hit to the stable product action identity.
    pub(crate) const fn action_id(self) -> &'static str {
        match self {
            Self::NewTab => action_id::NEW_TAB,
            Self::ToggleTabs => action_id::TOGGLE_TABS,
            Self::Settings => action_id::OPEN_SETTINGS,
            Self::ToggleLocale => action_id::TOGGLE_LOCALE,
            Self::FontDecrease => action_id::FONT_DECREASE,
            Self::FontIncrease => action_id::FONT_INCREASE,
        }
    }
}

/// Resolved toolbar label strings for compact vs full layout.
///
/// Labels consumed by rendering and `ui-snapshot` must stay identical; the
/// Linux adapter must not invent a second label table after the shared
/// contract wires through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinuxToolbarLabels {
    pub new_tab: &'static str,
    pub tabs_show: &'static str,
    pub tabs_hide: &'static str,
    pub settings: &'static str,
    pub font_decrease: &'static str,
    pub font_increase: &'static str,
}

impl LinuxToolbarLabels {
    pub(crate) const FULL: Self = Self {
        new_tab: "+",
        tabs_show: "Tabs",
        tabs_hide: "Hide",
        settings: "Settings",
        font_decrease: "A-",
        font_increase: "A+",
    };

    pub(crate) const COMPACT: Self = Self {
        new_tab: "+",
        tabs_show: ">T",
        tabs_hide: "<T",
        settings: "S",
        font_decrease: "A-",
        font_increase: "A+",
    };

    pub(crate) const fn for_compact(compact: bool) -> Self {
        if compact { Self::COMPACT } else { Self::FULL }
    }

    pub(crate) const fn tabs_label(self, tabs_visible: bool) -> &'static str {
        if tabs_visible {
            self.tabs_hide
        } else {
            self.tabs_show
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_hits_map_to_stable_action_ids() {
        assert_eq!(LinuxToolbarHit::ToggleTabs.action_id(), "toggle-tabs");
        assert_eq!(LinuxToolbarHit::ToggleLocale.action_id(), "toggle-locale");
        assert_eq!(LinuxToolbarHit::FontIncrease.action_id(), "font-increase");
        assert_eq!(LinuxToolbarHit::FontDecrease.action_id(), "font-decrease");
        assert_eq!(LinuxToolbarHit::NewTab.action_id(), "new-tab");
        assert_eq!(LinuxToolbarHit::Settings.action_id(), "open-settings");
    }

    #[test]
    fn compact_and_full_labels_stay_distinct_for_tabs() {
        let full = LinuxToolbarLabels::for_compact(false);
        let compact = LinuxToolbarLabels::for_compact(true);
        assert_eq!(full.tabs_label(true), "Hide");
        assert_eq!(full.tabs_label(false), "Tabs");
        assert_eq!(compact.tabs_label(true), "<T");
        assert_eq!(compact.tabs_label(false), ">T");
        assert_eq!(full.settings, "Settings");
        assert_eq!(compact.settings, "S");
    }
}
