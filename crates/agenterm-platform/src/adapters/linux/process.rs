//! Linux implementation of the process facade contract.

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
    parse_stat_observation(&stat)
}

fn parse_stat_observation(stat: &str) -> ProcessObservation {
    let Some(fields) = stat.rsplit_once(") ").map(|(_, fields)| fields) else {
        return ProcessObservation::Unknown {
            reason: "process_identity_parse_failed".to_owned(),
        };
    };
    let mut fields = fields.split_whitespace();
    let Some(state) = fields.next() else {
        return ProcessObservation::Unknown {
            reason: "process_identity_parse_failed".to_owned(),
        };
    };
    if matches!(state, "Z" | "X" | "x") {
        return ProcessObservation::Dead {
            reason: "process_exited_not_reaped".to_owned(),
        };
    }
    let Some(start_ticks) = fields
        .nth(18)
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
        let Ok(executable) = crate::selected::process_image::executable_path(id) else {
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
        let root_id = u32::try_from(self.process_group)
            .map_err(|_| "owned process group is outside the process ID range".to_owned())?;
        let descendants = list()
            .map(|processes| {
                crate::contract::process::transitive_descendant_ids(root_id, &processes)
                    .into_iter()
                    .filter_map(|id| match observe(id) {
                        ProcessObservation::Live {
                            start_identity: Some(identity),
                        } => Some((id, identity)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .map_err(|error| format!("owned process inventory failed: {error}"));
        let group_result = if unsafe { libc::killpg(self.process_group, libc::SIGKILL) } == 0 {
            Ok(())
        } else {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                Ok(())
            } else {
                Err(format!("killpg failed: {error}"))
            }
        };
        let mut failures = Vec::new();
        if let Err(error) = group_result {
            failures.push(error);
        }
        match descendants {
            Ok(descendants) => {
                for (id, identity) in descendants {
                    if !matches!(
                        observe(id),
                        ProcessObservation::Live { start_identity: Some(current) }
                            if current == identity
                    ) {
                        continue;
                    }
                    let Ok(native_id) = libc::pid_t::try_from(id) else {
                        failures.push(format!("descendant process ID {id} exceeds pid_t"));
                        continue;
                    };
                    if unsafe { libc::kill(native_id, libc::SIGKILL) } != 0 {
                        let error = std::io::Error::last_os_error();
                        if error.raw_os_error() != Some(libc::ESRCH) {
                            failures.push(format!("kill descendant {id} failed: {error}"));
                        }
                    }
                }
            }
            Err(error) => failures.push(error),
        }
        if !failures.is_empty() {
            return Err(failures.join("; "));
        }
        self.active = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_stat_observation;
    use crate::contract::process::ProcessObservation;

    fn stat_with_state(state: &str) -> String {
        format!(
            "42 (worker with spaces) {state} 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 12345"
        )
    }

    #[test]
    fn zombie_and_dead_states_are_not_reported_live() {
        for state in ["Z", "X", "x"] {
            assert!(matches!(
                parse_stat_observation(&stat_with_state(state)),
                ProcessObservation::Dead { ref reason }
                    if reason == "process_exited_not_reaped"
            ));
        }
    }

    #[test]
    fn live_state_preserves_start_identity() {
        assert!(matches!(
            parse_stat_observation(&stat_with_state("S")),
            ProcessObservation::Live { start_identity: Some(ref identity) }
                if identity == "proc-start-ticks:12345"
        ));
    }
}
