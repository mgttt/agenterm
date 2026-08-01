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
    handle: windows_sys::Win32::Foundation::HANDLE,
    active: bool,
}

unsafe impl Send for ProcessTreeGuard {}

pub(crate) fn configure_owned_command(_command: &mut Command) -> Result<(), String> {
    Ok(())
}

impl ProcessTreeGuard {
    pub fn attach(child: &Child) -> Result<Self, String> {
        use std::{ffi::c_void, mem, os::windows::io::AsRawHandle, ptr};
        use windows_sys::Win32::{
            Foundation::HANDLE,
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectExtendedLimitInformation, SetInformationJobObject,
            },
        };
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(format!(
                "CreateJobObjectW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let guard = Self {
            handle,
            active: true,
        };
        let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        information.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        if unsafe {
            SetInformationJobObject(
                guard.handle,
                JobObjectExtendedLimitInformation,
                (&information as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
                mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(format!(
                "SetInformationJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { AssignProcessToJobObject(guard.handle, child.as_raw_handle() as HANDLE) } == 0 {
            return Err(format!(
                "AssignProcessToJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(guard)
    }

    pub fn terminate(&mut self) -> Result<(), String> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        if !self.active {
            return Ok(());
        }
        if unsafe { TerminateJobObject(self.handle, 1) } == 0 {
            return Err(format!(
                "TerminateJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}
