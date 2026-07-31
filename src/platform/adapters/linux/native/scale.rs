//! Linux DPI / scale-factor adapter.
//! Adapter-private native mechanism selected only by platform::selected.
//!
//! X11/Wayland event extraction stays Linux-owned; conversion, validation,
//! metrics, and event classification consume the shared platform contract.

#![cfg(target_os = "linux")]

use crate::platform::{CapabilityStatus, DisplayBackendFacts};

use super::display_facts_from_env;

pub(crate) use crate::platform::selected::scale_contract::{
    GeometryAction as LinuxGeometryAction, GeometryEvent as LinuxGeometryEvent,
    WindowMetrics as LinuxWindowMetrics, classify_geometry_event,
};

pub(crate) fn scale_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    crate::platform::selected::scale::capability_status(facts)
}

pub(crate) fn scale_capability_status_from_env() -> CapabilityStatus {
    scale_capability_status(display_facts_from_env())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::selected::scale_contract::{ScaleError, ScaleFactor};

    #[test]
    fn linux_aliases_consume_shared_geometry_contract() {
        let scale = ScaleFactor::new(1.5).expect("valid fractional scale");
        assert_eq!(scale.physical_pixels(101.0), Ok(152));
        assert_eq!(scale.logical_points(152), Ok(101));
        assert_eq!(
            classify_geometry_event(LinuxGeometryEvent::ScaleFactorChanged {
                scale_factor: 2.0,
                physical_width: 1920,
                physical_height: 1200,
            }),
            Ok(LinuxGeometryAction::Apply(LinuxWindowMetrics {
                logical_width: 960,
                logical_height: 600,
                physical_width: 1920,
                physical_height: 1200,
            }))
        );
        assert_eq!(
            ScaleError::InvalidScaleFactor.code(),
            "invalid_scale_factor"
        );
    }

    #[test]
    fn headless_scale_capability_is_unsupported() {
        assert_eq!(
            scale_capability_status(DisplayBackendFacts {
                x11: false,
                wayland: false,
                headless: true,
            }),
            CapabilityStatus::Unsupported {
                reason: "headless-display"
            }
        );
    }
}
