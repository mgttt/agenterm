//! macOS identity and activation policy for the native Control Center shell.

#[path = "../unix/control_center_shell.rs"]
mod unix;

use crate::platform::services::control_center_shell::{
    ControlCenterShellHost, ControlCenterShellResult,
};

pub(crate) fn run_native_shell(
    host: Box<dyn ControlCenterShellHost>,
    no_activate: bool,
) -> ControlCenterShellResult<()> {
    unix::run_native_shell(
        host,
        no_activate,
        "macos",
        |attributes, no_activate| attributes.with_active(!no_activate),
        |builder, no_activate| {
            crate::platform::macos::activation::configure_event_loop(builder, no_activate);
        },
    )
}
