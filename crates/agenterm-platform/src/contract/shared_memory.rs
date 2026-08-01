//! Product-neutral named shared-memory contract.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SharedMemoryErrorKind {
    InvalidName,
    InvalidLength,
    AlreadyExists,
    NotFound,
    Create,
    Open,
    Resize,
    Map,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedMemoryError {
    kind: SharedMemoryErrorKind,
    detail: String,
}

impl SharedMemoryError {
    pub(crate) fn new(kind: SharedMemoryErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> SharedMemoryErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for SharedMemoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "shared memory {:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for SharedMemoryError {}
