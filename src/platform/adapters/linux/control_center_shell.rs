//! Linux identity and activation policy for the native Control Center shell.

#[path = "../unix/control_center_shell.rs"]
mod unix;

use crate::platform::services::control_center_shell::{
    ControlCenterShellError, ControlCenterShellHost, ControlCenterShellResult,
};

pub(crate) fn run_native_shell(
    host: Box<dyn ControlCenterShellHost>,
    no_activate: bool,
) -> ControlCenterShellResult<()> {
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(ControlCenterShellError::Unsupported {
            reason: "no X11 or Wayland display is available",
        });
    }
    unix::run_native_shell(
        host,
        no_activate,
        "linux",
        |attributes, no_activate| {
            crate::platform::linux::activation::configure_window_attributes(attributes, no_activate)
        },
        |_builder, _no_activate| {},
    )
}
