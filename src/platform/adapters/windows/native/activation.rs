//! AgenTerm compatibility projection over `agenterm-platform` activation.

pub(crate) use agenterm_platform::activation::ActivationError;
use agenterm_platform::activation::{ActivationRequest, NativeWindowHandle};

use crate::platform::CapabilityStatus;

pub(crate) fn to_capability_status(error: ActivationError) -> CapabilityStatus {
    match error {
        ActivationError::Unsupported { .. } => CapabilityStatus::Unsupported {
            reason: "native-window-activation-not-supported",
        },
        ActivationError::Failed { code, message } => CapabilityStatus::Failed {
            code: match code.as_ref() {
                "show_without_activation_failed" => "show_without_activation_failed",
                "foreground_activation_denied" => "foreground_activation_denied",
                _ => "activation_failed",
            },
            message,
        },
        other => CapabilityStatus::Failed {
            code: "activation_failed",
            message: other.to_string(),
        },
    }
}

fn native_window(raw_window: isize) -> NativeWindowHandle {
    // SAFETY: every caller passes its live AgenTerm top-level HWND and invokes
    // the operation synchronously while that window is owned by the caller.
    unsafe { NativeWindowHandle::from_raw(raw_window) }.expect("top-level window handle is nonzero")
}

pub(crate) fn show_without_activation(raw_window: isize) -> Result<(), ActivationError> {
    agenterm_platform::activation::apply(
        native_window(raw_window),
        ActivationRequest::ShowWithoutActivation,
    )
}

pub(crate) fn show_new_and_request_activation(raw_window: isize) {
    let _ = agenterm_platform::activation::apply(
        native_window(raw_window),
        ActivationRequest::ShowNewAndRequestActivation,
    );
}

pub(crate) fn restore_and_activate(raw_window: isize) -> Result<(), ActivationError> {
    agenterm_platform::activation::apply(
        native_window(raw_window),
        ActivationRequest::RestoreAndActivate,
    )
}
