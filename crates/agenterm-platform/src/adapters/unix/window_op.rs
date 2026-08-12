//! Unix placeholder: window operations are not yet wired on Linux/macOS.

use crate::contract::window_op::{WindowOpError, WindowShowState};
use crate::CapabilityStatus;

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Unsupported {
        reason: "window-op not wired on unix".into(),
    }
}

pub(crate) fn show(_handle: isize, _state: WindowShowState) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window-op not wired on unix".into(),
    })
}

pub(crate) fn move_window(
    _handle: isize,
    _x: i32,
    _y: i32,
    _width: u32,
    _height: u32,
) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window-op not wired on unix".into(),
    })
}

pub(crate) fn set_topmost(_handle: isize, _topmost: bool) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window-op not wired on unix".into(),
    })
}

pub(crate) fn close(_handle: isize) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window-op not wired on unix".into(),
    })
}
