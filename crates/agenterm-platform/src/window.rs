//! Shared logical-point / physical-pixel window geometry contract.
//!
//! Native adapters own event extraction and capability discovery. This module
//! owns the cross-platform validation and conversion semantics consumed by the
//! Linux and macOS GUI hot paths.

use std::borrow::Cow;

use crate::{CapabilityStatus, selected};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct DisplayBackendFacts {
    pub x11: bool,
    pub wayland: bool,
    pub headless: bool,
}

pub fn display_backend_facts() -> DisplayBackendFacts {
    selected::window::display_backend_facts()
}

pub fn capability_status() -> CapabilityStatus {
    if display_backend_facts().headless {
        CapabilityStatus::Unsupported {
            reason: "headless-display".into(),
        }
    } else {
        CapabilityStatus::Available
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleFactor(f64);

impl ScaleFactor {
    pub fn new(value: f64) -> Result<Self, ScaleError> {
        if value.is_finite() && value > 0.0 {
            Ok(Self(value))
        } else {
            Err(ScaleError::InvalidScaleFactor)
        }
    }

    /// Frozen forward conversion for adapters that receive logical extents.
    #[allow(dead_code)]
    pub fn physical_pixels(self, logical_points: f64) -> Result<u32, ScaleError> {
        if !logical_points.is_finite() || logical_points < 0.0 {
            return Err(ScaleError::InvalidLogicalExtent);
        }
        let pixels = (logical_points * self.0).round();
        if pixels > f64::from(u32::MAX) {
            return Err(ScaleError::PhysicalExtentOverflow);
        }
        Ok(pixels as u32)
    }

    pub fn logical_points(self, physical_pixels: u32) -> Result<u32, ScaleError> {
        let points = f64::from(physical_pixels) / self.0;
        if !points.is_finite() || points < 0.0 {
            return Err(ScaleError::InvalidPhysicalExtent);
        }
        if points > f64::from(u32::MAX) {
            return Err(ScaleError::LogicalExtentOverflow);
        }
        Ok(points.round() as u32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleError {
    InvalidScaleFactor,
    InvalidLogicalExtent,
    InvalidPhysicalExtent,
    PhysicalExtentOverflow,
    LogicalExtentOverflow,
}

impl ScaleError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidScaleFactor => "invalid_scale_factor",
            Self::InvalidLogicalExtent => "invalid_logical_extent",
            Self::InvalidPhysicalExtent => "invalid_physical_extent",
            Self::PhysicalExtentOverflow => "physical_extent_overflow",
            Self::LogicalExtentOverflow => "logical_extent_overflow",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowMetrics {
    pub logical_width: u32,
    pub logical_height: u32,
    /// Retained in the normalized contract for framebuffer/screenshot consumers.
    #[allow(dead_code)]
    pub physical_width: u32,
    /// Retained in the normalized contract for framebuffer/screenshot consumers.
    #[allow(dead_code)]
    pub physical_height: u32,
}

impl WindowMetrics {
    pub fn from_physical(
        physical_width: u32,
        physical_height: u32,
        scale_factor: f64,
    ) -> Result<(Self, ScaleFactor), ScaleError> {
        let scale = ScaleFactor::new(scale_factor)?;
        if physical_width == 0 || physical_height == 0 {
            return Err(ScaleError::InvalidPhysicalExtent);
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
pub enum GeometryEvent {
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
pub enum GeometryAction {
    Apply(WindowMetrics),
    Ignore,
}

pub fn classify_geometry_event(event: GeometryEvent) -> Result<GeometryAction, ScaleError> {
    let (physical_width, physical_height, scale_factor) = match event {
        GeometryEvent::Resized {
            physical_width,
            physical_height,
            scale_factor,
        }
        | GeometryEvent::ScaleFactorChanged {
            scale_factor,
            physical_width,
            physical_height,
        } => (physical_width, physical_height, scale_factor),
    };
    if physical_width == 0 || physical_height == 0 {
        return Ok(GeometryAction::Ignore);
    }
    let (metrics, _) = WindowMetrics::from_physical(physical_width, physical_height, scale_factor)?;
    Ok(GeometryAction::Apply(metrics))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NativeTextWindowFocus {
    Activate,
    NoActivate,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeTextFrame<'a> {
    pub pixels: &'a [u32],
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NativeTextWindowError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl NativeTextWindowError {
    pub fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code: Cow::Borrowed(code),
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for NativeTextWindowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { reason } => {
                write!(formatter, "native text window unsupported: {reason}")
            }
            Self::Failed { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for NativeTextWindowError {}

pub trait NativeTextWindowHost: Send {
    fn title(&self) -> String;
    fn lines(&self) -> Vec<String>;
    fn poll(&mut self) -> bool;
    fn close_requested(&self) -> bool;
    fn publish_native_window(&mut self, raw_handle: i64) -> Result<(), NativeTextWindowError>;
    fn take_focus_request(&mut self) -> Option<NativeTextWindowFocus>;
    fn capture_requested_screenshot(
        &mut self,
        frame: Option<NativeTextFrame<'_>>,
    ) -> Result<(), NativeTextWindowError>;
}

pub fn run_native_text_window(
    host: Box<dyn NativeTextWindowHost>,
    no_activate: bool,
) -> Result<(), NativeTextWindowError> {
    selected::window::run_native_text_window(host, no_activate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_and_fractional_scale_are_deterministic() {
        let unit = ScaleFactor::new(1.0).expect("unit scale");
        assert_eq!(unit.physical_pixels(960.0), Ok(960));
        assert_eq!(unit.logical_points(960), Ok(960));

        let fractional = ScaleFactor::new(1.5).expect("fractional scale");
        assert_eq!(fractional.physical_pixels(101.0), Ok(152));
        assert_eq!(fractional.logical_points(152), Ok(101));
    }

    #[test]
    fn invalid_metrics_fail_with_stable_codes() {
        assert_eq!(ScaleFactor::new(0.0), Err(ScaleError::InvalidScaleFactor));
        assert_eq!(
            ScaleFactor::new(f64::NAN),
            Err(ScaleError::InvalidScaleFactor)
        );
        let scale = ScaleFactor::new(2.0).expect("valid scale");
        assert_eq!(
            scale.physical_pixels(-1.0),
            Err(ScaleError::InvalidLogicalExtent)
        );
        assert_eq!(
            scale.physical_pixels(f64::MAX),
            Err(ScaleError::PhysicalExtentOverflow)
        );
        assert_eq!(
            ScaleError::InvalidScaleFactor.code(),
            "invalid_scale_factor"
        );
    }

    #[test]
    fn geometry_classification_is_shared() {
        assert_eq!(
            classify_geometry_event(GeometryEvent::ScaleFactorChanged {
                scale_factor: 2.0,
                physical_width: 1920,
                physical_height: 1200,
            }),
            Ok(GeometryAction::Apply(WindowMetrics {
                logical_width: 960,
                logical_height: 600,
                physical_width: 1920,
                physical_height: 1200,
            }))
        );
        assert_eq!(
            classify_geometry_event(GeometryEvent::Resized {
                physical_width: 0,
                physical_height: 600,
                scale_factor: 1.0,
            }),
            Ok(GeometryAction::Ignore)
        );
    }

    #[test]
    fn native_text_window_failures_remain_typed() {
        let unsupported = NativeTextWindowError::Unsupported {
            reason: "headless-display".into(),
        };
        let failed = NativeTextWindowError::failed("window-create-failed", "no window");
        assert!(unsupported.to_string().contains("unsupported"));
        assert_eq!(failed.to_string(), "window-create-failed: no window");
    }
}
