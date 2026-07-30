//! Linux native platform adapter (`prd/PRD_02_20_native_platform.md`).
//!
//! Ownership: Linux agent only. Do not edit shared contracts here.
//!
//! Implements shared **contract revision 1** (`crate::platform::CONTRACT_REVISION`)
//! for migration slice 1:
//! - toolbar hits → stable [`crate::platform::action`] identities
//! - keyboard committed text vs shortcut chord separation via shared
//!   [`crate::platform::classify_key_press`]
//!
//! Linux `unix_app` hot paths (toolbar click + key/IME text-vs-shortcut) call
//! into this module under `cfg(target_os = "linux")` and then dispatch to the
//! existing product handlers. macOS behavior stays on the unbridged path.
//!
//! Public capability identity stays on the Linux branch. Private reuse of
//! `crate::unix_app` helpers is allowed without merging macOS/Linux identities.

#![cfg(target_os = "linux")]

use crate::platform::{
    CONTRACT_REVISION, CapabilityKind, CapabilityStatus, DisplayBackendFacts, PlatformKind,
};

pub(crate) mod input;
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

/// Slice-1 capabilities this adapter currently speaks for.
pub(crate) fn slice1_capability_kinds() -> [CapabilityKind; 2] {
    [CapabilityKind::Window, CapabilityKind::Input]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implements_contract_revision_1() {
        assert_eq!(IMPLEMENTED_CONTRACT_REVISION, 1);
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
}
