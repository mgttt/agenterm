//! Unix single-process signal adapter shared by Linux and macOS.

use crate::contract::process_control::{
    ProcessControlError, ProcessControlErrorKind, TerminationMode,
};

pub(crate) fn terminate(pid: u32, mode: TerminationMode) -> Result<(), ProcessControlError> {
    let signal = match mode {
        TerminationMode::Graceful => libc::SIGTERM,
        TerminationMode::Forceful => libc::SIGKILL,
    };
    send_signal(pid, signal, ProcessControlErrorKind::Terminate)
}

pub(crate) fn suspend(pid: u32) -> Result<(), ProcessControlError> {
    send_signal(pid, libc::SIGSTOP, ProcessControlErrorKind::Suspend)
}

pub(crate) fn resume(pid: u32) -> Result<(), ProcessControlError> {
    send_signal(pid, libc::SIGCONT, ProcessControlErrorKind::Resume)
}

fn send_signal(
    pid: u32,
    signal: libc::c_int,
    error_kind: ProcessControlErrorKind,
) -> Result<(), ProcessControlError> {
    if pid == 0 {
        return Err(ProcessControlError::new(
            ProcessControlErrorKind::InvalidId,
            "process ID zero targets a process group on Unix",
        ));
    }
    let pid = libc::pid_t::try_from(pid).map_err(|_| {
        ProcessControlError::new(ProcessControlErrorKind::IdOutOfRange, "pid_t overflow")
    })?;
    if unsafe { libc::kill(pid, signal) } != 0 {
        return Err(ProcessControlError::new(
            error_kind,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}
