//! Typed, host-neutral accounting for native pixel presents.

#[cfg(any(feature = "native-pixel-window", feature = "portable-pixel-window"))]
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PixelPresentRegion {
    Full,
    Partial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PixelPresentOutcome {
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelPresentReceipt {
    /// Monotonic receipt number within one native window runner.
    pub sequence: u64,
    /// Monotonic elapsed duration of the native present call, in nanoseconds.
    pub elapsed_ns: u64,
    /// Pixels requested from the native present API.
    pub requested_pixels: u64,
    /// Pixels reported as completed by the native API.
    pub completed_pixels: u64,
    /// Whether the requested region covered the complete framebuffer.
    pub region: PixelPresentRegion,
    /// Whether the native API accepted or rejected the call.
    pub outcome: PixelPresentOutcome,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelPresentStats {
    /// Sequence number of the most recently completed present attempt.
    pub sequence: u64,
    /// Number of completed native present attempts, including failures.
    pub count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub last_ns: u64,
    pub total_ns: u64,
    pub max_ns: u64,
    /// Successfully completed pixels from full-region presents.
    pub full_pixels: u64,
    /// Successfully completed pixels from partial-region presents.
    pub partial_pixels: u64,
    /// Requested pixels from full-region presents, including failures.
    pub requested_full_pixels: u64,
    /// Requested pixels from partial-region presents, including failures.
    pub requested_partial_pixels: u64,
}

#[derive(Clone, Debug, Default)]
#[cfg(any(
    test,
    feature = "native-pixel-window",
    feature = "portable-pixel-window"
))]
pub(crate) struct PixelPresentLedger {
    stats: PixelPresentStats,
    last: Option<PixelPresentReceipt>,
}

#[cfg(any(
    test,
    feature = "native-pixel-window",
    feature = "portable-pixel-window"
))]
impl PixelPresentLedger {
    pub(crate) const fn new() -> Self {
        Self {
            stats: PixelPresentStats {
                sequence: 0,
                count: 0,
                success_count: 0,
                failure_count: 0,
                last_ns: 0,
                total_ns: 0,
                max_ns: 0,
                full_pixels: 0,
                partial_pixels: 0,
                requested_full_pixels: 0,
                requested_partial_pixels: 0,
            },
            last: None,
        }
    }

    pub(crate) fn record(
        &mut self,
        elapsed_ns: u64,
        requested_pixels: u64,
        completed_pixels: u64,
        region: PixelPresentRegion,
        outcome: PixelPresentOutcome,
    ) -> PixelPresentReceipt {
        let completed_pixels = match outcome {
            PixelPresentOutcome::Succeeded => completed_pixels.min(requested_pixels),
            // A failed native call never establishes that any submitted pixel
            // reached the host. Keep failure accounting conservative even if a
            // future adapter supplies a non-zero partial-copy count.
            PixelPresentOutcome::Failed => 0,
        };
        let sequence = self.stats.sequence.saturating_add(1);
        let receipt = PixelPresentReceipt {
            sequence,
            elapsed_ns,
            requested_pixels,
            completed_pixels,
            region,
            outcome,
        };

        self.stats.sequence = sequence;
        self.stats.count = self.stats.count.saturating_add(1);
        self.stats.last_ns = elapsed_ns;
        self.stats.total_ns = self.stats.total_ns.saturating_add(elapsed_ns);
        self.stats.max_ns = self.stats.max_ns.max(elapsed_ns);
        match outcome {
            PixelPresentOutcome::Succeeded => {
                self.stats.success_count = self.stats.success_count.saturating_add(1);
            }
            PixelPresentOutcome::Failed => {
                self.stats.failure_count = self.stats.failure_count.saturating_add(1);
            }
        }
        match region {
            PixelPresentRegion::Full => {
                self.stats.full_pixels = self.stats.full_pixels.saturating_add(completed_pixels);
                self.stats.requested_full_pixels = self
                    .stats
                    .requested_full_pixels
                    .saturating_add(requested_pixels);
            }
            PixelPresentRegion::Partial => {
                self.stats.partial_pixels =
                    self.stats.partial_pixels.saturating_add(completed_pixels);
                self.stats.requested_partial_pixels = self
                    .stats
                    .requested_partial_pixels
                    .saturating_add(requested_pixels);
            }
        }
        self.last = Some(receipt);
        receipt
    }

    pub(crate) const fn snapshot(&self) -> PixelPresentStats {
        self.stats
    }

    pub(crate) const fn last(&self) -> Option<PixelPresentReceipt> {
        self.last
    }
}

#[cfg(any(feature = "native-pixel-window", feature = "portable-pixel-window"))]
pub(crate) fn elapsed_ns_since(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_records_success_and_failure_without_resetting_history() {
        let mut ledger = PixelPresentLedger::new();
        let first = ledger.record(
            7,
            100,
            100,
            PixelPresentRegion::Full,
            PixelPresentOutcome::Succeeded,
        );
        let second = ledger.record(
            11,
            50,
            0,
            PixelPresentRegion::Partial,
            PixelPresentOutcome::Failed,
        );

        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(ledger.last(), Some(second));
        assert_eq!(
            ledger.snapshot(),
            PixelPresentStats {
                sequence: 2,
                count: 2,
                success_count: 1,
                failure_count: 1,
                last_ns: 11,
                total_ns: 18,
                max_ns: 11,
                full_pixels: 100,
                partial_pixels: 0,
                requested_full_pixels: 100,
                requested_partial_pixels: 50,
            }
        );
    }

    #[test]
    fn ledger_clamps_completed_pixels_and_saturates_counters() {
        let mut ledger = PixelPresentLedger::new();
        ledger.record(
            u64::MAX,
            u64::MAX,
            u64::MAX,
            PixelPresentRegion::Full,
            PixelPresentOutcome::Succeeded,
        );
        ledger.record(
            u64::MAX,
            u64::MAX,
            u64::MAX,
            PixelPresentRegion::Full,
            PixelPresentOutcome::Succeeded,
        );

        let stats = ledger.snapshot();
        assert_eq!(stats.count, 2);
        assert_eq!(stats.total_ns, u64::MAX);
        assert_eq!(stats.max_ns, u64::MAX);
        assert_eq!(stats.full_pixels, u64::MAX);
        assert_eq!(stats.requested_full_pixels, u64::MAX);
    }

    #[test]
    fn failed_present_never_counts_completed_pixels() {
        let mut ledger = PixelPresentLedger::new();
        let receipt = ledger.record(
            5,
            100,
            100,
            PixelPresentRegion::Partial,
            PixelPresentOutcome::Failed,
        );

        assert_eq!(receipt.completed_pixels, 0);
        assert_eq!(ledger.snapshot().partial_pixels, 0);
        assert_eq!(ledger.snapshot().requested_partial_pixels, 100);
    }

    #[test]
    fn ledger_starts_empty_and_has_no_last_receipt() {
        let ledger = PixelPresentLedger::new();
        assert_eq!(ledger.snapshot(), PixelPresentStats::default());
        assert_eq!(ledger.last(), None);
    }
}
