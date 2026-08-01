//! Platform-neutral keyboard modifier and text-classification contract.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModifierState {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
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

    pub const fn control_or_meta(self) -> bool {
        self.control || self.meta
    }

    pub const fn meta_only(self) -> bool {
        self.meta
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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

pub fn classify_key_press(
    is_shortcut: bool,
    modifiers: ModifierState,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> KeyClassification {
    if is_shortcut {
        if let Some(key) = logical_character.or(named_key) {
            return KeyClassification::Shortcut {
                key: key.to_owned(),
                modifiers,
            };
        }
        return KeyClassification::Ignored;
    }
    if let Some(text) = committed_text.filter(|value| !value.is_empty()) {
        return KeyClassification::TextCommit(text.to_owned());
    }
    if let Some(name) = named_key {
        return KeyClassification::ControlKey {
            name: name.to_owned(),
            modifiers,
        };
    }
    logical_character
        .filter(|value| !value.is_empty())
        .map_or(KeyClassification::Ignored, |text| {
            KeyClassification::TextCommit(text.to_owned())
        })
}

#[derive(Debug, Default)]
pub struct Utf16TextDecoder {
    pending_high_surrogate: Option<u16>,
}

impl Utf16TextDecoder {
    pub fn push(&mut self, value: u16) -> KeyClassification {
        let scalar = if (0xd800..=0xdbff).contains(&value) {
            self.pending_high_surrogate = Some(value);
            return KeyClassification::Ignored;
        } else if (0xdc00..=0xdfff).contains(&value) {
            let Some(high) = self.pending_high_surrogate.take() else {
                return KeyClassification::Ignored;
            };
            0x1_0000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(value) - 0xdc00)
        } else {
            self.pending_high_surrogate = None;
            u32::from(value)
        };
        char::from_u32(scalar).map_or(KeyClassification::Ignored, |character| {
            KeyClassification::TextCommit(character.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_text_and_named_controls_stay_distinct() {
        assert_eq!(
            classify_key_press(false, ModifierState::empty(), None, None, Some("你好")),
            KeyClassification::TextCommit("你好".to_owned())
        );
        assert!(matches!(
            classify_key_press(false, ModifierState::empty(), None, Some("Escape"), None),
            KeyClassification::ControlKey { .. }
        ));
    }

    #[test]
    fn meta_only_shortcuts_preserve_terminal_control_keys() {
        assert!(
            !ModifierState {
                control: true,
                ..ModifierState::empty()
            }
            .meta_only()
        );
        assert!(
            ModifierState {
                meta: true,
                ..ModifierState::empty()
            }
            .meta_only()
        );
    }

    #[test]
    fn utf16_decoder_preserves_surrogate_pairs() {
        let mut decoder = Utf16TextDecoder::default();
        assert_eq!(decoder.push(0xd83e), KeyClassification::Ignored);
        assert_eq!(
            decoder.push(0xdd80),
            KeyClassification::TextCommit("🦀".to_owned())
        );
    }
}
