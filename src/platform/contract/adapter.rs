//! Common validation contract for every operating-system adapter.

use crate::platform::{CONTRACT_REVISION, CapabilityKind, CapabilityStatus, PlatformKind};

#[derive(Debug, Clone)]
pub(crate) struct AdapterContractDeclaration {
    pub(crate) kind: PlatformKind,
    pub(crate) revision: u32,
    pub(crate) capabilities: [CapabilityKind; 8],
}

pub(crate) fn validate_adapter_contract(
    declaration: &AdapterContractDeclaration,
    unsupported: CapabilityStatus,
    failed: CapabilityStatus,
) -> Result<(), String> {
    if declaration.revision != CONTRACT_REVISION {
        return Err(format!(
            "{:?} declares contract revision {}, expected {CONTRACT_REVISION}",
            declaration.kind, declaration.revision
        ));
    }
    if declaration.capabilities != CapabilityKind::ALL {
        return Err(format!(
            "{:?} does not declare the complete capability surface",
            declaration.kind
        ));
    }
    match unsupported {
        CapabilityStatus::Unsupported { reason } if !reason.is_empty() => {}
        other => {
            return Err(format!(
                "{:?} has invalid Unsupported probe: {other:?}",
                declaration.kind
            ));
        }
    }
    match failed {
        CapabilityStatus::Failed { code, message } if !code.is_empty() && !message.is_empty() => {}
        other => {
            return Err(format!(
                "{:?} has invalid Failed probe: {other:?}",
                declaration.kind
            ));
        }
    }
    Ok(())
}
