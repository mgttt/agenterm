//! Process and owned-process-tree services behind a platform facade.
//!
//! Native handles, `/proc`, process groups, and platform C APIs stay here;
//! product and Script layers consume typed, OS-neutral results.

use std::process::{Child, Command};

/// Start the independently owned local server for a CLI client when that is a
/// host capability. `false` means the host deliberately does not provide this
/// autostart path; callers retain their normal connection retry behavior.
pub(crate) fn autostart_server(
    parameter_name: &str,
    parameter_value: &str,
) -> std::io::Result<bool> {
    autostart_server_native(parameter_name, parameter_value)
}

#[cfg(windows)]
fn autostart_server_native(parameter_name: &str, parameter_value: &str) -> std::io::Result<bool> {
    use std::{os::windows::process::CommandExt as _, process::Stdio};
    use windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB;

    let current = std::env::current_exe()?;
    let server = current.with_file_name("agenterm-server.exe");
    if !server.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "AgenTerm server executable was not found beside agenterm-cli: {}",
                server.display()
            ),
        ));
    }
    Command::new(server)
        .arg(parameter_name)
        .arg(parameter_value)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
        .spawn()?;
    Ok(true)
}

#[cfg(not(windows))]
fn autostart_server_native(_parameter_name: &str, _parameter_value: &str) -> std::io::Result<bool> {
    Ok(false)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProcessObservation {
    Live { start_identity: Option<String> },
    Dead { reason: String },
    Unknown { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessInfo {
    pub(crate) id: u32,
    pub(crate) parent_id: u32,
    pub(crate) executable_name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessErrorKind {
    IdOutOfRange,
    Inventory,
    InventoryTooLarge,
    KillOpen,
    Kill,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessError {
    pub(crate) kind: ProcessErrorKind,
    pub(crate) detail: String,
}

impl ProcessError {
    fn new(kind: ProcessErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

pub(crate) fn observe(pid: u32) -> ProcessObservation {
    observe_native(pid)
}

pub(crate) fn start_identity(pid: u32) -> Result<String, String> {
    match observe(pid) {
        ProcessObservation::Live {
            start_identity: Some(identity),
        } => Ok(identity),
        ProcessObservation::Live {
            start_identity: None,
        } => Err("process is live but its start identity is unavailable".to_owned()),
        ProcessObservation::Dead { reason } | ProcessObservation::Unknown { reason } => Err(reason),
    }
}

pub(crate) fn list() -> Result<Vec<ProcessInfo>, ProcessError> {
    list_native()
}

pub(crate) fn kill(pid: u32) -> Result<(), ProcessError> {
    kill_native(pid)
}

#[cfg(windows)]
fn observe_native(pid: u32) -> ProcessObservation {
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

#[cfg(target_os = "linux")]
fn observe_native(pid: u32) -> ProcessObservation {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ProcessObservation::Dead {
                reason: "process_not_found".to_owned(),
            };
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return ProcessObservation::Unknown {
                reason: "process_access_denied".to_owned(),
            };
        }
        Err(error) => {
            return ProcessObservation::Unknown {
                reason: format!("process_identity_read_failed:{error}"),
            };
        }
    };
    let Some(start_ticks) = stat
        .rsplit_once(") ")
        .map(|(_, fields)| fields)
        .and_then(|fields| fields.split_whitespace().nth(19))
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
    else {
        return ProcessObservation::Unknown {
            reason: "process_identity_parse_failed".to_owned(),
        };
    };
    ProcessObservation::Live {
        start_identity: Some(format!("proc-start-ticks:{start_ticks}")),
    }
}

#[cfg(target_os = "macos")]
fn observe_native(pid: u32) -> ProcessObservation {
    let Ok(pid) = i32::try_from(pid) else {
        return ProcessObservation::Dead {
            reason: "process_id_out_of_range".to_owned(),
        };
    };
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    if read != size {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ESRCH) => ProcessObservation::Dead {
                reason: "process_not_found".to_owned(),
            },
            Some(libc::EPERM) | Some(libc::EACCES) => ProcessObservation::Unknown {
                reason: "process_access_denied".to_owned(),
            },
            _ => ProcessObservation::Unknown {
                reason: format!("process_identity_read_failed:{error}"),
            },
        };
    }
    ProcessObservation::Live {
        start_identity: Some(format!(
            "macos-start-time:{}.{}",
            info.pbi_start_tvsec, info.pbi_start_tvusec
        )),
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn observe_native(pid: u32) -> ProcessObservation {
    let Ok(pid) = i32::try_from(pid) else {
        return ProcessObservation::Dead {
            reason: "process_id_out_of_range".to_owned(),
        };
    };
    if pid <= 0 {
        return ProcessObservation::Dead {
            reason: "process_id_out_of_range".to_owned(),
        };
    }
    if unsafe { libc::kill(pid, 0) } == 0 {
        ProcessObservation::Live {
            start_identity: None,
        }
    } else {
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => ProcessObservation::Dead {
                reason: "process_not_found".to_owned(),
            },
            Some(libc::EPERM) => ProcessObservation::Unknown {
                reason: "process_access_denied".to_owned(),
            },
            _ => ProcessObservation::Unknown {
                reason: format!("process_probe_failed:{error}"),
            },
        }
    }
}

#[cfg(windows)]
fn kill_native(pid: u32) -> Result<(), ProcessError> {
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

#[cfg(unix)]
fn kill_native(pid: u32) -> Result<(), ProcessError> {
    let pid = libc::pid_t::try_from(pid)
        .map_err(|_| ProcessError::new(ProcessErrorKind::IdOutOfRange, "pid_t overflow"))?;
    if unsafe { libc::kill(pid, libc::SIGKILL) } != 0 {
        return Err(ProcessError::new(
            ProcessErrorKind::Kill,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn kill_native(_pid: u32) -> Result<(), ProcessError> {
    Err(ProcessError::new(
        ProcessErrorKind::Unsupported,
        "process termination unsupported",
    ))
}

#[cfg(windows)]
fn list_native() -> Result<Vec<ProcessInfo>, ProcessError> {
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

#[cfg(target_os = "linux")]
fn list_native() -> Result<Vec<ProcessInfo>, ProcessError> {
    let entries = std::fs::read_dir("/proc")
        .map_err(|error| ProcessError::new(ProcessErrorKind::Inventory, error.to_string()))?;
    let mut processes = Vec::new();
    for entry in entries.flatten() {
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(executable) = std::fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        let Some(executable_name) = executable.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let parent_id = std::fs::read_to_string(entry.path().join("stat"))
            .ok()
            .and_then(|stat| {
                let end = stat.rfind(')')?;
                stat.get(end + 1..)?
                    .split_whitespace()
                    .nth(1)?
                    .parse::<u32>()
                    .ok()
            })
            .unwrap_or_default();
        processes.push(ProcessInfo {
            id,
            parent_id,
            executable_name: executable_name.to_owned(),
        });
    }
    Ok(processes)
}

#[cfg(target_os = "macos")]
fn list_native() -> Result<Vec<ProcessInfo>, ProcessError> {
    use std::{
        ffi::{CStr, c_char, c_int, c_void},
        mem::size_of,
    };
    const PROC_ALL_PIDS: u32 = 1;
    const PROC_PIDPATHINFO_MAXSIZE: u32 = 4 * 1024;
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_listpids(
            process_type: u32,
            type_info: u32,
            buffer: *mut c_void,
            buffer_size: c_int,
        ) -> c_int;
        fn proc_pidpath(pid: c_int, buffer: *mut c_void, buffer_size: u32) -> c_int;
    }
    let required = unsafe { proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    if required <= 0 {
        return Err(ProcessError::new(
            ProcessErrorKind::Inventory,
            "size failed",
        ));
    }
    let capacity = usize::try_from(required).unwrap_or_default() / size_of::<c_int>() + 32;
    let mut ids: Vec<c_int> = vec![0; capacity];
    let buffer_size = c_int::try_from(ids.len() * size_of::<c_int>())
        .map_err(|_| ProcessError::new(ProcessErrorKind::InventoryTooLarge, "buffer overflow"))?;
    let bytes = unsafe { proc_listpids(PROC_ALL_PIDS, 0, ids.as_mut_ptr().cast(), buffer_size) };
    if bytes <= 0 {
        return Err(ProcessError::new(
            ProcessErrorKind::Inventory,
            "snapshot failed",
        ));
    }
    ids.truncate(usize::try_from(bytes).unwrap_or_default() / size_of::<c_int>());
    let mut processes = Vec::new();
    for id in ids.into_iter().filter(|id| *id > 0) {
        let mut path: Vec<c_char> =
            vec![0; usize::try_from(PROC_PIDPATHINFO_MAXSIZE).unwrap_or_default()];
        let length =
            unsafe { proc_pidpath(id, path.as_mut_ptr().cast(), PROC_PIDPATHINFO_MAXSIZE) };
        if length <= 0 {
            continue;
        }
        let full_path = unsafe { CStr::from_ptr(path.as_ptr()) }.to_string_lossy();
        let executable_name = std::path::Path::new(full_path.as_ref())
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_owned();
        if !executable_name.is_empty()
            && let Ok(id) = u32::try_from(id)
        {
            processes.push(ProcessInfo {
                id,
                parent_id: 0,
                executable_name,
            });
        }
    }
    Ok(processes)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn list_native() -> Result<Vec<ProcessInfo>, ProcessError> {
    let executable_name = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "current-process".to_owned());
    Ok(vec![ProcessInfo {
        id: std::process::id(),
        parent_id: 0,
        executable_name,
    }])
}

#[cfg(windows)]
pub(crate) struct ProcessTreeGuard {
    handle: windows_sys::Win32::Foundation::HANDLE,
    active: bool,
}

#[cfg(windows)]
unsafe impl Send for ProcessTreeGuard {}

#[cfg(windows)]
pub(crate) fn configure_owned_command(_command: &mut Command) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
impl ProcessTreeGuard {
    pub(crate) fn attach(child: &Child) -> Result<Self, String> {
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

    pub(crate) fn terminate(&mut self) -> Result<(), String> {
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

#[cfg(windows)]
impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(unix)]
pub(crate) struct ProcessTreeGuard {
    process_group: libc::pid_t,
    active: bool,
}

#[cfg(unix)]
pub(crate) fn configure_owned_command(command: &mut Command) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    Ok(())
}

/// Backwards-compatible product-neutral verb used by Script Runtime.
pub(crate) fn configure_command(command: &mut Command) -> Result<(), String> {
    configure_owned_command(command)
}

#[cfg(unix)]
impl ProcessTreeGuard {
    pub(crate) fn attach(child: &Child) -> Result<Self, String> {
        let process_group = libc::pid_t::try_from(child.id())
            .map_err(|_| "child process ID exceeds pid_t".to_owned())?;
        Ok(Self {
            process_group,
            active: true,
        })
    }

    pub(crate) fn terminate(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        if unsafe { libc::killpg(self.process_group, libc::SIGKILL) } == 0 {
            self.active = false;
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            self.active = false;
            Ok(())
        } else {
            Err(format!("killpg failed: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_has_stable_observation_and_inventory_entry() {
        assert!(matches!(
            observe(std::process::id()),
            ProcessObservation::Live { .. }
        ));
        assert!(
            list()
                .expect("process inventory")
                .iter()
                .any(|entry| entry.id == std::process::id())
        );
    }
}
