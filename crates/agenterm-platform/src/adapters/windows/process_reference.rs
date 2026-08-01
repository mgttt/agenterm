use std::{
    io,
    os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle as _, OwnedHandle},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, DuplicateHandle, WAIT_FAILED, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    System::Threading::{
        GetCurrentProcess, GetProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        WaitForSingleObject,
    },
};

use crate::process_reference::{ProcessExitCodeHandle, ProcessReferenceHandle, ProcessWait};

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

    pub(crate) fn duplicate_handle_into<'a>(
        &'a self,
        source: BorrowedHandle<'_>,
    ) -> io::Result<RemoteHandleTransfer<'a>> {
        let mut remote = std::ptr::null_mut();
        if unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                source.as_raw_handle(),
                self.handle.as_raw_handle(),
                &raw mut remote,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(RemoteHandleTransfer {
            target: self,
            remote,
            committed: false,
        })
    }
}

pub struct RemoteHandleTransfer<'a> {
    target: &'a ProcessReference,
    remote: std::os::windows::io::RawHandle,
    committed: bool,
}

impl RemoteHandleTransfer<'_> {
    pub const fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.remote
    }

    pub fn into_raw_handle(mut self) -> std::os::windows::io::RawHandle {
        self.committed = true;
        self.remote
    }
}

/// Duplicates a current-process HANDLE into an exact retained target process.
///
/// Dropping the returned receipt before committing it closes the remote HANDLE.
pub fn duplicate_handle_into<'a>(
    target: &'a crate::process_reference::ProcessReference,
    source: BorrowedHandle<'_>,
) -> io::Result<RemoteHandleTransfer<'a>> {
    target.0.duplicate_handle_into(source)
}

impl Drop for RemoteHandleTransfer<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut local = std::ptr::null_mut();
        if unsafe {
            DuplicateHandle(
                self.target.handle.as_raw_handle(),
                self.remote,
                GetCurrentProcess(),
                &raw mut local,
                0,
                0,
                DUPLICATE_CLOSE_SOURCE | DUPLICATE_SAME_ACCESS,
            )
        } != 0
        {
            drop(unsafe { OwnedHandle::from_raw_handle(local) });
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

    fn wait_for_process_exit(self, timeout: Option<Duration>) -> io::Result<ProcessWait> {
        wait(self.as_raw_handle(), timeout)
    }
}

impl ProcessExitCodeHandle for BorrowedHandle<'_> {
    fn process_exit_code(self) -> io::Result<u32> {
        let mut code = 0;
        if unsafe {
            windows_sys::Win32::System::Threading::GetExitCodeProcess(
                self.as_raw_handle(),
                &raw mut code,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(code)
    }
}

impl crate::process_reference::ProcessContainmentGroup for BorrowedHandle<'_> {
    fn contains_process(
        self,
        process: &crate::process_reference::ProcessReference,
    ) -> io::Result<bool> {
        let mut contained = 0;
        if unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                process.as_raw_handle(),
                self.as_raw_handle(),
                &raw mut contained,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(contained != 0)
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
    use std::{
        io::{BufRead as _, Write as _},
        os::windows::io::RawHandle,
        process::{Command, Stdio},
    };
    use windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS;
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};

    const REMOTE_HANDLE_CHILD_ENV: &str = "AGENTERM_PLATFORM_REMOTE_HANDLE_CHILD";
    const CONTAINMENT_CHILD_ENV: &str = "AGENTERM_PLATFORM_CONTAINMENT_CHILD";

    #[test]
    fn current_process_handle_can_be_retained() {
        let handle = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess() as RawHandle) };
        let reference = crate::process_reference::ProcessReference::duplicate_from(handle)
            .expect("duplicate current process handle");
        assert_eq!(reference.id(), std::process::id());
        assert!(reference.is_alive().expect("current process liveness"));
    }

    #[test]
    fn retained_process_handle_reports_the_raw_exit_code() {
        let mut child = Command::new("cmd.exe")
            .args(["/d", "/c", "exit", "37"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn exit-code child");
        let reference =
            crate::process_reference::ProcessReference::duplicate_from(child.as_handle())
                .expect("retain exit-code child");
        let _ = child.wait().expect("reap exit-code child");
        assert_eq!(
            crate::process_reference::exit_code_handle(reference.as_handle())
                .expect("read raw process exit code"),
            37
        );
    }

    #[test]
    fn remote_handle_child() {
        if std::env::var_os(REMOTE_HANDLE_CHILD_ENV).is_none() {
            return;
        }
        let mut line = String::new();
        if std::io::stdin()
            .lock()
            .read_line(&mut line)
            .expect("read remote HANDLE")
            == 0
        {
            return;
        }
        let value = line.trim().parse::<usize>().expect("parse remote HANDLE");
        let process_id = unsafe { GetProcessId(value as RawHandle) };
        assert_ne!(process_id, 0, "GetProcessId failed");
        println!("remote-process-id={process_id}");
    }

    #[test]
    fn containment_child() {
        if std::env::var_os(CONTAINMENT_CHILD_ENV).is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn retained_process_membership_tracks_the_selected_job_object() {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        assert!(
            !job.is_null(),
            "CreateJobObjectW failed: {}",
            io::Error::last_os_error()
        );
        let job = unsafe { OwnedHandle::from_raw_handle(job) };
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "selected::process_reference::tests::containment_child",
                "--nocapture",
            ])
            .env(CONTAINMENT_CHILD_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn containment child");
        let reference =
            crate::process_reference::ProcessReference::duplicate_from(child.as_handle())
                .expect("retain containment child");
        assert!(
            !reference
                .is_member_of(job.as_handle())
                .expect("query membership before assignment")
        );
        assert_ne!(
            unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) },
            0,
            "AssignProcessToJobObject failed: {}",
            io::Error::last_os_error()
        );
        assert!(
            reference
                .is_member_of(job.as_handle())
                .expect("query membership after assignment")
        );
        child.kill().expect("terminate containment child");
        let _ = child.wait().expect("reap containment child");
    }

    #[test]
    fn remote_handle_transfer_rolls_back_until_committed() {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "selected::process_reference::tests::remote_handle_child",
                "--nocapture",
            ])
            .env(REMOTE_HANDLE_CHILD_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn remote HANDLE child");
        let reference =
            crate::process_reference::ProcessReference::duplicate_from(child.as_handle())
                .expect("retain child process HANDLE");

        let current = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess() as RawHandle) };
        let transfer = reference
            .duplicate_handle_into(current)
            .expect("duplicate rollback fixture");
        let stale_remote = transfer.as_raw_handle();
        drop(transfer);
        let mut unexpected = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                DuplicateHandle(
                    child.as_raw_handle(),
                    stale_remote,
                    GetCurrentProcess(),
                    &raw mut unexpected,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            },
            0,
            "dropped transfer left a live target-process HANDLE"
        );

        let transfer = reference
            .duplicate_handle_into(current)
            .expect("duplicate delivered fixture");
        writeln!(
            child.stdin.as_mut().expect("child stdin"),
            "{}",
            transfer.as_raw_handle() as usize
        )
        .expect("deliver remote HANDLE value");
        let _remote = transfer.into_raw_handle();
        let output = child.wait_with_output().expect("reap remote HANDLE child");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("child stdout UTF-8");
        assert!(
            stdout.contains(&format!("remote-process-id={}", std::process::id())),
            "child did not observe the duplicated current-process object: {stdout}"
        );
    }
}
