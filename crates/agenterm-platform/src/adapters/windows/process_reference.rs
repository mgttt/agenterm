use std::{
    io,
    os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle as _, OwnedHandle},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        DUPLICATE_SAME_ACCESS, DuplicateHandle, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::Threading::{
        GetCurrentProcess, GetProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        WaitForSingleObject,
    },
};

use crate::process_reference::{ProcessReferenceHandle, ProcessWait};

const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

pub struct ProcessReference {
    handle: OwnedHandle,
    process_id: u32,
}

impl ProcessReference {
    pub(crate) fn open(process_id: u32) -> io::Result<Self> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE_ACCESS,
                0,
                process_id,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle: unsafe { OwnedHandle::from_raw_handle(handle) },
            process_id,
        })
    }

    pub(crate) const fn id(&self) -> u32 {
        self.process_id
    }

    pub(crate) fn wait_for_exit(&self, timeout: Option<Duration>) -> io::Result<ProcessWait> {
        wait(self.handle.as_raw_handle(), timeout)
    }
}

impl AsRawHandle for crate::process_reference::ProcessReference {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.0.handle.as_raw_handle()
    }
}

impl AsHandle for crate::process_reference::ProcessReference {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.handle.as_handle()
    }
}

impl ProcessReferenceHandle for BorrowedHandle<'_> {
    fn duplicate_process_reference(self) -> io::Result<crate::process_reference::ProcessReference> {
        let process_id = unsafe { GetProcessId(self.as_raw_handle()) };
        if process_id == 0 {
            return Err(io::Error::last_os_error());
        }

        let current = unsafe { GetCurrentProcess() };
        let mut duplicate = std::ptr::null_mut();
        if unsafe {
            DuplicateHandle(
                current,
                self.as_raw_handle(),
                current,
                &raw mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }

        Ok(crate::process_reference::ProcessReference(
            ProcessReference {
                handle: unsafe { OwnedHandle::from_raw_handle(duplicate) },
                process_id,
            },
        ))
    }

    fn wait_for_process_exit(self, timeout: Option<Duration>) -> io::Result<ProcessWait> {
        wait(self.as_raw_handle(), timeout)
    }
}

fn wait(
    handle: std::os::windows::io::RawHandle,
    timeout: Option<Duration>,
) -> io::Result<ProcessWait> {
    const MAX_FINITE_WAIT_MS: u32 = u32::MAX - 1;

    let started = Instant::now();
    loop {
        let native_timeout = match timeout {
            None => u32::MAX,
            Some(limit) => {
                let remaining = limit.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    0
                } else {
                    remaining
                        .as_millis()
                        .saturating_add(1)
                        .min(u128::from(MAX_FINITE_WAIT_MS)) as u32
                }
            }
        };
        match unsafe { WaitForSingleObject(handle, native_timeout) } {
            WAIT_OBJECT_0 => return Ok(ProcessWait::Exited),
            WAIT_TIMEOUT if timeout.is_some_and(|limit| started.elapsed() >= limit) => {
                return Ok(ProcessWait::TimedOut);
            }
            WAIT_TIMEOUT => {}
            WAIT_FAILED => return Err(io::Error::last_os_error()),
            status => {
                return Err(io::Error::other(format!(
                    "unexpected process wait status {status}"
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::RawHandle;

    #[test]
    fn current_process_handle_can_be_retained() {
        let handle = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess() as RawHandle) };
        let reference = crate::process_reference::ProcessReference::duplicate_from(handle)
            .expect("duplicate current process handle");
        assert_eq!(reference.id(), std::process::id());
        assert!(reference.is_alive().expect("current process liveness"));
    }
}
