use agenterm_platform::input::{self, KeyClassification, ModifierState};

pub(crate) const fn linux_modifiers(
    control: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
) -> ModifierState {
    input::modifiers(control, shift, alt, super_key)
}

pub(crate) fn primary_shortcut(modifiers: ModifierState) -> bool {
    input::is_primary_shortcut(modifiers)
}

pub(crate) fn classify_key_press(
    modifiers: ModifierState,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> KeyClassification {
    input::classify_key_press(modifiers, logical_character, named_key, committed_text)
}

pub(crate) fn classify_ime_commit(text: &str) -> KeyClassification {
    input::classify_ime_commit(text)
}
