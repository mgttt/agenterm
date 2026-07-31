//! Windows keyboard and UTF-16 text adaptation.
//! Adapter-private native mechanism selected only by platform::selected.

#![cfg(target_os = "windows")]

use crate::platform::{KeyClassification, ModifierState, classify_key_press};

pub(crate) const fn windows_modifiers(
    control: bool,
    shift: bool,
    alt: bool,
    windows_key: bool,
) -> ModifierState {
    ModifierState {
        control,
        shift,
        alt,
        meta: windows_key,
    }
}

/// Product and terminal Control chords exclude Ctrl+Alt because Windows uses
/// that pair for AltGr on many keyboard layouts.
pub(crate) const fn primary_shortcut(modifiers: ModifierState) -> bool {
    modifiers.control && !modifiers.alt
}

pub(crate) fn classify_windows_key(
    modifiers: ModifierState,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> KeyClassification {
    classify_key_press(
        primary_shortcut(modifiers),
        modifiers,
        logical_character,
        named_key,
        committed_text,
    )
}

#[derive(Debug, Default)]
pub(crate) struct Utf16TextDecoder {
    pending_high_surrogate: Option<u16>,
}

impl Utf16TextDecoder {
    pub(crate) fn push(&mut self, value: u16) -> KeyClassification {
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
            let text = character.to_string();
            classify_windows_key(ModifierState::empty(), Some(&text), None, Some(&text))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_chord_is_a_shortcut() {
        let modifiers = windows_modifiers(true, false, false, false);
        assert_eq!(
            classify_windows_key(modifiers, Some("c"), None, Some("c")),
            KeyClassification::Shortcut {
                key: "c".to_string(),
                modifiers,
            }
        );
    }

    #[test]
    fn altgr_committed_text_is_not_swallowed_as_a_shortcut() {
        let modifiers = windows_modifiers(true, false, true, false);
        assert_eq!(
            classify_windows_key(modifiers, Some("q"), None, Some("@")),
            KeyClassification::TextCommit("@".to_string())
        );
    }

    #[test]
    fn utf16_decoder_preserves_bmp_and_surrogate_pair_text() {
        let mut decoder = Utf16TextDecoder::default();
        assert_eq!(
            decoder.push('你' as u16),
            KeyClassification::TextCommit("你".to_string())
        );
        assert_eq!(decoder.push(0xd83e), KeyClassification::Ignored);
        assert_eq!(
            decoder.push(0xdd80),
            KeyClassification::TextCommit("🦀".to_string())
        );
    }

    #[test]
    fn utf16_decoder_rejects_an_orphan_low_surrogate() {
        assert_eq!(
            Utf16TextDecoder::default().push(0xdc00),
            KeyClassification::Ignored
        );
    }
}
