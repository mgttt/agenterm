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
