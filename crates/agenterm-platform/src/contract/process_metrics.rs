//! Product-neutral cumulative resource counters for one host process.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageFaultCounters {
    /// All page faults attributed to the process by the host.
    pub total: u64,
    /// Faults resolved without an actual page-in, when the host separates them.
    pub soft: Option<u64>,
    /// Faults that required an actual page-in, when the host separates them.
    pub hard: Option<u64>,
}

impl PageFaultCounters {
    pub fn checked_delta_since(self, earlier: Self) -> Option<Self> {
        Some(Self {
            total: self.total.checked_sub(earlier.total)?,
            soft: checked_optional_delta(self.soft, earlier.soft)?,
            hard: checked_optional_delta(self.hard, earlier.hard)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessMetrics {
    pub cpu_time: Duration,
    pub resident_bytes: u64,
    pub page_faults: PageFaultCounters,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessMetricsErrorKind {
    InvalidId,
    Open,
    Read,
    Parse,
    Clock,
    InvalidValue,
    Overflow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessMetricsError {
    kind: ProcessMetricsErrorKind,
    detail: String,
}

impl ProcessMetricsError {
    pub(crate) fn new(kind: ProcessMetricsErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> ProcessMetricsErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for ProcessMetricsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "process metrics {:?}: {}",
            self.kind, self.detail
        )
    }
}

impl std::error::Error for ProcessMetricsError {}

pub(crate) fn checked_page_faults(
    total: u64,
    soft: Option<u64>,
    hard: Option<u64>,
) -> Result<PageFaultCounters, ProcessMetricsError> {
    if soft.is_some_and(|value| value > total) || hard.is_some_and(|value| value > total) {
        return Err(ProcessMetricsError::new(
            ProcessMetricsErrorKind::InvalidValue,
            "page-fault subtype exceeds total count",
        ));
    }
    if let (Some(soft), Some(hard)) = (soft, hard) {
        let classified = soft.checked_add(hard).ok_or_else(|| {
            ProcessMetricsError::new(
                ProcessMetricsErrorKind::Overflow,
                "classified page-fault count overflows u64",
            )
        })?;
        if classified > total {
            return Err(ProcessMetricsError::new(
                ProcessMetricsErrorKind::InvalidValue,
                "classified page-fault count exceeds total count",
            ));
        }
    }
    Ok(PageFaultCounters { total, soft, hard })
}

fn checked_optional_delta(current: Option<u64>, earlier: Option<u64>) -> Option<Option<u64>> {
    match (current, earlier) {
        (Some(current), Some(earlier)) => Some(Some(current.checked_sub(earlier)?)),
        (None, None) => Some(None),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_fault_delta_preserves_unknown_subtypes() {
        let earlier = PageFaultCounters {
            total: 10,
            soft: None,
            hard: None,
        };
        let current = PageFaultCounters {
            total: 15,
            soft: None,
            hard: None,
        };
        assert_eq!(
            current.checked_delta_since(earlier),
            Some(PageFaultCounters {
                total: 5,
                soft: None,
                hard: None,
            })
        );
        assert_eq!(earlier.checked_delta_since(current), None);
    }

    #[test]
    fn page_fault_validation_rejects_impossible_classification() {
        assert!(checked_page_faults(5, Some(4), Some(2)).is_err());
        assert!(checked_page_faults(5, Some(6), None).is_err());
        assert_eq!(
            checked_page_faults(5, Some(3), Some(2)).unwrap(),
            PageFaultCounters {
                total: 5,
                soft: Some(3),
                hard: Some(2),
            }
        );
    }
}
