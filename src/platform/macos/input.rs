//! Apple-native keyboard text and shortcut separation.
//!
//! Command drives product shortcuts. Control remains available to terminal
//! applications, while Shift, Option/dead-key composition, and IME output use
//! committed Unicode text supplied by the native window system.

#![cfg(target_os = "macos")]

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MacosModifiers {
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

impl MacosModifiers {
    pub(crate) const fn primary_shortcut(self) -> bool {
        self.command
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MacosKeyClass {
    ProductShortcut {
        key: String,
        modifiers: MacosModifiers,
    },
    TerminalControlChord {
        key: String,
        modifiers: MacosModifiers,
    },
    TextCommit(String),
    ControlKey {
        name: String,
        modifiers: MacosModifiers,
    },
    Ignored,
}

/// Classify a pressed macOS key without reconstructing text from physical keys.
///
/// `committed_text` is the text resolved by winit/Cocoa, including keyboard
/// layout, dead-key, and IME composition. Command and Control chords are
/// classified before text so they cannot accidentally type their key label.
pub(crate) fn classify_key_press(
    modifiers: MacosModifiers,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> MacosKeyClass {
    if modifiers.primary_shortcut() {
        return shortcut_key(logical_character, named_key)
            .map(|key| MacosKeyClass::ProductShortcut { key, modifiers })
            .unwrap_or(MacosKeyClass::Ignored);
    }

    if modifiers.control {
        if let Some(key) = logical_character.filter(|text| !text.is_empty()) {
            return MacosKeyClass::TerminalControlChord {
                key: key.to_string(),
                modifiers,
            };
        }
        if let Some(name) = named_key {
            return MacosKeyClass::ControlKey {
                name: name.to_string(),
                modifiers,
            };
        }
        return MacosKeyClass::Ignored;
    }

    if let Some(text) = committed_text.filter(|text| !text.is_empty()) {
        return MacosKeyClass::TextCommit(text.to_string());
    }

    if let Some(name) = named_key {
        return MacosKeyClass::ControlKey {
            name: name.to_string(),
            modifiers,
        };
    }

    MacosKeyClass::Ignored
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

    const NONE: MacosModifiers = MacosModifiers {
        command: false,
        control: false,
        option: false,
        shift: false,
    };
    const COMMAND: MacosModifiers = MacosModifiers {
        command: true,
        ..NONE
    };
    const CONTROL: MacosModifiers = MacosModifiers {
        control: true,
        ..NONE
    };
    const SHIFT: MacosModifiers = MacosModifiers {
        shift: true,
        ..NONE
    };
    const OPTION: MacosModifiers = MacosModifiers {
        option: true,
        ..NONE
    };

    #[test]
    fn command_is_the_product_shortcut_modifier() {
        assert!(COMMAND.primary_shortcut());
        assert!(!CONTROL.primary_shortcut());
        assert_eq!(
            classify_key_press(COMMAND, Some("c"), None, Some("c")),
            MacosKeyClass::ProductShortcut {
                key: "c".to_owned(),
                modifiers: COMMAND,
            }
        );
    }

    #[test]
    fn control_chords_stay_available_to_terminal_apps() {
        assert_eq!(
            classify_key_press(CONTROL, Some("c"), None, Some("c")),
            MacosKeyClass::TerminalControlChord {
                key: "c".to_owned(),
                modifiers: CONTROL,
            }
        );
    }

    #[test]
    fn shifted_punctuation_uses_committed_text() {
        assert_eq!(
            classify_key_press(SHIFT, Some("1"), None, Some("!")),
            MacosKeyClass::TextCommit("!".to_owned())
        );
    }

    #[test]
    fn option_dead_keys_use_composed_commit() {
        assert_eq!(
            classify_key_press(OPTION, Some("e"), None, Some("é")),
            MacosKeyClass::TextCommit("é".to_owned())
        );
    }

    #[test]
    fn ime_commit_is_not_reconstructed_from_logical_key() {
        assert_eq!(
            classify_key_press(NONE, Some("n"), None, Some("你好")),
            MacosKeyClass::TextCommit("你好".to_owned())
        );
    }

    #[test]
    fn space_and_named_controls_remain_distinct() {
        assert_eq!(
            classify_key_press(NONE, None, Some("Space"), Some(" ")),
            MacosKeyClass::TextCommit(" ".to_owned())
        );
        assert_eq!(
            classify_key_press(NONE, None, Some("Escape"), None),
            MacosKeyClass::ControlKey {
                name: "Escape".to_owned(),
                modifiers: NONE,
            }
        );
    }
}
