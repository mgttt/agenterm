//! Windows native platform adapter (`prd/PRD_02_20_native_platform.md`).
//! Adapter-private native mechanism selected only by platform::selected.
//!
//! Contract revision implemented by this adapter: 3.

#![cfg(target_os = "windows")]

pub(crate) mod activation;
pub(crate) mod input;
pub(crate) mod screenshot;

use crate::platform::{CapabilityKind, CapabilityStatus, PlatformKind};

#[allow(dead_code)]
pub const IMPLEMENTED_CONTRACT_REVISION: u32 = 3;

#[allow(dead_code)]
pub const fn platform_kind() -> PlatformKind {
    PlatformKind::Windows
}

#[allow(dead_code)]
pub fn capability_status(capability: CapabilityKind) -> CapabilityStatus {
    match capability {
        CapabilityKind::Window
        | CapabilityKind::Input
        | CapabilityKind::Clipboard
        | CapabilityKind::Screenshot
        | CapabilityKind::Activation => CapabilityStatus::Available,
        CapabilityKind::Ime => CapabilityStatus::Unsupported {
            reason: "ime-preedit-not-yet-adapted",
        },
        CapabilityKind::Font => crate::platform::project_capability_status(
            agenterm_platform::font::capability_status(),
            "font-unsupported",
            "font-failed",
        ),
        CapabilityKind::Integration => CapabilityStatus::Unsupported {
            reason: "windows-shell-integration-not-yet-declared",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::CONTRACT_REVISION;

    #[test]
    fn adapter_implements_current_contract_revision() {
        assert_eq!(IMPLEMENTED_CONTRACT_REVISION, CONTRACT_REVISION);
        assert_eq!(platform_kind(), PlatformKind::Windows);
    }

    #[test]
    fn capabilities_are_explicit() {
        assert_eq!(
            capability_status(CapabilityKind::Input),
            CapabilityStatus::Available
        );
        assert_eq!(
            capability_status(CapabilityKind::Ime),
            CapabilityStatus::Unsupported {
                reason: "ime-preedit-not-yet-adapted",
            }
        );
        assert_eq!(
            capability_status(CapabilityKind::Integration),
            CapabilityStatus::Unsupported {
                reason: "windows-shell-integration-not-yet-declared",
            }
        );
    }
}
