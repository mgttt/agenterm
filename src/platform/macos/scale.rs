//! macOS logical-point / backing-pixel adapter.
//!
//! Cocoa/winit event extraction stays macOS-owned; conversion, validation,
//! metrics, and event classification consume the shared platform contract.

#![cfg(target_os = "macos")]

pub(crate) use crate::platform::scale::{
    GeometryAction as MacosGeometryAction, GeometryEvent as MacosGeometryEvent,
    WindowMetrics as MacosWindowMetrics, classify_geometry_event,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::scale::{ScaleError, ScaleFactor};

    #[test]
    fn macos_aliases_consume_shared_geometry_contract() {
        let retina = ScaleFactor::new(2.0).expect("valid Retina scale");
        assert_eq!(retina.physical_pixels(960.0), Ok(1920));
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
        assert_eq!(ScaleFactor::new(0.0), Err(ScaleError::InvalidScaleFactor));
    }
}
