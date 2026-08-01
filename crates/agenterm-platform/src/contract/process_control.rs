//! Product-neutral single-process termination contract.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TerminationMode {
    Graceful,
    Forceful,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessControlErrorKind {
    InvalidId,
    IdOutOfRange,
    Open,
    Terminate,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessControlError {
    kind: ProcessControlErrorKind,
    detail: String,
}

impl ProcessControlError {
    pub(crate) fn new(kind: ProcessControlErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> ProcessControlErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for ProcessControlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "process control {:?}: {}",
            self.kind, self.detail
        )
    }
}

impl std::error::Error for ProcessControlError {}
