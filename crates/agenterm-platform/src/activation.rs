//! Selected native activation facade.

pub use crate::contract::activation::{
    ActivationError, ActivationPolicy, ActivationRequest, NativeWindowHandle,
};

pub fn apply(
    window: NativeWindowHandle,
    request: ActivationRequest,
) -> Result<(), ActivationError> {
    crate::selected::activation::apply(window, request)
}

#[cfg(target_os = "linux")]
pub fn configure_window_attributes(
    attributes: winit::window::WindowAttributes,
    no_activate: bool,
) -> winit::window::WindowAttributes {
    crate::selected::activation::configure_window_attributes(attributes, no_activate)
}

#[cfg(target_os = "macos")]
pub fn configure_event_loop<T>(
    builder: &mut winit::event_loop::EventLoopBuilder<T>,
    no_activate: bool,
) {
    crate::selected::activation::configure_event_loop(builder, no_activate);
}
