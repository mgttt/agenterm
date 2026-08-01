use std::borrow::Cow;

use winit::platform::macos::EventLoopBuilderExtMacOS;

use crate::contract::activation::{ActivationError, ActivationRequest, NativeWindowHandle};

pub(crate) fn apply(
    _window: NativeWindowHandle,
    _request: ActivationRequest,
) -> Result<(), ActivationError> {
    Err(ActivationError::Unsupported {
        reason: Cow::Borrowed("native-window-activation-not-supported"),
    })
}

pub(crate) fn configure_event_loop<T>(
    builder: &mut winit::event_loop::EventLoopBuilder<T>,
    no_activate: bool,
) {
    builder.with_activate_ignoring_other_apps(!no_activate);
}
