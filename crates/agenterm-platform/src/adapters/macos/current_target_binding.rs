use crate::{
    CapabilityStatus,
    contract::current_target_binding::{CurrentTargetBindingError, CurrentTargetBindingErrorKind},
};

pub(crate) struct NativeCurrentSessionFacts;

impl NativeCurrentSessionFacts {
    pub(crate) const fn as_bytes(&self) -> &[u8] {
        &[]
    }
}

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Unsupported {
        reason: "current-target-binding-macos-not-implemented".into(),
    }
}

pub(crate) fn current_session_facts() -> Result<NativeCurrentSessionFacts, CurrentTargetBindingError>
{
    Err(CurrentTargetBindingError::new(
        CurrentTargetBindingErrorKind::Unsupported,
        "current-target-binding-macos-not-implemented",
        "macOS current target binding is not implemented",
    ))
}

pub(crate) fn validate_private_key_file(
    _: &std::path::Path,
) -> Result<(), CurrentTargetBindingError> {
    Ok(())
}
