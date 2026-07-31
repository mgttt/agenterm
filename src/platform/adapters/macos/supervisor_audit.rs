//! macOS worker-supervisor and audit coordination adapter.

use std::{
    fs::{File, OpenOptions},
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command},
};

use crate::platform::contract::supervisor_audit::{SupervisorAuditError, SupervisorAuditErrorKind};

pub(crate) struct GlobalConcurrencyPermit(File);

impl GlobalConcurrencyPermit {
    pub(crate) fn try_acquire(limit: usize) -> Result<Self, SupervisorAuditError> {
        for slot in 0..limit {
            let path =
                std::env::temp_dir().join(format!("agenterm-script-supervisor-slot-{slot}.lock"));
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(path)
                .map_err(|error| {
                    SupervisorAuditError::new(
                        SupervisorAuditErrorKind::LockOpen,
                        format!("supervisor slot lock open failed: {error}"),
                    )
                })?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self(file));
            }
        }
        Err(concurrency_limit_error())
    }
}

impl Drop for GlobalConcurrencyPermit {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub(crate) struct NamedAuditLock(File);

impl NamedAuditLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, SupervisorAuditError> {
        let lock_path = path.with_extension("jsonl.lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                SupervisorAuditError::new(
                    SupervisorAuditErrorKind::LockOpen,
                    format!("failed to create script audit lock directory: {error}"),
                )
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|error| {
                SupervisorAuditError::new(
                    SupervisorAuditErrorKind::LockOpen,
                    format!("failed to open script audit lock: {error}"),
                )
            })?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(SupervisorAuditError::new(
                SupervisorAuditErrorKind::LockWait,
                format!(
                    "script audit flock failed: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        Ok(Self(file))
    }
}

impl Drop for NamedAuditLock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub(crate) fn concurrency_limit_error() -> SupervisorAuditError {
    SupervisorAuditError::new(
        SupervisorAuditErrorKind::LockWait,
        "global worker concurrency limit reached",
    )
}

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
