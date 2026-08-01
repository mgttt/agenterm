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
    use agenterm_platform::activation::WindowAttributesActivationExt as _;
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(ControlCenterShellError::Unsupported {
            reason: "no X11 or Wayland display is available",
        });
    }
    unix::run_native_shell(
        host,
        no_activate,
        "linux",
        |attributes, no_activate| attributes.with_platform_activation(no_activate),
        |_builder, _no_activate| {},
    )
}
