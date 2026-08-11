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

/// A target-specific set of native objects that can participate in one
/// coordinated process-inheritance transaction.
///
/// Windows adapters implement this for borrowed HANDLE slices. Other hosts do
/// not pretend that descriptor inheritance has the same mutation semantics.
pub trait HandleInheritanceSet {
    #[doc(hidden)]
    fn with_inheritable_handles<T>(self, operation: impl FnOnce() -> T) -> std::io::Result<T>;
}

/// Runs one process-creation operation while the selected native objects are
/// temporarily inheritable.
///
/// The adapter holds the same process-wide coordination lock used by platform
/// spawn entry points and restores every source object before returning. The
/// generic boundary keeps target-native handle types out of this public API.
pub fn with_inheritable_handles<T>(
    handles: impl HandleInheritanceSet,
    operation: impl FnOnce() -> T,
) -> std::io::Result<T> {
    handles.with_inheritable_handles(operation)
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
///
/// On Windows this sets `command`'s creation flags outright rather than
/// merging with any the caller already set: `CommandExt::creation_flags`
/// replaces the prior value on each call (verified empirically — it does not
/// OR bits across calls), so any flags set on `command` before this call are
/// discarded. Give this function (or [`spawn_detached_child`]) an otherwise
/// unconfigured `Command`.
pub fn configure_detached_command(command: &mut Command) -> Result<(), String> {
    crate::selected::process_spawn::configure_detached_command(command)
}

/// Configure an owned console child so Windows does not allocate a new console
/// window when the parent is a GUI-subsystem process.
///
/// Unlike [`configure_detached_command`], this preserves the caller's Job and
/// lifetime ownership. Standard handles remain inherited according to the
/// caller's `Command` configuration.
pub fn configure_owned_headless_command(command: &mut Command) -> Result<(), String> {
    crate::selected::process_spawn::configure_owned_headless_command(command)
}

/// Configure a child that may outlive the caller Job/session while remaining a
/// normal visible GUI process (no `CREATE_NO_WINDOW` / equivalent).
pub fn configure_breakaway_visible_command(command: &mut Command) -> Result<(), String> {
    crate::selected::process_spawn::configure_breakaway_visible_command(command)
}

/// Whether a spawn failure means the host denied Job breakaway (Windows
/// `ERROR_ACCESS_DENIED` on breakaway flags). Other hosts always return false.
///
/// Prefer [`spawn_breakaway_visible_child`] over matching this in product code.
#[must_use]
pub fn is_breakaway_denied(error: &std::io::Error) -> bool {
    crate::selected::process_spawn::is_breakaway_denied(error)
}

/// Spawn a detached child while retaining a handle for startup and exit
/// observation.
///
/// Executable discovery, arguments, environment, stdio, restart policy and
/// child lifetime remain caller concerns. Windows retries inside the caller's
/// job only when the host explicitly denies breakaway, and reports that
/// fallback instead of claiming independence.
///
/// On Windows the fallback uses `CREATE_NO_WINDOW` (headless/server style).
/// For a **visible GUI** sibling that must survive Job denial, use
/// [`spawn_breakaway_visible_child`] instead.
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

/// Spawn a **visible** GUI sibling that prefers Job/session breakaway.
///
/// Unlike [`spawn_detached_child`], the Windows breakaway-denied fallback does
/// **not** set `CREATE_NO_WINDOW` — the child keeps a normal desktop window
/// (Control Center, extra agenterm GUI, …). Host denial classification stays
/// inside this crate; embedding apps must not hard-code `ERROR_ACCESS_DENIED`.
pub fn spawn_breakaway_visible_child(
    command: &mut Command,
) -> std::io::Result<(Child, DetachedSpawnMode)> {
    configure_breakaway_visible_command(command).map_err(std::io::Error::other)?;
    match crate::selected::process_spawn::spawn(command) {
        Ok(child) => Ok((child, DetachedSpawnMode::Independent)),
        Err(error) if crate::selected::process_spawn::is_breakaway_denied(&error) => {
            crate::selected::process_spawn::configure_visible_in_caller_job(command)
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
    crate::threading::spawn_named(
        "agenterm-platform-child-reaper",
        Box::new(move || {
            let _ = child.wait();
        }),
    )?;
    Ok(mode)
}

/// Fire-and-reap visible breakaway launch (see [`spawn_breakaway_visible_child`]).
pub fn spawn_breakaway_visible_command(
    command: &mut Command,
) -> std::io::Result<DetachedSpawnMode> {
    let (mut child, mode) = spawn_breakaway_visible_child(command)?;
    crate::threading::spawn_named(
        "agenterm-platform-visible-child-reaper",
        Box::new(move || {
            let _ = child.wait();
        }),
    )?;
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE_ENV: &str = "AGENTERM_PLATFORM_DETACHED_CHILD_PROBE";
    const VISIBLE_PROBE_ENV: &str = "AGENTERM_PLATFORM_VISIBLE_BREAKAWAY_CHILD_PROBE";
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

    #[cfg(windows)]
    #[test]
    fn public_inheritance_transaction_accepts_borrowed_handle_sets() {
        use std::os::windows::io::AsHandle as _;

        let path = std::env::temp_dir().join(format!(
            "agenterm-platform-public-inheritance-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let file = std::fs::File::create(&path).expect("create inheritance fixture");
        let handles = [file.as_handle()];
        let observed = with_inheritable_handles(handles.as_slice(), || 37)
            .expect("public inheritance transaction");
        assert_eq!(observed, 37);
        drop(file);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn detached_child_probe() {
        #[cfg(unix)]
        if std::env::var_os(PROBE_ENV).is_some() {
            assert_eq!(unsafe { libc::getsid(0) }, unsafe { libc::getpid() });
        }
        // A visible breakaway child owns a fresh process GROUP (`setpgid`),
        // not a new session — the unix implementation is deliberately
        // group-only ("Visible GUI siblings only need a new process group").
        // Reusing the detached probe's session-leader assertion here is what
        // kept the ubuntu/macos contract lanes red.
        #[cfg(unix)]
        if std::env::var_os(VISIBLE_PROBE_ENV).is_some() {
            assert_eq!(unsafe { libc::getpgrp() }, unsafe { libc::getpid() });
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
    fn is_breakaway_denied_is_windows_access_denied_only() {
        let access_denied = std::io::Error::from_raw_os_error(5);
        let not_found = std::io::Error::from_raw_os_error(2);
        #[cfg(windows)]
        {
            assert!(is_breakaway_denied(&access_denied));
            assert!(!is_breakaway_denied(&not_found));
        }
        #[cfg(not(windows))]
        {
            assert!(!is_breakaway_denied(&access_denied));
            assert!(!is_breakaway_denied(&not_found));
        }
    }

    #[test]
    fn breakaway_visible_child_can_be_observed_and_reaped() {
        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        command
            .arg("--exact")
            .arg("process_spawn::tests::detached_child_probe")
            .env(VISIBLE_PROBE_ENV, "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let (mut child, mode) =
            spawn_breakaway_visible_child(&mut command).expect("spawn visible breakaway probe");
        assert!(matches!(
            mode,
            DetachedSpawnMode::Independent | DetachedSpawnMode::CallerJobFallback
        ));
        assert!(
            child
                .wait()
                .expect("wait visible breakaway probe")
                .success()
        );
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
            if let Ok(value) = std::fs::read_to_string(&pid_file)
                && let Ok(pid) = value.trim().parse::<u32>()
            {
                break pid;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "detached child did not publish a complete PID"
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
