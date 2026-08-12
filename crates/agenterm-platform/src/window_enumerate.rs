//! Window enumeration facade (portable entry point).
//!
//! Consumers (e.g. `agenterm-cu`) call these functions; the OS-level
//! mechanism lives in the platform adapter selected by `crate::selected`.

pub use crate::contract::window_enumerate::{WindowBounds, WindowEnumerateError, WindowInfo};
use crate::CapabilityStatus;

pub fn capability_status() -> CapabilityStatus {
    crate::selected::window_enumerate::capability_status()
}

pub fn enumerate_top_level() -> Result<Vec<WindowInfo>, WindowEnumerateError> {
    crate::selected::window_enumerate::enumerate_top_level()
}
