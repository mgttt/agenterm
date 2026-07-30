//! Native platform abstraction contracts (`prd/PRD_02_20_native_platform.md`).
//!
//! **Contract revision 1** freezes normalized product-action identities, key
//! classification, capability status, and display-backend facts with
//! table-driven unit tests. No GUI behavior is moved in this revision.
//!
//! Ownership (PRD parallel rules):
//! - primary owns this file, shared semantics, Windows adaptation, and final
//!   integration;
//! - macOS / Linux agents own only their adapter trees and native evidence;
//! - adapter agents must request a contract change instead of editing this
//!   file's semantics.
//!
//! Adapter modules are declared only when the corresponding tree exists.

#![allow(dead_code)]

/// Frozen shared-contract revision implemented by this module.
pub const CONTRACT_REVISION: u32 = 1;

#[cfg(target_os = "linux")]
pub mod linux;

/// Stable product action identities consumed by toolbar / shortcut surfaces.
///
/// Win32 control IDs, winit events, and HTML elements remain adapter details.
pub mod action {
    pub const NEW_TAB: &str = "new-tab";
    pub const TOGGLE_TABS: &str = "toggle-tabs";
    pub const OPEN_SETTINGS: &str = "open-settings";
    pub const TOGGLE_LOCALE: &str = "toggle-locale";
    pub const FONT_DECREASE: &str = "font-decrease";
    pub const FONT_INCREASE: &str = "font-increase";
}

/// Which operating-system adapter identity is speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformKind {
    Windows,
    Macos,
    Linux,
}

/// Capability surface an adapter may expose (capability-oriented, not one
/// global `OsLayer` object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityKind {
    Window,
    Input,
    Ime,
    Clipboard,
    Font,
    Screenshot,
    Activation,
    Integration,
}

/// Explicit availability of a capability. Missing behavior must not be hidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityStatus {
    Available,
    Unsupported { reason: &'static str },
    Failed { code: &'static str, message: String },
}

/// Modifier bits carried on normalized key events.
///
/// Which modifier is the platform *primary* shortcut chord remains an adapter
/// decision (Control vs Super/Command); shared helpers only expose the bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModifierState {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    /// Super on Linux, Win on Windows, Command on macOS.
    pub meta: bool,
}

impl ModifierState {
    pub const fn empty() -> Self {
        Self {
            control: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    /// Control or meta held — common primary-chord probe used by slice-1
    /// adapters. Platform-specific primary policy still belongs in the adapter.
    pub const fn control_or_meta(self) -> bool {
        self.control || self.meta
    }
}

/// Classification of one key press before product surfaces consume it.
///
/// Text commits must stay distinct from shortcut chords so Shift punctuation,
/// layouts, dead keys, CJK, and terminal control keys keep native meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyClassification {
    Shortcut {
        key: String,
        modifiers: ModifierState,
    },
    TextCommit(String),
    ControlKey {
        name: String,
        modifiers: ModifierState,
    },
    Ignored,
}

/// Display / window-system discovery facts (not auth).
///
/// Linux populates X11/Wayland; other platforms leave those flags false and
/// report headless through their own window capability diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DisplayBackendFacts {
    pub x11: bool,
    pub wayland: bool,
    pub headless: bool,
}

/// Separate shortcut chords from committed Unicode text.
///
/// `committed_text` is whatever the native path already resolved. When present
/// and no control/meta shortcut chord is held, prefer [`KeyClassification::TextCommit`]
/// over physical-key synthesis.
pub fn classify_key_press(
    modifiers: ModifierState,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> KeyClassification {
    if modifiers.control_or_meta() {
        if let Some(ch) = logical_character {
            return KeyClassification::Shortcut {
                key: ch.to_string(),
                modifiers,
            };
        }
        if let Some(name) = named_key {
            return KeyClassification::Shortcut {
                key: name.to_string(),
                modifiers,
            };
        }
        return KeyClassification::Ignored;
    }

    if let Some(text) = committed_text.filter(|value| !value.is_empty()) {
        return KeyClassification::TextCommit(text.to_string());
    }

    if let Some(name) = named_key {
        return KeyClassification::ControlKey {
            name: name.to_string(),
            modifiers,
        };
    }

    if let Some(ch) = logical_character.filter(|value| !value.is_empty()) {
        return KeyClassification::TextCommit(ch.to_string());
    }

    KeyClassification::Ignored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_revision_is_frozen_at_one() {
        assert_eq!(CONTRACT_REVISION, 1);
    }

    #[test]
    fn product_action_identities_match_prd_examples() {
        let expected = [
            ("new-tab", action::NEW_TAB),
            ("toggle-tabs", action::TOGGLE_TABS),
            ("open-settings", action::OPEN_SETTINGS),
            ("toggle-locale", action::TOGGLE_LOCALE),
            ("font-decrease", action::FONT_DECREASE),
            ("font-increase", action::FONT_INCREASE),
        ];
        for (want, got) in expected {
            assert_eq!(got, want);
        }
    }

    #[test]
    fn key_classification_table() {
        let none = ModifierState::empty();
        let ctrl = ModifierState {
            control: true,
            ..ModifierState::empty()
        };
        let shift = ModifierState {
            shift: true,
            ..ModifierState::empty()
        };

        let cases = [
            (
                "primary shortcut stays distinct from text",
                ctrl,
                Some("c"),
                None,
                Some("c"),
                KeyClassification::Shortcut {
                    key: "c".to_string(),
                    modifiers: ctrl,
                },
            ),
            (
                "shift punctuation uses committed text",
                shift,
                Some("!"),
                None,
                Some("!"),
                KeyClassification::TextCommit("!".to_string()),
            ),
            (
                "space without shortcut is text commit",
                none,
                None,
                Some("Space"),
                Some(" "),
                KeyClassification::TextCommit(" ".to_string()),
            ),
            (
                "named control without text is control key",
                none,
                None,
                Some("Escape"),
                None,
                KeyClassification::ControlKey {
                    name: "Escape".to_string(),
                    modifiers: none,
                },
            ),
            (
                "native committed text wins over logical character",
                none,
                Some("a"),
                None,
                Some("à"),
                KeyClassification::TextCommit("à".to_string()),
            ),
        ];

        for (label, modifiers, logical, named, committed, want) in cases {
            assert_eq!(
                classify_key_press(modifiers, logical, named, committed),
                want,
                "{label}"
            );
        }
    }

    #[test]
    fn capability_status_keeps_failures_explicit() {
        let unsupported = CapabilityStatus::Unsupported {
            reason: "headless-display",
        };
        let failed = CapabilityStatus::Failed {
            code: "clipboard_unavailable",
            message: "wl-clipboard missing".to_string(),
        };
        assert!(matches!(
            unsupported,
            CapabilityStatus::Unsupported {
                reason: "headless-display"
            }
        ));
        assert!(matches!(
            failed,
            CapabilityStatus::Failed {
                code: "clipboard_unavailable",
                ..
            }
        ));
    }
}
