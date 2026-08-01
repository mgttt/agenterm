//! Platform-neutral keyboard modifier and text-classification contract.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModifierState {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyPressState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NamedKey {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Backspace,
    Delete,
    End,
    Enter,
    Escape,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Home,
    Insert,
    PageDown,
    PageUp,
    Space,
    Tab,
}

impl NamedKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArrowDown => "ArrowDown",
            Self::ArrowLeft => "ArrowLeft",
            Self::ArrowRight => "ArrowRight",
            Self::ArrowUp => "ArrowUp",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::End => "End",
            Self::Enter => "Enter",
            Self::Escape => "Escape",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
            Self::Home => "Home",
            Self::Insert => "Insert",
            Self::PageDown => "PageDown",
            Self::PageUp => "PageUp",
            Self::Space => "Space",
            Self::Tab => "Tab",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PhysicalKeyCode {
    Letter(char),
    Digit(u8),
    Backspace,
    Enter,
    Space,
    Tab,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LogicalKey {
    Character(String),
    Named(NamedKey),
    Unidentified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedKeyEvent {
    pub logical: LogicalKey,
    pub physical: PhysicalKeyCode,
    pub text: Option<String>,
    pub state: KeyPressState,
    pub repeat: bool,
    pub modifiers: ModifierState,
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
    if let Some(text) = committed_text
        .filter(|value| !value.is_empty() && value.chars().any(|character| !character.is_control()))
    {
        return KeyClassification::TextCommit(text.to_owned());
    }
    if let Some(name) = named_key {
        return KeyClassification::ControlKey {
            name: name.to_owned(),
            modifiers,
        };
    }
    if let Some(text) = committed_text.filter(|value| !value.is_empty()) {
        return KeyClassification::TextCommit(text.to_owned());
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
        assert!(matches!(
            classify_key_press(
                false,
                ModifierState::empty(),
                None,
                Some("Enter"),
                Some("\r"),
            ),
            KeyClassification::ControlKey { name, .. } if name == "Enter"
        ));
        assert_eq!(
            classify_key_press(
                false,
                ModifierState::empty(),
                None,
                Some("Space"),
                Some(" "),
            ),
            KeyClassification::TextCommit(" ".to_owned())
        );
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
    fn normalized_shift_tab_keeps_named_key_and_modifier() {
        let event = NormalizedKeyEvent {
            logical: LogicalKey::Named(NamedKey::Tab),
            physical: PhysicalKeyCode::Tab,
            text: None,
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers: ModifierState {
                shift: true,
                ..ModifierState::empty()
            },
        };
        assert_eq!(event.logical, LogicalKey::Named(NamedKey::Tab));
        assert!(event.modifiers.shift);
        assert_eq!(NamedKey::Tab.as_str(), "Tab");
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
