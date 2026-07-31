//! Windows worker-supervisor and audit coordination adapter.

use std::{
    ffi::c_void,
    mem,
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    process::{Child, Command},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
    System::Threading::{CreateMutexW, INFINITE, ReleaseMutex, WaitForSingleObject},
};

use crate::platform::contract::supervisor_audit::{SupervisorAuditError, SupervisorAuditErrorKind};

pub(crate) struct GlobalConcurrencyPermit(HANDLE);

impl GlobalConcurrencyPermit {
    pub(crate) fn try_acquire(limit: usize) -> Result<Self, SupervisorAuditError> {
        for slot in 0..limit {
            let mut name: Vec<u16> = format!("Local\\AgenTermScriptSupervisorV1Slot{slot}")
                .encode_utf16()
                .collect();
            name.push(0);
            let mutex = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
            if mutex.is_null() {
                return Err(SupervisorAuditError::new(
                    SupervisorAuditErrorKind::LockOpen,
                    format!("CreateMutexW failed: {}", std::io::Error::last_os_error()),
                ));
            }
            let wait = unsafe { WaitForSingleObject(mutex, 0) };
            if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
                return Ok(Self(mutex));
            }
            unsafe { CloseHandle(mutex) };
        }
        Err(concurrency_limit_error())
    }
}

impl Drop for GlobalConcurrencyPermit {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

pub(crate) struct NamedAuditLock(HANDLE);

impl NamedAuditLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, SupervisorAuditError> {
        let identity = fingerprint(&path.to_string_lossy().to_ascii_lowercase());
        let mut name: Vec<u16> = format!("Local\\AgenTermScriptAudit-{identity:016x}")
            .encode_utf16()
            .collect();
        name.push(0);
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(SupervisorAuditError::new(
                SupervisorAuditErrorKind::LockOpen,
                format!(
                    "CreateMutexW for script audit failed: {}",
                    std::io::Error::last_os_error()
                ),
            ));
        }
        let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            unsafe { CloseHandle(handle) };
            return Err(SupervisorAuditError::new(
                SupervisorAuditErrorKind::LockWait,
                format!("script audit mutex wait failed: {wait}"),
            ));
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
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("AgenTerm")
        .join("script-audit.jsonl")
}

fn fingerprint(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

pub(crate) fn configure_worker_command(_command: &mut Command) -> Result<(), String> {
    Ok(())
}

pub(crate) struct ProcessTreeGuard(HANDLE);

impl ProcessTreeGuard {
    pub(crate) fn attach(child: &Child) -> Result<Self, String> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(format!(
                "CreateJobObjectW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
                mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            unsafe { CloseHandle(handle) };
            return Err(format!(
                "SetInformationJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) } == 0 {
            unsafe { CloseHandle(handle) };
            return Err(format!(
                "AssignProcessToJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(handle))
    }

    pub(crate) fn terminate(&mut self, exit_code: u32) -> Result<(), String> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        if unsafe { TerminateJobObject(self.0, exit_code) } == 0 {
            return Err(format!(
                "TerminateJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
