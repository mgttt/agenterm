use std::{
    io,
    os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle as _, OwnedHandle},
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

use crate::process_reference::ProcessReferenceHandle;

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

    pub(crate) fn is_alive(&self) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            status => Err(io::Error::other(format!(
                "unexpected process wait status {status}"
            ))),
        }
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
