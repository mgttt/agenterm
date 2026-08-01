use std::{
    collections::HashSet,
    path::Path,
    ptr,
    sync::{Mutex, OnceLock},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
    System::Threading::{CreateMutexW, INFINITE, ReleaseMutex, WaitForSingleObject},
};

use crate::locking::{LockError, LockErrorKind};

pub struct PathLock(HANDLE);

impl PathLock {
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        acquire_named(
            &format!("path-{:016x}", fingerprint(&path.to_string_lossy())),
            INFINITE,
        )
        .map(Self)
    }
}

impl Drop for PathLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

pub struct SlotPermit {
    handle: HANDLE,
    identity: String,
}

impl SlotPermit {
    pub fn try_acquire(directory: &Path, namespace: &str, limit: usize) -> Result<Self, LockError> {
        let scope = format!("{}:{namespace}", directory.to_string_lossy());
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

fn local_slots() -> &'static Mutex<HashSet<String>> {
    static SLOTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn reserve_local(identity: &str) -> bool {
    local_slots()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(identity.to_owned())
}

fn release_local(identity: &str) {
    local_slots()
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
        let kind = if timeout == 0 {
            LockErrorKind::Contended
        } else {
            LockErrorKind::Wait
        };
        Err(LockError::new(
            kind,
            format!("native mutex wait returned {wait}"),
        ))
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
