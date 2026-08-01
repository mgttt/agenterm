//! Detached child-process launch without product executable or stdio policy.

use std::process::{Child, Command};

pub use crate::contract::process_spawn::DetachedSpawnMode;

/// Configure a child that must outlive the caller's terminal session or owned
/// process tree.
///
/// Prefer [`spawn_detached_child`] when spawning immediately: that entry point
/// also applies the Windows ambient-standard-handle guard.
pub fn configure_detached_command(command: &mut Command) -> Result<(), String> {
    crate::selected::process_spawn::configure_detached_command(command)
}

/// Spawn a detached child while retaining a handle for startup and exit
/// observation.
///
/// Executable discovery, arguments, environment, stdio, restart policy and
/// child lifetime remain caller concerns. Windows retries inside the caller's
/// job only when the host explicitly denies breakaway, and reports that
/// fallback instead of claiming independence.
pub fn spawn_detached_child(command: &mut Command) -> std::io::Result<(Child, DetachedSpawnMode)> {
    configure_detached_command(command).map_err(std::io::Error::other)?;
    match crate::selected::process_spawn::spawn(command) {
        Ok(child) => Ok((child, DetachedSpawnMode::Independent)),
        Err(error) if crate::selected::process_spawn::is_breakaway_denied(&error) => {
            crate::selected::process_spawn::configure_caller_job_fallback(command)
                .map_err(std::io::Error::other)?;
            let child = crate::selected::process_spawn::spawn(command)?;
            Ok((child, DetachedSpawnMode::CallerJobFallback))
        }
        Err(error) => Err(error),
    }
}

/// Backwards-compatible fire-and-forget launch.
///
/// Callers that need startup or exit evidence should use
/// [`spawn_detached_child`] instead of discarding the child handle.
pub fn spawn_detached_command(command: &mut Command) -> std::io::Result<DetachedSpawnMode> {
    let (child, mode) = spawn_detached_child(command)?;
    drop(child);
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE_ENV: &str = "AGENTERM_PLATFORM_DETACHED_CHILD_PROBE";

    #[test]
    fn detached_child_probe() {
        #[cfg(unix)]
        if std::env::var_os(PROBE_ENV).is_some() {
            assert_eq!(unsafe { libc::getsid(0) }, unsafe { libc::getpid() });
        }
    }

    #[test]
    fn detached_child_can_be_observed_and_reaped() {
        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        command
            .arg("--exact")
            .arg("process_spawn::tests::detached_child_probe")
            .env(PROBE_ENV, "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let (mut child, mode) = spawn_detached_child(&mut command).expect("spawn detached probe");
        assert!(matches!(
            mode,
            DetachedSpawnMode::Independent | DetachedSpawnMode::CallerJobFallback
        ));
        assert!(child.wait().expect("wait detached probe").success());
    }
}
