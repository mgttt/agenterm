//! Windows single-process termination adapter.

use std::os::windows::io::{AsHandle as _, AsRawHandle as _, BorrowedHandle, FromRawHandle as _};

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
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE};
    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if process.is_null() {
        return Err(ProcessControlError::new(
            ProcessControlErrorKind::Open,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let process = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(process) };
    crate::process_control::terminate_handle(process.as_handle(), 1).map_err(|error| {
        ProcessControlError::new(ProcessControlErrorKind::Terminate, error.to_string())
    })
}

impl crate::process_control::ProcessTerminationHandle for BorrowedHandle<'_> {
    fn terminate_process(self, exit_code: u32) -> std::io::Result<()> {
        if unsafe {
            windows_sys::Win32::System::Threading::TerminateProcess(self.as_raw_handle(), exit_code)
        } == 0
        {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
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
