//! Lightweight single-process control without inventory, jobs, or pipe APIs.

pub use crate::contract::process_control::{
    ProcessControlError, ProcessControlErrorKind, TerminationMode,
};

/// Request termination of one host process.
///
/// This does not discover descendants or imply process-tree ownership.
pub fn terminate(pid: u32, mode: TerminationMode) -> Result<(), ProcessControlError> {
    crate::selected::process_control::terminate(pid, mode)
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

    #[cfg(unix)]
    #[test]
    fn graceful_termination_stops_a_real_process() {
        let mut child = spawn_sleeper();
        terminate(child.id(), TerminationMode::Graceful).expect("signal fixture");
        wait_for_exit(&mut child);
    }

    #[cfg(windows)]
    #[test]
    fn graceful_termination_is_explicitly_unsupported() {
        let error = terminate(u32::MAX, TerminationMode::Graceful)
            .expect_err("Windows has no generic graceful process signal");
        assert_eq!(error.kind(), ProcessControlErrorKind::Unsupported);
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
    }
}
