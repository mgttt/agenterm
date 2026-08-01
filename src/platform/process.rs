//! Product-facing process facade compatibility surface.
//!
//! Native process mechanics come from the external `agenterm-platform` crate.
//! AgenTerm-specific sibling-server discovery remains product policy here.

use std::process::{Command, Stdio};

#[allow(unused_imports)] // Compatibility exports consumed by product modules per target.
pub(crate) use agenterm_platform::contract::process::{
    ProcessError, ProcessErrorKind, ProcessInfo, ProcessObservation,
};
#[allow(unused_imports)] // Compatibility exports consumed by product modules per target.
pub(crate) use agenterm_platform::process::{
    ProcessTreeGuard, configure_command, configure_owned_command, kill, list, observe,
    start_identity,
};

pub(crate) fn autostart_server(
    parameter_name: &str,
    parameter_value: &str,
) -> std::io::Result<bool> {
    if agenterm_platform::platform_kind() != agenterm_platform::PlatformKind::Windows {
        return Ok(false);
    }
    let current = std::env::current_exe()?;
    let server = current.with_file_name("agenterm-server.exe");
    if !server.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "AgenTerm server executable was not found beside the current client: {}",
                server.display()
            ),
        ));
    }
    let mut command = Command::new(server);
    command
        .arg(parameter_name)
        .arg(parameter_value)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    agenterm_platform::process::configure_detached_command(&mut command)
        .map_err(std::io::Error::other)?;
    command.spawn()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_has_stable_observation_and_inventory_entry() {
        assert!(matches!(
            observe(std::process::id()),
            ProcessObservation::Live { .. }
        ));
        assert!(
            list()
                .expect("process inventory")
                .iter()
                .any(|entry| entry.id == std::process::id())
        );
    }
}
