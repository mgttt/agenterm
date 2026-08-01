//! Selected native activation facade.

pub use crate::contract::activation::{
    ActivationError, ActivationPolicy, ActivationRequest, NativeWindowHandle,
};
pub use crate::selected::activation::{EventLoopActivationExt, WindowAttributesActivationExt};

pub fn apply(
    window: NativeWindowHandle,
    request: ActivationRequest,
) -> Result<(), ActivationError> {
    crate::selected::activation::apply(window, request)
}

/// Post one adapter-defined application wake event to a native window.
pub fn post_application_wake(window: NativeWindowHandle) -> Result<(), ActivationError> {
    crate::selected::activation::post_application_wake(window)
}
