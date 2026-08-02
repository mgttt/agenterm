use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    ptr,
    sync::{Mutex, OnceLock},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{CreateMutexW, INFINITE, ReleaseMutex, WaitForSingleObject},
};

use crate::locking::{LockError, LockErrorKind};

pub struct PathLock {
    handle: HANDLE,
    identity: String,
}

impl PathLock {
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        Self::acquire_with_timeout(path, INFINITE)
    }

    pub fn try_acquire(path: &Path) -> Result<Self, LockError> {
        Self::acquire_with_timeout(path, 0)
    }

    fn acquire_with_timeout(path: &Path, timeout: u32) -> Result<Self, LockError> {
        let identity = format!("path-{:016x}", fingerprint(&normalized_path(path)?));
        if !reserve_local(&identity) {
            return Err(LockError::new(
                LockErrorKind::Contended,
                "path lock is already held in this process",
            ));
        }
        match acquire_named(&identity, timeout) {
            Ok(handle) => Ok(Self { handle, identity }),
            Err(error) => {
                release_local(&identity);
                Err(error)
            }
        }
    }
}

impl Drop for PathLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
        release_local(&self.identity);
    }
}

pub struct SlotPermit {
    handle: HANDLE,
    identity: String,
}

impl SlotPermit {
    pub fn try_acquire(directory: &Path, namespace: &str, limit: usize) -> Result<Self, LockError> {
        let scope = format!("{}:{namespace}", normalized_path(directory)?);
        for slot in 0..limit {
            let identity = format!("slot-{:016x}-{slot}", fingerprint(&scope));
            if !reserve_local(&identity) {
                continue;
            }
            match acquire_named(&identity, 0) {
                Ok(handle) => return Ok(Self { handle, identity }),
                Err(error) if error.kind() == LockErrorKind::Contended => {
                    release_local(&identity);
                }
                Err(error) => {
                    release_local(&identity);
                    return Err(error);
                }
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
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
        release_local(&self.identity);
    }
}

fn local_reservations() -> &'static Mutex<HashSet<String>> {
    static RESERVATIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    RESERVATIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn reserve_local(identity: &str) -> bool {
    local_reservations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(identity.to_owned())
}

fn release_local(identity: &str) {
    local_reservations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(identity);
}

fn acquire_named(identity: &str, timeout: u32) -> Result<HANDLE, LockError> {
    let mut name = format!("Local\\AgentermPlatform-{identity}")
        .encode_utf16()
        .collect::<Vec<_>>();
    name.push(0);
    let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(LockError::new(
            LockErrorKind::Open,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let wait = unsafe { WaitForSingleObject(handle, timeout) };
    if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
        Ok(handle)
    } else {
        unsafe { CloseHandle(handle) };
        let kind = wait_error_kind(wait, timeout);
        let message = if wait == WAIT_FAILED {
            std::io::Error::last_os_error().to_string()
        } else {
            format!("native mutex wait returned {wait}")
        };
        Err(LockError::new(kind, message))
    }
}

fn wait_error_kind(wait: u32, timeout: u32) -> LockErrorKind {
    if wait == WAIT_TIMEOUT && timeout == 0 {
        LockErrorKind::Contended
    } else {
        LockErrorKind::Wait
    }
}

fn fingerprint(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn normalized_path(path: &Path) -> Result<String, LockError> {
    let absolute = if path.is_absolute() {
        lexical_normalize(path)
    } else {
        lexical_normalize(&std::env::current_dir().map_err(open_error)?.join(path))
    };
    let resolved = canonicalize_with_missing_tail(&absolute).unwrap_or(absolute);
    let mut value = resolved.to_string_lossy().replace('/', "\\");
    if let Some(without_prefix) = value.strip_prefix(r"\\?\UNC\") {
        value = format!(r"\\{without_prefix}");
    } else if let Some(without_prefix) = value.strip_prefix(r"\\?\") {
        value = without_prefix.to_owned();
    }
    Ok(value.to_lowercase())
}

fn lexical_normalize(path: &Path) -> PathBuf {
    crate::filesystem::lexical_normalize(path)
}

fn canonicalize_with_missing_tail(path: &Path) -> std::io::Result<PathBuf> {
    let mut ancestor = path;
    let mut tail = Vec::<OsString>::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no path ancestor"))?;
        tail.push(name.to_os_string());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no path parent"))?;
    }
    let mut resolved = std::fs::canonicalize(ancestor)?;
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn open_error(error: std::io::Error) -> LockError {
    LockError::new(LockErrorKind::Open, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Component;

    #[test]
    fn only_nonblocking_timeout_is_contention() {
        assert_eq!(wait_error_kind(WAIT_TIMEOUT, 0), LockErrorKind::Contended);
        assert_eq!(wait_error_kind(WAIT_FAILED, 0), LockErrorKind::Wait);
        assert_eq!(wait_error_kind(WAIT_TIMEOUT, INFINITE), LockErrorKind::Wait);
    }

    #[test]
    fn slot_aliases_share_one_identity() {
        let directory = std::env::temp_dir().join(format!(
            "agenterm-platform-slot-alias-{}",
            std::process::id()
        ));
        let child = directory.join("child");
        std::fs::create_dir_all(&child).expect("create slot alias fixture");
        let alias = child.join("..");

        let first = SlotPermit::try_acquire(&directory, "alias", 1).expect("first slot");
        let error = match SlotPermit::try_acquire(&alias, "alias", 1) {
            Ok(_) => panic!("directory alias bypassed the existing slot"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), LockErrorKind::Contended);
        drop(first);
        SlotPermit::try_acquire(&alias, "alias", 1).expect("alias slot is released");
        std::fs::remove_dir_all(directory).expect("remove slot alias fixture");
    }

    #[test]
    fn unicode_case_aliases_share_one_path_lock_identity() {
        let directory = std::env::temp_dir().join(format!(
            "agenterm-platform-unicode-path-alias-{}",
            std::process::id()
        ));
        let child = directory.join("child");
        std::fs::create_dir_all(&child).expect("create Unicode path alias fixture");
        let canonical = directory.join("Å-state.lock");
        std::fs::write(&canonical, b"unicode-path-lock-alias").expect("create Unicode lock target");
        let alias = child.join("..").join("å-STATE.LOCK");

        let first = PathLock::acquire(&canonical).expect("acquire Unicode canonical path");
        let error = match PathLock::try_acquire(&alias) {
            Ok(_) => panic!("Unicode case alias bypassed the existing lock"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), LockErrorKind::Contended);
        drop(first);
        PathLock::try_acquire(&alias).expect("Unicode case alias is released");
        std::fs::remove_dir_all(directory).expect("remove Unicode path alias fixture");
    }

    #[test]
    fn lexical_normalize_does_not_pop_past_the_host_root() {
        let current = std::env::current_dir().expect("read current directory");
        let mut anchor = PathBuf::new();
        for component in current.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
                _ => break,
            }
        }
        let escaped = anchor.join("..").join("path-lock-root");
        assert_eq!(lexical_normalize(&escaped), anchor.join("path-lock-root"));
    }
}
