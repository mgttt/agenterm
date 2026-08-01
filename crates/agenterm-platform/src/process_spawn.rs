//! Detached child-process launch without product executable or stdio policy.

use std::process::{Child, Command, ExitStatus};

pub use crate::contract::process_spawn::{DetachedSpawnMode, ProcessExit};

/// Process-wide coordination for temporary `HANDLE_FLAG_INHERIT` mutation.
///
/// Windows handle inheritance flags belong to the process, not one
/// `CreateProcessW` call. Raw-process launchers that temporarily change those
/// flags must hold this guard through process creation so they cannot race with
/// [`spawn_detached_child`] or another cooperating launcher. The guard exposes
/// no native handle and is a plain serialization scope on non-Windows hosts.
/// Do not call a platform spawn entry point while already holding it.
#[must_use = "the lock guard must be held through handle mutation and process creation"]
pub struct HandleInheritanceLock {
    _guard: std::sync::MutexGuard<'static, ()>,
}

/// Enter the shared handle-inheritance mutation window.
pub fn lock_handle_inheritance() -> HandleInheritanceLock {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    HandleInheritanceLock {
        _guard: LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    }
}

/// Classify a native child status without inventing a product fallback code.
#[must_use]
pub fn classify_exit_status(status: &ExitStatus) -> ProcessExit {
    crate::selected::process_spawn::classify_exit_status(status)
}

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

/// Backwards-compatible fire-and-reap launch.
///
/// Callers that need startup or exit evidence should use
/// [`spawn_detached_child`]. This compatibility entry point retains no handle,
/// but a detached waiter always reaps the child instead of leaving a Unix
/// zombie while the parent remains alive.
pub fn spawn_detached_command(command: &mut Command) -> std::io::Result<DetachedSpawnMode> {
    let (mut child, mode) = spawn_detached_child(command)?;
    std::thread::Builder::new()
        .name("agenterm-platform-child-reaper".to_owned())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE_ENV: &str = "AGENTERM_PLATFORM_DETACHED_CHILD_PROBE";
    const PROBE_PID_FILE_ENV: &str = "AGENTERM_PLATFORM_DETACHED_CHILD_PID_FILE";

    #[test]
    fn handle_inheritance_lock_serializes_cooperating_threads() {
        let first = lock_handle_inheritance();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let contender = std::thread::spawn(move || {
            started_tx.send(()).expect("publish contender start");
            let _second = lock_handle_inheritance();
            acquired_tx.send(()).expect("publish lock acquisition");
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("contender did not start");
        assert!(
            acquired_rx
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err(),
            "a second inheritance mutation entered the guarded window"
        );

        drop(first);
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("contender did not acquire the released lock");
        contender
            .join()
            .expect("inheritance-lock contender panicked");
    }

    #[test]
    fn detached_child_probe() {
        #[cfg(unix)]
        if std::env::var_os(PROBE_ENV).is_some() {
            assert_eq!(unsafe { libc::getsid(0) }, unsafe { libc::getpid() });
        }
        if let Some(path) = std::env::var_os(PROBE_PID_FILE_ENV) {
            std::fs::write(path, std::process::id().to_string()).expect("write child PID probe");
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

    #[test]
    fn exit_status_classification_preserves_native_outcome() {
        #[cfg(windows)]
        let status = Command::new("cmd.exe")
            .args(["/d", "/c", "exit", "37"])
            .status()
            .expect("run exit-code probe");
        #[cfg(unix)]
        let status = Command::new("sh")
            .args(["-c", "exit 37"])
            .status()
            .expect("run exit-code probe");
        assert_eq!(classify_exit_status(&status), ProcessExit::Code(37));

        #[cfg(unix)]
        {
            let status = Command::new("sh")
                .args(["-c", "kill -TERM $$"])
                .status()
                .expect("run signal probe");
            assert_eq!(classify_exit_status(&status), ProcessExit::Signal(15));
        }
    }

    #[test]
    fn compatibility_launch_reaps_the_child() {
        let pid_file =
            std::env::temp_dir().join(format!("agenterm-platform-reaper-{}", std::process::id()));
        let _ = std::fs::remove_file(&pid_file);
        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        command
            .arg("--exact")
            .arg("process_spawn::tests::detached_child_probe")
            .env(PROBE_ENV, "1")
            .env(PROBE_PID_FILE_ENV, &pid_file)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        spawn_detached_command(&mut command).expect("spawn compatibility probe");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let _pid = loop {
            if let Ok(value) = std::fs::read_to_string(&pid_file) {
                break value.parse::<u32>().expect("parse child PID probe");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "detached child did not publish its PID"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };

        #[cfg(unix)]
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        #[cfg(unix)]
        loop {
            let present = unsafe { libc::kill(_pid as libc::pid_t, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH);
            if !present {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "detached child remained present after its exit"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(pid_file);
    }
}
