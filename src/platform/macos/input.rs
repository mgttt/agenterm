//! Apple-native keyboard text and shortcut separation.
//!
//! Command drives product shortcuts. Control remains available to terminal
//! applications, while Shift, Option/dead-key composition, and IME output use
//! committed Unicode text supplied by the native window system.

#![cfg(target_os = "macos")]

use crate::platform::{KeyClassification, ModifierState};

pub(crate) const fn is_product_shortcut(modifiers: ModifierState) -> bool {
    modifiers.meta
}

/// Classify a pressed macOS key without reconstructing text from physical keys.
///
/// `committed_text` is the text resolved by winit/Cocoa, including keyboard
/// layout, dead-key, and IME composition. Command and Control chords are
/// classified before text so they cannot accidentally type their key label.
pub(crate) fn classify_key_press(
    modifiers: ModifierState,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> KeyClassification {
    if is_product_shortcut(modifiers) || modifiers.control {
        return shortcut_key(logical_character, named_key)
            .map(|key| KeyClassification::Shortcut { key, modifiers })
            .unwrap_or(KeyClassification::Ignored);
    }

    if let Some(text) = committed_text.filter(|text| !text.is_empty()) {
        return KeyClassification::TextCommit(text.to_string());
    }

    if let Some(name) = named_key {
        return KeyClassification::ControlKey {
            name: name.to_string(),
            modifiers,
        };
    }

    logical_character
        .filter(|text| !text.is_empty())
        .map(|text| KeyClassification::TextCommit(text.to_string()))
        .unwrap_or(KeyClassification::Ignored)
}

fn shortcut_key(logical_character: Option<&str>, named_key: Option<&str>) -> Option<String> {
    logical_character
        .filter(|text| !text.is_empty())
        .or(named_key)
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: ModifierState = ModifierState {
        control: false,
        shift: false,
        alt: false,
        meta: false,
    };
    const COMMAND: ModifierState = ModifierState { meta: true, ..NONE };
    const CONTROL: ModifierState = ModifierState {
        control: true,
        ..NONE
    };
    const SHIFT: ModifierState = ModifierState {
        shift: true,
        ..NONE
    };
    const OPTION: ModifierState = ModifierState { alt: true, ..NONE };

    #[test]
    fn command_is_the_product_shortcut_modifier() {
        assert!(is_product_shortcut(COMMAND));
        assert!(!is_product_shortcut(CONTROL));
        assert_eq!(
            classify_key_press(COMMAND, Some("c"), None, Some("c")),
            KeyClassification::Shortcut {
                key: "c".to_owned(),
                modifiers: COMMAND,
            }
        );
    }

    #[test]
    fn control_chords_stay_available_to_terminal_apps() {
        assert_eq!(
            classify_key_press(CONTROL, Some("c"), None, Some("c")),
            KeyClassification::Shortcut {
                key: "c".to_owned(),
                modifiers: CONTROL,
            }
        );
    }

    #[test]
    fn shifted_punctuation_uses_committed_text() {
        assert_eq!(
            classify_key_press(SHIFT, Some("1"), None, Some("!")),
            KeyClassification::TextCommit("!".to_owned())
        );
    }

    #[test]
    fn option_dead_keys_use_composed_commit() {
        assert_eq!(
            classify_key_press(OPTION, Some("e"), None, Some("é")),
            KeyClassification::TextCommit("é".to_owned())
        );
    }

    #[test]
    fn ime_commit_is_not_reconstructed_from_logical_key() {
        assert_eq!(
            classify_key_press(NONE, Some("n"), None, Some("你好")),
            KeyClassification::TextCommit("你好".to_owned())
        );
    }

    #[test]
    fn space_and_named_controls_remain_distinct() {
        assert_eq!(
            classify_key_press(NONE, None, Some("Space"), Some(" ")),
            KeyClassification::TextCommit(" ".to_owned())
        );
        assert_eq!(
            classify_key_press(NONE, None, Some("Escape"), None),
            KeyClassification::ControlKey {
                name: "Escape".to_owned(),
                modifiers: NONE,
            }
        );
    }
}
