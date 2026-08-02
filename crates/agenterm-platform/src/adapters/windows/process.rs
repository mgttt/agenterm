//! Windows implementation of the process facade contract.

use std::process::{Child, ChildStderr, ChildStdout, Command};

use crate::contract::process::{PipeProbeError, PipeProbeToken};
use crate::contract::process::{ProcessError, ProcessErrorKind, ProcessInfo};

pub(crate) fn write_parent_console_stderr(message: &str) -> bool {
    use std::{fs::OpenOptions, io::Write as _};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{
            ATTACH_PARENT_PROCESS, AttachConsole, FreeConsole, GetStdHandle, STD_ERROR_HANDLE,
        },
    };

    let payload = format!("{message}\n");
    let stderr_handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if !stderr_handle.is_null() && stderr_handle != INVALID_HANDLE_VALUE {
        let mut stderr = std::io::stderr().lock();
        if stderr.write_all(payload.as_bytes()).is_ok() && stderr.flush().is_ok() {
            return true;
        }
    }
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return false;
    }
    let written = OpenOptions::new()
        .write(true)
        .open("CONOUT$")
        .is_ok_and(|mut console| {
            console.write_all(payload.as_bytes()).is_ok() && console.flush().is_ok()
        });
    unsafe { FreeConsole() };
    written
}

pub(crate) fn stdout_probe_token(reader: &ChildStdout) -> Option<PipeProbeToken> {
    use std::os::windows::io::AsRawHandle as _;
    Some(PipeProbeToken(reader.as_raw_handle() as usize))
}

pub(crate) fn stderr_probe_token(reader: &ChildStderr) -> Option<PipeProbeToken> {
    use std::os::windows::io::AsRawHandle as _;
    Some(PipeProbeToken(reader.as_raw_handle() as usize))
}

pub(crate) fn pipe_available(token: PipeProbeToken) -> Result<usize, PipeProbeError> {
    use windows_sys::Win32::{
        Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, GetLastError},
        System::Pipes::PeekNamedPipe,
    };
    let mut available = 0_u32;
    if unsafe {
        PeekNamedPipe(
            token.0 as windows_sys::Win32::Foundation::HANDLE,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Ok(available as usize);
    }
    let error = unsafe { GetLastError() };
    if error == ERROR_BROKEN_PIPE || error == ERROR_NO_DATA {
        Err(PipeProbeError::Closed)
    } else {
        Err(PipeProbeError::Failed)
    }
}

pub(crate) fn list() -> Result<Vec<ProcessInfo>, ProcessError> {
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(ProcessError::new(
            ProcessErrorKind::Inventory,
            "snapshot failed",
        ));
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut processes = Vec::new();
    let mut present = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while present {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let executable_name = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if !executable_name.is_empty() {
            processes.push(ProcessInfo {
                id: entry.th32ProcessID,
                parent_id: entry.th32ParentProcessID,
                executable_name,
            });
        }
        present = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    Ok(processes)
}

pub struct ProcessTreeGuard {
    containment: crate::process_containment::ProcessContainment,
    active: bool,
}

pub(crate) fn configure_owned_command(_command: &mut Command) -> Result<(), String> {
    Ok(())
}

impl ProcessTreeGuard {
    pub fn attach(child: &Child) -> Result<Self, String> {
        use crate::{
            process_containment::{ProcessContainment, ProcessContainmentOptions},
            process_reference::ProcessReference,
        };
        use std::os::windows::io::AsHandle as _;
        let containment = ProcessContainment::create(
            None,
            ProcessContainmentOptions {
                terminate_on_last_close: true,
                allow_breakaway: true,
                ..ProcessContainmentOptions::default()
            },
        )
        .map_err(|error| error.to_string())?;
        let process = ProcessReference::duplicate_from(child.as_handle())
            .map_err(|error| format!("retain child process failed: {error}"))?;
        containment
            .assign(&process)
            .map_err(|error| error.to_string())?;
        let guard = Self {
            containment,
            active: true,
        };
        Ok(guard)
    }

    pub fn terminate(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        self.containment
            .terminate(1)
            .map_err(|error| error.to_string())?;
        self.active = false;
        Ok(())
    }
}
