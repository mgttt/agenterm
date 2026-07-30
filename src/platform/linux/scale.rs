//! Linux DPI / scale-factor bridge for platform migration slice-2
//! (contract revision 1).
//!
//! X11/Wayland via winit report layout in logical points and framebuffer
//! surfaces in physical pixels. Invalid native scale factors become typed
//! failures — never a silent Available with bad metrics.
//!
//! Scale factor types stay Linux-private (same pattern as macOS). Shared
//! contract already has Window + [`CapabilityStatus`]; do not invent shared
//! DPI fields here without a contract revision request.

#![cfg(target_os = "linux")]

use crate::platform::{CapabilityStatus, DisplayBackendFacts};

use super::display_facts_from_env;

/// Validated Linux window scale factor (logical point → physical pixel).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LinuxScaleFactor(f64);

impl LinuxScaleFactor {
    pub(crate) fn new(value: f64) -> Result<Self, LinuxScaleError> {
        if value.is_finite() && value > 0.0 {
            Ok(Self(value))
        } else {
            Err(LinuxScaleError::InvalidScaleFactor)
        }
    }

    pub(crate) const fn get(self) -> f64 {
        self.0
    }

    pub(crate) fn physical_pixels(self, logical_points: f64) -> Result<u32, LinuxScaleError> {
        if !logical_points.is_finite() || logical_points < 0.0 {
            return Err(LinuxScaleError::InvalidLogicalExtent);
        }
        let pixels = (logical_points * self.0).round();
        if pixels > f64::from(u32::MAX) {
            return Err(LinuxScaleError::PhysicalExtentOverflow);
        }
        Ok(pixels as u32)
    }

    pub(crate) fn logical_points(self, physical_pixels: u32) -> Result<u32, LinuxScaleError> {
        let points = f64::from(physical_pixels) / self.0;
        if !points.is_finite() || points < 0.0 {
            return Err(LinuxScaleError::InvalidPhysicalExtent);
        }
        if points > f64::from(u32::MAX) {
            return Err(LinuxScaleError::LogicalExtentOverflow);
        }
        Ok(points.round().max(0.0) as u32)
    }
}

/// Typed Linux scale / DPI failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinuxScaleError {
    InvalidScaleFactor,
    InvalidLogicalExtent,
    InvalidPhysicalExtent,
    PhysicalExtentOverflow,
    LogicalExtentOverflow,
}

impl LinuxScaleError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::InvalidScaleFactor => "invalid_scale_factor",
            Self::InvalidLogicalExtent => "invalid_logical_extent",
            Self::InvalidPhysicalExtent => "invalid_physical_extent",
            Self::PhysicalExtentOverflow => "physical_extent_overflow",
            Self::LogicalExtentOverflow => "logical_extent_overflow",
        }
    }

    pub(crate) fn to_capability_status(self) -> CapabilityStatus {
        CapabilityStatus::Failed {
            code: self.code(),
            message: self.code().replace('_', " "),
        }
    }
}

/// Snapshot of window metrics after a resize or scale-factor change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinuxWindowMetrics {
    pub logical_width: u32,
    pub logical_height: u32,
    pub physical_width: u32,
    pub physical_height: u32,
}

impl LinuxWindowMetrics {
    /// Build metrics from native physical size + scale factor.
    pub(crate) fn from_physical(
        physical_width: u32,
        physical_height: u32,
        scale_factor: f64,
    ) -> Result<(Self, LinuxScaleFactor), LinuxScaleError> {
        let scale = LinuxScaleFactor::new(scale_factor)?;
        if physical_width == 0 || physical_height == 0 {
            return Err(LinuxScaleError::InvalidPhysicalExtent);
        }
        let logical_width = scale.logical_points(physical_width)?.max(1);
        let logical_height = scale.logical_points(physical_height)?.max(1);
        Ok((
            Self {
                logical_width,
                logical_height,
                physical_width,
                physical_height,
            },
            scale,
        ))
    }
}

/// Linux-local window geometry event (adapter detail).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LinuxGeometryEvent {
    Resized {
        physical_width: u32,
        physical_height: u32,
        scale_factor: f64,
    },
    ScaleFactorChanged {
        scale_factor: f64,
        physical_width: u32,
        physical_height: u32,
    },
}

/// Action the Linux GUI host should take after classifying a geometry event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinuxGeometryAction {
    /// Apply new metrics and request redraw / PTY resize.
    Apply(LinuxWindowMetrics),
    /// Ignore (zero-sized iconic resize, etc.).
    Ignore,
}

/// Classify resize / scale-factor changes into typed metrics or Ignore.
pub(crate) fn classify_geometry_event(
    event: LinuxGeometryEvent,
) -> Result<LinuxGeometryAction, LinuxScaleError> {
    let (physical_width, physical_height, scale_factor) = match event {
        LinuxGeometryEvent::Resized {
            physical_width,
            physical_height,
            scale_factor,
        }
        | LinuxGeometryEvent::ScaleFactorChanged {
            scale_factor,
            physical_width,
            physical_height,
        } => (physical_width, physical_height, scale_factor),
    };
    if physical_width == 0 || physical_height == 0 {
        return Ok(LinuxGeometryAction::Ignore);
    }
    let (metrics, _) =
        LinuxWindowMetrics::from_physical(physical_width, physical_height, scale_factor)?;
    Ok(LinuxGeometryAction::Apply(metrics))
}

/// DPI/scale rides the Window capability: Available with a display backend.
pub(crate) fn scale_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    if facts.headless {
        CapabilityStatus::Unsupported {
            reason: "headless-display",
        }
    } else {
        CapabilityStatus::Available
    }
}

pub(crate) fn scale_capability_status_from_env() -> CapabilityStatus {
    scale_capability_status(display_facts_from_env())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_scale_maps_points_one_to_one() {
        let scale = LinuxScaleFactor::new(1.0).expect("valid scale");
        assert_eq!(scale.physical_pixels(960.0), Ok(960));
        assert_eq!(scale.logical_points(960), Ok(960));
    }

    #[test]
    fn fractional_scale_uses_nearest_physical_pixel() {
        let scale = LinuxScaleFactor::new(1.5).expect("valid fractional scale");
        assert_eq!(scale.physical_pixels(101.0), Ok(152));
        assert_eq!(scale.logical_points(152), Ok(101));
    }

    #[test]
    fn hi_dpi_scale_maps_points_to_backing_pixels() {
        let scale = LinuxScaleFactor::new(2.0).expect("valid 2x scale");
        assert_eq!(scale.physical_pixels(960.0), Ok(1920));
        assert_eq!(scale.logical_points(1920), Ok(960));
    }

    #[test]
    fn invalid_native_metrics_are_typed_failures() {
        assert_eq!(
            LinuxScaleFactor::new(0.0),
            Err(LinuxScaleError::InvalidScaleFactor)
        );
        assert_eq!(
            LinuxScaleFactor::new(f64::NAN),
            Err(LinuxScaleError::InvalidScaleFactor)
        );
        let scale = LinuxScaleFactor::new(2.0).expect("valid scale");
        assert_eq!(
            scale.physical_pixels(-1.0),
            Err(LinuxScaleError::InvalidLogicalExtent)
        );
        assert!(matches!(
            LinuxScaleError::InvalidScaleFactor.to_capability_status(),
            CapabilityStatus::Failed {
                code: "invalid_scale_factor",
                ..
            }
        ));
    }

    #[test]
    fn zero_physical_resize_is_ignored_not_available_claim() {
        let action = classify_geometry_event(LinuxGeometryEvent::Resized {
            physical_width: 0,
            physical_height: 600,
            scale_factor: 1.0,
        })
        .expect("typed");
        assert_eq!(action, LinuxGeometryAction::Ignore);
    }

    #[test]
    fn scale_factor_change_applies_new_metrics() {
        let action = classify_geometry_event(LinuxGeometryEvent::ScaleFactorChanged {
            scale_factor: 2.0,
            physical_width: 1920,
            physical_height: 1200,
        })
        .expect("typed");
        assert_eq!(
            action,
            LinuxGeometryAction::Apply(LinuxWindowMetrics {
                logical_width: 960,
                logical_height: 600,
                physical_width: 1920,
                physical_height: 1200,
            })
        );
    }

    #[test]
    fn invalid_scale_factor_event_fails_explicitly() {
        let err = classify_geometry_event(LinuxGeometryEvent::ScaleFactorChanged {
            scale_factor: 0.0,
            physical_width: 800,
            physical_height: 600,
        })
        .expect_err("invalid");
        assert_eq!(err, LinuxScaleError::InvalidScaleFactor);
    }

    #[test]
    fn headless_scale_capability_is_unsupported() {
        let status = scale_capability_status(DisplayBackendFacts {
            x11: false,
            wayland: false,
            headless: true,
        });
        assert!(matches!(
            status,
            CapabilityStatus::Unsupported {
                reason: "headless-display"
            }
        ));
    }
}
