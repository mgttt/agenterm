use crate::contract::input::ModifierState;

pub(crate) const fn is_primary_shortcut(modifiers: ModifierState) -> bool {
    modifiers.control_or_meta()
}
