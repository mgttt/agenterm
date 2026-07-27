use std::{
    ffi::c_void,
    io::{Read, Write},
    mem,
    os::windows::io::AsRawHandle,
    path::Path,
    process::{Child, Command, Stdio},
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
    sync::mpsc,
    thread,
    time::Duration,
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

use crate::script_protocol::{
    SCRIPT_FRAME_MAX_BYTES, SCRIPT_FRAME_VERSION, ScriptFrame, ScriptFramePayload,
    ScriptInvocation, ScriptResult,
};

const PROCESS_CONCURRENCY_LIMIT: usize = 2;
const GLOBAL_CONCURRENCY_LIMIT: usize = 4;
static PROCESS_ACTIVE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub(crate) enum SupervisorError {
    ConcurrencyLimit,
    Spawn(String),
    Transport(String),
    Protocol(String),
    HardTimeout {
        worker_pid: u32,
    },
    WorkerCrash {
        worker_pid: u32,
        exit_code: Option<i32>,
    },
}

#[derive(Debug)]
pub(crate) struct SupervisedResult {
    pub(crate) result: ScriptResult,
    pub(crate) worker_pid: u32,
}

pub(crate) struct WorkerSupervisor;

impl WorkerSupervisor {
    pub(crate) fn invoke(
        executable: &Path,
        invocation: ScriptInvocation,
        deadline: Duration,
        cancel_grace: Duration,
    ) -> Result<SupervisedResult, SupervisorError> {
        let _permit = ConcurrencyPermit::try_acquire()?;
        let mut child = Command::new(executable)
            .arg("--framed-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| SupervisorError::Spawn(error.to_string()))?;
        let worker_pid = child.id();
        let job = Job::new().map_err(|error| {
            let _ = child.kill();
            let _ = child.wait();
            SupervisorError::Spawn(error)
        })?;
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SupervisorError::Spawn(error));
        }

        let mut stdin = child.stdin.take().ok_or_else(|| {
            SupervisorError::Transport("worker stdin pipe is unavailable".to_owned())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            SupervisorError::Transport("worker stdout pipe is unavailable".to_owned())
        })?;
        let invocation_id = invocation.invocation_id.clone();
        let invoke_frame_id = format!("invoke-{invocation_id}");
        let invoke_frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: invoke_frame_id.clone(),
            payload: ScriptFramePayload::Invoke(invocation),
        };
        write_frame(&mut stdin, &invoke_frame)?;

        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let _ = sender.send(read_frame(stdout));
        });
        let response = match receiver.recv_timeout(deadline) {
            Ok(response) => response,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child.try_wait().ok().flatten();
                drop(stdin);
                let _ = child.wait();
                let _ = reader.join();
                return Err(SupervisorError::WorkerCrash {
                    worker_pid,
                    exit_code: status.and_then(|status| status.code()),
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let cancel = ScriptFrame {
                    frame_version: SCRIPT_FRAME_VERSION,
                    frame_id: format!("cancel-{invocation_id}"),
                    payload: ScriptFramePayload::Cancel {
                        invocation_id: invocation_id.clone(),
                    },
                };
                let _ = write_frame(&mut stdin, &cancel);
                match receiver.recv_timeout(cancel_grace) {
                    Ok(response) => response,
                    Err(_) => {
                        let _ = job.terminate(124);
                        drop(stdin);
                        let _ = child.wait();
                        let _ = reader.join();
                        return Err(SupervisorError::HardTimeout { worker_pid });
                    }
                }
            }
        };
        drop(stdin);
        let status = child.wait().map_err(|error| {
            SupervisorError::Transport(format!("failed to wait for worker: {error}"))
        })?;
        let _ = reader.join();
        let frame = match response {
            Ok(frame) => frame,
            Err(error) if status.success() => return Err(error),
            Err(_) => {
                return Err(SupervisorError::WorkerCrash {
                    worker_pid,
                    exit_code: status.code(),
                });
            }
        };
        if frame.frame_id != invoke_frame_id {
            return Err(SupervisorError::Protocol(format!(
                "worker returned frame_id {}, expected {invoke_frame_id}",
                frame.frame_id
            )));
        }
        let mut result = match frame.payload {
            ScriptFramePayload::Result(result) => result,
            _ => {
                return Err(SupervisorError::Protocol(
                    "worker returned a non-result frame".to_owned(),
                ));
            }
        };
        if !status.success() {
            return Err(SupervisorError::WorkerCrash {
                worker_pid,
                exit_code: status.code(),
            });
        }
        if result
            .failure
            .as_ref()
            .is_some_and(|failure| failure.code == "limit_cancelled")
            && let Some(failure) = result.failure.as_mut()
        {
            failure.code = "limit_wall_time".to_owned();
            failure.message =
                "host deadline reached; worker stopped during cooperative cancellation".to_owned();
        }
        Ok(SupervisedResult { result, worker_pid })
    }
}

fn write_frame(output: &mut impl Write, frame: &ScriptFrame) -> Result<(), SupervisorError> {
    let bytes = serde_json::to_vec(frame)
        .map_err(|error| SupervisorError::Transport(format!("failed to encode frame: {error}")))?;
    if bytes.len() > SCRIPT_FRAME_MAX_BYTES as usize {
        return Err(SupervisorError::Protocol(format!(
            "outbound frame exceeds the {SCRIPT_FRAME_MAX_BYTES} byte limit"
        )));
    }
    output
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| output.write_all(&bytes))
        .and_then(|_| output.flush())
        .map_err(|error| SupervisorError::Transport(format!("failed to write frame: {error}")))
}

fn read_frame(mut input: impl Read) -> Result<ScriptFrame, SupervisorError> {
    let mut header = [0_u8; 4];
    input.read_exact(&mut header).map_err(|error| {
        SupervisorError::Transport(format!("failed to read frame header: {error}"))
    })?;
    let length = u32::from_be_bytes(header);
    if length > SCRIPT_FRAME_MAX_BYTES {
        return Err(SupervisorError::Protocol(format!(
            "worker frame length {length} exceeds the {SCRIPT_FRAME_MAX_BYTES} byte limit"
        )));
    }
    let mut bytes = vec![0_u8; length as usize];
    input.read_exact(&mut bytes).map_err(|error| {
        SupervisorError::Transport(format!("failed to read frame payload: {error}"))
    })?;
    let frame: ScriptFrame = serde_json::from_slice(&bytes)
        .map_err(|error| SupervisorError::Protocol(format!("invalid worker frame: {error}")))?;
    if frame.frame_version != SCRIPT_FRAME_VERSION {
        return Err(SupervisorError::Protocol(format!(
            "worker returned frame version {}, expected {SCRIPT_FRAME_VERSION}",
            frame.frame_version
        )));
    }
    Ok(frame)
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

struct ConcurrencyPermit {
    mutex: HANDLE,
}

impl ConcurrencyPermit {
    fn try_acquire() -> Result<Self, SupervisorError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_process_concurrency_is_bounded_without_spawning() {
        let first = ConcurrencyPermit::try_acquire().expect("first permit");
        let second = ConcurrencyPermit::try_acquire().expect("second permit");
        assert!(matches!(
            ConcurrencyPermit::try_acquire(),
            Err(SupervisorError::ConcurrencyLimit)
        ));
        drop(first);
        assert!(ConcurrencyPermit::try_acquire().is_ok());
        drop(second);
    }
}
