use std::borrow::Cow;

use crate::contract::activation::{ActivationError, ActivationRequest, NativeWindowHandle};

pub(crate) fn apply(
    _window: NativeWindowHandle,
    _request: ActivationRequest,
) -> Result<(), ActivationError> {
    Err(ActivationError::Unsupported {
        reason: Cow::Borrowed("native-window-activation-not-supported"),
    })
}

pub(crate) fn configure_window_attributes(
    attributes: winit::window::WindowAttributes,
    no_activate: bool,
) -> winit::window::WindowAttributes {
    attributes.with_active(!no_activate)
}
