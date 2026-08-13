//! macOS top-level window enumeration via CGWindowList.

#![cfg(target_os = "macos")]

use crate::CapabilityStatus;
use crate::contract::window_enumerate::{ScreenInfo, WindowEnumerateError, WindowInfo};

use crate::selected::macos_foreign_windows as foreign_windows;

pub(crate) fn capability_status() -> CapabilityStatus {
    foreign_windows::capability_status()
}

pub(crate) fn enumerate_top_level() -> Result<Vec<WindowInfo>, WindowEnumerateError> {
    foreign_windows::enumerate_top_level()
}

pub(crate) fn list_screens() -> Result<Vec<ScreenInfo>, WindowEnumerateError> {
    foreign_windows::list_screens()
}
