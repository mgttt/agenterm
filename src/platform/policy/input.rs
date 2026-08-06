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

/// Multi-click grouping window for double/triple click gestures.
///
/// This is host individuality, not a Unix commonality: macOS publishes the
/// user's setting as `NSEvent.doubleClickInterval` (System Settings ▸ Mouse),
/// while GNOME and KDE each keep their own separate preference. Until an
/// adapter reads the live value, every host uses the same conservative
/// default, and the divergence stays visible here rather than hiding as a bare
/// constant in one frontend.
///
/// TODO(macos): read `NSEvent.doubleClickInterval` instead of the default.
/// TODO(linux): read `org.gnome.desktop.peripherals.mouse double-click` (and
/// the KDE equivalent) instead of the default.
#[allow(dead_code)]
pub(crate) fn multi_click_interval_ms() -> u64 {
    DEFAULT_MULTI_CLICK_INTERVAL_MS
}

/// Matches the historical Unix frontend constant and Windows' own default.
const DEFAULT_MULTI_CLICK_INTERVAL_MS: u64 = 500;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_click_interval_is_positive_and_host_neutral_for_now() {
        // Pins the current answer so a host that starts reading a live system
        // preference has to update this test deliberately.
        assert_eq!(multi_click_interval_ms(), DEFAULT_MULTI_CLICK_INTERVAL_MS);
        assert!(multi_click_interval_ms() > 0);
    }

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
