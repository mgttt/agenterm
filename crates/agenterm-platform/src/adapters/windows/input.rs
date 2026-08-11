use crate::contract::input::ModifierState;

// Shared winit key-event normalization (winit types → platform NormalizedKeyEvent).
// Included here so the pixel-window host can run on Windows too.
#[cfg(feature = "portable-pixel-window")]
#[path = "../unix/input.rs"]
mod unix;

pub(crate) const fn is_primary_shortcut(modifiers: ModifierState) -> bool {
    modifiers.control && !modifiers.alt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn altgr_is_not_a_primary_shortcut() {
        assert!(!is_primary_shortcut(ModifierState {
            control: true,
            alt: true,
            shift: false,
            meta: false
        }));
    }
}
