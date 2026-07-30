//! Apple-native keyboard text and shortcut separation.
//!
//! Command drives product shortcuts. Control remains available to terminal
//! applications, while Shift, Option/dead-key composition, and IME output use
//! committed Unicode text supplied by the native window system.

#![cfg(target_os = "macos")]

use crate::platform::{KeyClassification, ModifierState, classify_key_press as classify_shared};

pub(crate) const fn macos_modifiers(
    control: bool,
    shift: bool,
    option: bool,
    command: bool,
) -> ModifierState {
    ModifierState {
        control,
        shift,
        alt: option,
        meta: command,
    }
}

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
    classify_shared(
        is_product_shortcut(modifiers) || modifiers.control,
        modifiers,
        logical_character,
        named_key,
        committed_text,
    )
}

pub(crate) fn classify_ime_commit(text: &str) -> KeyClassification {
    classify_key_press(ModifierState::empty(), None, None, Some(text))
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
    fn modifier_bridge_preserves_cocoa_semantics() {
        assert_eq!(
            macos_modifiers(true, true, true, true),
            ModifierState {
                control: true,
                shift: true,
                alt: true,
                meta: true,
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
    fn ime_commit_helper_uses_committed_unicode() {
        assert_eq!(
            classify_ime_commit("你好"),
            KeyClassification::TextCommit("你好".to_owned())
        );
        assert_eq!(classify_ime_commit(""), KeyClassification::Ignored);
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
