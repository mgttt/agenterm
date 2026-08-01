//! Windows implementation of the process facade contract.

use std::process::{Child, Command};

use crate::contract::process::{ProcessError, ProcessErrorKind, ProcessInfo, ProcessObservation};

pub(crate) fn configure_detached_command(command: &mut Command) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};
    command.creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW);
    Ok(())
}

pub(crate) fn observe(pid: u32) -> ProcessObservation {
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, GetLastError,
            STILL_ACTIVE,
        },
        System::Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let error = unsafe { GetLastError() };
        return if error == ERROR_INVALID_PARAMETER {
            ProcessObservation::Dead {
                reason: "process_not_found".to_owned(),
            }
        } else if error == ERROR_ACCESS_DENIED {
            ProcessObservation::Unknown {
                reason: "process_access_denied".to_owned(),
            }
        } else {
            ProcessObservation::Unknown {
                reason: format!("process_open_failed:{error}"),
            }
        };
    }
    let mut exit_code = 0;
    if unsafe { GetExitCodeProcess(process, &mut exit_code) } == 0 {
        unsafe { CloseHandle(process) };
        return ProcessObservation::Unknown {
            reason: "process_exit_query_failed".to_owned(),
        };
    }
    if exit_code != STILL_ACTIVE as u32 {
        unsafe { CloseHandle(process) };
        return ProcessObservation::Dead {
            reason: "process_exited".to_owned(),
        };
    }
    let mut creation: FILETIME = unsafe { std::mem::zeroed() };
    let mut exit: FILETIME = unsafe { std::mem::zeroed() };
    let mut kernel: FILETIME = unsafe { std::mem::zeroed() };
    let mut user: FILETIME = unsafe { std::mem::zeroed() };
    let queried =
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } != 0;
    unsafe { CloseHandle(process) };
    if !queried {
        return ProcessObservation::Unknown {
            reason: "process_start_identity_query_failed".to_owned(),
        };
    }
    let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    ProcessObservation::Live {
        start_identity: Some(format!("windows-filetime:{ticks}")),
    }
}

pub(crate) fn kill(pid: u32) -> Result<(), ProcessError> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
    };
    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if process.is_null() {
        return Err(ProcessError::new(ProcessErrorKind::KillOpen, "open failed"));
    }
    let terminated = unsafe { TerminateProcess(process, 1) };
    unsafe { CloseHandle(process) };
    if terminated == 0 {
        return Err(ProcessError::new(
            ProcessErrorKind::Kill,
            "terminate failed",
        ));
    }
    Ok(())
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
