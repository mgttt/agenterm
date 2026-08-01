//! Linux native platform adapter (`prd/PRD_02_20_native_platform.md`).
//! Adapter-private native mechanism selected only by platform::selected.
//!
//! Ownership: Linux agent only. Do not edit shared contracts here.
//!
//! Implements shared **contract revision 3** (`crate::platform::CONTRACT_REVISION`):
//! - slice-1: toolbar action ids + keyboard text/shortcut separation
//! - slice-2: IME + clipboard + DPI/scale + font + screenshot + activation
//!
//! Linux `unix_app` hot paths call into this module under
//! `cfg(target_os = "linux")`. macOS behavior stays on the unbridged path.
//!
//! Public capability identity stays on the Linux branch.

#![cfg(target_os = "linux")]

use crate::platform::{
    CONTRACT_REVISION, CapabilityKind, CapabilityStatus, DisplayBackendFacts, PlatformKind,
};

pub(crate) mod activation;
pub(crate) mod font;
pub(crate) mod ime;
pub(crate) mod input;
pub(crate) mod scale;
pub(crate) mod screenshot;
pub(crate) mod toolbar;

/// Contract revision this Linux adapter tree implements.
pub(crate) const IMPLEMENTED_CONTRACT_REVISION: u32 = CONTRACT_REVISION;

/// Linux adapter identity for capability / evidence surfaces.
pub(crate) const fn platform_kind() -> PlatformKind {
    PlatformKind::Linux
}

/// Best-effort X11/Wayland discovery from environment variables.
///
/// Facts are capability metadata only (not authorization). Headless must
/// surface as a typed unsupported/failure status — never claim Available.
pub(crate) fn display_facts_from_env() -> DisplayBackendFacts {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    DisplayBackendFacts {
        x11,
        wayland,
        headless: !(x11 || wayland),
    }
}

/// Window capability status derived from display-backend facts.
pub(crate) fn window_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    if facts.headless {
        CapabilityStatus::Unsupported {
            reason: "headless-display",
        }
    } else {
        CapabilityStatus::Available
    }
}

/// Input capability is available whenever a display backend is present.
pub(crate) fn input_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    window_capability_status(facts)
}

/// Slice-1 capabilities this adapter speaks for.
pub(crate) fn slice1_capability_kinds() -> [CapabilityKind; 2] {
    [CapabilityKind::Window, CapabilityKind::Input]
}

/// Slice-2 first-cut capabilities (IME + clipboard) in addition to slice-1.
pub(crate) fn slice2_ime_clipboard_capability_kinds() -> [CapabilityKind; 2] {
    [CapabilityKind::Ime, CapabilityKind::Clipboard]
}

/// Slice-2 second-cut: DPI/scale rides Window (facts + typed scale helpers).
pub(crate) fn slice2_dpi_scale_capability_kinds() -> [CapabilityKind; 1] {
    [CapabilityKind::Window]
}

/// Slice-2 third-cut: font discovery / primary metrics.
pub(crate) fn slice2_font_capability_kinds() -> [CapabilityKind; 1] {
    [CapabilityKind::Font]
}

/// Slice-2 fourth-cut: screenshot encode + activation / no-activate policy.
pub(crate) fn slice2_screenshot_activation_capability_kinds() -> [CapabilityKind; 2] {
    [CapabilityKind::Screenshot, CapabilityKind::Activation]
}

/// Resolve a capability kind to its current Linux status.
pub(crate) fn capability_status(kind: CapabilityKind) -> CapabilityStatus {
    let facts = display_facts_from_env();
    match kind {
        CapabilityKind::Window => {
            // Window includes DPI/scale discovery when a display backend exists.
            let window = window_capability_status(facts);
            if matches!(window, CapabilityStatus::Available) {
                scale::scale_capability_status(facts)
            } else {
                window
            }
        }
        CapabilityKind::Input => input_capability_status(facts),
        CapabilityKind::Ime => ime::ime_capability_status(facts),
        CapabilityKind::Clipboard => CapabilityStatus::Available,
        CapabilityKind::Font => font::font_capability_status(),
        CapabilityKind::Screenshot => screenshot::screenshot_capability_status(facts),
        CapabilityKind::Activation => activation::activation_capability_status(facts),
        CapabilityKind::Integration => CapabilityStatus::Unsupported {
            reason: "deferred-slice",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implements_contract_revision_3() {
        assert_eq!(IMPLEMENTED_CONTRACT_REVISION, 3);
        assert_eq!(IMPLEMENTED_CONTRACT_REVISION, CONTRACT_REVISION);
        assert_eq!(platform_kind(), PlatformKind::Linux);
    }

    #[test]
    fn display_facts_bridge_to_shared_type() {
        let facts = display_facts_from_env();
        // This Desktop agent runs with DISPLAY=:1; treat missing both as headless.
        if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
            assert!(!facts.headless);
            assert!(matches!(
                window_capability_status(facts),
                CapabilityStatus::Available
            ));
            assert!(matches!(
                input_capability_status(facts),
                CapabilityStatus::Available
            ));
        } else {
            assert!(facts.headless);
            assert!(matches!(
                window_capability_status(facts),
                CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
            ));
        }
    }

    #[test]
    fn slice1_exposes_window_and_input_kinds() {
        assert_eq!(
            slice1_capability_kinds(),
            [CapabilityKind::Window, CapabilityKind::Input]
        );
    }

    #[test]
    fn slice2_first_cut_exposes_ime_and_clipboard_kinds() {
        assert_eq!(
            slice2_ime_clipboard_capability_kinds(),
            [CapabilityKind::Ime, CapabilityKind::Clipboard]
        );
    }

    #[test]
    fn slice2_dpi_scale_rides_window_capability() {
        assert_eq!(
            slice2_dpi_scale_capability_kinds(),
            [CapabilityKind::Window]
        );
        let status = capability_status(CapabilityKind::Window);
        if display_facts_from_env().headless {
            assert!(matches!(
                status,
                CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
            ));
        } else {
            assert_eq!(status, CapabilityStatus::Available);
            assert_eq!(
                scale::scale_capability_status_from_env(),
                CapabilityStatus::Available
            );
        }
    }

    #[test]
    fn slice2_font_exposes_font_capability() {
        assert_eq!(slice2_font_capability_kinds(), [CapabilityKind::Font]);
        let status = capability_status(CapabilityKind::Font);
        assert!(
            matches!(
                status,
                CapabilityStatus::Available
                    | CapabilityStatus::Failed {
                        code: "font_unavailable" | "font_metrics_failed",
                        ..
                    }
            ),
            "font must be Available or typed Failed, got {status:?}"
        );
        assert_ne!(
            status,
            CapabilityStatus::Unsupported {
                reason: "deferred-slice"
            }
        );
    }

    #[test]
    fn slice2_screenshot_and_activation_are_available_on_desktop() {
        assert_eq!(
            slice2_screenshot_activation_capability_kinds(),
            [CapabilityKind::Screenshot, CapabilityKind::Activation]
        );
        let screenshot = capability_status(CapabilityKind::Screenshot);
        let activation = capability_status(CapabilityKind::Activation);
        if display_facts_from_env().headless {
            assert!(matches!(
                screenshot,
                CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
            ));
            assert!(matches!(
                activation,
                CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
            ));
        } else {
            assert_eq!(screenshot, CapabilityStatus::Available);
            assert_eq!(activation, CapabilityStatus::Available);
        }
        assert_ne!(
            screenshot,
            CapabilityStatus::Unsupported {
                reason: "deferred-slice"
            }
        );
        assert_ne!(
            activation,
            CapabilityStatus::Unsupported {
                reason: "deferred-slice"
            }
        );
    }

    #[test]
    fn deferred_capabilities_are_unsupported_not_available() {
        assert!(matches!(
            capability_status(CapabilityKind::Integration),
            CapabilityStatus::Unsupported {
                reason: "deferred-slice"
            }
        ));
    }

    #[test]
    fn ime_and_clipboard_statuses_are_typed_on_desktop() {
        let ime = capability_status(CapabilityKind::Ime);
        let clipboard = capability_status(CapabilityKind::Clipboard);
        // Never silently Available when headless; on DISPLAY=:1 expect Available
        // or an explicit Failed if helpers are missing.
        if display_facts_from_env().headless {
            assert!(matches!(
                ime,
                CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
            ));
            assert!(matches!(
                clipboard,
                CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
            ));
        } else {
            assert_eq!(ime, CapabilityStatus::Available);
            assert!(matches!(
                clipboard,
                CapabilityStatus::Available
                    | CapabilityStatus::Failed {
                        code: "clipboard_unavailable",
                        ..
                    }
            ));
        }
    }
}
