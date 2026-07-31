//! Native Control Center shell facade.

pub(crate) use crate::platform::contract::control_center_shell::{
    ControlCenterFocusRequest, ControlCenterFrame, ControlCenterShellError, ControlCenterShellHost,
    ControlCenterShellResult,
};

pub(crate) fn run_native_shell(
    host: Box<dyn ControlCenterShellHost>,
    no_activate: bool,
) -> ControlCenterShellResult<()> {
    crate::platform::selected::control_center_shell::run_native_shell(host, no_activate)
}
