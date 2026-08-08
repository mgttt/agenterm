use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::AtomicUsize,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::script_protocol::{
    SCRIPT_FRAME_MAX_BYTES, SCRIPT_FRAME_VERSION, ScriptBrokerRequest, ScriptBrokerResponse,
    ScriptExitClass, ScriptFailureCategory, ScriptFrame, ScriptFramePayload, ScriptInvocation,
    ScriptResult,
};

use crate::platform::services::supervisor_audit as platform;
use platform::ConcurrencyPermit;

#[allow(dead_code)] // Foreground REPL integration is the next dependent leaf.
pub(crate) mod persistent;

pub(crate) const PROCESS_CONCURRENCY_LIMIT: usize = 2;
pub(crate) const GLOBAL_CONCURRENCY_LIMIT: usize = 8;
pub(crate) static PROCESS_ACTIVE: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
pub(crate) static PROCESS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    pub(crate) cancel_requested: bool,
    pub(crate) broker_operation_ids: Vec<String>,
}

pub(crate) struct WorkerSupervisor;

impl WorkerSupervisor {
    pub(crate) fn invoke<F>(
        executable: &Path,
        working_directory: Option<&Path>,
        invocation: ScriptInvocation,
        deadline: Duration,
        cancel_grace: Duration,
        mut broker: F,
    ) -> Result<SupervisedResult, SupervisorError>
    where
        F: FnMut(&ScriptBrokerRequest, Duration) -> ScriptBrokerResponse,
    {
        let _permit = try_acquire_permit()?;
        let mut command = Command::new(executable);
        command
            .arg("--framed-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        platform::configure_worker_command(&mut command)
            .map_err(|error| SupervisorError::Spawn(error.message))?;
        configure_script_backend(&mut command);
        if let Some(working_directory) = working_directory {
            command.current_dir(working_directory);
        }
        let mut child = command
            .spawn()
            .map_err(|error| SupervisorError::Spawn(error.to_string()))?;
        let worker_pid = child.id();
        let mut tree = platform::ProcessTreeGuard::attach(&child).map_err(|error| {
            platform::terminate_worker(&mut child, worker_pid);
            SupervisorError::Spawn(error.message)
        })?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            SupervisorError::Transport("worker stdin pipe is unavailable".to_owned())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            SupervisorError::Transport("worker stdout pipe is unavailable".to_owned())
        })?;
        let invocation_id = invocation.invocation_id.clone();
        let broker_request_limit = invocation.budgets.broker_requests;
        let broker_return_limit = invocation.budgets.broker_return_bytes;
        let invoke_frame_id = format!("invoke-{invocation_id}");
        let invoke_frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: invoke_frame_id.clone(),
            payload: ScriptFramePayload::Invoke(invocation),
        };
        write_frame(&mut stdin, &invoke_frame)?;

        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut stdout = stdout;
            loop {
                let response = read_frame(&mut stdout);
                let terminal = match response.as_ref() {
                    Ok(frame) => matches!(frame.payload, ScriptFramePayload::Result(_)),
                    Err(_) => true,
                };
                if sender.send(response).is_err() || terminal {
                    break;
                }
            }
        });
        let mut cancel_requested = false;
        let started = Instant::now();
        let mut broker_operation_ids = Vec::new();
        let mut broker_request_ids = std::collections::HashSet::new();
        let mut broker_requests = 0_usize;
        let response = loop {
            let remaining = deadline.saturating_sub(started.elapsed());
            let response = match receiver.recv_timeout(remaining) {
                Ok(response) => response,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let status = child.try_wait().ok().flatten();
                    platform::terminate_worker(&mut child, worker_pid);
                    let _ = reader.join();
                    return Err(SupervisorError::WorkerCrash {
                        worker_pid,
                        exit_code: status.and_then(|status| status.code()),
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    cancel_requested = true;
                    let cancel = ScriptFrame {
                        frame_version: SCRIPT_FRAME_VERSION,
                        frame_id: format!("cancel-{invocation_id}"),
                        payload: ScriptFramePayload::Cancel {
                            invocation_id: invocation_id.clone(),
                        },
                    };
                    let _ = write_frame(&mut stdin, &cancel);
                    match receiver.recv_timeout(cancel_grace) {
                        Ok(response) => break response,
                        Err(_) => {
                            let _ = tree.terminate(124);
                            platform::terminate_worker(&mut child, worker_pid);
                            let _ = reader.join();
                            return Err(SupervisorError::HardTimeout { worker_pid });
                        }
                    }
                }
            };
            let frame = match response {
                Ok(frame) => frame,
                Err(error) => break Err(error),
            };
            match frame.payload {
                ScriptFramePayload::BrokerRequest {
                    invocation_id: request_invocation_id,
                    request_id,
                    request,
                } => {
                    if request_invocation_id != invocation_id {
                        break Err(SupervisorError::Protocol(
                            "worker broker request used a mismatched invocation_id".to_owned(),
                        ));
                    }
                    broker_requests += 1;
                    if broker_requests > broker_request_limit {
                        break Err(SupervisorError::Protocol(
                            "worker exceeded the broker request budget".to_owned(),
                        ));
                    }
                    if !broker_request_ids.insert(request_id.clone()) {
                        break Err(SupervisorError::Protocol(
                            "worker reused a broker request_id".to_owned(),
                        ));
                    }
                    let operation = if request.operation == "fleet.call" {
                        request
                            .arguments
                            .get("operation_id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("fleet.call.invalid")
                            .to_owned()
                    } else {
                        request.operation.clone()
                    };
                    let remaining = deadline.saturating_sub(started.elapsed());
                    if remaining.is_zero() {
                        continue;
                    }
                    let mut broker_response = broker(&request, remaining);
                    if started.elapsed() >= deadline {
                        continue;
                    }
                    let encoded_len = serde_json::to_vec(&broker_response)
                        .map(|bytes| bytes.len())
                        .unwrap_or(usize::MAX);
                    if encoded_len > broker_return_limit {
                        broker_response = ScriptBrokerResponse {
                            ok: false,
                            value: None,
                            error: Some(crate::script_protocol::ScriptBrokerError {
                                code: "broker_return_too_large".to_owned(),
                                message: format!(
                                    "broker response exceeds the {broker_return_limit} byte budget"
                                ),
                                details: None,
                            }),
                        };
                    }
                    broker_operation_ids.push(operation);
                    let response_frame = ScriptFrame {
                        frame_version: SCRIPT_FRAME_VERSION,
                        frame_id: format!("response-{request_id}"),
                        payload: ScriptFramePayload::BrokerResponse {
                            invocation_id: invocation_id.clone(),
                            request_id,
                            response: broker_response,
                        },
                    };
                    write_frame(&mut stdin, &response_frame)?;
                }
                ScriptFramePayload::Result(_) => break Ok(frame),
                _ => {
                    break Err(SupervisorError::Protocol(
                        "worker returned an unexpected frame kind".to_owned(),
                    ));
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
            failure.category = ScriptFailureCategory::Limit;
            result.exit_class = ScriptExitClass::Limit;
        }
        Ok(SupervisedResult {
            result,
            worker_pid,
            cancel_requested,
            broker_operation_ids,
        })
    }
}

fn configure_script_backend(command: &mut Command) {
    command.env(
        "AGENTERM_SCRIPT_BACKEND",
        script_backend_environment(std::env::var_os("AGENTERM_SCRIPT_BACKEND")),
    );
}

fn script_backend_environment(inherited: Option<OsString>) -> OsString {
    match inherited {
        Some(value) if value == "rhai" => OsString::from("rh"),
        Some(value) => value,
        None => OsString::from("rh"),
    }
}

fn try_acquire_permit() -> Result<ConcurrencyPermit, SupervisorError> {
    platform::ConcurrencyPermit::try_acquire(
        &PROCESS_ACTIVE,
        PROCESS_CONCURRENCY_LIMIT,
        GLOBAL_CONCURRENCY_LIMIT,
    )
    .map_err(|error| match error.kind {
        crate::platform::contract::supervisor_audit::SupervisorAuditErrorKind::LockWait => {
            SupervisorError::ConcurrencyLimit
        }
        _ => SupervisorError::Spawn(error.message),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_backend_defaults_to_rh_and_normalizes_retired_rhai() {
        assert_eq!(script_backend_environment(None), OsString::from("rh"));
        assert_eq!(
            script_backend_environment(Some(OsString::from("rhai"))),
            OsString::from("rh")
        );
        assert_eq!(
            script_backend_environment(Some(OsString::from("lua"))),
            OsString::from("lua")
        );
    }

    #[test]
    fn per_process_concurrency_is_bounded_without_spawning() {
        let _test_guard = PROCESS_TEST_LOCK.lock().expect("process test lock");
        let first = try_acquire_permit().expect("first permit");
        let second = try_acquire_permit().expect("second permit");
        assert!(matches!(
            try_acquire_permit(),
            Err(SupervisorError::ConcurrencyLimit)
        ));
        drop(first);
        assert!(try_acquire_permit().is_ok());
        drop(second);
    }
}
