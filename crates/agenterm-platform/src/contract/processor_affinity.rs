//! Product-neutral current-process processor-affinity facts.

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogicalProcessorLocation {
    /// Native processor-group namespace. Unix adapters use group zero.
    pub group: u16,
    /// Logical processor index within `group`.
    pub index: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessorSetSemantics {
    /// The host scheduler's effective allowed-CPU mask for this process.
    SchedulerAllowed,
    /// The Windows process affinity mask for one assigned processor group.
    ///
    /// CPU Sets and thread-specific policies may narrow individual scheduling
    /// further; this variant deliberately does not claim otherwise.
    ProcessAffinityMask,
}

impl ProcessorSetSemantics {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SchedulerAllowed => "scheduler-allowed",
            Self::ProcessAffinityMask => "process-affinity-mask",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessorAffinityFacts {
    processors: Vec<LogicalProcessorLocation>,
    semantics: ProcessorSetSemantics,
}

impl ProcessorAffinityFacts {
    pub fn from_locations(
        mut processors: Vec<LogicalProcessorLocation>,
        semantics: ProcessorSetSemantics,
    ) -> Result<Self, ProcessorAffinityError> {
        if processors.is_empty() {
            return Err(ProcessorAffinityError::new(
                ProcessorAffinityErrorKind::InvalidValue,
                "host returned an empty processor set",
            ));
        }
        processors.sort_unstable();
        if processors.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ProcessorAffinityError::new(
                ProcessorAffinityErrorKind::MalformedNativeData,
                "host returned duplicate logical processor locations",
            ));
        }
        Ok(Self {
            processors,
            semantics,
        })
    }

    #[must_use]
    pub fn processors(&self) -> &[LogicalProcessorLocation] {
        &self.processors
    }

    #[must_use]
    pub fn count(&self) -> std::num::NonZeroUsize {
        std::num::NonZeroUsize::new(self.processors.len())
            .expect("validated processor sets are nonempty")
    }

    #[must_use]
    pub const fn semantics(&self) -> ProcessorSetSemantics {
        self.semantics
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessorAffinityErrorKind {
    Unsupported,
    Query,
    InvalidValue,
    MalformedNativeData,
}

impl ProcessorAffinityErrorKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Query => "query",
            Self::InvalidValue => "invalid-value",
            Self::MalformedNativeData => "malformed-native-data",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessorAffinityError {
    kind: ProcessorAffinityErrorKind,
    detail: String,
}

impl ProcessorAffinityError {
    pub(crate) fn new(kind: ProcessorAffinityErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ProcessorAffinityErrorKind {
        self.kind
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for ProcessorAffinityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "processor affinity {:?}: {}",
            self.kind, self.detail
        )
    }
}

impl std::error::Error for ProcessorAffinityError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_sets_are_nonempty_sorted_and_unique() {
        let facts = ProcessorAffinityFacts::from_locations(
            vec![
                LogicalProcessorLocation { group: 1, index: 2 },
                LogicalProcessorLocation { group: 0, index: 7 },
            ],
            ProcessorSetSemantics::SchedulerAllowed,
        )
        .unwrap();
        assert_eq!(
            facts.processors(),
            &[
                LogicalProcessorLocation { group: 0, index: 7 },
                LogicalProcessorLocation { group: 1, index: 2 },
            ]
        );
        assert_eq!(facts.count().get(), 2);

        assert_eq!(
            ProcessorAffinityFacts::from_locations(
                Vec::new(),
                ProcessorSetSemantics::SchedulerAllowed
            )
            .unwrap_err()
            .kind(),
            ProcessorAffinityErrorKind::InvalidValue
        );
        assert_eq!(
            ProcessorAffinityFacts::from_locations(
                vec![
                    LogicalProcessorLocation { group: 0, index: 1 },
                    LogicalProcessorLocation { group: 0, index: 1 },
                ],
                ProcessorSetSemantics::SchedulerAllowed
            )
            .unwrap_err()
            .kind(),
            ProcessorAffinityErrorKind::MalformedNativeData
        );
    }

    #[test]
    fn semantics_and_error_kinds_have_stable_names() {
        assert_eq!(
            ProcessorSetSemantics::SchedulerAllowed.as_str(),
            "scheduler-allowed"
        );
        assert_eq!(
            ProcessorSetSemantics::ProcessAffinityMask.as_str(),
            "process-affinity-mask"
        );
        assert_eq!(
            ProcessorAffinityErrorKind::MalformedNativeData.as_str(),
            "malformed-native-data"
        );
    }
}
