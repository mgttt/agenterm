//! Lightweight single-process control without inventory, jobs, or pipe APIs.

pub use crate::contract::process_control::{
    ProcessControlError, ProcessControlErrorKind, TerminationMode,
};

/// A target-native reference that can terminate one exact process object.
///
/// This extension is intentionally narrower than [`terminate`]: it does not
/// reopen a process by ID, discover descendants, or imply process-tree
/// ownership. The Windows adapter implements it for
/// [`std::os::windows::io::BorrowedHandle`].
pub trait ProcessTerminationHandle {
    #[doc(hidden)]
    fn terminate_process(self, exit_code: u32) -> std::io::Result<()>;
}

/// Forcefully terminates the exact process object represented by `process`.
///
/// The caller owns the product policy for `exit_code`. This operation does not
/// terminate descendants; use an owning container primitive when process-tree
/// semantics are required.
pub fn terminate_handle(
    process: impl ProcessTerminationHandle,
    exit_code: u32,
) -> std::io::Result<()> {
    process.terminate_process(exit_code)
}

/// Request termination of one host process.
///
/// This does not discover descendants or imply process-tree ownership.
pub fn terminate(pid: u32, mode: TerminationMode) -> Result<(), ProcessControlError> {
    crate::selected::process_control::terminate(pid, mode)
}

/// Suspend scheduling of one host process until it is resumed externally.
///
/// This does not discover or suspend descendants and does not imply process-tree
/// ownership. Windows has no reliable generic single-process suspension primitive,
/// so the Windows adapter reports [`ProcessControlErrorKind::Unsupported`].
pub fn suspend(pid: u32) -> Result<(), ProcessControlError> {
    crate::selected::process_control::suspend(pid)
}

/// Resume scheduling of one suspended host process.
///
/// This does not discover or resume descendants and does not imply process-tree
/// ownership. Windows has no reliable generic single-process resumption primitive,
/// so the Windows adapter reports [`ProcessControlErrorKind::Unsupported`].
pub fn resume(pid: u32) -> Result<(), ProcessControlError> {
    crate::selected::process_control::resume(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_sleeper() -> std::process::Child {
        #[cfg(windows)]
        let mut command = std::process::Command::new("cmd");
        #[cfg(windows)]
        command.args(["/c", "ping -n 30 127.0.0.1 >nul"]);
        #[cfg(unix)]
        let mut command = std::process::Command::new("/bin/sleep");
        #[cfg(unix)]
        command.arg("30");
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn process-control fixture")
    }

    fn wait_for_exit(child: &mut std::process::Child) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while child.try_wait().expect("query fixture").is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "terminated process remained alive"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn forceful_termination_stops_a_real_process() {
        let mut child = spawn_sleeper();
        terminate(child.id(), TerminationMode::Forceful).expect("terminate fixture");
        wait_for_exit(&mut child);
    }

    #[cfg(windows)]
    #[test]
    fn handle_termination_targets_the_open_process_and_preserves_exit_code() {
        use std::os::windows::io::AsHandle as _;

        let mut child = spawn_sleeper();
        terminate_handle(child.as_handle(), 37).expect("terminate exact process handle");
        let status = child.wait().expect("wait for exact process handle");
        assert_eq!(status.code(), Some(37));
    }

    #[cfg(unix)]
    #[test]
    fn graceful_termination_stops_a_real_process() {
        let mut child = spawn_sleeper();
        terminate(child.id(), TerminationMode::Graceful).expect("signal fixture");
        wait_for_exit(&mut child);
    }

    #[cfg(unix)]
    #[test]
    fn suspend_and_resume_control_a_real_process() {
        let mut child = spawn_sleeper();
        suspend(child.id()).expect("suspend fixture");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let mut status = 0;
            let waited = unsafe {
                libc::waitpid(
                    child.id() as libc::pid_t,
                    &mut status,
                    libc::WNOHANG | libc::WUNTRACED,
                )
            };
            assert!(waited >= 0, "query suspended fixture failed");
            if waited > 0 && libc::WIFSTOPPED(status) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "suspended process never entered a stopped state"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        resume(child.id()).expect("resume fixture");
        terminate(child.id(), TerminationMode::Forceful).expect("terminate resumed fixture");
        wait_for_exit(&mut child);
    }

    #[cfg(windows)]
    #[test]
    fn graceful_termination_is_explicitly_unsupported() {
        let error = terminate(u32::MAX, TerminationMode::Graceful)
            .expect_err("Windows has no generic graceful process signal");
        assert_eq!(error.kind(), ProcessControlErrorKind::Unsupported);
    }

    #[cfg(windows)]
    #[test]
    fn suspension_and_resumption_are_explicitly_unsupported() {
        let suspend_error = suspend(1).expect_err("Windows suspension must be explicit");
        assert_eq!(suspend_error.kind(), ProcessControlErrorKind::Unsupported);
        let resume_error = resume(1).expect_err("Windows resumption must be explicit");
        assert_eq!(resume_error.kind(), ProcessControlErrorKind::Unsupported);
    }

    #[test]
    fn missing_process_is_not_reported_as_terminated() {
        assert!(terminate(u32::MAX, TerminationMode::Forceful).is_err());
    }

    #[test]
    fn zero_never_targets_the_unix_process_group() {
        let error = terminate(0, TerminationMode::Forceful)
            .expect_err("zero must not be interpreted as one process");
        assert_eq!(error.kind(), ProcessControlErrorKind::InvalidId);
        assert_eq!(
            suspend(0)
                .expect_err("zero must not suspend a process group")
                .kind(),
            ProcessControlErrorKind::InvalidId
        );
        assert_eq!(
            resume(0)
                .expect_err("zero must not resume a process group")
                .kind(),
            ProcessControlErrorKind::InvalidId
        );
    }
}
