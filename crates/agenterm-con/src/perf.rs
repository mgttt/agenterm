//! Performance counters and their stable public JSON projection.
//!
//! This module owns measurement state only; render, PTY, control, and host
//! policy remain in the caller.

use std::time::Duration;

use agenterm_platform::contract::pixel_present::PixelPresentStats;
use agenterm_ui_core::DirtyRegion;

use crate::json;

#[derive(Default)]
pub(super) struct PerfStats {
    pub(super) frames: u64,
    pub(super) observed_frames: u64,
    pub(super) render_total_us: u128,
    pub(super) render_last_us: u64,
    pub(super) render_max_us: u64,
    pub(super) pty_drained_bytes: u64,
    pub(super) pty_budget_yields: u64,
    pub(super) control_requests: u64,
    pub(super) control_budget_yields: u64,
    pub(super) full_candidate_frames: u64,
    pub(super) partial_candidate_frames: u64,
    pub(super) dirty_pixels: u64,
    pub(super) frame_pixels: u64,
    pub(super) host_direct_frames: u64,
    pub(super) host_copy_frames: u64,
    pub(super) host_copy_pixels: u64,
    pub(super) discarded_capture_frames: u64,
    pub(super) platform_present: PixelPresentStats,
    pub(super) present_baseline: PixelPresentStats,
    pub(super) present_sequence_seen: u64,
    pub(super) present_last_ns: u64,
    pub(super) present_max_ns: u64,
}

impl PerfStats {
    pub(super) fn record_frame(&mut self, elapsed: Duration) {
        let micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        self.frames = self.frames.saturating_add(1);
        self.observed_frames = self.observed_frames.saturating_add(1);
        self.render_total_us = self.render_total_us.saturating_add(u128::from(micros));
        self.render_last_us = micros;
        self.render_max_us = self.render_max_us.max(micros);
    }

    /// Records only after the full raster function has returned successfully.
    /// These are candidate numbers, not claims about native present support.
    pub(super) fn record_raster_candidate(
        &mut self,
        candidate: DirtyRegion,
        width: u32,
        height: u32,
    ) {
        let frame_pixels = u64::from(width).saturating_mul(u64::from(height));
        self.frame_pixels = self.frame_pixels.saturating_add(frame_pixels);
        if candidate.is_full() {
            self.full_candidate_frames = self.full_candidate_frames.saturating_add(1);
            self.dirty_pixels = self.dirty_pixels.saturating_add(frame_pixels);
        } else {
            // An empty candidate is still a valid non-full observation (for
            // example a screenshot-only redraw) and contributes zero pixels.
            self.partial_candidate_frames = self.partial_candidate_frames.saturating_add(1);
            self.dirty_pixels = self
                .dirty_pixels
                .saturating_add(candidate.dirty_pixels(width, height));
        }
    }

    pub(super) fn record_host_direct_frame(&mut self) {
        self.host_direct_frames = self.host_direct_frames.saturating_add(1);
    }

    pub(super) fn record_host_copy_frame(&mut self, width: u32, height: u32) {
        self.host_copy_frames = self.host_copy_frames.saturating_add(1);
        let pixels = u64::from(width).saturating_mul(u64::from(height));
        self.host_copy_pixels = self.host_copy_pixels.saturating_add(pixels);
    }

    /// Samples the platform's cumulative ledger without adding a second
    /// synchronization primitive. `PixelWindow::present_stats` is a GUI-thread
    /// value copy; native adapters own any internal synchronization.
    pub(super) fn sync_present_stats(&mut self, current: PixelPresentStats) {
        if current.sequence > self.present_sequence_seen {
            self.present_sequence_seen = current.sequence;
            self.present_last_ns = current.last_ns;
            self.present_max_ns = self.present_max_ns.max(current.last_ns);
        }
        self.platform_present = current;
    }

    pub(super) fn present_delta(&self) -> PixelPresentStats {
        let current = self.platform_present;
        let baseline = self.present_baseline;
        PixelPresentStats {
            sequence: current.sequence.saturating_sub(baseline.sequence),
            count: current.count.saturating_sub(baseline.count),
            success_count: current.success_count.saturating_sub(baseline.success_count),
            failure_count: current.failure_count.saturating_sub(baseline.failure_count),
            last_ns: self.present_last_ns,
            total_ns: current.total_ns.saturating_sub(baseline.total_ns),
            // A cumulative max is not subtractable. This is the maximum
            // latest-present sample observed after reset, and is zero until a
            // post-reset present sequence is observed.
            max_ns: self.present_max_ns,
            full_pixels: current.full_pixels.saturating_sub(baseline.full_pixels),
            partial_pixels: current
                .partial_pixels
                .saturating_sub(baseline.partial_pixels),
            requested_full_pixels: current
                .requested_full_pixels
                .saturating_sub(baseline.requested_full_pixels),
            requested_partial_pixels: current
                .requested_partial_pixels
                .saturating_sub(baseline.requested_partial_pixels),
        }
    }

    pub(super) fn reset(&mut self, present: PixelPresentStats) {
        *self = Self::default();
        self.platform_present = present;
        self.present_baseline = present;
        self.present_sequence_seen = present.sequence;
    }

    pub(super) fn json(&self) -> json::JsonValue {
        let average = if self.frames == 0 {
            0
        } else {
            (self.render_total_us / u128::from(self.frames)).min(u128::from(u64::MAX)) as u64
        };
        let present = self.present_delta();
        json::object(vec![
            ("frames", self.frames.into()),
            ("observed_frames", self.observed_frames.into()),
            ("render_last_us", self.render_last_us.into()),
            ("render_average_us", average.into()),
            ("render_max_us", self.render_max_us.into()),
            ("pty_drained_bytes", self.pty_drained_bytes.into()),
            ("pty_budget_yields", self.pty_budget_yields.into()),
            ("control_requests", self.control_requests.into()),
            ("control_budget_yields", self.control_budget_yields.into()),
            ("full_candidate_frames", self.full_candidate_frames.into()),
            (
                "partial_candidate_frames",
                self.partial_candidate_frames.into(),
            ),
            ("dirty_pixels", self.dirty_pixels.into()),
            ("frame_pixels", self.frame_pixels.into()),
            ("host_direct_frames", self.host_direct_frames.into()),
            ("host_copy_frames", self.host_copy_frames.into()),
            ("host_copy_pixels", self.host_copy_pixels.into()),
            (
                "discarded_capture_frames",
                self.discarded_capture_frames.into(),
            ),
            ("present_count", present.count.into()),
            ("present_success", present.success_count.into()),
            ("present_failure", present.failure_count.into()),
            ("last_ns", present.last_ns.into()),
            ("total_ns", present.total_ns.into()),
            ("max_ns", present.max_ns.into()),
            ("full_pixels", present.full_pixels.into()),
            ("partial_pixels", present.partial_pixels.into()),
            (
                "requested_full_pixels",
                present.requested_full_pixels.into(),
            ),
            (
                "requested_partial_pixels",
                present.requested_partial_pixels.into(),
            ),
        ])
    }
}

#[cfg(test)]
mod perf_stats_tests {
    use super::*;
    use crate::{
        CandidateRedrawRequest, HostPixelRect, PixelBackingRetention, PixelFrameWrite,
        candidate_redraw_request, frame_write_for_candidate,
    };
    use agenterm_ui_core::PixelRect;

    #[test]
    pub(super) fn raster_candidate_fields_serialize_and_reset() {
        let mut stats = PerfStats::default();
        stats.record_frame(Duration::from_micros(7));
        stats.record_raster_candidate(DirtyRegion::full_frame(10, 20), 10, 20);
        stats.record_frame(Duration::from_micros(3));
        let mut partial = DirtyRegion::empty();
        partial.mark_rect(PixelRect::from_xywh(1, 2, 3, 4));
        stats.record_raster_candidate(partial, 10, 20);
        let serialized = String::from_utf8(json::to_vec(&stats.json())).expect("JSON is UTF-8");
        for field in [
            "observed_frames",
            "full_candidate_frames",
            "partial_candidate_frames",
            "dirty_pixels",
            "frame_pixels",
            "host_direct_frames",
            "host_copy_frames",
            "host_copy_pixels",
            "discarded_capture_frames",
            "control_requests",
            "control_budget_yields",
        ] {
            assert!(serialized.contains(field), "missing {field}: {serialized}");
        }
        assert_eq!(stats.observed_frames, 2);
        stats.reset(PixelPresentStats::default());
        assert_eq!(stats.observed_frames, 0);
        assert_eq!(stats.full_candidate_frames, 0);
        assert_eq!(stats.partial_candidate_frames, 0);
        assert_eq!(stats.dirty_pixels, 0);
        assert_eq!(stats.frame_pixels, 0);
        assert_eq!(stats.host_direct_frames, 0);
        assert_eq!(stats.host_copy_frames, 0);
        assert_eq!(stats.host_copy_pixels, 0);
    }

    #[test]
    pub(super) fn host_copy_stats_count_actual_pixels_and_saturate() {
        let mut stats = PerfStats::default();
        stats.record_host_direct_frame();
        stats.record_host_copy_frame(10, 20);
        assert_eq!(stats.host_direct_frames, 1);
        assert_eq!(stats.host_copy_frames, 1);
        assert_eq!(stats.host_copy_pixels, 200);

        stats.host_copy_pixels = u64::MAX - 1;
        stats.record_host_copy_frame(u32::MAX, u32::MAX);
        assert_eq!(stats.host_copy_pixels, u64::MAX);
    }

    #[test]
    pub(super) fn platform_present_baseline_delta_json_and_reset_semantics() {
        let baseline = PixelPresentStats {
            sequence: 4,
            count: 4,
            success_count: 3,
            failure_count: 1,
            last_ns: 8,
            total_ns: 30,
            max_ns: 8,
            full_pixels: 100,
            partial_pixels: 50,
            requested_full_pixels: 120,
            requested_partial_pixels: 60,
        };
        let current = PixelPresentStats {
            sequence: 6,
            count: 6,
            success_count: 4,
            failure_count: 2,
            last_ns: 5,
            total_ns: 43,
            max_ns: 8,
            full_pixels: 140,
            partial_pixels: 65,
            requested_full_pixels: 170,
            requested_partial_pixels: 80,
        };

        let mut stats = PerfStats::default();
        stats.reset(baseline);
        assert_eq!(stats.present_delta(), PixelPresentStats::default());

        stats.sync_present_stats(current);
        let delta = stats.present_delta();
        assert_eq!(delta.count, 2);
        assert_eq!(delta.success_count, 1);
        assert_eq!(delta.failure_count, 1);
        assert_eq!(delta.last_ns, 5);
        assert_eq!(delta.total_ns, 13);
        // The cumulative platform max (8ns) is not subtracted. After reset,
        // max is the maximum post-reset sample observed by the GUI (5ns).
        assert_eq!(delta.max_ns, 5);
        assert_eq!(delta.full_pixels, 40);
        assert_eq!(delta.partial_pixels, 15);
        assert_eq!(delta.requested_full_pixels, 50);
        assert_eq!(delta.requested_partial_pixels, 20);

        let serialized = String::from_utf8(json::to_vec(&stats.json())).expect("JSON is UTF-8");
        for field in [
            "present_count",
            "present_success",
            "present_failure",
            "last_ns",
            "total_ns",
            "max_ns",
            "full_pixels",
            "partial_pixels",
            "requested_full_pixels",
            "requested_partial_pixels",
        ] {
            assert!(serialized.contains(field), "missing {field}: {serialized}");
        }

        stats.reset(current);
        let after_reset = stats.present_delta();
        assert_eq!(after_reset.count, 0);
        assert_eq!(after_reset.last_ns, 0);
        assert_eq!(after_reset.max_ns, 0);
    }

    #[test]
    pub(super) fn candidate_redraw_request_converts_bounds_without_product_semantics() {
        let mut candidate = DirtyRegion::empty();
        candidate.mark_rect(PixelRect::from_xywh(4, 6, 16, 24));
        assert_eq!(
            candidate_redraw_request(candidate, 100, 80),
            CandidateRedrawRequest::Partial(HostPixelRect::new(4, 6, 20, 30))
        );
        assert_eq!(
            candidate_redraw_request(DirtyRegion::full(), 100, 80),
            CandidateRedrawRequest::Full
        );
        assert_eq!(
            candidate_redraw_request(DirtyRegion::empty(), 100, 80),
            CandidateRedrawRequest::None
        );
    }

    #[test]
    pub(super) fn frame_write_mapping_for_direct_and_transient_hosts() {
        let mut partial = DirtyRegion::empty();
        partial.mark_rect(PixelRect::from_xywh(4, 6, 16, 24));

        assert_eq!(
            frame_write_for_candidate(
                PixelBackingRetention::RetainedAcrossFrames,
                false,
                partial,
                100,
                80,
            ),
            PixelFrameWrite::Full
        );
        assert_eq!(
            frame_write_for_candidate(
                PixelBackingRetention::RetainedAcrossFrames,
                true,
                partial,
                100,
                80,
            ),
            PixelFrameWrite::Partial(HostPixelRect::new(4, 6, 20, 30))
        );
        assert_eq!(
            frame_write_for_candidate(
                PixelBackingRetention::RetainedAcrossFrames,
                true,
                DirtyRegion::empty(),
                100,
                80,
            ),
            PixelFrameWrite::None
        );
        assert_eq!(
            frame_write_for_candidate(PixelBackingRetention::Transient, true, partial, 100, 80,),
            PixelFrameWrite::Full
        );
    }
}
