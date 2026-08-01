//! macOS worker-supervisor and audit coordination adapter.

use std::{
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, Command},
};

use crate::platform::contract::supervisor_audit::{SupervisorAuditError, SupervisorAuditErrorKind};

pub(crate) fn process_tree_error(message: String) -> SupervisorAuditError {
    SupervisorAuditError::new(SupervisorAuditErrorKind::ProcessTree, message)
}

pub(crate) fn default_audit_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(path)
            .join("agenterm")
            .join("script-audit.jsonl")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("agenterm")
            .join("script-audit.jsonl")
    } else {
        std::env::temp_dir().join("agenterm-script-audit.jsonl")
    }
}

pub(crate) fn configure_worker_command(command: &mut Command) -> Result<(), String> {
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

pub(crate) struct ProcessTreeGuard(libc::pid_t);
impl ProcessTreeGuard {
    pub(crate) fn attach(child: &Child) -> Result<Self, String> {
        libc::pid_t::try_from(child.id())
            .map(Self)
            .map_err(|_| "child process ID exceeds pid_t".to_owned())
    }
    pub(crate) fn terminate(&mut self, _exit_code: u32) -> Result<(), String> {
        if unsafe { libc::killpg(self.0, libc::SIGKILL) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            Ok(())
        } else {
            Err(format!(
                "killpg failed: {}",
                std::io::Error::last_os_error()
            ))
        }
    }
}
