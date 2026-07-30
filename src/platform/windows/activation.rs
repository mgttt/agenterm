//! Win32 activation and show-without-activation capability.

#![cfg(target_os = "windows")]

use std::{fmt, ptr};

use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GetForegroundWindow, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
    },
};

use crate::platform::CapabilityStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationError {
    ShowWithoutActivationFailed,
    ForegroundDenied,
}

impl fmt::Display for ActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ShowWithoutActivationFailed => {
                "Windows could not show the window without activation"
            }
            Self::ForegroundDenied => "Windows denied foreground activation",
        })
    }
}

impl std::error::Error for ActivationError {}

impl ActivationError {
    pub(crate) fn to_capability_status(self) -> CapabilityStatus {
        let code = match self {
            Self::ShowWithoutActivationFailed => "show_without_activation_failed",
            Self::ForegroundDenied => "foreground_activation_denied",
        };
        CapabilityStatus::Failed {
            code,
            message: self.to_string(),
        }
    }
}

pub(crate) fn show_without_activation(window: HWND) -> Result<(), ActivationError> {
    let foreground = unsafe { GetForegroundWindow() };
    let (insert_after, flags) = if !foreground.is_null() && foreground != window {
        (
            foreground,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    } else {
        (
            ptr::null_mut(),
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
    if unsafe { SetWindowPos(window, insert_after, 0, 0, 0, 0, flags) } == 0 {
        return Err(ActivationError::ShowWithoutActivationFailed);
    }
    unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
    Ok(())
}

fn show_and_activate(window: HWND, command: i32) -> Result<(), ActivationError> {
    unsafe { ShowWindow(window, command) };
    if unsafe { SetForegroundWindow(window) } == 0 {
        return Err(ActivationError::ForegroundDenied);
    }
    Ok(())
}

/// New-window activation is intentionally best-effort: Windows foreground
/// policy may deny the request, but that must not turn a visible GUI launch
/// into process failure.
pub(crate) fn show_new_and_request_activation(window: HWND) {
    unsafe {
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
    }
}

pub(crate) fn restore_and_activate(window: HWND) -> Result<(), ActivationError> {
    show_and_activate(window, SW_RESTORE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_errors_have_stable_diagnostics() {
        assert_eq!(
            ActivationError::ShowWithoutActivationFailed.to_string(),
            "Windows could not show the window without activation"
        );
        assert_eq!(
            ActivationError::ForegroundDenied.to_string(),
            "Windows denied foreground activation"
        );
        assert_eq!(
            ActivationError::ForegroundDenied.to_capability_status(),
            CapabilityStatus::Failed {
                code: "foreground_activation_denied",
                message: "Windows denied foreground activation".to_string(),
            }
        );
    }
}
