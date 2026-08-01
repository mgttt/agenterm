use agenterm_platform::input::{self, KeyClassification, ModifierState};

pub(crate) const fn macos_modifiers(
    control: bool,
    shift: bool,
    option: bool,
    command: bool,
) -> ModifierState {
    input::modifiers(control, shift, option, command)
}

pub(crate) fn is_product_shortcut(modifiers: ModifierState) -> bool {
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
