//! Owned process references that preserve native process-object identity.

use std::io;

pub struct ProcessReference(pub(crate) crate::selected::process_reference::ProcessReference);

impl ProcessReference {
    pub fn open(process_id: u32) -> io::Result<Self> {
        if process_id == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "process id must be nonzero",
            ));
        }
        crate::selected::process_reference::ProcessReference::open(process_id).map(Self)
    }

    #[must_use]
    pub fn id(&self) -> u32 {
        self.0.id()
    }

    pub fn is_alive(&self) -> io::Result<bool> {
        self.0.is_alive()
    }

    /// Duplicates an already-open native process reference.
    ///
    /// This avoids reopening a process by PID when the caller already owns a
    /// handle whose object identity must remain stable.
    pub fn duplicate_from(process: impl ProcessReferenceHandle) -> io::Result<Self> {
        process.duplicate_process_reference()
    }
}

/// A native process handle that can be retained as an owned process reference.
///
/// Platform adapters implement this for their native borrowed handle type.
pub trait ProcessReferenceHandle {
    #[doc(hidden)]
    fn duplicate_process_reference(self) -> io::Result<ProcessReference>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Command, thread, time::Duration};

    const CHILD_ENV: &str = "AGENTERM_PLATFORM_PROCESS_REFERENCE_CHILD";

    #[test]
    fn current_process_reference_is_live_and_stable() {
        let reference = ProcessReference::open(std::process::id()).expect("current process ref");
        assert_eq!(reference.id(), std::process::id());
        assert!(reference.is_alive().expect("current process liveness"));
    }

    #[test]
    fn process_reference_child() {
        if std::env::var_os(CHILD_ENV).is_some() {
            thread::sleep(Duration::from_millis(250));
        }
    }

    #[test]
    fn reference_observes_the_exact_child_exit() {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "process_reference::tests::process_reference_child",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .spawn()
            .expect("spawn reference child");
        let reference = ProcessReference::open(child.id()).expect("child process ref");
        assert_eq!(reference.id(), child.id());
        assert!(reference.is_alive().expect("live child"));
        assert!(child.wait().expect("wait child").success());

        for _ in 0..100 {
            if !reference.is_alive().expect("exited child observation") {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("owned process reference did not observe child exit");
    }

    #[test]
    fn native_handle_contract_does_not_expose_a_native_type() {
        struct TestHandle;

        impl ProcessReferenceHandle for TestHandle {
            fn duplicate_process_reference(self) -> io::Result<ProcessReference> {
                ProcessReference::open(std::process::id())
            }
        }

        let reference = ProcessReference::duplicate_from(TestHandle).expect("duplicate test ref");
        assert_eq!(reference.id(), std::process::id());
    }
}
