//! Windows native platform adapter (`prd/PRD_02_20_native_platform.md`).
//!
//! Contract revision implemented by this adapter: 1.

#![cfg(target_os = "windows")]

pub(crate) mod activation;
pub(crate) mod clipboard;
pub(crate) mod input;
pub(crate) mod screenshot;
pub(crate) mod toolbar;

use super::{CapabilityKind, CapabilityStatus, PlatformKind};

#[allow(dead_code)]
pub const IMPLEMENTED_CONTRACT_REVISION: u32 = 1;

#[allow(dead_code)]
pub const fn platform_kind() -> PlatformKind {
    PlatformKind::Windows
}

#[allow(dead_code)]
pub fn capability_status(capability: CapabilityKind) -> CapabilityStatus {
    match capability {
        CapabilityKind::Window
        | CapabilityKind::Input
        | CapabilityKind::Ime
        | CapabilityKind::Clipboard
        | CapabilityKind::Font
        | CapabilityKind::Screenshot
        | CapabilityKind::Activation => CapabilityStatus::Available,
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
            capability_status(CapabilityKind::Integration),
            CapabilityStatus::Unsupported {
                reason: "windows-shell-integration-not-yet-declared",
            }
        );
    }
}
