use std::borrow::Cow;

use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GetForegroundWindow, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
    },
};

use crate::contract::activation::{ActivationError, ActivationRequest, NativeWindowHandle};

pub trait WindowAttributesActivationExt: Sized {
    fn with_platform_activation(self, no_activate: bool) -> Self;
}

pub trait EventLoopActivationExt {
    fn configure_platform_activation(&mut self, no_activate: bool);
}

pub(crate) fn apply(
    window: NativeWindowHandle,
    request: ActivationRequest,
) -> Result<(), ActivationError> {
    let window = window.raw() as HWND;
    match request {
        ActivationRequest::ShowWithoutActivation => show_without_activation(window),
        ActivationRequest::ShowNewAndRequestActivation => {
            unsafe {
                ShowWindow(window, SW_SHOW);
                SetForegroundWindow(window);
            }
            Ok(())
        }
        ActivationRequest::RestoreAndActivate => show_and_activate(window, SW_RESTORE),
    }
}

fn show_without_activation(window: HWND) -> Result<(), ActivationError> {
    let foreground = unsafe { GetForegroundWindow() };
    let (insert_after, flags) = if !foreground.is_null() && foreground != window {
        (
            foreground,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    } else {
        (
            std::ptr::null_mut(),
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
    if unsafe { SetWindowPos(window, insert_after, 0, 0, 0, 0, flags) } == 0 {
        return Err(failed(
            "show_without_activation_failed",
            "Windows could not show the window without activation",
        ));
    }
    unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
    Ok(())
}

fn show_and_activate(window: HWND, command: i32) -> Result<(), ActivationError> {
    unsafe { ShowWindow(window, command) };
    if unsafe { SetForegroundWindow(window) } == 0 {
        return Err(failed(
            "foreground_activation_denied",
            "Windows denied foreground activation",
        ));
    }
    Ok(())
}

fn failed(code: &'static str, message: &'static str) -> ActivationError {
    ActivationError::Failed {
        code: Cow::Borrowed(code),
        message: message.to_owned(),
    }
}
