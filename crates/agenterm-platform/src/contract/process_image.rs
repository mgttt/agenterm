//! Product-neutral executable identity for one host process.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessImageErrorKind {
    InvalidId,
    IdOutOfRange,
    NotFound,
    Open,
    Query,
    InvalidData,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessImageError {
    kind: ProcessImageErrorKind,
    detail: String,
}

impl ProcessImageError {
    pub(crate) fn new(kind: ProcessImageErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> ProcessImageErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for ProcessImageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "process image {:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for ProcessImageError {}
