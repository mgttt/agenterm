//! Product-neutral host memory geometry and capacity.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostMemoryFacts {
    /// Smallest unit used for page protection and commitment.
    pub page_size: std::num::NonZeroUsize,
    /// Alignment required for native file/view mapping offsets.
    pub allocation_granularity: std::num::NonZeroUsize,
    /// Installed physical memory visible to the host OS.
    ///
    /// This is not a container, cgroup, job-object, or process memory budget.
    pub physical_bytes: std::num::NonZeroU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HostMemoryErrorKind {
    Query,
    InvalidValue,
    Overflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostMemoryError {
    kind: HostMemoryErrorKind,
    detail: String,
}

impl HostMemoryError {
    pub(crate) fn new(kind: HostMemoryErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> HostMemoryErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for HostMemoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "host memory {:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for HostMemoryError {}
