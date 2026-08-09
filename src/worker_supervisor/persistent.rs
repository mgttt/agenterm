use std::{
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use crate::script_protocol::{
    ReplResponseValidator, ReplSessionCommand, ReplSessionEvent, ReplSessionQuery,
    ReplSessionRequest, ReplSessionResponse, ReplSessionWireConfig, ReplWireCellResult,
    ReplWireInputState, ReplWireQueryResult, SCRIPT_FRAME_VERSION, ScriptBrokerRequest,
    ScriptBrokerResponse, ScriptExitClass, ScriptFailureCategory, ScriptFrame, ScriptFramePayload,
    ScriptFrameRead, ScriptInvocation, read_script_frame,
};

use super::{
    ConcurrencyPermit, SupervisedResult, SupervisorError, configure_script_backend, platform,
    try_acquire_permit, write_frame,
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

fn write_shared_stdin(
    stdin: &Option<Arc<Mutex<ChildStdin>>>,
    frame: &ScriptFrame,
) -> Result<(), SupervisorError> {
    let mut stdin = stdin
        .as_ref()
        .ok_or_else(|| SupervisorError::Transport("worker stdin is unavailable".to_owned()))?
        .lock()
        .map_err(|_| SupervisorError::Transport("worker stdin lock is poisoned".to_owned()))?;
    write_frame(&mut *stdin, frame)
}

/// One bounded foreground `--framed-worker` process.
///
/// This object never restarts itself and never replays an invocation. Callers
/// must explicitly shut it down and construct a replacement after EOF, crash,
/// hard timeout, protocol failure, or invocation-limit exhaustion.
pub(crate) struct PersistentWorkerClient {
    child: Option<Child>,
    stdin: Option<Arc<Mutex<ChildStdin>>>,
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
            .args(super::SCRIPT_WORKER_ENGINE_ARGS)
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
            stdin: Some(Arc::new(Mutex::new(stdin))),
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
        if let Err(error) = write_shared_stdin(&self.stdin, &frame) {
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
                    if let Err(error) = write_shared_stdin(&self.stdin, &cancel) {
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
                    if let Err(error) = write_shared_stdin(&self.stdin, &response_frame) {
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

const REPL_CANCEL_GRACE: Duration = Duration::from_millis(150);
const REPL_RESPONSE_POLL: Duration = Duration::from_millis(10);
const REPL_CONTROL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const REPL_CLOSE_RESPONSE_TIMEOUT: Duration = Duration::from_millis(750);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FreshSessionReason {
    GenerationLimit,
    WorkerCrash,
    HardInterrupt,
    ProtocolFailure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FreshSessionReceipt {
    pub(crate) reason: FreshSessionReason,
    pub(crate) language_state_fresh: bool,
    pub(crate) history_fresh: bool,
    pub(crate) side_effects_replayed: bool,
    pub(crate) old_worker_pid: u32,
    pub(crate) new_worker_pid: u32,
    pub(crate) old_generation: u64,
    pub(crate) new_generation: u64,
}

#[derive(Debug)]
pub(crate) enum PersistentReplError {
    Worker(PersistentWorkerError),
    Protocol(String),
    Host(String),
    Interrupted {
        worker_pid: u32,
        generation: u64,
        fresh_required: bool,
    },
}

impl From<PersistentWorkerError> for PersistentReplError {
    fn from(error: PersistentWorkerError) -> Self {
        Self::Worker(error)
    }
}

impl From<SupervisorError> for PersistentReplError {
    fn from(error: SupervisorError) -> Self {
        Self::Worker(error.into())
    }
}

#[derive(Debug)]
pub(crate) struct ReplReply<T> {
    pub(crate) value: T,
    pub(crate) fresh_session: Option<FreshSessionReceipt>,
}

struct ReplControlState {
    stdin: Option<Arc<Mutex<ChildStdin>>>,
    session_id: String,
    generation: u64,
    next_sequence: u64,
    active_cell: Option<String>,
    cancel_requested: bool,
}

#[derive(Clone)]
pub(crate) struct PersistentReplControl {
    state: Arc<Mutex<ReplControlState>>,
    wake: mpsc::Sender<()>,
}

impl PersistentReplControl {
    pub(crate) fn cancel(&self) -> Result<bool, PersistentReplError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PersistentReplError::Host("REPL control lock poisoned".to_owned()))?;
        let Some(cell_id) = state.active_cell.clone() else {
            return Ok(false);
        };
        if state.cancel_requested {
            return Ok(true);
        }
        let sequence = state.next_sequence;
        state.next_sequence = sequence.checked_add(1).ok_or_else(|| {
            PersistentReplError::Protocol("protocol_repl_sequence_overflow".to_owned())
        })?;
        let frame = repl_request_frame(
            &state.session_id,
            state.generation,
            sequence,
            ReplSessionCommand::Cancel { cell_id },
        );
        write_shared_stdin(&state.stdin, &frame)?;
        state.cancel_requested = true;
        let _ = self.wake.send(());
        Ok(true)
    }
}

pub(crate) struct PersistentReplClient {
    executable: PathBuf,
    working_directory: Option<PathBuf>,
    config: ReplSessionWireConfig,
    session_id: String,
    generation: u64,
    evaluations: usize,
    worker: Option<PersistentWorkerClient>,
    responses: ReplResponseValidator,
    control_state: Arc<Mutex<ReplControlState>>,
    control_wake_rx: mpsc::Receiver<()>,
    control_wake_tx: mpsc::Sender<()>,
    replacement_required: Option<(FreshSessionReason, u32, u64)>,
}

impl PersistentReplClient {
    pub(crate) fn spawn(
        executable: &Path,
        working_directory: Option<&Path>,
        session_id: String,
        config: ReplSessionWireConfig,
    ) -> Result<Self, PersistentReplError> {
        let (control_wake_tx, control_wake_rx) = mpsc::channel();
        let control_state = Arc::new(Mutex::new(ReplControlState {
            stdin: None,
            session_id: session_id.clone(),
            generation: 1,
            next_sequence: 0,
            active_cell: None,
            cancel_requested: false,
        }));
        let mut client = Self {
            executable: executable.to_path_buf(),
            working_directory: working_directory.map(Path::to_path_buf),
            config,
            session_id,
            generation: 1,
            evaluations: 0,
            worker: None,
            responses: ReplResponseValidator::default(),
            control_state,
            control_wake_rx,
            control_wake_tx,
            replacement_required: None,
        };
        client.open_worker()?;
        Ok(client)
    }

    pub(crate) fn control(&self) -> PersistentReplControl {
        PersistentReplControl {
            state: Arc::clone(&self.control_state),
            wake: self.control_wake_tx.clone(),
        }
    }

    pub(crate) fn worker_pid(&self) -> u32 {
        self.worker
            .as_ref()
            .map(PersistentWorkerClient::worker_pid)
            .unwrap_or(0)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn inspect(
        &mut self,
        source: String,
    ) -> Result<ReplReply<ReplWireInputState>, PersistentReplError> {
        let fresh = self.ensure_worker()?;
        self.send_live(ReplSessionCommand::Inspect { source })?;
        let response = self.next_response(REPL_CONTROL_RESPONSE_TIMEOUT)?;
        match response.event {
            ReplSessionEvent::InputState { state } => Ok(ReplReply {
                value: state,
                fresh_session: fresh,
            }),
            event => {
                Err(self.protocol_failure(format!("unexpected REPL response event: {event:?}")))
            }
        }
    }

    pub(crate) fn query(
        &mut self,
        query: ReplSessionQuery,
    ) -> Result<ReplReply<ReplWireQueryResult>, PersistentReplError> {
        let fresh = self.ensure_worker()?;
        self.send_live(ReplSessionCommand::Query { query })?;
        let response = self.next_response(REPL_CONTROL_RESPONSE_TIMEOUT)?;
        match response.event {
            ReplSessionEvent::QueryResult {
                query: response_query,
                result,
            } if response_query == query => Ok(ReplReply {
                value: result,
                fresh_session: fresh,
            }),
            event => {
                Err(self.protocol_failure(format!("unexpected REPL response event: {event:?}")))
            }
        }
    }

    pub(crate) fn reset(&mut self) -> Result<ReplReply<()>, PersistentReplError> {
        let fresh = self.ensure_worker()?;
        self.send_live(ReplSessionCommand::Reset)?;
        let response = self.next_response(REPL_CONTROL_RESPONSE_TIMEOUT)?;
        match response.event {
            ReplSessionEvent::ResetDone => Ok(ReplReply {
                value: (),
                fresh_session: fresh,
            }),
            event => {
                Err(self.protocol_failure(format!("unexpected REPL response event: {event:?}")))
            }
        }
    }

    pub(crate) fn evaluate<F>(
        &mut self,
        cell_id: String,
        source: String,
        deadline: Duration,
        mut broker: F,
    ) -> Result<ReplReply<ReplWireCellResult>, PersistentReplError>
    where
        F: FnMut(&ScriptBrokerRequest, Duration) -> ScriptBrokerResponse,
    {
        let mut fresh = self.ensure_worker()?;
        if self.evaluations >= PERSISTENT_WORKER_INVOCATION_LIMIT {
            fresh = Some(self.replace(FreshSessionReason::GenerationLimit)?);
        }
        if let Err(error) = self.send_evaluate(cell_id.clone(), source) {
            return match error {
                error @ PersistentReplError::Worker(_) => {
                    self.mark_replacement_required(FreshSessionReason::WorkerCrash);
                    Err(error)
                }
                PersistentReplError::Protocol(message) => Err(self.protocol_failure(message)),
                other => Err(other),
            };
        }
        self.evaluations += 1;
        let started = Instant::now();
        let mut cancel_started = None;
        let mut broker_request_ids = std::collections::HashSet::new();
        let mut broker_requests = 0_usize;
        loop {
            let _ = self.control_wake_rx.try_recv();
            let cancel_requested = self
                .control_state
                .lock()
                .map_err(|_| PersistentReplError::Host("REPL control lock poisoned".to_owned()))?
                .cancel_requested;
            if cancel_requested && cancel_started.is_none() {
                cancel_started = Some(Instant::now());
            }
            if cancel_started.is_some_and(|at| at.elapsed() >= REPL_CANCEL_GRACE) {
                let pid = self.worker_pid();
                let generation = self.generation;
                self.hard_interrupt();
                self.replacement_required =
                    Some((FreshSessionReason::HardInterrupt, pid, generation));
                return Err(PersistentReplError::Interrupted {
                    worker_pid: pid,
                    generation,
                    fresh_required: true,
                });
            }
            if started.elapsed() >= deadline && !cancel_requested {
                self.control().cancel()?;
                cancel_started = Some(Instant::now());
            }
            let event = match self
                .worker
                .as_ref()
                .expect("ensured REPL worker")
                .responses
                .recv_timeout(REPL_RESPONSE_POLL)
            {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(self.record_worker_loss());
                }
            };
            let frame = match event {
                ReaderEvent::Frame(frame) => *frame,
                ReaderEvent::Rejected(error) => {
                    self.mark_replacement_required(FreshSessionReason::ProtocolFailure);
                    return Err(error.into());
                }
                ReaderEvent::Eof => return Err(self.record_worker_loss()),
            };
            match frame.payload {
                ScriptFramePayload::BrokerRequest {
                    invocation_id,
                    request_id,
                    request,
                } if invocation_id == cell_id => {
                    broker_requests += 1;
                    if broker_requests > self.config.budgets.broker_requests
                        || !broker_request_ids.insert(request_id.clone())
                    {
                        return Err(self.protocol_failure(
                            "REPL worker exceeded or reused its broker request budget".to_owned(),
                        ));
                    }
                    let remaining = deadline.saturating_sub(started.elapsed());
                    let mut response = broker(&request, remaining);
                    if serde_json::to_vec(&response)
                        .map(|bytes| bytes.len())
                        .unwrap_or(usize::MAX)
                        > self.config.budgets.broker_return_bytes
                    {
                        response = ScriptBrokerResponse {
                            ok: false,
                            value: None,
                            error: Some(crate::script_protocol::ScriptBrokerError {
                                code: "broker_return_too_large".to_owned(),
                                message: "broker response exceeds the REPL return byte budget"
                                    .to_owned(),
                                details: None,
                            }),
                        };
                    }
                    let frame = ScriptFrame {
                        frame_version: SCRIPT_FRAME_VERSION,
                        frame_id: format!("repl-broker-response-{request_id}"),
                        payload: ScriptFramePayload::BrokerResponse {
                            invocation_id,
                            request_id,
                            response,
                        },
                    };
                    let worker = self.worker.as_ref().expect("ensured REPL worker");
                    write_shared_stdin(&worker.stdin, &frame)?;
                }
                ScriptFramePayload::ReplResponse(response) => {
                    if let Err(error) = self.responses.admit(&response) {
                        return Err(
                            self.protocol_failure(format!("{}: {}", error.code, error.message))
                        );
                    }
                    match response.event {
                        ReplSessionEvent::CellStarted {
                            cell_id: response_cell,
                        } if response_cell == cell_id => {}
                        ReplSessionEvent::CellResult {
                            cell_id: response_cell,
                            result,
                        } if response_cell == cell_id => {
                            let mut state = self.control_state.lock().map_err(|_| {
                                PersistentReplError::Host("REPL control lock poisoned".to_owned())
                            })?;
                            state.active_cell = None;
                            state.cancel_requested = false;
                            return Ok(ReplReply {
                                value: result,
                                fresh_session: fresh,
                            });
                        }
                        ReplSessionEvent::Failure { failure } => {
                            return Err(self.protocol_failure(format!(
                                "{}: {}",
                                failure.code, failure.message
                            )));
                        }
                        event => {
                            return Err(self.protocol_failure(format!(
                                "unexpected REPL response event: {event:?}"
                            )));
                        }
                    }
                }
                _ => {
                    return Err(self.protocol_failure(
                        "unexpected worker frame during REPL evaluation".to_owned(),
                    ));
                }
            }
        }
    }

    pub(crate) fn close(mut self) -> Result<(), PersistentReplError> {
        if self.worker.is_some() {
            self.close_current()?;
        }
        Ok(())
    }

    fn send(&self, command: ReplSessionCommand) -> Result<(), PersistentReplError> {
        let mut state = self
            .control_state
            .lock()
            .map_err(|_| PersistentReplError::Host("REPL control lock poisoned".to_owned()))?;
        let sequence = state.next_sequence;
        state.next_sequence = sequence.checked_add(1).ok_or_else(|| {
            PersistentReplError::Protocol("protocol_repl_sequence_overflow".to_owned())
        })?;
        let frame = repl_request_frame(&state.session_id, state.generation, sequence, command);
        write_shared_stdin(&state.stdin, &frame)?;
        Ok(())
    }

    fn send_live(&mut self, command: ReplSessionCommand) -> Result<(), PersistentReplError> {
        match self.send(command) {
            Ok(()) => Ok(()),
            Err(error @ PersistentReplError::Worker(_)) => {
                self.mark_replacement_required(FreshSessionReason::WorkerCrash);
                Err(error)
            }
            Err(PersistentReplError::Protocol(message)) => Err(self.protocol_failure(message)),
            Err(error) => Err(error),
        }
    }

    fn send_evaluate(&self, cell_id: String, source: String) -> Result<(), PersistentReplError> {
        let mut state = self
            .control_state
            .lock()
            .map_err(|_| PersistentReplError::Host("REPL control lock poisoned".to_owned()))?;
        let sequence = state.next_sequence;
        state.next_sequence = sequence.checked_add(1).ok_or_else(|| {
            PersistentReplError::Protocol("protocol_repl_sequence_overflow".to_owned())
        })?;
        state.active_cell = Some(cell_id.clone());
        state.cancel_requested = false;
        let frame = repl_request_frame(
            &state.session_id,
            state.generation,
            sequence,
            ReplSessionCommand::Evaluate { cell_id, source },
        );
        if let Err(error) = write_shared_stdin(&state.stdin, &frame) {
            state.active_cell = None;
            return Err(error.into());
        }
        Ok(())
    }

    fn next_response(
        &mut self,
        timeout: Duration,
    ) -> Result<ReplSessionResponse, PersistentReplError> {
        self.next_response_with_timeout(timeout)
    }

    fn next_response_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<ReplSessionResponse, PersistentReplError> {
        let event = match self
            .worker
            .as_ref()
            .expect("REPL response requires worker")
            .responses
            .recv_timeout(timeout)
        {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(self.record_worker_loss()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let worker_pid = self.worker_pid();
                self.mark_replacement_required(FreshSessionReason::ProtocolFailure);
                return Err(
                    PersistentWorkerError::Supervisor(SupervisorError::HardTimeout { worker_pid })
                        .into(),
                );
            }
        };
        match event {
            ReaderEvent::Frame(frame) => match frame.payload {
                ScriptFramePayload::ReplResponse(response) => {
                    if let Err(error) = self.responses.admit(&response) {
                        return Err(
                            self.protocol_failure(format!("{}: {}", error.code, error.message))
                        );
                    }
                    if let ReplSessionEvent::Failure { failure } = &response.event {
                        return Err(
                            self.protocol_failure(format!("{}: {}", failure.code, failure.message))
                        );
                    }
                    Ok(response)
                }
                _ => Err(self.protocol_failure(
                    "unexpected worker frame outside REPL evaluation".to_owned(),
                )),
            },
            ReaderEvent::Rejected(error) => {
                self.mark_replacement_required(FreshSessionReason::ProtocolFailure);
                Err(error.into())
            }
            ReaderEvent::Eof => Err(self.record_worker_loss()),
        }
    }

    fn open_worker(&mut self) -> Result<(), PersistentReplError> {
        let worker =
            PersistentWorkerClient::spawn(&self.executable, self.working_directory.as_deref())?;
        let pid = worker.worker_pid();
        {
            let mut state = self
                .control_state
                .lock()
                .map_err(|_| PersistentReplError::Host("REPL control lock poisoned".to_owned()))?;
            state.stdin = worker.stdin.as_ref().map(Arc::clone);
            state.generation = self.generation;
            state.next_sequence = 0;
            state.active_cell = None;
            state.cancel_requested = false;
        }
        self.worker = Some(worker);
        self.responses = ReplResponseValidator::default();
        if let Err(error) = self.send(ReplSessionCommand::Open {
            config: self.config.clone(),
        }) {
            self.mark_replacement_required(FreshSessionReason::ProtocolFailure);
            return Err(error);
        }
        let response = self.next_response(REPL_CONTROL_RESPONSE_TIMEOUT)?;
        match response.event {
            ReplSessionEvent::Ready { worker_pid } if worker_pid == pid => Ok(()),
            event => {
                Err(self.protocol_failure(format!("unexpected REPL response event: {event:?}")))
            }
        }
    }

    fn ensure_worker(&mut self) -> Result<Option<FreshSessionReceipt>, PersistentReplError> {
        if let Some((reason, old_pid, old_generation)) = self.replacement_required {
            self.generation = old_generation.checked_add(1).ok_or_else(|| {
                PersistentReplError::Protocol("protocol_repl_generation_overflow".to_owned())
            })?;
            self.evaluations = 0;
            if let Err(error) = self.open_worker() {
                self.replacement_required = Some((reason, old_pid, old_generation));
                return Err(error);
            }
            self.replacement_required = None;
            return Ok(Some(FreshSessionReceipt {
                reason,
                language_state_fresh: true,
                history_fresh: true,
                side_effects_replayed: false,
                old_worker_pid: old_pid,
                new_worker_pid: self.worker_pid(),
                old_generation,
                new_generation: self.generation,
            }));
        }
        Ok(None)
    }

    fn replace(
        &mut self,
        reason: FreshSessionReason,
    ) -> Result<FreshSessionReceipt, PersistentReplError> {
        let old_worker_pid = self.worker_pid();
        let old_generation = self.generation;
        self.close_current()?;
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            PersistentReplError::Protocol("protocol_repl_generation_overflow".to_owned())
        })?;
        self.evaluations = 0;
        self.open_worker()?;
        Ok(FreshSessionReceipt {
            reason,
            language_state_fresh: true,
            history_fresh: true,
            side_effects_replayed: false,
            old_worker_pid,
            new_worker_pid: self.worker_pid(),
            old_generation,
            new_generation: self.generation,
        })
    }

    fn close_current(&mut self) -> Result<(), PersistentReplError> {
        if let Err(error) = self.send(ReplSessionCommand::Close) {
            self.mark_replacement_required(FreshSessionReason::ProtocolFailure);
            return Err(error);
        }
        let response = self.next_response(REPL_CLOSE_RESPONSE_TIMEOUT)?;
        if !matches!(response.event, ReplSessionEvent::Closed) {
            return Err(self.protocol_failure(format!(
                "unexpected REPL response event: {:?}",
                response.event
            )));
        }
        self.control_state
            .lock()
            .map_err(|_| PersistentReplError::Host("REPL control lock poisoned".to_owned()))?
            .stdin = None;
        let mut worker = self.worker.take().expect("closing REPL worker");
        let status = worker.wait_and_reap()?;
        if status.success() {
            Ok(())
        } else {
            Err(PersistentWorkerError::WorkerCrash {
                worker_pid: worker.worker_pid,
                exit_code: status.code(),
            }
            .into())
        }
    }

    fn hard_interrupt(&mut self) {
        self.control_state
            .lock()
            .map(|mut state| {
                state.stdin = None;
                state.active_cell = None;
                state.cancel_requested = false;
            })
            .ok();
        if let Some(worker) = self.worker.as_mut() {
            worker.force_terminate();
        }
        self.worker = None;
    }

    fn mark_replacement_required(&mut self, reason: FreshSessionReason) {
        let pid = self.worker_pid();
        let generation = self.generation;
        self.hard_interrupt();
        self.replacement_required = Some((reason, pid, generation));
    }

    fn protocol_failure(&mut self, message: String) -> PersistentReplError {
        self.mark_replacement_required(FreshSessionReason::ProtocolFailure);
        PersistentReplError::Protocol(message)
    }

    fn record_worker_loss(&mut self) -> PersistentReplError {
        let pid = self.worker_pid();
        let generation = self.generation;
        let error = self
            .worker
            .as_mut()
            .map(PersistentWorkerClient::finish_eof)
            .unwrap_or(PersistentWorkerError::Unavailable { worker_pid: pid });
        self.worker = None;
        self.control_state
            .lock()
            .map(|mut state| state.stdin = None)
            .ok();
        self.replacement_required = Some((FreshSessionReason::WorkerCrash, pid, generation));
        PersistentReplError::Worker(error)
    }
}

impl Drop for PersistentReplClient {
    fn drop(&mut self) {
        // Drop must remain bounded even if the session thread is inside a
        // blocking native call. Explicit `close` owns the graceful path.
        if self.worker.is_some() {
            self.hard_interrupt();
        }
    }
}

fn repl_request_frame(
    session_id: &str,
    generation: u64,
    sequence: u64,
    command: ReplSessionCommand,
) -> ScriptFrame {
    ScriptFrame {
        frame_version: SCRIPT_FRAME_VERSION,
        frame_id: format!("repl-request-{generation}-{sequence}"),
        payload: ScriptFramePayload::ReplRequest(ReplSessionRequest {
            session_id: session_id.to_owned(),
            generation,
            sequence,
            command,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_protocol::{
        SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, ScriptBudgets, ScriptOperation, ScriptProfile,
    };

    /// Resolves the main `agenterm` PE — `PersistentWorkerClient::spawn` now
    /// always prefixes `Command::new(executable)` with
    /// `SCRIPT_WORKER_ENGINE_ARGS` (`__agenterm-internal-engine rh`), so the
    /// executable itself must be the main PE, not the retired standalone
    /// `agenterm-rh` binary. `AGENTERM_TEST_SCRIPT_WORKER` still overrides
    /// this outright when set — its semantics are unchanged, only what it
    /// must now point at has (a main-PE-shaped binary, not a standalone rh
    /// exe).
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
            .join(format!("agenterm{suffix}"));
        assert!(
            target.is_file(),
            "build agenterm first or set AGENTERM_TEST_SCRIPT_WORKER: {}",
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

    fn wait_for_original_process_exit(pid: u32, identity: &str, deadline: Instant) {
        loop {
            let observation = crate::platform::process::observe(pid);
            match &observation {
                crate::platform::process::ProcessObservation::Dead { .. } => return,
                crate::platform::process::ProcessObservation::Live {
                    start_identity: Some(current),
                } if current != identity => return,
                _ if Instant::now() < deadline => thread::yield_now(),
                _ => panic!(
                    "owned process {pid} with start identity {identity:?} survived cleanup: {observation:?}"
                ),
            }
        }
    }
    // The four repl_* tests that lived here exercised the interactive REPL,
    // which the framed worker now rejects outright
    // ("protocol_repl_unavailable: interactive REPL was removed with the Rh
    // interpreter" -- see script_worker.rs). They could never pass again and
    // their spawn failure poisoned PROCESS_TEST_LOCK, cascading into the
    // unrelated supervisor tests below. The PersistentReplClient itself is
    // graybox-retired (no production callers) pending deletion.

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
                invocation("persistent-one", "fn entry() { 40 + 1 }"),
                Duration::from_secs(120),
                Duration::from_millis(150),
                no_broker,
            )
            .expect("first invocation");
        let two = first
            .invoke(
                invocation("persistent-two", "fn entry() { 40 + 2 }"),
                Duration::from_secs(120),
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
                invocation("persistent-after-kill", "fn entry() { 43 }"),
                Duration::from_secs(120),
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
                invocation("persistent-replacement", "fn entry() { 6 * 7 }"),
                Duration::from_secs(120),
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
                invocation("persistent-over-limit", "fn entry() { 42 }"),
                Duration::from_secs(120),
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
            crate::platform::process::observe(dropped_pid),
            crate::platform::process::ProcessObservation::Dead { .. }
        ));
    }
}
