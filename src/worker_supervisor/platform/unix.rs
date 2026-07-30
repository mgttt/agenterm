use std::{
    fs::{File, OpenOptions},
    os::unix::io::AsRawFd,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, Command},
    sync::atomic::Ordering,
};

use crate::worker_supervisor::{
    GLOBAL_CONCURRENCY_LIMIT, PROCESS_ACTIVE, PROCESS_CONCURRENCY_LIMIT, SupervisorError,
};

pub(crate) struct ProcessTreeGuard {
    pid: u32,
}

pub(crate) fn configure_worker_command(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
}

impl ProcessTreeGuard {
    pub(crate) fn attach(child: &mut Child) -> Result<Self, String> {
        Ok(Self { pid: child.id() })
    }

    pub(crate) fn terminate(&self, _exit_code: u32) -> Result<(), String> {
        let pgid = self.pid as i32;
        unsafe {
            if libc::killpg(pgid, libc::SIGKILL) != 0 {
                let _ = libc::kill(self.pid as i32, libc::SIGKILL);
            }
        }
        Ok(())
    }
}

pub(crate) fn terminate_worker(child: &mut Child, pid: u32) {
    let pgid = pid as i32;
    unsafe {
        if libc::killpg(pgid, libc::SIGKILL) != 0 {
            let _ = libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub(crate) struct ConcurrencyPermit {
    _slot: usize,
    _file: File,
}

impl ConcurrencyPermit {
    pub(crate) fn try_acquire() -> Result<Self, SupervisorError> {
        let previous = PROCESS_ACTIVE.fetch_add(1, Ordering::AcqRel);
        if previous >= PROCESS_CONCURRENCY_LIMIT {
            PROCESS_ACTIVE.fetch_sub(1, Ordering::AcqRel);
            return Err(SupervisorError::ConcurrencyLimit);
        }
        for slot in 0..GLOBAL_CONCURRENCY_LIMIT {
            let path = slot_lock_path(slot);
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&path)
                .map_err(|error| {
                    PROCESS_ACTIVE.fetch_sub(1, Ordering::AcqRel);
                    SupervisorError::Spawn(format!("supervisor slot lock open failed: {error}"))
                })?;
            if try_lock_exclusive(&file) {
                return Ok(Self {
                    _slot: slot,
                    _file: file,
                });
            }
        }
        PROCESS_ACTIVE.fetch_sub(1, Ordering::AcqRel);
        Err(SupervisorError::ConcurrencyLimit)
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        unlock_file(&self._file);
        PROCESS_ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

fn slot_lock_path(slot: usize) -> PathBuf {
    std::env::temp_dir().join(format!("agenterm-script-supervisor-slot-{slot}.lock"))
}

fn try_lock_exclusive(file: &File) -> bool {
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

fn unlock_file(file: &File) {
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}
