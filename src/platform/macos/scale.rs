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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MacosScaleError {
    InvalidScaleFactor,
    InvalidLogicalExtent,
    PhysicalExtentOverflow,
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
}
