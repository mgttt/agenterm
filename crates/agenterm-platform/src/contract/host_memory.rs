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

pub(crate) fn checked_facts(
    page_size: u64,
    allocation_granularity: u64,
    physical_bytes: u64,
) -> Result<HostMemoryFacts, HostMemoryError> {
    let page_size = nonzero_usize("page size", page_size)?;
    let allocation_granularity = nonzero_usize("allocation granularity", allocation_granularity)?;
    if allocation_granularity.get() < page_size.get()
        || allocation_granularity.get() % page_size.get() != 0
    {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            "allocation granularity is not a positive multiple of page size",
        ));
    }
    let physical_bytes = std::num::NonZeroU64::new(physical_bytes).ok_or_else(|| {
        HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            "host reported zero physical memory",
        )
    })?;
    if physical_bytes.get() < page_size.get() as u64 {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            "physical memory is smaller than one host page",
        ));
    }
    Ok(HostMemoryFacts {
        page_size,
        allocation_granularity,
        physical_bytes,
    })
}

fn nonzero_usize(name: &str, value: u64) -> Result<std::num::NonZeroUsize, HostMemoryError> {
    usize::try_from(value)
        .ok()
        .and_then(std::num::NonZeroUsize::new)
        .ok_or_else(|| {
            HostMemoryError::new(
                HostMemoryErrorKind::InvalidValue,
                format!("host reported invalid {name}: {value}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_facts_reject_zero_and_incoherent_geometry() {
        for result in [
            checked_facts(0, 65_536, 1 << 30),
            checked_facts(4096, 0, 1 << 30),
            checked_facts(4096, 2048, 1 << 30),
            checked_facts(4096, 6144, 1 << 30),
            checked_facts(4096, 4096, 0),
            checked_facts(4096, 4096, 2048),
        ] {
            assert_eq!(
                result.expect_err("reject invalid raw facts").kind(),
                HostMemoryErrorKind::InvalidValue
            );
        }
    }
}
