//! OS-neutral process facts and typed failures consumed by facade services.

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessObservation {
    Live { start_identity: Option<String> },
    Dead { reason: String },
    Unknown { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessInfo {
    pub id: u32,
    pub parent_id: u32,
    pub executable_name: String,
}

#[allow(dead_code)] // A target builds the full three-adapter error contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessErrorKind {
    IdOutOfRange,
    Inventory,
    InventoryTooLarge,
    KillOpen,
    Kill,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessError {
    kind: ProcessErrorKind,
    detail: String,
}

impl ProcessError {
    pub(crate) fn new(kind: ProcessErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> ProcessErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "process {:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for ProcessError {}

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
