//! Product-neutral host processor and NUMA topology facts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessorTopologyFacts {
    /// Logical processors currently online in the host OS.
    ///
    /// This is deliberately distinct from process-scoped available parallelism,
    /// which may be reduced by affinity, jobs, containers or schedulers.
    pub system_logical_processors: std::num::NonZeroUsize,
    pub physical_cores: Option<std::num::NonZeroUsize>,
    pub packages: Option<std::num::NonZeroUsize>,
    pub numa_nodes: Option<std::num::NonZeroUsize>,
    /// Windows processor groups. Other hosts normally report `None`.
    pub processor_groups: Option<std::num::NonZeroUsize>,
}

impl ProcessorTopologyFacts {
    pub fn from_counts(
        system_logical_processors: u64,
        physical_cores: Option<u64>,
        packages: Option<u64>,
        numa_nodes: Option<u64>,
        processor_groups: Option<u64>,
    ) -> Result<Self, ProcessorTopologyError> {
        Ok(Self {
            system_logical_processors: nonzero(
                "system logical processors",
                system_logical_processors,
            )?,
            physical_cores: optional_nonzero("physical cores", physical_cores)?,
            packages: optional_nonzero("processor packages", packages)?,
            numa_nodes: optional_nonzero("NUMA nodes", numa_nodes)?,
            processor_groups: optional_nonzero("processor groups", processor_groups)?,
        })
    }

    #[must_use]
    pub fn uniform_threads_per_core(self) -> Option<std::num::NonZeroUsize> {
        let physical = self.physical_cores?.get();
        let logical = self.system_logical_processors.get();
        logical
            .is_multiple_of(physical)
            .then(|| std::num::NonZeroUsize::new(logical / physical))
            .flatten()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessorTopologyErrorKind {
    Query,
    InvalidValue,
    MalformedNativeData,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessorTopologyError {
    kind: ProcessorTopologyErrorKind,
    detail: String,
}

impl ProcessorTopologyError {
    pub(crate) fn new(kind: ProcessorTopologyErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ProcessorTopologyErrorKind {
        self.kind
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for ProcessorTopologyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "processor topology {:?}: {}",
            self.kind, self.detail
        )
    }
}

impl std::error::Error for ProcessorTopologyError {}

fn optional_nonzero(
    name: &str,
    value: Option<u64>,
) -> Result<Option<std::num::NonZeroUsize>, ProcessorTopologyError> {
    value.map(|value| nonzero(name, value)).transpose()
}

fn nonzero(name: &str, value: u64) -> Result<std::num::NonZeroUsize, ProcessorTopologyError> {
    usize::try_from(value)
        .ok()
        .and_then(std::num::NonZeroUsize::new)
        .ok_or_else(|| {
            ProcessorTopologyError::new(
                ProcessorTopologyErrorKind::InvalidValue,
                format!("host reported invalid {name}: {value}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_facts_reject_zero_and_overflow_without_inventing_optional_values() {
        assert_eq!(
            ProcessorTopologyFacts::from_counts(0, None, None, None, None)
                .unwrap_err()
                .kind(),
            ProcessorTopologyErrorKind::InvalidValue
        );
        for invalid in [
            ProcessorTopologyFacts::from_counts(8, Some(0), None, None, None),
            ProcessorTopologyFacts::from_counts(8, None, Some(0), None, None),
            ProcessorTopologyFacts::from_counts(8, None, None, Some(0), None),
            ProcessorTopologyFacts::from_counts(8, None, None, None, Some(0)),
        ] {
            assert_eq!(
                invalid.unwrap_err().kind(),
                ProcessorTopologyErrorKind::InvalidValue
            );
        }

        let facts = ProcessorTopologyFacts::from_counts(8, None, None, None, None).unwrap();
        assert_eq!(facts.system_logical_processors.get(), 8);
        assert_eq!(facts.physical_cores, None);
        assert_eq!(facts.numa_nodes, None);
    }

    #[test]
    fn uniform_smt_width_requires_an_exact_nonzero_ratio() {
        let uniform =
            ProcessorTopologyFacts::from_counts(8, Some(4), Some(1), Some(1), Some(1)).unwrap();
        assert_eq!(uniform.uniform_threads_per_core().unwrap().get(), 2);

        let nonuniform = ProcessorTopologyFacts::from_counts(6, Some(4), None, None, None).unwrap();
        assert_eq!(nonuniform.uniform_threads_per_core(), None);
    }
}
