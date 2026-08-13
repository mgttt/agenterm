//! macOS window operations via Accessibility set-rect.

#![cfg(target_os = "macos")]

use crate::CapabilityStatus;
use crate::contract::window_enumerate::WindowBounds;
use crate::contract::window_op::{WindowOpError, WindowShowState};

use crate::selected::macos_foreign_windows as foreign_windows;

pub(crate) fn capability_status() -> CapabilityStatus {
    foreign_windows::capability_status()
}

pub(crate) fn show(handle: isize, state: WindowShowState) -> Result<(), WindowOpError> {
    foreign_windows::show(handle, state)
}

pub(crate) fn move_window(
    handle: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), WindowOpError> {
    foreign_windows::move_window(handle, x, y, width, height)
}

pub(crate) fn window_rect(handle: isize) -> Result<WindowBounds, WindowOpError> {
    foreign_windows::window_rect(handle)
}

pub(crate) fn set_topmost(handle: isize, topmost: bool) -> Result<(), WindowOpError> {
    foreign_windows::set_topmost(handle, topmost)
}

pub(crate) fn close(handle: isize) -> Result<(), WindowOpError> {
    foreign_windows::close(handle)
}
