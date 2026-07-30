//! Linux keyboard text / shortcut separation for platform migration slice 1.
//!
//! PRD invariants this adapter must preserve once wired:
//! - committed Unicode text is not reconstructed from physical key codes when
//!   the window system supplies text
//! - shortcuts and text commits remain distinct (Shift punctuation, layouts,
//!   dead keys, CJK, terminal controls, primary modifiers)
//!
//! Shared normalized event types are primary-owned. This file only holds
//! Linux-local classification helpers so the first vertical slice can land
//! without inventing `src/platform/mod.rs` contracts.

#![cfg(target_os = "linux")]

/// Linux primary shortcut modifier: Control or Super (not Option/Alt alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinuxModifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl LinuxModifiers {
    pub(crate) const fn primary_shortcut(self) -> bool {
        self.control || self.super_key
    }
}

/// Classification of one key press before product surfaces consume it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LinuxKeyClass {
    /// Chord intended as a shortcut (primary modifier held).
    Shortcut {
        key: String,
        modifiers: LinuxModifiers,
    },
    /// Committed Unicode text from the native window system / IME.
    TextCommit(String),
    /// Named control key without text payload (Enter, Escape, Backspace, …).
    ControlKey {
        name: String,
        modifiers: LinuxModifiers,
    },
    /// Ignore (release, repeat policy, or non-input).
    Ignored,
}

/// Separate shortcut chords from text commits.
///
/// `committed_text` is whatever the native path already resolved (winit
/// `text` / IME commit). When present and no primary shortcut modifier is
/// held, prefer [`LinuxKeyClass::TextCommit`] over physical-key synthesis.
pub(crate) fn classify_key_press(
    modifiers: LinuxModifiers,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> LinuxKeyClass {
    if modifiers.primary_shortcut() {
        if let Some(ch) = logical_character {
            return LinuxKeyClass::Shortcut {
                key: ch.to_string(),
                modifiers,
            };
        }
        if let Some(name) = named_key {
            return LinuxKeyClass::Shortcut {
                key: name.to_string(),
                modifiers,
            };
        }
        return LinuxKeyClass::Ignored;
    }

    if let Some(text) = committed_text.filter(|t| !t.is_empty()) {
        return LinuxKeyClass::TextCommit(text.to_string());
    }

    if let Some(name) = named_key {
        return LinuxKeyClass::ControlKey {
            name: name.to_string(),
            modifiers,
        };
    }

    if let Some(ch) = logical_character.filter(|t| !t.is_empty()) {
        return LinuxKeyClass::TextCommit(ch.to_string());
    }

    LinuxKeyClass::Ignored
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: LinuxModifiers = LinuxModifiers {
        control: false,
        shift: false,
        alt: false,
        super_key: false,
    };

    const CTRL: LinuxModifiers = LinuxModifiers {
        control: true,
        shift: false,
        alt: false,
        super_key: false,
    };

    const SHIFT: LinuxModifiers = LinuxModifiers {
        control: false,
        shift: true,
        alt: false,
        super_key: false,
    };

    #[test]
    fn primary_shortcut_stays_distinct_from_text() {
        let class = classify_key_press(CTRL, Some("c"), None, Some("c"));
        assert!(matches!(class, LinuxKeyClass::Shortcut { .. }));
    }

    #[test]
    fn shift_punctuation_uses_committed_text_not_shortcut() {
        let class = classify_key_press(SHIFT, Some("!"), None, Some("!"));
        assert_eq!(class, LinuxKeyClass::TextCommit("!".to_string()));
    }

    #[test]
    fn space_without_shortcut_is_text_commit() {
        let class = classify_key_press(NONE, None, Some("Space"), Some(" "));
        assert_eq!(class, LinuxKeyClass::TextCommit(" ".to_string()));
    }

    #[test]
    fn named_control_without_text_is_control_key() {
        let class = classify_key_press(NONE, None, Some("Escape"), None);
        assert_eq!(
            class,
            LinuxKeyClass::ControlKey {
                name: "Escape".to_string(),
                modifiers: NONE,
            }
        );
    }

    #[test]
    fn prefers_native_committed_text_over_logical_character() {
        let class = classify_key_press(NONE, Some("a"), None, Some("à"));
        assert_eq!(class, LinuxKeyClass::TextCommit("à".to_string()));
    }
}
