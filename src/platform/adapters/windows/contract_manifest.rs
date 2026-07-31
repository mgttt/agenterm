use crate::platform::{
    CapabilityKind, CapabilityStatus, PlatformKind, contract::adapter::AdapterContractDeclaration,
};

pub(crate) const DECLARATION: AdapterContractDeclaration = AdapterContractDeclaration {
    kind: PlatformKind::Windows,
    revision: 3,
    capabilities: CapabilityKind::ALL,
};

pub(crate) fn unsupported_probe() -> CapabilityStatus {
    CapabilityStatus::Unsupported {
        reason: "windows-adapter-capability-unavailable",
    }
}

pub(crate) fn failed_probe() -> CapabilityStatus {
    CapabilityStatus::Failed {
        code: "windows_adapter_failed",
        message: "Windows native operation failed".to_owned(),
    }
}
