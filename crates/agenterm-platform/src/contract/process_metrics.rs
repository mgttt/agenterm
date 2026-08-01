//! Product-neutral cumulative resource counters for one host process.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessMetrics {
    pub cpu_time: Duration,
    pub resident_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessMetricsErrorKind {
    InvalidId,
    Open,
    Read,
    Parse,
    Clock,
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
