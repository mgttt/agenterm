//! Stable native control-window facade.

pub use crate::contract::control_window::*;

pub fn run_control_window(
    options: ControlWindowOptions,
    application: Box<dyn ControlWindowApplication>,
) -> Result<(), ControlWindowError> {
    if options.initial_size.width == 0 || options.initial_size.height == 0 {
        return Err(ControlWindowError::failed(
            "control_window_invalid_initial_size",
            "initial size must be non-zero",
        ));
    }
    crate::selected::control_window::run_control_window(options, application)
}
