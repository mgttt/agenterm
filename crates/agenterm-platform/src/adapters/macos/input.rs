use crate::contract::input::ModifierState;

#[path = "../unix/input.rs"]
mod unix;

pub(crate) const fn is_primary_shortcut(modifiers: ModifierState) -> bool {
    modifiers.meta_only()
}
