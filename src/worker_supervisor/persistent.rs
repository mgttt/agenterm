use std::{
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::script_protocol::{
    SCRIPT_FRAME_VERSION, ScriptBrokerRequest, ScriptBrokerResponse, ScriptExitClass,
    ScriptFailureCategory, ScriptFrame, ScriptFramePayload, ScriptFrameRead, ScriptInvocation,
    read_script_frame,
};

use super::{
    ConcurrencyPermit, SupervisedResult, SupervisorError, platform, try_acquire_permit, write_frame,
};

/// Bounds the worker-side frame tracker, completed-invocation set, and retained
/// join handles. A replacement is explicit once this many invocations have run.
pub(crate) const PERSISTENT_WORKER_INVOCATION_LIMIT: usize = 32;

#[derive(Debug)]
pub(crate) enum PersistentWorkerError {
    Supervisor(SupervisorError),
    Busy {
        worker_pid: u32,
    },
    InvocationLimit {
        worker_pid: u32,
        limit: usize,
    },
    WorkerEof {
        worker_pid: u32,
        exit_code: Option<i32>,
    },
    WorkerCrash {
        worker_pid: u32,
        exit_code: Option<i32>,
    },
    Unavailable {
        worker_pid: u32,
    },
}

impl From<SupervisorError> for PersistentWorkerError {
    fn from(error: SupervisorError) -> Self {
        Self::Supervisor(error)
    }
}

enum ReaderEvent {
    Frame(Box<ScriptFrame>),
    Rejected(SupervisorError),
    Eof,
}

/// One bounded foreground `--framed-worker` process.
///
/// This object never restarts itself and never replays an invocation. Callers
/// must explicitly shut it down and construct a replacement after EOF, crash,
/// hard timeout, protocol failure, or invocation-limit exhaustion.
pub(crate) struct PersistentWorkerClient {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    responses: mpsc::Receiver<ReaderEvent>,
    reader: Option<thread::JoinHandle<()>>,
    tree: Option<platform::ProcessTreeGuard>,
    _permit: Option<ConcurrencyPermit>,
    worker_pid: u32,
    invocation_count: usize,
    active: bool,
    unavailable: bool,
}

impl PersistentWorkerClient {
    pub(crate) fn spawn(
        executable: &Path,
        working_directory: Option<&Path>,
    ) -> Result<Self, PersistentWorkerError> {
        let permit = try_acquire_permit()?;
        let mut command = Command::new(executable);
        command
            .arg("--framed-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        platform::configure_worker_command(&mut command)
            .map_err(|error| SupervisorError::Spawn(error.message))?;
        if let Some(working_directory) = working_directory {
            command.current_dir(working_directory);
        }
        let mut child = command
            .spawn()
            .map_err(|error| SupervisorError::Spawn(error.to_string()))?;
        let worker_pid = child.id();
        let tree = platform::ProcessTreeGuard::attach(&child).map_err(|error| {
            platform::terminate_worker(&mut child, worker_pid);
            SupervisorError::Spawn(error.message)
        })?;
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                platform::terminate_worker(&mut child, worker_pid);
                return Err(SupervisorError::Transport(
                    "persistent worker stdin pipe is unavailable".to_owned(),
                )
                .into());
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                platform::terminate_worker(&mut child, worker_pid);
                return Err(SupervisorError::Transport(
                    "persistent worker stdout pipe is unavailable".to_owned(),
                )
                .into());
            }
        };
        let (sender, responses) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut stdout = stdout;
            loop {
                let event = match read_script_frame(&mut stdout) {
                    Ok(ScriptFrameRead::Frame(frame)) => {
                        if frame.frame_version != SCRIPT_FRAME_VERSION {
                            ReaderEvent::Rejected(SupervisorError::Protocol(format!(
                                "worker returned frame version {}, expected {SCRIPT_FRAME_VERSION}",
                                frame.frame_version
                            )))
                        } else {
                            ReaderEvent::Frame(frame)
                        }
                    }
                    Ok(ScriptFrameRead::Rejected(rejection)) => {
                        ReaderEvent::Rejected(SupervisorError::Protocol(format!(
                            "worker returned rejected frame {}: {}",
                            rejection.code, rejection.message
                        )))
                    }
                    Ok(ScriptFrameRead::Eof) => ReaderEvent::Eof,
                    Err(error) => ReaderEvent::Rejected(SupervisorError::Transport(format!(
                        "failed to read persistent worker frame: {error}"
                    ))),
                };
                let terminal = matches!(event, ReaderEvent::Rejected(_) | ReaderEvent::Eof);
                if sender.send(event).is_err() || terminal {
                    break;
                }
            }
        });
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            responses,
            reader: Some(reader),
            tree: Some(tree),
            _permit: Some(permit),
            worker_pid,
            invocation_count: 0,
            active: false,
            unavailable: false,
        })
    }

    pub(crate) fn worker_pid(&self) -> u32 {
        self.worker_pid
    }

    pub(crate) fn invocation_count(&self) -> usize {
        self.invocation_count
    }

    pub(crate) fn invoke<F>(
        &mut self,
        invocation: ScriptInvocation,
        deadline: Duration,
        cancel_grace: Duration,
        mut broker: F,
    ) -> Result<SupervisedResult, PersistentWorkerError>
    where
        F: FnMut(&ScriptBrokerRequest, Duration) -> ScriptBrokerResponse,
    {
        if self.unavailable || self.child.is_none() {
            return Err(PersistentWorkerError::Unavailable {
                worker_pid: self.worker_pid,
            });
        }
        if self.active {
            return Err(PersistentWorkerError::Busy {
                worker_pid: self.worker_pid,
            });
        }
        if self.invocation_count >= PERSISTENT_WORKER_INVOCATION_LIMIT {
            return Err(PersistentWorkerError::InvocationLimit {
                worker_pid: self.worker_pid,
                limit: PERSISTENT_WORKER_INVOCATION_LIMIT,
            });
        }
        let status = self
            .child
            .as_mut()
            .expect("available persistent worker has a child")
            .try_wait();
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                self.force_terminate();
                return Err(SupervisorError::Transport(format!(
                    "failed to query persistent worker {}: {error}",
                    self.worker_pid
                ))
                .into());
            }
        };
        if status.is_some() {
            return Err(self.finish_eof());
        }

        self.active = true;
        self.invocation_count += 1;
        let result = self.invoke_active(invocation, deadline, cancel_grace, &mut broker);
        self.active = false;
        if matches!(
            &result,
            Err(PersistentWorkerError::Supervisor(
                SupervisorError::Transport(_)
                    | SupervisorError::Protocol(_)
                    | SupervisorError::HardTimeout { .. }
                    | SupervisorError::WorkerCrash { .. }
            )) | Err(
                PersistentWorkerError::WorkerEof { .. } | PersistentWorkerError::WorkerCrash { .. }
            )
        ) {
            self.unavailable = true;
        }
        result
    }

    fn invoke_active<F>(
        &mut self,
        invocation: ScriptInvocation,
        deadline: Duration,
        cancel_grace: Duration,
        broker: &mut F,
    ) -> Result<SupervisedResult, PersistentWorkerError>
    where
        F: FnMut(&ScriptBrokerRequest, Duration) -> ScriptBrokerResponse,
    {
        let invocation_id = invocation.invocation_id.clone();
        let broker_request_limit = invocation.budgets.broker_requests;
        let broker_return_limit = invocation.budgets.broker_return_bytes;
        let frame_id = format!("persistent-{}-{invocation_id}", self.invocation_count);
        let frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: frame_id.clone(),
            payload: ScriptFramePayload::Invoke(invocation),
        };
        if let Err(error) = write_frame(
            self.stdin
                .as_mut()
                .expect("available persistent worker has stdin"),
            &frame,
        ) {
            self.force_terminate();
            return Err(error.into());
        }

        let started = Instant::now();
        let mut cancel_requested = false;
        let mut cancel_deadline = None;
        let mut broker_operation_ids = Vec::new();
        let mut broker_request_ids = std::collections::HashSet::new();
        let mut broker_requests = 0_usize;
        loop {
            let wait = cancel_deadline.map_or_else(
                || deadline.saturating_sub(started.elapsed()),
                |deadline: Instant| deadline.saturating_duration_since(Instant::now()),
            );
            let event = match self.responses.recv_timeout(wait) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(self.finish_eof()),
                Err(mpsc::RecvTimeoutError::Timeout) if cancel_requested => {
                    self.force_terminate();
                    return Err(SupervisorError::HardTimeout {
                        worker_pid: self.worker_pid,
                    }
                    .into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    cancel_requested = true;
                    cancel_deadline = Instant::now().checked_add(cancel_grace);
                    let cancel = ScriptFrame {
                        frame_version: SCRIPT_FRAME_VERSION,
                        frame_id: format!("persistent-cancel-{}", self.invocation_count),
                        payload: ScriptFramePayload::Cancel {
                            invocation_id: invocation_id.clone(),
                        },
                    };
                    if let Err(error) = write_frame(
                        self.stdin
                            .as_mut()
                            .expect("available persistent worker has stdin"),
                        &cancel,
                    ) {
                        self.force_terminate();
                        return Err(error.into());
                    }
                    continue;
                }
            };
            let frame = match event {
                ReaderEvent::Frame(frame) => *frame,
                ReaderEvent::Rejected(error) => {
                    self.force_terminate();
                    return Err(error.into());
                }
                ReaderEvent::Eof => return Err(self.finish_eof()),
            };
            match frame.payload {
                ScriptFramePayload::BrokerRequest {
                    invocation_id: request_invocation_id,
                    request_id,
                    request,
                } => {
                    if request_invocation_id != invocation_id {
                        self.force_terminate();
                        return Err(SupervisorError::Protocol(
                            "persistent worker broker request used a mismatched invocation_id"
                                .to_owned(),
                        )
                        .into());
                    }
                    broker_requests += 1;
                    if broker_requests > broker_request_limit
                        || !broker_request_ids.insert(request_id.clone())
                    {
                        self.force_terminate();
                        return Err(SupervisorError::Protocol(
                            "persistent worker exceeded or reused its broker request budget"
                                .to_owned(),
                        )
                        .into());
                    }
                    let remaining = deadline.saturating_sub(started.elapsed());
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
                    let mut response = broker(&request, remaining);
                    if serde_json::to_vec(&response)
                        .map(|bytes| bytes.len())
                        .unwrap_or(usize::MAX)
                        > broker_return_limit
                    {
                        response = ScriptBrokerResponse {
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
                        frame_id: format!(
                            "persistent-response-{}-{request_id}",
                            self.invocation_count
                        ),
                        payload: ScriptFramePayload::BrokerResponse {
                            invocation_id: invocation_id.clone(),
                            request_id,
                            response,
                        },
                    };
                    if let Err(error) = write_frame(
                        self.stdin
                            .as_mut()
                            .expect("available persistent worker has stdin"),
                        &response_frame,
                    ) {
                        self.force_terminate();
                        return Err(error.into());
                    }
                }
                ScriptFramePayload::Result(mut result) => {
                    if frame.frame_id != frame_id || result.invocation_id != invocation_id {
                        self.force_terminate();
                        return Err(SupervisorError::Protocol(
                            "persistent worker returned a mismatched result identity".to_owned(),
                        )
                        .into());
                    }
                    if cancel_requested
                        && result
                            .failure
                            .as_ref()
                            .is_some_and(|failure| failure.code == "limit_cancelled")
                        && let Some(failure) = result.failure.as_mut()
                    {
                        failure.code = "limit_wall_time".to_owned();
                        failure.message = "host deadline reached; persistent worker stopped during cooperative cancellation".to_owned();
                        failure.category = ScriptFailureCategory::Limit;
                        result.exit_class = ScriptExitClass::Limit;
                    }
                    return Ok(SupervisedResult {
                        result,
                        worker_pid: self.worker_pid,
                        cancel_requested,
                        broker_operation_ids,
                    });
                }
                _ => {
                    self.force_terminate();
                    return Err(SupervisorError::Protocol(
                        "persistent worker returned an unexpected frame kind".to_owned(),
                    )
                    .into());
                }
            }
        }
    }

    pub(crate) fn shutdown(mut self) -> Result<(), PersistentWorkerError> {
        self.stdin.take();
        let status = self.wait_and_reap()?;
        if status.success() {
            Ok(())
        } else {
            Err(PersistentWorkerError::WorkerCrash {
                worker_pid: self.worker_pid,
                exit_code: status.code(),
            })
        }
    }

    fn finish_eof(&mut self) -> PersistentWorkerError {
        self.unavailable = true;
        match self.wait_and_reap() {
            Ok(status) if status.success() => PersistentWorkerError::WorkerEof {
                worker_pid: self.worker_pid,
                exit_code: status.code(),
            },
            Ok(status) => PersistentWorkerError::WorkerCrash {
                worker_pid: self.worker_pid,
                exit_code: status.code(),
            },
            Err(error) => error,
        }
    }

    fn wait_and_reap(&mut self) -> Result<ExitStatus, PersistentWorkerError> {
        self.stdin.take();
        let status = self
            .child
            .take()
            .expect("persistent worker wait requires a child")
            .wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.tree.take();
        self._permit.take();
        status.map_err(|error| {
            SupervisorError::Transport(format!(
                "failed to wait for persistent worker {}: {error}",
                self.worker_pid
            ))
            .into()
        })
    }

    fn force_terminate(&mut self) {
        self.stdin.take();
        if let Some(tree) = self.tree.as_mut() {
            let _ = tree.terminate(124);
        }
        if let Some(child) = self.child.as_mut() {
            platform::terminate_worker(child, self.worker_pid);
        }
        self.child.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.tree.take();
        self._permit.take();
        self.unavailable = true;
    }

    #[cfg(test)]
    fn force_worker_exit_for_test(&mut self) {
        if let Some(tree) = self.tree.as_mut() {
            let _ = tree.terminate(91);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self
            .child
            .as_mut()
            .is_some_and(|child| child.try_wait().is_ok_and(|status| status.is_none()))
        {
            assert!(
                Instant::now() < deadline,
                "forced persistent worker did not exit"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[cfg(test)]
    fn process_reaped_for_test(&self) -> bool {
        self.child.is_none()
    }
}

impl Drop for PersistentWorkerClient {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.force_terminate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_protocol::{
        SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, ScriptBudgets, ScriptOperation, ScriptProfile,
    };

    fn worker_executable() -> std::path::PathBuf {
        if let Some(path) = std::env::var_os("AGENTERM_TEST_SCRIPT_WORKER") {
            return path.into();
        }
        let suffix = std::env::consts::EXE_SUFFIX;
        let current = std::env::current_exe().expect("current test executable");
        let target = current
            .parent()
            .and_then(Path::parent)
            .expect("target profile directory")
            .join(format!("agenterm-script{suffix}"));
        assert!(
            target.is_file(),
            "build agenterm-script first or set AGENTERM_TEST_SCRIPT_WORKER: {}",
            target.display()
        );
        target
    }

    fn invocation(id: &str, source: &str) -> ScriptInvocation {
        ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: id.to_owned(),
            api_version: SCRIPT_API_VERSION,
            operation: ScriptOperation::Eval,
            profile: ScriptProfile::Local,
            source_label: "persistent-worker-test".to_owned(),
            source: source.to_owned(),
            project_root: None,
            invocation_temp_root: None,
            arguments: Vec::new(),
            budgets: ScriptBudgets::default(),
            observation: None,
        }
    }

    fn no_broker(_: &ScriptBrokerRequest, _: Duration) -> ScriptBrokerResponse {
        ScriptBrokerResponse {
            ok: false,
            value: None,
            error: Some(crate::script_protocol::ScriptBrokerError {
                code: "unexpected_broker_request".to_owned(),
                message: "test invocation does not use the broker".to_owned(),
                details: None,
            }),
        }
    }

    #[test]
    fn same_pid_reuse_crash_replacement_and_reap_are_explicit() {
        let _test_guard = super::super::PROCESS_TEST_LOCK
            .lock()
            .expect("process test lock");
        let executable = worker_executable();
        let mut first = PersistentWorkerClient::spawn(&executable, None).expect("first worker");
        let first_pid = first.worker_pid();
        let one = first
            .invoke(
                invocation("persistent-one", "40 + 1"),
                Duration::from_secs(5),
                Duration::from_millis(150),
                no_broker,
            )
            .expect("first invocation");
        let two = first
            .invoke(
                invocation("persistent-two", "40 + 2"),
                Duration::from_secs(5),
                Duration::from_millis(150),
                no_broker,
            )
            .expect("second invocation");
        assert_eq!(one.worker_pid, first_pid);
        assert_eq!(two.worker_pid, first_pid);
        assert_eq!(one.result.value, Some(serde_json::json!(41)));
        assert_eq!(two.result.value, Some(serde_json::json!(42)));
        assert_eq!(first.invocation_count(), 2);

        first.force_worker_exit_for_test();
        let failure = first
            .invoke(
                invocation("persistent-after-kill", "43"),
                Duration::from_secs(5),
                Duration::from_millis(150),
                no_broker,
            )
            .expect_err("killed worker must fail explicitly");
        assert!(matches!(
            failure,
            PersistentWorkerError::WorkerCrash {
                worker_pid,
                exit_code: _
            } if worker_pid == first_pid
        ));
        assert!(first.process_reaped_for_test());
        drop(first);

        let mut replacement =
            PersistentWorkerClient::spawn(&executable, None).expect("replacement worker");
        let replacement_pid = replacement.worker_pid();
        assert_ne!(replacement_pid, first_pid);
        let recovered = replacement
            .invoke(
                invocation("persistent-replacement", "6 * 7"),
                Duration::from_secs(5),
                Duration::from_millis(150),
                no_broker,
            )
            .expect("replacement invocation");
        assert_eq!(recovered.worker_pid, replacement_pid);
        assert_eq!(recovered.result.value, Some(serde_json::json!(42)));
        for index in 2..=PERSISTENT_WORKER_INVOCATION_LIMIT {
            replacement
                .invoke(
                    invocation(&format!("persistent-bounded-{index}"), "42"),
                    Duration::from_secs(5),
                    Duration::from_millis(150),
                    no_broker,
                )
                .expect("bounded replacement invocation");
        }
        assert_eq!(
            replacement.invocation_count(),
            PERSISTENT_WORKER_INVOCATION_LIMIT
        );
        assert!(matches!(
            replacement.invoke(
                invocation("persistent-over-limit", "42"),
                Duration::from_secs(5),
                Duration::from_millis(150),
                no_broker,
            ),
            Err(PersistentWorkerError::InvocationLimit {
                worker_pid,
                limit: PERSISTENT_WORKER_INVOCATION_LIMIT
            }) if worker_pid == replacement_pid
        ));
        replacement
            .shutdown()
            .expect("explicit replacement shutdown");

        let dropped = PersistentWorkerClient::spawn(&executable, None).expect("drop-owned worker");
        let dropped_pid = dropped.worker_pid();
        drop(dropped);
        assert!(matches!(
            agenterm_platform::process::observe(dropped_pid),
            agenterm_platform::contract::process::ProcessObservation::Dead { .. }
        ));
    }
}
