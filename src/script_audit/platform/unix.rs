use std::{
    fs::{File, OpenOptions},
    os::unix::io::AsRawFd,
    path::Path,
};

pub(crate) struct NamedAuditLock(File);

impl NamedAuditLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, String> {
        let lock_path = path.with_extension("jsonl.lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("failed to create script audit lock directory: {error}")
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| format!("failed to open script audit lock: {error}"))?;
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) == 0 };
        if !locked {
            return Err(format!(
                "script audit flock failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(file))
    }
}

impl Drop for NamedAuditLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
