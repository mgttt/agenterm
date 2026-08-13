//! Window enumeration facade (portable entry point).
//!
//! Consumers (e.g. `agenterm-cu`) call these functions; the OS-level
//! mechanism lives in the platform adapter selected by `crate::selected`.

use crate::CapabilityStatus;
pub use crate::contract::window_enumerate::{
    ScreenInfo, WindowBounds, WindowEnumerateError, WindowInfo,
};

pub fn capability_status() -> CapabilityStatus {
    crate::selected::window_enumerate::capability_status()
}

pub fn enumerate_top_level() -> Result<Vec<WindowInfo>, WindowEnumerateError> {
    crate::selected::window_enumerate::enumerate_top_level()
}

pub fn list_screens() -> Result<Vec<ScreenInfo>, WindowEnumerateError> {
    crate::selected::window_enumerate::list_screens()
}
