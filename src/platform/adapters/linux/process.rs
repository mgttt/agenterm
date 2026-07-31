//! Linux implementation of the process facade contract.

use std::process::{Child, Command};

use crate::platform::contract::process::{
    ProcessError, ProcessErrorKind, ProcessInfo, ProcessObservation,
};

pub(crate) fn autostart_server(
    _parameter_name: &str,
    _parameter_value: &str,
) -> std::io::Result<bool> {
    Ok(false)
}

pub(crate) fn observe(pid: u32) -> ProcessObservation {
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

pub(crate) fn kill(pid: u32) -> Result<(), ProcessError> {
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

pub(crate) fn list() -> Result<Vec<ProcessInfo>, ProcessError> {
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

pub(crate) struct ProcessTreeGuard {
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
