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
    ProcessTreeGuard, configure_breakaway_visible_command, configure_command,
    configure_owned_command, kill, list, observe, start_identity,
};

pub(crate) fn autostart_server(
    parameter_name: &str,
    parameter_value: &str,
) -> std::io::Result<bool> {
    autostart_server_impl(parameter_name, parameter_value)
}

fn autostart_server_impl(parameter_name: &str, parameter_value: &str) -> std::io::Result<bool> {
    if !matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    ) {
        return Ok(false);
    }
    // Authority is `agenterm server` in a separate process of the same PE.
    // Replacing `agenterm.exe` on disk while that process lives may fail on
    // Windows until the authority is stopped (accepted product trade-off).
    let current = std::env::current_exe()?;
    let mut command = Command::new(current);
    command
        .arg("server")
        .arg(parameter_name)
        .arg(parameter_value)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let spawn_mode = agenterm_platform::process::spawn_detached_command(&mut command)?;
    if spawn_mode == agenterm_platform::process::DetachedSpawnMode::CallerJobFallback {
        let _ = agenterm_platform::process::write_parent_console_stderr(
            "AgenTerm server started inside the caller's process job because Windows denied job breakaway; it may stop when that owning job closes.",
        );
    }
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
