//! OS-neutral process facts and typed failures consumed by facade services.

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProcessObservation {
    Live { start_identity: Option<String> },
    Dead { reason: String },
    Unknown { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessInfo {
    pub(crate) id: u32,
    pub(crate) parent_id: u32,
    pub(crate) executable_name: String,
}

#[allow(dead_code)] // A target builds the full three-adapter error contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessErrorKind {
    IdOutOfRange,
    Inventory,
    InventoryTooLarge,
    KillOpen,
    Kill,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessError {
    pub(crate) kind: ProcessErrorKind,
    pub(crate) detail: String,
}

impl ProcessError {
    pub(crate) fn new(kind: ProcessErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_error_preserves_typed_kind_and_diagnostic() {
        let error = ProcessError::new(ProcessErrorKind::Unsupported, "adapter unavailable");
        assert_eq!(error.kind, ProcessErrorKind::Unsupported);
        assert_eq!(error.detail, "adapter unavailable");
    }
}
