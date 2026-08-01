pub(crate) use agenterm_platform::input::Utf16TextDecoder;
use agenterm_platform::input::{self, ModifierState};

pub(crate) const fn windows_modifiers(
    control: bool,
    shift: bool,
    alt: bool,
    windows_key: bool,
) -> ModifierState {
    input::modifiers(control, shift, alt, windows_key)
}

pub(crate) fn primary_shortcut(modifiers: ModifierState) -> bool {
    input::is_primary_shortcut(modifiers)
}
