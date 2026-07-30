use std::{
    ffi::c_void,
    mem,
    os::windows::io::AsRawHandle,
    process::{Child, Command},
    ptr,
    sync::atomic::Ordering,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject,
        },
        Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    },
};

use crate::worker_supervisor::{
    GLOBAL_CONCURRENCY_LIMIT, PROCESS_ACTIVE, PROCESS_CONCURRENCY_LIMIT, SupervisorError,
};

pub(crate) struct ProcessTreeGuard(Job);

pub(crate) fn configure_worker_command(_command: &mut Command) {}

impl ProcessTreeGuard {
    pub(crate) fn attach(child: &mut Child) -> Result<Self, String> {
        let job = Job::new()?;
        job.assign(child)?;
        Ok(Self(job))
    }

    pub(crate) fn terminate(&self, exit_code: u32) -> Result<(), String> {
        self.0.terminate(exit_code)
    }
}

pub(crate) fn terminate_worker(child: &mut Child, _pid: u32) {
    let _ = child.kill();
    let _ = child.wait();
}

struct Job(HANDLE);

impl Job {
    fn new() -> Result<Self, String> {
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(format!(
                "CreateJobObjectW failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&information as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
                mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            unsafe { CloseHandle(handle) };
            return Err(format!(
                "SetInformationJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(handle))
    }

    fn assign(&self, child: &Child) -> Result<(), String> {
        let process = child.as_raw_handle() as HANDLE;
        if unsafe { AssignProcessToJobObject(self.0, process) } == 0 {
            return Err(format!(
                "AssignProcessToJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    fn terminate(&self, exit_code: u32) -> Result<(), String> {
        if unsafe { TerminateJobObject(self.0, exit_code) } == 0 {
            return Err(format!(
                "TerminateJobObject failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) struct ConcurrencyPermit {
    mutex: HANDLE,
}

impl ConcurrencyPermit {
    pub(crate) fn try_acquire() -> Result<Self, SupervisorError> {
        let previous = PROCESS_ACTIVE.fetch_add(1, Ordering::AcqRel);
        if previous >= PROCESS_CONCURRENCY_LIMIT {
            PROCESS_ACTIVE.fetch_sub(1, Ordering::AcqRel);
            return Err(SupervisorError::ConcurrencyLimit);
        }
        for slot in 0..GLOBAL_CONCURRENCY_LIMIT {
            let mut name: Vec<u16> = format!("Local\\AgenTermScriptSupervisorV1Slot{slot}")
                .encode_utf16()
                .collect();
            name.push(0);
            let mutex = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
            if mutex.is_null() {
                PROCESS_ACTIVE.fetch_sub(1, Ordering::AcqRel);
                return Err(SupervisorError::Spawn(format!(
                    "CreateMutexW failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            let wait = unsafe { WaitForSingleObject(mutex, 0) };
            if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
                return Ok(Self { mutex });
            }
            unsafe { CloseHandle(mutex) };
        }
        PROCESS_ACTIVE.fetch_sub(1, Ordering::AcqRel);
        Err(SupervisorError::ConcurrencyLimit)
    }
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.mutex);
            CloseHandle(self.mutex);
        }
        PROCESS_ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}
