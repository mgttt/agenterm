//! Windows single-process termination adapter.

use crate::contract::process_control::{
    ProcessControlError, ProcessControlErrorKind, TerminationMode,
};

pub(crate) fn terminate(pid: u32, mode: TerminationMode) -> Result<(), ProcessControlError> {
    validate_pid(pid)?;
    if mode == TerminationMode::Graceful {
        return Err(ProcessControlError::new(
            ProcessControlErrorKind::Unsupported,
            "Windows has no generic graceful signal for an arbitrary process",
        ));
    }
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
    };
    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if process.is_null() {
        return Err(ProcessControlError::new(
            ProcessControlErrorKind::Open,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let terminated = unsafe { TerminateProcess(process, 1) };
    let error = if terminated == 0 {
        Some(std::io::Error::last_os_error())
    } else {
        None
    };
    unsafe { CloseHandle(process) };
    if let Some(error) = error {
        Err(ProcessControlError::new(
            ProcessControlErrorKind::Terminate,
            error.to_string(),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn suspend(pid: u32) -> Result<(), ProcessControlError> {
    unsupported_suspension(pid, "suspend")
}

pub(crate) fn resume(pid: u32) -> Result<(), ProcessControlError> {
    unsupported_suspension(pid, "resume")
}

fn unsupported_suspension(pid: u32, operation: &str) -> Result<(), ProcessControlError> {
    validate_pid(pid)?;
    Err(ProcessControlError::new(
        ProcessControlErrorKind::Unsupported,
        format!("Windows has no reliable generic single-process {operation} primitive"),
    ))
}

fn validate_pid(pid: u32) -> Result<(), ProcessControlError> {
    if pid == 0 {
        Err(ProcessControlError::new(
            ProcessControlErrorKind::InvalidId,
            "process ID zero does not identify one process",
        ))
    } else {
        Ok(())
    }
}
