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

/// A native process handle that can report process-security facts.
///
/// Platform adapters implement this trait for their native borrowed handle
/// type. Callers should normally use [`process`] unless they already hold a
/// process handle whose identity must remain stable across the query.
pub trait ProcessSecurityHandle {
    #[doc(hidden)]
    fn process_security_facts(self) -> io::Result<ProcessSecurityFacts>;
}

/// Queries security facts through an already-open native process handle.
///
/// This keeps the facade platform-neutral while preserving the Windows call
/// form `process_handle(borrowed_handle)` through the selected adapter's trait
/// implementation.
pub fn process_handle(process: impl ProcessSecurityHandle) -> io::Result<ProcessSecurityFacts> {
    process.process_security_facts()
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

    #[test]
    fn handle_query_delegates_without_exposing_a_native_type() {
        struct TestHandle;

        impl ProcessSecurityHandle for TestHandle {
            fn process_security_facts(self) -> io::Result<ProcessSecurityFacts> {
                Ok(ProcessSecurityFacts::new(
                    ProcessPrincipal::Posix {
                        effective_user_id: 10,
                        effective_group_id: 20,
                    },
                    None,
                ))
            }
        }

        assert_eq!(
            process_handle(TestHandle)
                .expect("platform-neutral handle query")
                .principal(),
            &ProcessPrincipal::Posix {
                effective_user_id: 10,
                effective_group_id: 20,
            }
        );
    }
}
