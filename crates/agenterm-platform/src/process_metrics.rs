//! Lightweight resource observation without process inventory or ownership APIs.

pub use crate::contract::process_metrics::{
    ProcessMetrics, ProcessMetricsError, ProcessMetricsErrorKind,
};

/// Read cumulative CPU time and resident memory for one host process.
///
/// The caller owns PID selection, aggregation, sampling intervals and policy.
pub fn metrics(pid: u32) -> Result<ProcessMetrics, ProcessMetricsError> {
    crate::selected::process_metrics::metrics(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observes_the_current_process() {
        let sample = metrics(std::process::id()).expect("observe current process");
        assert!(sample.resident_bytes > 0);
    }

    #[test]
    fn zero_is_not_a_single_process() {
        let error = metrics(0).expect_err("reject PID zero");
        assert_eq!(error.kind(), ProcessMetricsErrorKind::InvalidId);
    }
}
