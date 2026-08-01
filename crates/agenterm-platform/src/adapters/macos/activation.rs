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

pub trait WindowAttributesActivationExt: Sized {
    fn with_platform_activation(self, no_activate: bool) -> Self;
}

impl WindowAttributesActivationExt for winit::window::WindowAttributes {
    fn with_platform_activation(self, no_activate: bool) -> Self {
        self.with_active(!no_activate)
    }
}

pub trait EventLoopActivationExt {
    fn configure_platform_activation(&mut self, no_activate: bool);
}

impl<T> EventLoopActivationExt for winit::event_loop::EventLoopBuilder<T> {
    fn configure_platform_activation(&mut self, no_activate: bool) {
        self.with_activate_ignoring_other_apps(!no_activate);
    }
}
