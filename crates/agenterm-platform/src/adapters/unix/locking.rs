use std::{
    fs::{File, OpenOptions},
    os::fd::AsRawFd,
    path::Path,
};

use crate::locking::{LockError, LockErrorKind};

pub struct PathLock(File);

impl PathLock {
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        Self::acquire_with_operation(path, libc::LOCK_EX)
    }

    pub fn try_acquire(path: &Path) -> Result<Self, LockError> {
        Self::acquire_with_operation(path, libc::LOCK_EX | libc::LOCK_NB)
    }

    fn acquire_with_operation(path: &Path, operation: i32) -> Result<Self, LockError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(open_error)?;
        }
        let file = open(path)?;
        if unsafe { libc::flock(file.as_raw_fd(), operation) } != 0 {
            let error = std::io::Error::last_os_error();
            let raw_error = error.raw_os_error();
            let would_block =
                raw_error == Some(libc::EAGAIN) || raw_error == Some(libc::EWOULDBLOCK);
            let kind = if operation & libc::LOCK_NB != 0 && would_block {
                LockErrorKind::Contended
            } else {
                LockErrorKind::Wait
            };
            return Err(LockError::new(kind, error.to_string()));
        }
        Ok(Self(file))
    }
}

impl Drop for PathLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub struct SlotPermit(File);

impl SlotPermit {
    pub fn try_acquire(directory: &Path, namespace: &str, limit: usize) -> Result<Self, LockError> {
        std::fs::create_dir_all(directory).map_err(open_error)?;
        let scope = fingerprint(namespace);
        for slot in 0..limit {
            let file = open(&directory.join(format!("platform-{scope:016x}-{slot}.lock")))?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self(file));
            }
        }
        Err(LockError::new(
            LockErrorKind::Contended,
            "all lock slots are occupied",
        ))
    }
}

impl Drop for SlotPermit {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn open(path: &Path) -> Result<File, LockError> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(open_error)
}

fn open_error(error: std::io::Error) -> LockError {
    LockError::new(LockErrorKind::Open, error.to_string())
}

fn fingerprint(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}
