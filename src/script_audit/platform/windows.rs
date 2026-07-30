use std::{path::Path, ptr};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
    System::Threading::{CreateMutexW, INFINITE, ReleaseMutex, WaitForSingleObject},
};

use super::super::source_fingerprint;

pub(crate) struct NamedAuditLock(HANDLE);

impl NamedAuditLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, String> {
        let identity = source_fingerprint(&path.to_string_lossy().to_ascii_lowercase());
        let mut name: Vec<u16> = format!("Local\\AgenTermScriptAudit-{}", &identity[9..])
            .encode_utf16()
            .collect();
        name.push(0);
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "CreateMutexW for script audit failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            unsafe { CloseHandle(handle) };
            return Err(format!("script audit mutex wait failed: {wait}"));
        }
        Ok(Self(handle))
    }
}

impl Drop for NamedAuditLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}
