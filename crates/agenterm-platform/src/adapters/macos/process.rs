//! macOS implementation of the process facade contract.

use std::process::{Child, ChildStderr, ChildStdout, Command};

use crate::contract::process::{PipeProbeError, PipeProbeToken};
use crate::contract::process::{ProcessError, ProcessErrorKind, ProcessInfo, ProcessObservation};

pub(crate) fn write_parent_console_stderr(message: &str) -> bool {
    use std::io::Write as _;
    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "{message}").is_ok() && stderr.flush().is_ok()
}

pub(crate) fn stdout_probe_token(_reader: &ChildStdout) -> Option<PipeProbeToken> {
    None
}
pub(crate) fn stderr_probe_token(_reader: &ChildStderr) -> Option<PipeProbeToken> {
    None
}
pub(crate) fn pipe_available(_token: PipeProbeToken) -> Result<usize, PipeProbeError> {
    Err(PipeProbeError::Failed)
}

pub(crate) fn configure_detached_command(_command: &mut Command) -> Result<(), String> {
    Err("detached process configuration is not implemented on macOS".to_owned())
}

pub(crate) fn is_breakaway_denied(_error: &std::io::Error) -> bool {
    false
}

pub(crate) fn configure_caller_job_fallback(_command: &mut Command) -> Result<(), String> {
    Err("caller-job process fallback is not implemented on macOS".to_owned())
}

pub(crate) fn observe(pid: u32) -> ProcessObservation {
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

pub(crate) fn list() -> Result<Vec<ProcessInfo>, ProcessError> {
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
            let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
            let info_size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            let info_bytes = unsafe {
                libc::proc_pidinfo(
                    id as libc::pid_t,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    (&raw mut info).cast(),
                    info_size,
                )
            };
            let parent_id = if info_bytes == info_size {
                info.pbi_ppid
            } else {
                0
            };
            processes.push(ProcessInfo {
                id,
                parent_id,
                executable_name,
            });
        }
    }
    Ok(processes)
}

pub struct ProcessTreeGuard {
    process_group: libc::pid_t,
    active: bool,
}

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

impl ProcessTreeGuard {
    pub fn attach(child: &Child) -> Result<Self, String> {
        let process_group = libc::pid_t::try_from(child.id())
            .map_err(|_| "child process ID exceeds pid_t".to_owned())?;
        Ok(Self {
            process_group,
            active: true,
        })
    }

    pub fn terminate(&mut self) -> Result<(), String> {
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
