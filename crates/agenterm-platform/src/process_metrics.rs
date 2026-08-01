//! Lightweight resource observation without process inventory or ownership APIs.

pub use crate::contract::process_metrics::{
    PageFaultCounters, ProcessMetrics, ProcessMetricsError, ProcessMetricsErrorKind,
};

/// Read cumulative CPU time, resident memory and page faults for one host process.
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
        assert!(sample.page_faults.total > 0);
        #[cfg(windows)]
        assert_eq!(
            (sample.page_faults.soft, sample.page_faults.hard),
            (None, None)
        );
        #[cfg(target_os = "linux")]
        assert!(sample.page_faults.soft.is_some() && sample.page_faults.hard.is_some());
        #[cfg(target_os = "macos")]
        assert!(sample.page_faults.soft.is_none() && sample.page_faults.hard.is_some());
    }

    #[test]
    fn page_fault_counter_advances_when_new_pages_are_touched() {
        let before = metrics(std::process::id()).expect("sample before page touches");
        let mut pages = vec![0_u8; 16 * 1024 * 1024];
        for page in pages.chunks_mut(4096) {
            // Volatile access forces physical backing without relying on optimizer behavior.
            unsafe { page.as_mut_ptr().write_volatile(1) };
        }
        std::hint::black_box(&pages);
        let after = metrics(std::process::id()).expect("sample after page touches");
        let delta = after
            .page_faults
            .checked_delta_since(before.page_faults)
            .expect("short-lived cumulative counter must not wrap");
        assert!(
            delta.total > 0,
            "touching new pages produced no page faults"
        );
    }

    #[test]
    fn zero_is_not_a_single_process() {
        let error = metrics(0).expect_err("reject PID zero");
        assert_eq!(error.kind(), ProcessMetricsErrorKind::InvalidId);
    }

    #[test]
    fn a_missing_process_is_distinct_from_an_observation_failure() {
        let error = metrics(u32::MAX).expect_err("maximum PID must not exist");
        assert_eq!(error.kind(), ProcessMetricsErrorKind::NotFound);
    }
}
