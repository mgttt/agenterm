//! macOS native platform adapter (`prd/PRD_02_20_native_platform.md`).
//!
//! Ownership: macOS agent only. Shared contracts remain primary-owned.
//!
//! The private helpers below preserve Apple-native behavior while returning
//! primary-owned shared event and capability types.
//!
//! Contract revision implemented by this adapter: 1.

#![cfg(target_os = "macos")]

pub(crate) mod activation;
pub(crate) mod clipboard;
pub(crate) mod input;
pub(crate) mod scale;
pub(crate) mod screenshot;
pub(crate) mod toolbar;

use super::{CapabilityKind, CapabilityStatus, PlatformKind};

pub const IMPLEMENTED_CONTRACT_REVISION: u32 = 1;

pub const fn platform_kind() -> PlatformKind {
    PlatformKind::Macos
}

pub fn capability_status(capability: CapabilityKind) -> CapabilityStatus {
    match capability {
        CapabilityKind::Window
        | CapabilityKind::Input
        | CapabilityKind::Ime
        | CapabilityKind::Font
        | CapabilityKind::Screenshot
        | CapabilityKind::Activation => CapabilityStatus::Available,
        CapabilityKind::Clipboard => clipboard::capability_status(),
        CapabilityKind::Integration => CapabilityStatus::Unsupported {
            reason: "signed-macos-app-bundle-pending",
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
        assert_eq!(platform_kind(), PlatformKind::Macos);
    }

    #[test]
    fn capabilities_report_missing_bundle_integration_honestly() {
        assert_eq!(
            capability_status(CapabilityKind::Input),
            CapabilityStatus::Available
        );
        assert_eq!(
            capability_status(CapabilityKind::Integration),
            CapabilityStatus::Unsupported {
                reason: "signed-macos-app-bundle-pending",
            }
        );
    }
}
