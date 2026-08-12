//! Window operation facade (portable entry point).

pub use crate::contract::window_op::{WindowOpError, WindowShowState};
use crate::CapabilityStatus;

pub fn capability_status() -> CapabilityStatus {
    crate::selected::window_op::capability_status()
}

pub fn show(handle: isize, state: WindowShowState) -> Result<(), WindowOpError> {
    crate::selected::window_op::show(handle, state)
}

pub fn move_window(
    handle: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), WindowOpError> {
    crate::selected::window_op::move_window(handle, x, y, width, height)
}

pub fn set_topmost(handle: isize, topmost: bool) -> Result<(), WindowOpError> {
    crate::selected::window_op::set_topmost(handle, topmost)
}

pub fn close(handle: isize) -> Result<(), WindowOpError> {
    crate::selected::window_op::close(handle)
}
