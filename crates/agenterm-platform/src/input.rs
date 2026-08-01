//! Selected keyboard classification facade.

pub use crate::contract::input::{
    KeyClassification, KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent,
    PhysicalKeyCode, Utf16TextDecoder,
};

/// Adapter extension implemented for a native key-event type without exposing
/// that third-party type in the platform contract.
pub trait NativeKeyEventExt {
    fn to_normalized_key_event(&self, modifiers: ModifierState) -> NormalizedKeyEvent;
}
use crate::{contract::input::classify_key_press as classify_shared, selected};

pub const fn modifiers(control: bool, shift: bool, alt: bool, meta: bool) -> ModifierState {
    ModifierState {
        control,
        shift,
        alt,
        meta,
    }
}

pub fn is_primary_shortcut(modifiers: ModifierState) -> bool {
    selected::input::is_primary_shortcut(modifiers)
}

pub fn classify_key_press(
    modifiers: ModifierState,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> KeyClassification {
    classify_shared(
        is_primary_shortcut(modifiers),
        modifiers,
        logical_character,
        named_key,
        committed_text,
    )
}

pub fn classify_ime_commit(text: &str) -> KeyClassification {
    classify_key_press(ModifierState::empty(), None, None, Some(text))
}
