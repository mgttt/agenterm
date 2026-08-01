//! Owned process references that preserve native process-object identity.

use std::{io, time::Duration};

#[cfg(windows)]
use std::os::windows::io::{BorrowedHandle, RawHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessWait {
    Exited,
    TimedOut,
}

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
        self.wait_for_exit(Some(Duration::ZERO))
            .map(|result| result == ProcessWait::TimedOut)
    }

    /// Waits for this exact process object to exit.
    ///
    /// `None` waits indefinitely. A finite timeout is measured monotonically;
    /// native timeout limits and interrupted waits are handled internally.
    pub fn wait_for_exit(&self, timeout: Option<Duration>) -> io::Result<ProcessWait> {
        self.0.wait_for_exit(timeout)
    }

    /// Duplicates an already-open native process reference.
    ///
    /// This avoids reopening a process by PID when the caller already owns a
    /// handle whose object identity must remain stable.
    pub fn duplicate_from(process: impl ProcessReferenceHandle) -> io::Result<Self> {
        process.duplicate_process_reference()
    }

    /// Duplicates a current-process HANDLE into this exact target process.
    ///
    /// The returned transfer rolls the remote HANDLE back if it is dropped.
    /// Call [`RemoteHandleTransfer::into_raw_handle`] only after the target has
    /// successfully received the numeric HANDLE value.
    ///
    /// The retained target HANDLE must include `PROCESS_DUP_HANDLE`. References
    /// duplicated from a process-creation HANDLE normally preserve that right;
    /// [`ProcessReference::open`] intentionally requests only query/wait access,
    /// so this operation can return `PermissionDenied` for such a reference.
    #[cfg(windows)]
    pub fn duplicate_handle_into(
        &self,
        source: BorrowedHandle<'_>,
    ) -> io::Result<RemoteHandleTransfer<'_>> {
        self.0
            .duplicate_handle_into(source)
            .map(RemoteHandleTransfer)
    }
}

/// A duplicated HANDLE that is valid in one retained target process.
///
/// Dropping this value before committing it closes the target-process HANDLE.
/// This is a delivery receipt, not a locally usable or locally owned HANDLE.
#[cfg(windows)]
#[must_use = "dropping an uncommitted remote HANDLE transfer rolls it back"]
pub struct RemoteHandleTransfer<'a>(crate::selected::process_reference::RemoteHandleTransfer<'a>);

#[cfg(windows)]
impl RemoteHandleTransfer<'_> {
    /// Returns the target-process HANDLE value without committing the transfer.
    #[must_use]
    pub fn as_raw_handle(&self) -> RawHandle {
        self.0.as_raw_handle()
    }

    /// Commits delivery and returns the HANDLE value owned by the target process.
    #[must_use]
    pub fn into_raw_handle(self) -> RawHandle {
        self.0.into_raw_handle()
    }
}

/// A native process handle that can be retained as an owned process reference.
///
/// Platform adapters implement this for their native borrowed handle type.
pub trait ProcessReferenceHandle {
    #[doc(hidden)]
    fn duplicate_process_reference(self) -> io::Result<ProcessReference>;

    #[doc(hidden)]
    fn wait_for_process_exit(self, timeout: Option<Duration>) -> io::Result<ProcessWait>;
}

/// Waits through an already-open native process handle without reopening by PID.
pub fn wait_handle(
    process: impl ProcessReferenceHandle,
    timeout: Option<Duration>,
) -> io::Result<ProcessWait> {
    process.wait_for_process_exit(timeout)
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
        assert_eq!(
            reference
                .wait_for_exit(Some(Duration::ZERO))
                .expect("current process zero-time wait"),
            ProcessWait::TimedOut
        );
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
        assert_eq!(
            reference
                .wait_for_exit(Some(Duration::from_millis(5)))
                .expect("bounded live-child wait"),
            ProcessWait::TimedOut
        );
        assert_eq!(
            reference
                .wait_for_exit(Some(Duration::from_secs(5)))
                .expect("child exit wait"),
            ProcessWait::Exited
        );
        assert_eq!(
            reference
                .wait_for_exit(Some(Duration::ZERO))
                .expect("repeated child exit wait"),
            ProcessWait::Exited
        );
        assert!(child.wait().expect("reap child").success());
    }

    #[test]
    fn native_handle_contract_does_not_expose_a_native_type() {
        struct TestHandle;

        impl ProcessReferenceHandle for TestHandle {
            fn duplicate_process_reference(self) -> io::Result<ProcessReference> {
                ProcessReference::open(std::process::id())
            }

            fn wait_for_process_exit(self, _timeout: Option<Duration>) -> io::Result<ProcessWait> {
                Ok(ProcessWait::TimedOut)
            }
        }

        let reference = ProcessReference::duplicate_from(TestHandle).expect("duplicate test ref");
        assert_eq!(reference.id(), std::process::id());
        assert_eq!(
            wait_handle(TestHandle, Some(Duration::ZERO)).expect("generic native handle wait"),
            ProcessWait::TimedOut
        );
    }
}
