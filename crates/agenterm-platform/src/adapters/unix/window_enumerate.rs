//! Unix placeholder: window enumeration is not yet wired on Linux/macOS.

use crate::CapabilityStatus;
use crate::contract::window_enumerate::{WindowEnumerateError, WindowInfo};

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Unsupported {
        reason: "window-enum not wired on unix".into(),
    }
}

pub(crate) fn enumerate_top_level() -> Result<Vec<WindowInfo>, WindowEnumerateError> {
    Err(WindowEnumerateError::Unsupported {
        reason: "window-enum not wired on unix".into(),
    })
}
