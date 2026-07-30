//! macOS logical-point to backing-pixel conversion.
//!
//! Cocoa/winit reports layout in logical points and rendering/screenshot
//! surfaces in physical pixels. The adapter rejects unusable scale factors
//! instead of claiming scale-factor accuracy with invalid native data.

#![cfg(target_os = "macos")]

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MacosScaleFactor(f64);

impl MacosScaleFactor {
    pub(crate) fn new(value: f64) -> Result<Self, MacosScaleError> {
        if value.is_finite() && value > 0.0 {
            Ok(Self(value))
        } else {
            Err(MacosScaleError::InvalidScaleFactor)
        }
    }

    pub(crate) const fn get(self) -> f64 {
        self.0
    }

    pub(crate) fn physical_pixels(self, logical_points: f64) -> Result<u32, MacosScaleError> {
        if !logical_points.is_finite() || logical_points < 0.0 {
            return Err(MacosScaleError::InvalidLogicalExtent);
        }
        let pixels = (logical_points * self.0).round();
        if pixels > u32::MAX as f64 {
            return Err(MacosScaleError::PhysicalExtentOverflow);
        }
        Ok(pixels as u32)
    }

    pub(crate) fn logical_points(self, physical_pixels: u32) -> Result<u32, MacosScaleError> {
        let points = f64::from(physical_pixels) / self.0;
        if !points.is_finite() || points < 0.0 {
            return Err(MacosScaleError::InvalidPhysicalExtent);
        }
        if points > f64::from(u32::MAX) {
            return Err(MacosScaleError::LogicalExtentOverflow);
        }
        Ok(points.round().max(0.0) as u32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MacosScaleError {
    InvalidScaleFactor,
    InvalidLogicalExtent,
    InvalidPhysicalExtent,
    PhysicalExtentOverflow,
    LogicalExtentOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MacosWindowMetrics {
    pub logical_width: u32,
    pub logical_height: u32,
    pub physical_width: u32,
    pub physical_height: u32,
}

impl MacosWindowMetrics {
    pub(crate) fn from_physical(
        physical_width: u32,
        physical_height: u32,
        scale_factor: f64,
    ) -> Result<(Self, MacosScaleFactor), MacosScaleError> {
        let scale = MacosScaleFactor::new(scale_factor)?;
        if physical_width == 0 || physical_height == 0 {
            return Err(MacosScaleError::InvalidPhysicalExtent);
        }
        Ok((
            Self {
                logical_width: scale.logical_points(physical_width)?.max(1),
                logical_height: scale.logical_points(physical_height)?.max(1),
                physical_width,
                physical_height,
            },
            scale,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MacosGeometryEvent {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MacosGeometryAction {
    Apply(MacosWindowMetrics),
    Ignore,
}

pub(crate) fn classify_geometry_event(
    event: MacosGeometryEvent,
) -> Result<MacosGeometryAction, MacosScaleError> {
    let (physical_width, physical_height, scale_factor) = match event {
        MacosGeometryEvent::Resized {
            physical_width,
            physical_height,
            scale_factor,
        }
        | MacosGeometryEvent::ScaleFactorChanged {
            scale_factor,
            physical_width,
            physical_height,
        } => (physical_width, physical_height, scale_factor),
    };
    if physical_width == 0 || physical_height == 0 {
        return Ok(MacosGeometryAction::Ignore);
    }
    let (metrics, _) =
        MacosWindowMetrics::from_physical(physical_width, physical_height, scale_factor)?;
    Ok(MacosGeometryAction::Apply(metrics))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retina_scale_maps_points_to_backing_pixels() {
        let retina = MacosScaleFactor::new(2.0).expect("valid Retina scale");
        assert_eq!(retina.get(), 2.0);
        assert_eq!(retina.physical_pixels(960.0), Ok(1920));
        assert_eq!(retina.physical_pixels(600.0), Ok(1200));
    }

    #[test]
    fn fractional_scale_uses_nearest_physical_pixel() {
        let scale = MacosScaleFactor::new(1.5).expect("valid fractional scale");
        assert_eq!(scale.physical_pixels(101.0), Ok(152));
        assert_eq!(scale.logical_points(152), Ok(101));
    }

    #[test]
    fn invalid_native_metrics_are_typed_failures() {
        assert_eq!(
            MacosScaleFactor::new(0.0),
            Err(MacosScaleError::InvalidScaleFactor)
        );
        assert_eq!(
            MacosScaleFactor::new(f64::NAN),
            Err(MacosScaleError::InvalidScaleFactor)
        );
        let scale = MacosScaleFactor::new(2.0).expect("valid scale");
        assert_eq!(
            scale.physical_pixels(-1.0),
            Err(MacosScaleError::InvalidLogicalExtent)
        );
        assert_eq!(
            scale.physical_pixels(f64::MAX),
            Err(MacosScaleError::PhysicalExtentOverflow)
        );
    }

    #[test]
    fn retina_change_applies_logical_and_physical_metrics() {
        assert_eq!(
            classify_geometry_event(MacosGeometryEvent::ScaleFactorChanged {
                scale_factor: 2.0,
                physical_width: 1920,
                physical_height: 1200,
            }),
            Ok(MacosGeometryAction::Apply(MacosWindowMetrics {
                logical_width: 960,
                logical_height: 600,
                physical_width: 1920,
                physical_height: 1200,
            }))
        );
    }

    #[test]
    fn zero_sized_native_resize_is_ignored() {
        assert_eq!(
            classify_geometry_event(MacosGeometryEvent::Resized {
                physical_width: 0,
                physical_height: 600,
                scale_factor: 2.0,
            }),
            Ok(MacosGeometryAction::Ignore)
        );
    }
}
