//! OS-neutral host contract for the native Control Center projection shell.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlCenterFocusRequest {
    Activate,
    NoActivate,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ControlCenterFrame<'a> {
    pub(crate) pixels: &'a [u32],
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale_factor: f64,
}

#[derive(Debug)]
pub(crate) enum ControlCenterShellError {
    #[allow(dead_code)] // Constructed by the Linux adapter on display-less hosts.
    Unsupported {
        reason: &'static str,
    },
    Failed {
        code: &'static str,
        message: String,
    },
}

impl ControlCenterShellError {
    pub(crate) fn failed(code: &'static str, error: impl fmt::Display) -> Self {
        Self::Failed {
            code,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for ControlCenterShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => {
                write!(formatter, "control center shell unsupported: {reason}")
            }
            Self::Failed { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for ControlCenterShellError {}

pub(crate) type ControlCenterShellResult<T> = Result<T, ControlCenterShellError>;

pub(crate) trait ControlCenterShellHost: Send {
    fn title(&self) -> String;
    fn lines(&self) -> Vec<String>;
    fn poll(&mut self) -> bool;
    fn close_requested(&self) -> bool;
    fn publish_native_window(&mut self, raw_handle: i64) -> ControlCenterShellResult<()>;
    fn take_focus_request(&mut self) -> Option<ControlCenterFocusRequest>;
    fn capture_requested_screenshot(
        &mut self,
        frame: Option<ControlCenterFrame<'_>>,
    ) -> ControlCenterShellResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_shell_failures_distinguish_unsupported_and_failed() {
        let unsupported = ControlCenterShellError::Unsupported { reason: "headless" };
        let failed = ControlCenterShellError::failed("window_create_failed", "no display");

        assert!(unsupported.to_string().contains("unsupported"));
        assert_eq!(failed.to_string(), "window_create_failed: no display");
    }
}
