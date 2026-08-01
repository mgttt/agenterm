//! Product-neutral process principal and sandbox identity facts.

use std::io;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ProcessPrincipal {
    Posix {
        effective_user_id: u32,
        effective_group_id: u32,
    },
    WindowsSid(Vec<u8>),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ProcessSandboxIdentity {
    WindowsAppContainerSid(Vec<u8>),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProcessSecurityFacts {
    principal: ProcessPrincipal,
    sandbox: Option<ProcessSandboxIdentity>,
}

impl ProcessSecurityFacts {
    pub(crate) const fn new(
        principal: ProcessPrincipal,
        sandbox: Option<ProcessSandboxIdentity>,
    ) -> Self {
        Self { principal, sandbox }
    }

    #[must_use]
    pub const fn principal(&self) -> &ProcessPrincipal {
        &self.principal
    }

    #[must_use]
    pub const fn sandbox_identity(&self) -> Option<&ProcessSandboxIdentity> {
        self.sandbox.as_ref()
    }

    #[must_use]
    pub fn windows_app_container_sid(&self) -> Option<&[u8]> {
        match self.sandbox.as_ref() {
            Some(ProcessSandboxIdentity::WindowsAppContainerSid(sid)) => Some(sid),
            None => None,
        }
    }
}

pub fn current_process() -> io::Result<ProcessSecurityFacts> {
    crate::selected::process_security::current_process()
}

pub fn process(process_id: u32) -> io::Result<ProcessSecurityFacts> {
    if process_id == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process id must be nonzero",
        ));
    }
    crate::selected::process_security::process(process_id)
}

#[cfg(windows)]
pub fn process_handle(
    process: std::os::windows::io::BorrowedHandle<'_>,
) -> io::Result<ProcessSecurityFacts> {
    crate::selected::process_security::process_handle(process)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_pid_and_current_process_report_the_same_identity() {
        let direct = current_process().expect("current process security");
        let by_pid = process(std::process::id()).expect("current PID security");
        assert_eq!(direct, by_pid);
    }

    #[test]
    fn zero_pid_is_rejected_before_native_query() {
        assert_eq!(process(0).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(windows)]
    #[test]
    fn current_process_handle_reports_a_valid_user_sid() {
        use std::os::windows::io::{BorrowedHandle, RawHandle};
        use windows_sys::Win32::{Security::IsValidSid, System::Threading::GetCurrentProcess};

        let handle = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess() as RawHandle) };
        let facts = process_handle(handle).expect("current process handle security");
        let ProcessPrincipal::WindowsSid(sid) = facts.principal() else {
            panic!("Windows process must report a SID principal");
        };
        assert_ne!(unsafe { IsValidSid(sid.as_ptr().cast_mut().cast()) }, 0);
    }
}
