//! Linux frontend display availability for the product screenshot projection.
//!
//! Framebuffer validation and PNG encoding live in `agenterm-platform`; this
//! module only reports whether the AgenTerm frontend has a graphical display.

#![cfg(target_os = "linux")]

use crate::platform::{CapabilityStatus, DisplayBackendFacts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScreenshotBackend {
    X11,
    Wayland,
    X11AndWayland,
}

impl ScreenshotBackend {
    fn from_display(facts: DisplayBackendFacts) -> Option<Self> {
        match (facts.x11, facts.wayland, facts.headless) {
            (_, _, true) | (false, false, false) => None,
            (true, true, false) => Some(Self::X11AndWayland),
            (true, false, false) => Some(Self::X11),
            (false, true, false) => Some(Self::Wayland),
        }
    }
}

pub(crate) fn screenshot_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    if ScreenshotBackend::from_display(facts).is_some() {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unsupported {
            reason: "headless-display",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_screenshot_is_unsupported() {
        assert_eq!(
            screenshot_capability_status(DisplayBackendFacts {
                x11: false,
                wayland: false,
                headless: true,
            }),
            CapabilityStatus::Unsupported {
                reason: "headless-display",
            }
        );
    }

    #[test]
    fn x11_and_wayland_screenshot_are_available() {
        for facts in [
            DisplayBackendFacts {
                x11: true,
                wayland: false,
                headless: false,
            },
            DisplayBackendFacts {
                x11: false,
                wayland: true,
                headless: false,
            },
        ] {
            assert_eq!(
                screenshot_capability_status(facts),
                CapabilityStatus::Available
            );
        }
    }
}
