//! Unix single-process signal adapter shared by Linux and macOS.

use crate::contract::process_control::{
    ProcessControlError, ProcessControlErrorKind, TerminationMode,
};

pub(crate) fn terminate(pid: u32, mode: TerminationMode) -> Result<(), ProcessControlError> {
    if pid == 0 {
        return Err(ProcessControlError::new(
            ProcessControlErrorKind::InvalidId,
            "process ID zero targets a process group on Unix",
        ));
    }
    let pid = libc::pid_t::try_from(pid).map_err(|_| {
        ProcessControlError::new(ProcessControlErrorKind::IdOutOfRange, "pid_t overflow")
    })?;
    let signal = match mode {
        TerminationMode::Graceful => libc::SIGTERM,
        TerminationMode::Forceful => libc::SIGKILL,
    };
    if unsafe { libc::kill(pid, signal) } != 0 {
        return Err(ProcessControlError::new(
            ProcessControlErrorKind::Terminate,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}
