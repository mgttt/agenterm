//! Linux keyboard text / shortcut separation for platform migration slice 1.
//!
//! Bridges to shared contract revision 1 types:
//! [`crate::platform::ModifierState`], [`crate::platform::KeyClassification`],
//! and [`crate::platform::classify_key_press`].
//!
//! Linux primary shortcut policy: Control **or** Super (mapped to `meta`).
//! Alt alone never forms a product shortcut chord. Committed Unicode from the
//! native path / IME is preferred over physical-key synthesis.

#![cfg(target_os = "linux")]

use crate::platform::{KeyClassification, ModifierState, classify_key_press as classify_shared};

/// Build shared [`ModifierState`] from Linux modifier bits (`Super` → `meta`).
pub(crate) const fn linux_modifiers(
    control: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
) -> ModifierState {
    ModifierState {
        control,
        shift,
        alt,
        meta: super_key,
    }
}

/// Linux primary product-shortcut probe (Control or Super).
pub(crate) const fn primary_shortcut(modifiers: ModifierState) -> bool {
    modifiers.control_or_meta()
}

/// Classify a Linux key press using the shared contract helper.
///
/// `committed_text` is whatever winit / IME already resolved. When present and
/// no primary shortcut modifier is held, text commit wins over logical keys.
pub(crate) fn classify_key_press(
    modifiers: ModifierState,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> KeyClassification {
    classify_shared(modifiers, logical_character, named_key, committed_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: ModifierState = ModifierState::empty();
    const CTRL: ModifierState = linux_modifiers(true, false, false, false);
    const SUPER: ModifierState = linux_modifiers(false, false, false, true);
    const SHIFT: ModifierState = linux_modifiers(false, true, false, false);
    const ALT: ModifierState = linux_modifiers(false, false, true, false);

    #[test]
    fn control_and_super_are_primary_shortcuts() {
        assert!(primary_shortcut(CTRL));
        assert!(primary_shortcut(SUPER));
        assert!(!primary_shortcut(SHIFT));
        assert!(!primary_shortcut(ALT));
        assert!(!primary_shortcut(NONE));
    }

    #[test]
    fn primary_shortcut_stays_distinct_from_text() {
        let class = classify_key_press(CTRL, Some("c"), None, Some("c"));
        assert_eq!(
            class,
            KeyClassification::Shortcut {
                key: "c".to_string(),
                modifiers: CTRL,
            }
        );
    }

    #[test]
    fn super_chord_is_shortcut_not_text() {
        let class = classify_key_press(SUPER, Some("t"), None, Some("t"));
        assert_eq!(
            class,
            KeyClassification::Shortcut {
                key: "t".to_string(),
                modifiers: SUPER,
            }
        );
    }

    #[test]
    fn shift_punctuation_uses_committed_text_not_shortcut() {
        let class = classify_key_press(SHIFT, Some("!"), None, Some("!"));
        assert_eq!(class, KeyClassification::TextCommit("!".to_string()));
    }

    #[test]
    fn space_without_shortcut_is_text_commit() {
        let class = classify_key_press(NONE, None, Some("Space"), Some(" "));
        assert_eq!(class, KeyClassification::TextCommit(" ".to_string()));
    }

    #[test]
    fn named_control_without_text_is_control_key() {
        let class = classify_key_press(NONE, None, Some("Escape"), None);
        assert_eq!(
            class,
            KeyClassification::ControlKey {
                name: "Escape".to_string(),
                modifiers: NONE,
            }
        );
    }

    #[test]
    fn prefers_native_committed_text_over_logical_character() {
        let class = classify_key_press(NONE, Some("a"), None, Some("à"));
        assert_eq!(class, KeyClassification::TextCommit("à".to_string()));
    }

    #[test]
    fn cjk_ime_commit_is_not_reconstructed_from_logical_key() {
        let class = classify_key_press(NONE, Some("n"), None, Some("你好"));
        assert_eq!(class, KeyClassification::TextCommit("你好".to_string()));
    }
}
