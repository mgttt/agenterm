//! Windows Control Center product bridge over the platform native text window.

use std::borrow::Cow;

use crate::platform::services::control_center_shell::{
    ControlCenterFocusRequest, ControlCenterFrame, ControlCenterShellError, ControlCenterShellHost,
    ControlCenterShellResult,
};

struct HostBridge {
    host: Box<dyn ControlCenterShellHost>,
}

impl agenterm_platform::window::NativeTextWindowHost for HostBridge {
    fn title(&self) -> String {
        self.host.title()
    }

    fn lines(&self) -> Vec<String> {
        self.host.lines()
    }

    fn poll(&mut self) -> bool {
        self.host.poll()
    }

    fn close_requested(&self) -> bool {
        self.host.close_requested()
    }

    fn publish_native_window(
        &mut self,
        raw_handle: i64,
    ) -> Result<(), agenterm_platform::window::NativeTextWindowError> {
        self.host
            .publish_native_window(raw_handle)
            .map_err(to_platform_error)
    }

    fn take_focus_request(&mut self) -> Option<agenterm_platform::window::NativeTextWindowFocus> {
        self.host.take_focus_request().map(|request| match request {
            ControlCenterFocusRequest::Activate => {
                agenterm_platform::window::NativeTextWindowFocus::Activate
            }
            ControlCenterFocusRequest::NoActivate => {
                agenterm_platform::window::NativeTextWindowFocus::NoActivate
            }
        })
    }

    fn capture_requested_screenshot(
        &mut self,
        frame: Option<agenterm_platform::window::NativeTextFrame<'_>>,
    ) -> Result<(), agenterm_platform::window::NativeTextWindowError> {
        self.host
            .capture_requested_screenshot(frame.map(|frame| ControlCenterFrame {
                pixels: frame.pixels,
                width: frame.width,
                height: frame.height,
                scale_factor: frame.scale_factor,
            }))
            .map_err(to_platform_error)
    }
}

pub(crate) fn run_native_shell(
    host: Box<dyn ControlCenterShellHost>,
    no_activate: bool,
) -> ControlCenterShellResult<()> {
    agenterm_platform::window::run_native_text_window(Box::new(HostBridge { host }), no_activate)
        .map_err(from_platform_error)
}

fn to_platform_error(
    error: ControlCenterShellError,
) -> agenterm_platform::window::NativeTextWindowError {
    match error {
        ControlCenterShellError::Unsupported { reason } => {
            agenterm_platform::window::NativeTextWindowError::Unsupported {
                reason: Cow::Borrowed(reason),
            }
        }
        ControlCenterShellError::Failed { code, message } => {
            agenterm_platform::window::NativeTextWindowError::Failed {
                code: Cow::Borrowed(code),
                message,
            }
        }
    }
}

fn from_platform_error(
    error: agenterm_platform::window::NativeTextWindowError,
) -> ControlCenterShellError {
    match error {
        agenterm_platform::window::NativeTextWindowError::Unsupported { .. } => {
            ControlCenterShellError::Unsupported {
                reason: "native-text-window-unsupported",
            }
        }
        agenterm_platform::window::NativeTextWindowError::Failed { code, message } => {
            ControlCenterShellError::Failed {
                code: match code.as_ref() {
                    "native_text_window_module_handle_failed" => {
                        "control_center_module_handle_failed"
                    }
                    "native_text_window_already_initialized" => {
                        "control_center_shell_already_initialized"
                    }
                    "native_text_window_class_register_failed" => {
                        "control_center_window_class_register_failed"
                    }
                    "native_text_window_create_failed" => "control_center_window_create_failed",
                    "native_text_window_timer_failed" => "control_center_window_timer_failed",
                    "native_text_window_message_loop_failed" => {
                        "control_center_message_loop_failed"
                    }
                    _ => "control_center_native_window_failed",
                },
                message,
            }
        }
        _ => ControlCenterShellError::Failed {
            code: "control_center_native_window_failed",
            message: "native text window returned an unknown failure".to_owned(),
        },
    }
}
