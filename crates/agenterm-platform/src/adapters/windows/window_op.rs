//! Windows window operations (user32 FFI): show/move/topmost/close.

use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        MoveWindow, PostMessageW, SetWindowPos, ShowWindow, HWND_NOTOPMOST, HWND_TOPMOST,
        SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE,
        SW_SHOW, WM_CLOSE,
    },
};

use crate::contract::window_op::{WindowOpError, WindowShowState};
use crate::CapabilityStatus;

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

pub(crate) fn show(handle: isize, state: WindowShowState) -> Result<(), WindowOpError> {
    let cmd = match state {
        WindowShowState::Hide => SW_HIDE,
        WindowShowState::Show => SW_SHOW,
        WindowShowState::Minimize => SW_MINIMIZE,
        WindowShowState::Maximize => SW_MAXIMIZE,
        WindowShowState::Restore => SW_RESTORE,
    };
    unsafe {
        ShowWindow(handle as HWND, cmd);
    }
    Ok(())
}

pub(crate) fn move_window(
    handle: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), WindowOpError> {
    unsafe {
        if MoveWindow(handle as HWND, x, y, width as i32, height as i32, 1) == 0 {
            return Err(WindowOpError::failed(
                "move_window_failed",
                "MoveWindow returned 0",
            ));
        }
    }
    Ok(())
}

pub(crate) fn set_topmost(handle: isize, topmost: bool) -> Result<(), WindowOpError> {
    let after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    unsafe {
        if SetWindowPos(handle as HWND, after, 0, 0, 0, 0, flags) == 0 {
            return Err(WindowOpError::failed(
                "set_window_pos_failed",
                "SetWindowPos returned 0",
            ));
        }
    }
    Ok(())
}

pub(crate) fn close(handle: isize) -> Result<(), WindowOpError> {
    unsafe {
        if PostMessageW(handle as HWND, WM_CLOSE, 0, 0) == 0 {
            return Err(WindowOpError::failed(
                "post_close_failed",
                "PostMessage(WM_CLOSE) returned 0",
            ));
        }
    }
    Ok(())
}
