//! Product input/UX policy shared by frontend hosts.
//!
//! The platform crate owns normalized key classification; these tables decide
//! the AgenTerm-level shortcut shape and empty-copy behavior per host.

use crate::platform::ModifierState;

#[allow(dead_code)]
pub(crate) fn primary_text_field_shortcut_modifiers() -> ModifierState {
    if matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Macos
    ) {
        ModifierState {
            control: false,
            shift: false,
            alt: false,
            meta: true,
        }
    } else {
        ModifierState {
            control: true,
            shift: false,
            alt: false,
            meta: false,
        }
    }
}

#[allow(dead_code)]
pub(crate) fn is_primary_shortcut_via_meta() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Macos
    )
}

#[allow(dead_code)]
pub(crate) fn terminal_shortcut_empty_copy_action_is_suppressed() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Macos
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_primary_shortcut_uses_meta() {
        if is_primary_shortcut_via_meta() {
            assert!(primary_text_field_shortcut_modifiers().meta);
            assert!(!primary_text_field_shortcut_modifiers().control);
        }
    }

    #[test]
    fn empty_copy_suppression_is_explicit() {
        assert_eq!(
            terminal_shortcut_empty_copy_action_is_suppressed(),
            is_primary_shortcut_via_meta()
        );
    }
}
