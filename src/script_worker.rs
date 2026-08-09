use std::{
    collections::HashSet,
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[cfg(test)]
use crate::script_protocol::ScriptProfile;
use crate::script_engine::ScriptEngineBackend as _;
use crate::script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, SCRIPT_FRAME_MAX_BYTES, SCRIPT_FRAME_VERSION,
    SCRIPT_INVOCATION_MAX_BYTES, ScriptBrokerRequest, ScriptBrokerResponse, ScriptBudgets,
    ScriptCancelDisposition, ScriptExitClass, ScriptFailure, ScriptFailureCategory, ScriptFrame,
    ScriptFrameEncodeError, ScriptFramePayload, ScriptFrameRead, ScriptFrameRejection,
    ScriptFrameTracker, ScriptInvocation, ScriptOperation, ScriptResult, encode_script_frame,
    read_script_frame, write_encoded_script_frame,
};
use rhai::EvalAltResult;

type PendingBroker = Option<(String, mpsc::SyncSender<ScriptBrokerResponse>)>;
type SharedFrameOutput = Arc<Mutex<Box<dyn Write + Send>>>;

#[derive(Clone)]
struct BrokerClient {
    invocation_id: String,
    output: SharedFrameOutput,
    pending: Arc<Mutex<PendingBroker>>,
    next_request: Arc<std::sync::atomic::AtomicUsize>,
    requests_remaining: Arc<std::sync::atomic::AtomicUsize>,
    timeout: Duration,
}

impl BrokerClient {
    fn call_json(
        &self,
        operation: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if self
            .requests_remaining
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_sub(1)
            })
            .is_err()
        {
            return Err("broker_request_budget_exceeded".to_owned());
        }
        let sequence = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("broker-{sequence}");
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut pending = self.pending.lock().expect("pending broker lock poisoned");
            if pending.is_some() {
                return Err("broker_request_already_outstanding".to_owned());
            }
            *pending = Some((request_id.clone(), sender));
        }
        let frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: format!("{}-{request_id}", self.invocation_id),
            payload: ScriptFramePayload::BrokerRequest {
                invocation_id: self.invocation_id.clone(),
                request_id: request_id.clone(),
                request: ScriptBrokerRequest {
                    operation: operation.to_owned(),
                    arguments,
                },
            },
        };
        if let Err(error) = write_shared_frame(&self.output, &frame) {
            self.pending
                .lock()
                .expect("pending broker lock poisoned")
                .take();
            return Err(format!("broker_request_send_failed: {error}"));
        }
        let response = receiver.recv_timeout(self.timeout).map_err(|_| {
            self.pending
                .lock()
                .expect("pending broker lock poisoned")
                .take();
            "broker_response_timeout".to_owned()
        })?;
        if let Some(error) = response.error {
            return Err(format!("{}: {}", error.code, error.message));
        }
        Ok(response.value.unwrap_or(serde_json::Value::Null))
    }
}

pub fn run_legacy_worker_stdio() -> anyhow::Result<u8> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(SCRIPT_INVOCATION_MAX_BYTES + 1)
        .read_to_end(&mut input)?;
    let result = if input.len() as u64 > SCRIPT_INVOCATION_MAX_BYTES {
        protocol_failure(
            "protocol_invocation_too_large",
            format!("invocation exceeds the {SCRIPT_INVOCATION_MAX_BYTES} byte protocol limit"),
        )
    } else {
        match serde_json::from_slice(&input) {
            Ok(invocation) => execute(invocation),
            Err(error) => protocol_failure("protocol_invalid_invocation", error.to_string()),
        }
    };
    serde_json::to_writer(std::io::stdout().lock(), &result)?;
    std::io::stdout().lock().write_all(b"\n")?;
    Ok(u8::from(!result.ok))
}

pub fn run_framed_worker_stdio() -> anyhow::Result<u8> {
    let _interrupt_guard = crate::install_console_interrupt_ignore_guard()?;
    process_concurrent_framed_worker(std::io::stdin().lock(), std::io::stdout())?;
    Ok(0)
}

fn process_concurrent_framed_worker<R: Read>(
    mut input: R,
    output: impl Write + Send + 'static,
) -> anyhow::Result<()> {
    let output: SharedFrameOutput = Arc::new(Mutex::new(Box::new(output)));
    let active = Arc::new(Mutex::new(None::<(String, Arc<AtomicBool>)>));
    let pending_broker = Arc::new(Mutex::new(
        None::<(String, mpsc::SyncSender<ScriptBrokerResponse>)>,
    ));
    let completed = Arc::new(Mutex::new(HashSet::<String>::new()));
    let mut frame_tracker = ScriptFrameTracker::default();
    let mut workers = Vec::new();
    loop {
        let frame = match read_script_frame(&mut input)? {
            ScriptFrameRead::Eof => break,
            ScriptFrameRead::Frame(frame) => *frame,
            ScriptFrameRead::Rejected(rejection) => {
                let recoverable = rejection.recoverable;
                write_shared_rejection(&output, rejection)?;
                if !recoverable {
                    break;
                }
                continue;
            }
        };
        let (invocation_id, code, message) = match &frame.payload {
            ScriptFramePayload::ReplRequest(request) => (
                request.session_id.as_str(),
                "protocol_repl_unavailable",
                "interactive REPL was removed with the Rhai interpreter",
            ),
            ScriptFramePayload::ReplResponse(response) => (
                response.session_id.as_str(),
                "protocol_repl_unexpected_response",
                "REPL response frames are worker output and cannot be sent to the worker",
            ),
            _ => {
                let frame = match frame_tracker.admit(frame) {
                    Ok(frame) => frame,
                    Err(rejection) => {
                        write_shared_rejection(&output, rejection)?;
                        continue;
                    }
                };
                let frame_id = frame.frame_id;
                match frame.payload {
                    ScriptFramePayload::Invoke(invocation) => {
                        let invocation_id = invocation.invocation_id.clone();
                        let cancellation = Arc::new(AtomicBool::new(false));
                        {
                            let mut active_guard =
                                active.lock().expect("active invocation lock poisoned");
                            if active_guard.is_some() {
                                write_shared_protocol_frame(
                                    &output,
                                    &frame_id,
                                    &invocation_id,
                                    "protocol_worker_busy",
                                    "this worker already has an active invocation",
                                )?;
                                continue;
                            }
                            *active_guard =
                                Some((invocation_id.clone(), Arc::clone(&cancellation)));
                        }
                        let output_for_worker = Arc::clone(&output);
                        let active_for_worker = Arc::clone(&active);
                        let completed_for_worker = Arc::clone(&completed);
                        let pending_for_worker = Arc::clone(&pending_broker);
                        let broker = if matches!(
                            invocation.operation,
                            ScriptOperation::Eval | ScriptOperation::Run
                        ) {
                            Some(BrokerClient {
                                invocation_id: invocation_id.clone(),
                                output: Arc::clone(&output),
                                pending: pending_for_worker,
                                next_request: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
                                requests_remaining: Arc::new(std::sync::atomic::AtomicUsize::new(
                                    invocation.budgets.broker_requests,
                                )),
                                timeout: Duration::from_millis(invocation.budgets.wait_time_ms),
                            })
                        } else {
                            None
                        };
                        workers.push(std::thread::spawn(move || {
                            let result = execute_with_cancellation_and_broker(
                                invocation,
                                Some(cancellation),
                                broker,
                            );
                            let mut active_guard = active_for_worker
                                .lock()
                                .expect("active invocation lock poisoned");
                            let _ = write_shared_frame(
                                &output_for_worker,
                                &result_frame(frame_id, result),
                            );
                            completed_for_worker
                                .lock()
                                .expect("completed invocation lock poisoned")
                                .insert(invocation_id);
                            *active_guard = None;
                        }));
                    }
                    ScriptFramePayload::Cancel { invocation_id } => {
                        let active_invocation = active
                            .lock()
                            .expect("active invocation lock poisoned")
                            .as_ref()
                            .map(|(active_id, cancellation)| {
                                (active_id.clone(), Arc::clone(cancellation))
                            });
                        let disposition = ScriptCancelDisposition::classify(
                            &invocation_id,
                            active_invocation
                                .as_ref()
                                .map(|(active_id, _)| active_id.as_str()),
                            completed
                                .lock()
                                .expect("completed invocation lock poisoned")
                                .contains(&invocation_id),
                        );
                        match disposition {
                            ScriptCancelDisposition::Requested => active_invocation
                                .expect("requested cancellation has an active invocation")
                                .1
                                .store(true, Ordering::Relaxed),
                            ScriptCancelDisposition::TooLate
                            | ScriptCancelDisposition::Unknown => {
                                let (code, message) = disposition
                                    .rejection()
                                    .expect("non-requested cancellation has a rejection");
                                write_shared_protocol_frame(
                                    &output,
                                    &frame_id,
                                    &invocation_id,
                                    code,
                                    message,
                                )?;
                            }
                        }
                    }
                    ScriptFramePayload::BrokerResponse {
                        invocation_id,
                        request_id,
                        response,
                    } => {
                        let legacy_active_matches = active
                            .lock()
                            .expect("active invocation lock poisoned")
                            .as_ref()
                            .is_some_and(|(active_id, _)| active_id == &invocation_id);
                        let pending = pending_broker
                            .lock()
                            .expect("pending broker lock poisoned")
                            .take();
                        match pending {
                            Some((expected, sender))
                                if legacy_active_matches && expected == request_id =>
                            {
                                let _ = sender.send(response);
                            }
                            Some(pending) => {
                                *pending_broker
                                    .lock()
                                    .expect("pending broker lock poisoned") = Some(pending);
                                write_shared_protocol_frame(
                                    &output,
                                    &frame_id,
                                    &invocation_id,
                                    "protocol_broker_response_mismatch",
                                    "broker response does not match the active request",
                                )?;
                            }
                            None => write_shared_protocol_frame(
                                &output,
                                &frame_id,
                                &invocation_id,
                                "protocol_broker_response_unexpected",
                                "no broker request is outstanding",
                            )?,
                        }
                    }
                    ScriptFramePayload::BrokerRequest { invocation_id, .. } => {
                        write_shared_protocol_frame(
                            &output,
                            &frame_id,
                            &invocation_id,
                            "protocol_unexpected_broker_request",
                            "broker request frames are worker output and cannot be sent to the worker",
                        )?;
                    }
                    ScriptFramePayload::Result(result) => {
                        write_shared_protocol_frame(
                            &output,
                            &frame_id,
                            &result.invocation_id,
                            "protocol_unexpected_result",
                            "result frames are worker output and cannot be sent to the worker",
                        )?;
                    }
                    ScriptFramePayload::ReplRequest(_) | ScriptFramePayload::ReplResponse(_) => {
                        unreachable!("REPL frames are rejected before the legacy tracker")
                    }
                }
                continue;
            }
        };
        write_shared_protocol_frame(&output, &frame.frame_id, invocation_id, code, message)?;
    }
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}


fn write_shared_frame(output: &SharedFrameOutput, frame: &ScriptFrame) -> anyhow::Result<()> {
    let mut output = output.lock().expect("framed stdout lock poisoned");
    write_frame(&mut *output, frame)
}

fn write_shared_protocol_frame(
    output: &SharedFrameOutput,
    frame_id: &str,
    invocation_id: &str,
    code: &str,
    message: impl Into<String>,
) -> anyhow::Result<()> {
    write_shared_frame(
        output,
        &result_frame(
            frame_id.to_owned(),
            protocol_failure_for(invocation_id, code, message),
        ),
    )
}

fn write_shared_rejection(
    output: &SharedFrameOutput,
    rejection: ScriptFrameRejection,
) -> anyhow::Result<()> {
    write_shared_protocol_frame(
        output,
        &rejection.frame_id,
        &rejection.invocation_id,
        rejection.code,
        rejection.message,
    )
}

#[cfg(test)]
fn process_framed_stream<R: Read, W: Write>(mut input: R, mut output: W) -> anyhow::Result<()> {
    let mut frame_tracker = ScriptFrameTracker::default();
    let mut completed_invocations = HashSet::new();
    loop {
        let frame = match read_script_frame(&mut input)? {
            ScriptFrameRead::Eof => return Ok(()),
            ScriptFrameRead::Frame(frame) => *frame,
            ScriptFrameRead::Rejected(rejection) => {
                let recoverable = rejection.recoverable;
                write_protocol_frame(
                    &mut output,
                    &rejection.frame_id,
                    &rejection.invocation_id,
                    rejection.code,
                    rejection.message,
                )?;
                if !recoverable {
                    return Ok(());
                }
                continue;
            }
        };
        let frame = match frame_tracker.admit(frame) {
            Ok(frame) => frame,
            Err(rejection) => {
                write_protocol_frame(
                    &mut output,
                    &rejection.frame_id,
                    &rejection.invocation_id,
                    rejection.code,
                    rejection.message,
                )?;
                continue;
            }
        };
        let response = process_frame(frame, &mut completed_invocations);
        write_frame(&mut output, &response)?;
    }
}

#[cfg(test)]
fn process_frame(frame: ScriptFrame, completed_invocations: &mut HashSet<String>) -> ScriptFrame {
    let frame_id = frame.frame_id;
    let result = match frame.payload {
        ScriptFramePayload::Invoke(invocation) => {
            completed_invocations.insert(invocation.invocation_id.clone());
            execute(invocation)
        }
        ScriptFramePayload::Cancel { invocation_id } => {
            let disposition = ScriptCancelDisposition::classify(
                &invocation_id,
                None,
                completed_invocations.contains(&invocation_id),
            );
            let (code, message) = disposition
                .rejection()
                .expect("synchronous harness has no active invocation");
            protocol_failure_for(&invocation_id, code, message)
        }
        ScriptFramePayload::BrokerRequest { invocation_id, .. }
        | ScriptFramePayload::BrokerResponse { invocation_id, .. } => protocol_failure_for(
            &invocation_id,
            "protocol_broker_unavailable",
            "broker frames are reserved but unavailable in this worker version",
        ),
        ScriptFramePayload::Result(result) => protocol_failure_for(
            &result.invocation_id,
            "protocol_unexpected_result",
            "result frames are worker output and cannot be sent to the worker",
        ),
        ScriptFramePayload::ReplRequest(request) => protocol_failure_for(
            &request.session_id,
            "protocol_repl_unavailable",
            "interactive REPL was removed with the Rhai interpreter",
        ),
        ScriptFramePayload::ReplResponse(response) => protocol_failure_for(
            &response.session_id,
            "protocol_repl_unexpected_response",
            "REPL response frames are worker output and cannot be sent to the worker",
        ),
    };
    result_frame(frame_id, result)
}

fn result_frame(frame_id: String, result: ScriptResult) -> ScriptFrame {
    ScriptFrame {
        frame_version: SCRIPT_FRAME_VERSION,
        frame_id,
        payload: ScriptFramePayload::Result(result),
    }
}

#[cfg(test)]
fn write_protocol_frame<W: Write>(
    output: &mut W,
    frame_id: &str,
    invocation_id: &str,
    code: &str,
    message: impl Into<String>,
) -> anyhow::Result<()> {
    write_frame(
        output,
        &result_frame(
            frame_id.to_owned(),
            protocol_failure_for(invocation_id, code, message),
        ),
    )
}

fn write_frame<W: Write>(output: &mut W, frame: &ScriptFrame) -> anyhow::Result<()> {
    let bytes = match encode_script_frame(frame) {
        Ok(bytes) => bytes,
        Err(ScriptFrameEncodeError::TooLarge { .. }) => {
            let invocation_id = match &frame.payload {
                ScriptFramePayload::Result(result) => result.invocation_id.as_str(),
                _ => "unknown",
            };
            let replacement = result_frame(
                frame.frame_id.clone(),
                protocol_failure_for(
                    invocation_id,
                    "protocol_result_frame_too_large",
                    format!("encoded result exceeds the {SCRIPT_FRAME_MAX_BYTES} byte frame limit"),
                ),
            );
            encode_script_frame(&replacement)?
        }
        Err(error) => return Err(error.into()),
    };
    write_encoded_script_frame(output, &bytes)?;
    Ok(())
}

fn execute(invocation: ScriptInvocation) -> ScriptResult {
    execute_with_cancellation(invocation, None)
}

fn execute_with_cancellation(
    invocation: ScriptInvocation,
    cancellation: Option<Arc<AtomicBool>>,
) -> ScriptResult {
    execute_with_cancellation_and_broker(invocation, cancellation, None)
}

fn execute_with_cancellation_and_broker(
    invocation: ScriptInvocation,
    cancellation: Option<Arc<AtomicBool>>,
    broker: Option<BrokerClient>,
) -> ScriptResult {
    let started = Instant::now();
    let mut result = ScriptResult {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id: invocation.invocation_id.clone(),
        api_version: SCRIPT_API_VERSION,
        ok: false,
        exit_class: ScriptExitClass::Configuration,
        operation: Some(invocation.operation),
        profile: Some(invocation.profile),
        stdout: String::new(),
        value: None,
        failure: None,
        duration_ms: 0,
    };
    let execution = execute_inner(&invocation, cancellation, broker);
    match execution {
        Ok((stdout, value)) => {
            result.ok = true;
            result.exit_class = ScriptExitClass::Success;
            result.stdout = stdout;
            result.value = value;
        }
        Err(failure) => {
            result.exit_class = match failure.category {
                ScriptFailureCategory::Configuration => ScriptExitClass::Configuration,
                ScriptFailureCategory::Limit => ScriptExitClass::Limit,
                ScriptFailureCategory::Script => ScriptExitClass::Script,
                ScriptFailureCategory::Child => ScriptExitClass::Child,
                ScriptFailureCategory::Cancelled => ScriptExitClass::Cancelled,
                ScriptFailureCategory::Fleet => ScriptExitClass::Fleet,
                ScriptFailureCategory::Protocol => ScriptExitClass::Protocol,
                ScriptFailureCategory::Host => ScriptExitClass::Host,
            };
            result.failure = Some(failure);
        }
    }
    result.duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    result
}

fn execute_inner(
    invocation: &ScriptInvocation,
    _cancellation: Option<Arc<AtomicBool>>,
    broker: Option<BrokerClient>,
) -> Result<(String, Option<serde_json::Value>), ScriptFailure> {
    let _temp_scope = crate::script_stdlib::enter_invocation_temp_root(
        invocation
            .invocation_temp_root
            .as_deref()
            .map(std::path::Path::new),
    );
    if invocation.envelope_version != SCRIPT_ENVELOPE_VERSION {
        return Err(protocol_error(
            "unsupported_envelope",
            format!(
                "worker supports envelope {}, requested {}",
                SCRIPT_ENVELOPE_VERSION, invocation.envelope_version
            ),
        ));
    }
    if invocation.api_version != SCRIPT_API_VERSION {
        return Err(protocol_error(
            "unsupported_api",
            format!(
                "worker supports API {}, requested {}",
                SCRIPT_API_VERSION, invocation.api_version
            ),
        ));
    }
    validate_budgets(&invocation.budgets)?;
    if invocation.source.len() > invocation.budgets.source_bytes {
        return Err(limit_error(
            "limit_source_bytes",
            "script source exceeds its byte budget",
        ));
    }
    if invocation.operation == ScriptOperation::Api {
        return Ok((String::new(), Some(crate::script_catalog::catalog())));
    }

    // Shared invocation options + fleet_bridge, built once (Trait-M3: this
    // used to be reconstructed identically per backend — see design
    // §1.4/§2.4). `fleet_bridge` wraps `BrokerClient::call_json("fleet.call",
    // ...)` exactly like rh's `script_rh_host::broker_fleet_bridge` already
    // did; `RhEngineBackend::execute` (script_engine.rs) converts this Arc
    // into the `Box`-shaped `script_rh_host::FleetBridgeFn` internally.
    let options = crate::script_engine::ScriptInvocationOptions {
        project_root: invocation
            .project_root
            .as_ref()
            .map(std::path::PathBuf::from),
        arguments: serde_json::to_value(&invocation.arguments).ok(),
        budgets: Some(invocation.budgets.clone()),
    };
    let fleet_bridge: Option<crate::script_engine::ScriptFleetBridgeFn> =
        broker.as_ref().map(|broker| {
            let broker = broker.clone();
            let bridge: crate::script_engine::ScriptFleetBridgeFn = Arc::new(
                move |op_id: &str, params: &str| -> Result<String, String> {
                    let arguments =
                        serde_json::from_str(params).unwrap_or(serde_json::json!({}));
                    broker
                        .call_json(
                            "fleet.call",
                            serde_json::json!({
                                "operation_id": op_id,
                                "parameters": arguments,
                            }),
                        )
                        .map(|value| value.to_string())
                },
            );
            bridge
        });

    // rh backend: no `#[cfg(not(test))]` gate — rh's `execute_inner` unit
    // tests (below) rely on this branch actually running.
    if crate::script_engine::RhEngineBackend.enabled() {
        return dispatch_via_engine(
            &crate::script_engine::RhEngineBackend,
            invocation.operation,
            &invocation.source,
            &options,
            fleet_bridge,
        );
    }

    // Lua backend: enabled via AGENTERM_SCRIPT_BACKEND=lua or `.lua` entry.
    #[cfg(not(test))]
    if crate::script_engine::LuaEngineBackend.enabled() {
        return dispatch_via_engine(
            &crate::script_engine::LuaEngineBackend,
            invocation.operation,
            &invocation.source,
            &options,
            fleet_bridge,
        );
    }

    // qjs backend: enabled via AGENTERM_SCRIPT_BACKEND=qjs or `.js`/`.mjs` entry.
    #[cfg(not(test))]
    if crate::script_engine::QjsEngineBackend.enabled() {
        return dispatch_via_engine(
            &crate::script_engine::QjsEngineBackend,
            invocation.operation,
            &invocation.source,
            &options,
            fleet_bridge,
        );
    }

    Err(configuration_error(
        "script_backend_unavailable",
        "no script backend handled this invocation; set AGENTERM_SCRIPT_BACKEND to rh, lua, or qjs with matching source",
    ))
}

/// Routes a single invocation through the `ScriptEngineBackend` trait layer
/// (`src/script_engine.rs`) once its owning `execute_inner` call site has
/// already confirmed `engine.enabled()`. Mirrors what each
/// `try_execute_*`'s `Check` vs `Run|Eval` match arms did inline before
/// Trait-M3 — kept as one shared function (instead of `ScriptEngine::all()`
/// looped dispatch) so the three call sites above keep their independent
/// `#[cfg(not(test))]` attributes per the M3 phase's conservative scope.
fn dispatch_via_engine(
    engine: &dyn crate::script_engine::ScriptEngineBackend,
    operation: ScriptOperation,
    source: &str,
    options: &crate::script_engine::ScriptInvocationOptions,
    fleet_bridge: Option<crate::script_engine::ScriptFleetBridgeFn>,
) -> Result<(String, Option<serde_json::Value>), ScriptFailure> {
    let backend_code = format!("{}_backend", engine.backend_id().as_str());
    match operation {
        ScriptOperation::Check => {
            engine
                .check(source, options)
                .map_err(|error| configuration_error(backend_code, error))?;
            Ok((String::new(), None))
        }
        ScriptOperation::Run | ScriptOperation::Eval => {
            let result = engine
                .execute(source, options, fleet_bridge)
                .map_err(|error| configuration_error(backend_code, error))?;
            Ok((result.stdout, result.value))
        }
        // Unreachable in practice: `execute_inner` short-circuits
        // `ScriptOperation::Api` before any backend dispatch (see the
        // `invocation.operation == ScriptOperation::Api` check above).
        ScriptOperation::Api => Err(configuration_error(
            "script_backend_unavailable",
            "Api operation is handled before backend dispatch",
        )),
    }
}

fn validate_budgets(budgets: &ScriptBudgets) -> Result<(), ScriptFailure> {
    let maximums = ScriptBudgets::hard_limits();
    macro_rules! validate {
        ($field:ident) => {
            if budgets.$field == 0 || budgets.$field > maximums.$field {
                return Err(configuration_error(
                    concat!("configuration_budget_", stringify!($field)),
                    format!(
                        "{} must be from 1 to {}",
                        stringify!($field),
                        maximums.$field
                    ),
                ));
            }
        };
    }
    validate!(source_bytes);
    validate!(operations);
    validate!(call_depth);
    validate!(expression_depth);
    validate!(collection_items);
    validate!(string_bytes);
    validate!(output_bytes);
    validate!(wall_time_ms);
    validate!(broker_requests);
    validate!(broker_return_bytes);
    validate!(capture_bytes);
    validate!(event_items);
    validate!(wait_time_ms);
    Ok(())
}

fn protocol_failure(code: impl Into<String>, message: impl Into<String>) -> ScriptResult {
    protocol_failure_for("unknown", code, message)
}

fn protocol_failure_for(
    invocation_id: &str,
    code: impl Into<String>,
    message: impl Into<String>,
) -> ScriptResult {
    ScriptResult {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id: invocation_id.to_owned(),
        api_version: SCRIPT_API_VERSION,
        ok: false,
        exit_class: ScriptExitClass::Protocol,
        operation: None,
        profile: None,
        stdout: String::new(),
        value: None,
        failure: Some(protocol_error(code, message)),
        duration_ms: 0,
    }
}

fn configuration_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Configuration)
}

fn limit_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Limit)
}

fn script_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Script)
}

fn child_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Child)
}

fn cancelled_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Cancelled)
}

fn fleet_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Fleet)
}

fn classify_runtime_error(error: &EvalAltResult, message: String) -> ScriptFailure {
    if let Some(fields) = crate::script_error::runtime_error_fields(error) {
        return match fields.class.as_str() {
            "configuration" => configuration_error(fields.code, fields.safe_message),
            "limit" => limit_error(fields.code, fields.safe_message),
            "child" => child_error(fields.code, fields.safe_message),
            "cancelled" => cancelled_error(fields.code, fields.safe_message),
            "fleet" => fleet_error(fields.code, fields.safe_message),
            "protocol" => protocol_error(fields.code, fields.safe_message),
            "host" => failure(
                fields.code,
                fields.safe_message,
                ScriptFailureCategory::Host,
            ),
            _ => script_error(fields.code, fields.safe_message),
        };
    }
    let classified = message
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        })
        .find_map(|token| {
            if matches!(
                token,
                "process_spawn"
                    | "child_nonzero"
                    | "process_stdout_unavailable"
                    | "process_stderr_unavailable"
                    | "process_stdin_write"
                    | "process_child_state_poisoned"
                    | "process_child_missing"
                    | "process_child_completed"
                    | "process_try_wait"
                    | "process_kill"
                    | "process_timeout"
                    | "process_stdout_not_utf8"
                    | "process_stderr_not_utf8"
            ) {
                Some((ScriptFailureCategory::Child, token.to_owned()))
            } else if matches!(
                token,
                "fleet_catalog_encode"
                    | "fleet_receipt_invalid"
                    | "fleet_result_decode"
                    | "broker_host_error"
                    | "broker_invalid_receipt"
                    | "broker_invalid_response"
                    | "broker_post_state_missing"
                    | "broker_receipt_missing"
                    | "broker_transport"
                    | "broker_response_timeout"
                    | "server_restart"
                    | "journal_gap"
                    | "future_sequence"
                    | "event_wait_timeout"
            ) {
                Some((ScriptFailureCategory::Fleet, token.to_owned()))
            } else {
                None
            }
        });
    match classified {
        Some((ScriptFailureCategory::Child, code)) => child_error(code, message),
        Some((ScriptFailureCategory::Fleet, code)) => fleet_error(code, message),
        _ => script_error("script_runtime", message),
    }
}

fn protocol_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Protocol)
}

fn failure(
    code: impl Into<String>,
    message: impl Into<String>,
    category: ScriptFailureCategory,
) -> ScriptFailure {
    ScriptFailure {
        code: code.into(),
        message: message.into(),
        category,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_api_validate::external_function_calls;
    use std::io::{Cursor, Read, Write};

    struct ChannelReader {
        receiver: mpsc::Receiver<Vec<u8>>,
        current: Cursor<Vec<u8>>,
    }

    impl ChannelReader {
        fn new(receiver: mpsc::Receiver<Vec<u8>>) -> Self {
            Self {
                receiver,
                current: Cursor::new(Vec::new()),
            }
        }
    }

    impl Read for ChannelReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            loop {
                let read = self.current.read(buffer)?;
                if read != 0 {
                    return Ok(read);
                }
                match self.receiver.recv() {
                    Ok(bytes) => self.current = Cursor::new(bytes),
                    Err(_) => return Ok(0),
                }
            }
        }
    }

    #[derive(Clone, Default)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("shared test output lock")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn rh_entry_source(body: &str) -> String {
        if body.contains("fn entry") {
            body.to_owned()
        } else {
            format!("fn entry() {{ {body} }}")
        }
    }

    fn invocation(operation: ScriptOperation, source: &str) -> ScriptInvocation {
        let source = match operation {
            ScriptOperation::Run | ScriptOperation::Eval => rh_entry_source(source),
            _ => source.to_owned(),
        };
        ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: "unit-invocation".to_owned(),
            api_version: SCRIPT_API_VERSION,
            operation,
            profile: ScriptProfile::Pure,
            source_label: "unit".to_owned(),
            source: source.to_owned(),
            project_root: None,
            invocation_temp_root: None,
            arguments: Vec::new(),
            budgets: ScriptBudgets::default(),
            observation: None,
        }
    }

    fn failure_code(result: &ScriptResult) -> &str {
        result
            .failure
            .as_ref()
            .map(|failure| failure.code.as_str())
            .expect("expected failure")
    }

    fn invoke_frame(frame_id: &str, invocation_id: &str, source: &str) -> ScriptFrame {
        let mut invocation = invocation(ScriptOperation::Eval, source);
        invocation.invocation_id = invocation_id.to_owned();
        ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: frame_id.to_owned(),
            payload: ScriptFramePayload::Invoke(invocation),
        }
    }

    fn encoded_frame(frame: &ScriptFrame) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, frame).expect("encode frame");
        bytes
    }

    fn decoded_frames(bytes: &[u8]) -> Vec<ScriptFrame> {
        let mut input = Cursor::new(bytes);
        let mut frames = Vec::new();
        loop {
            match read_script_frame(&mut input).expect("decode frame") {
                ScriptFrameRead::Eof => break,
                ScriptFrameRead::Frame(frame) => frames.push(*frame),
                ScriptFrameRead::Rejected(rejection) => {
                    panic!("unexpected rejected frame: {rejection:?}")
                }
            }
        }
        frames
    }

    fn frame_result(frame: &ScriptFrame) -> &ScriptResult {
        match &frame.payload {
            ScriptFramePayload::Result(result) => result,
            _ => panic!("expected result frame"),
        }
    }

    #[test]
    fn api_catalog_reports_defaults_maximums_availability_and_exit_classes() {
        let result = execute(invocation(ScriptOperation::Api, ""));
        assert!(result.ok);
        assert_eq!(result.operation, Some(ScriptOperation::Api));
        assert_eq!(result.profile, Some(ScriptProfile::Pure));
        let catalog = result.value.expect("API catalog");
        assert_eq!(
            catalog["limits"]["defaults"]["wall_time_ms"],
            ScriptBudgets::default().wall_time_ms
        );
        assert_eq!(
            catalog["limits"]["hard_maximums"]["wall_time_ms"],
            ScriptBudgets::hard_limits().wall_time_ms
        );
        assert_eq!(
            catalog["limits"]["invocation_bytes"],
            SCRIPT_INVOCATION_MAX_BYTES
        );
        assert_eq!(catalog["exit_classes"]["configuration"], 2);
        assert_eq!(catalog["exit_classes"]["limit"], 3);
        assert_eq!(catalog["exit_classes"]["child"], 4);
        assert_eq!(catalog["exit_classes"]["cancelled"], 5);
        assert_eq!(catalog["exit_classes"]["fleet"], 6);
        assert_eq!(
            catalog["typed_error"]["catchable_slices"][0],
            "std.process.Output.require_success"
        );
        assert_eq!(
            catalog["typed_error"]["fields"].as_array().map(Vec::len),
            Some(8)
        );
        assert_eq!(catalog["schema_version"], 3);
        let apis = catalog["entries"].as_array().expect("API entries");
        let new_tab = apis
            .iter()
            .find(|api| api["stable_id"] == "fleet.tabs.new")
            .expect("deferred control API");
        assert_eq!(new_tab["status"], "planned");
        let workspace = apis
            .iter()
            .find(|api| api["surface_path"] == "fleet.workspace.info")
            .expect("typed workspace API");
        assert_eq!(workspace["operation_id"], "workspace.info");
        assert_eq!(workspace["status"], "shipped");
        assert_eq!(
            catalog["limits"]["defaults"]["broker_requests"],
            ScriptBudgets::default().broker_requests
        );
    }

    #[test]
    fn invalid_or_excessive_budget_is_configuration_failure() {
        let mut zero = invocation(ScriptOperation::Eval, "1");
        zero.budgets.operations = 0;
        let zero = execute(zero);
        assert_eq!(failure_code(&zero), "configuration_budget_operations");
        assert_eq!(zero.exit_class, ScriptExitClass::Configuration);

        let mut excessive = invocation(ScriptOperation::Eval, "1");
        excessive.budgets.output_bytes = ScriptBudgets::hard_limits().output_bytes + 1;
        assert_eq!(
            failure_code(&execute(excessive)),
            "configuration_budget_output_bytes"
        );
    }

    #[test]
    fn runtime_failures_preserve_child_and_fleet_exit_classes() {
        let child_error: Box<EvalAltResult> = "process_spawn: executable missing (line 1)".into();
        let child = classify_runtime_error(&child_error, child_error.to_string());
        assert_eq!(child.code, "process_spawn");
        assert_eq!(child.category, ScriptFailureCategory::Child);

        let fleet_error: Box<EvalAltResult> = "server_restart: epoch changed (line 1)".into();
        let fleet = classify_runtime_error(&fleet_error, fleet_error.to_string());
        assert_eq!(fleet.code, "server_restart");
        assert_eq!(fleet.category, ScriptFailureCategory::Fleet);

        let script_error_value: Box<EvalAltResult> = "user failure (line 1)".into();
        let script = classify_runtime_error(&script_error_value, script_error_value.to_string());
        assert_eq!(script.code, "script_runtime");
        assert_eq!(script.category, ScriptFailureCategory::Script);

        let user_error: Box<EvalAltResult> = "user operation failed (line 1)".into();
        let classified = classify_runtime_error(&user_error, user_error.to_string());
        assert_eq!(classified.code, "script_runtime");
        assert_eq!(classified.category, ScriptFailureCategory::Script);
    }

    #[test]
    fn source_byte_limit_is_typed() {
        let mut source = invocation(ScriptOperation::Check, "12345");
        source.budgets.source_bytes = 4;
        assert_eq!(failure_code(&execute(source)), "limit_source_bytes");
    }

    #[test]
    fn malformed_and_oversized_invocations_have_protocol_envelopes() {
        let malformed = protocol_failure("protocol_invalid_invocation", "bad JSON");
        assert!(!malformed.ok);
        assert_eq!(malformed.exit_class, ScriptExitClass::Protocol);
        assert_eq!(
            malformed
                .failure
                .as_ref()
                .expect("protocol failure")
                .category,
            ScriptFailureCategory::Protocol
        );
        assert!(malformed.operation.is_none());
        assert!(malformed.profile.is_none());

        let oversized = vec![0_u8; SCRIPT_INVOCATION_MAX_BYTES as usize + 1];
        assert!(oversized.len() as u64 > SCRIPT_INVOCATION_MAX_BYTES);
    }

    #[test]
    fn api_scanner_ignores_strings_comments_and_method_calls() {
        let source = r#"
            // hidden_api()
            let text = "also_hidden()";
            values.len();
            /* another_hidden() */
        "#;
        assert!(external_function_calls(source).is_empty());
    }

    #[test]
    fn api_scanner_accepts_forward_function_declarations() {
        let source = "twice(21); fn twice(value) { value * 2 }";
        assert!(external_function_calls(source).is_empty());
    }

    #[test]
    fn api_scanner_treats_parenthesized_throw_as_a_keyword() {
        let source = r#"throw ("typed_failure:" + value.to_string());"#;
        assert!(external_function_calls(source).is_empty());
    }

    #[test]
    fn framed_worker_runs_multiple_invocations_without_stdout_corruption() {
        let mut input = encoded_frame(&invoke_frame(
            "frame-one",
            "invocation-one",
            r#"print("inside-result"); 21 * 2"#,
        ));
        input.extend(encoded_frame(&invoke_frame(
            "frame-two",
            "invocation-two",
            "6 * 7",
        )));
        let mut output = Vec::new();
        process_framed_stream(Cursor::new(input), &mut output).expect("framed stream");

        let frames = decoded_frames(&output);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].frame_id, "frame-one");
        assert_eq!(frame_result(&frames[0]).stdout, "inside-result\n");
        assert_eq!(frame_result(&frames[0]).value, Some(serde_json::json!(42)));
        assert_eq!(frames[1].frame_id, "frame-two");
        assert_eq!(frame_result(&frames[1]).value, Some(serde_json::json!(42)));
    }

    #[test]
    fn framed_worker_recovers_after_malformed_and_oversized_frames() {
        let mut input = Vec::new();
        input.extend(1_u32.to_be_bytes());
        input.push(b'{');
        let oversized = SCRIPT_FRAME_MAX_BYTES + 1;
        input.extend(oversized.to_be_bytes());
        input.resize(input.len() + oversized as usize, b'x');
        input.extend(encoded_frame(&invoke_frame(
            "recovery-frame",
            "recovery-invocation",
            "40 + 2",
        )));
        let mut output = Vec::new();
        process_framed_stream(Cursor::new(input), &mut output).expect("framed stream");

        let frames = decoded_frames(&output);
        assert_eq!(frames.len(), 3);
        assert_eq!(
            failure_code(frame_result(&frames[0])),
            "protocol_malformed_frame"
        );
        assert_eq!(
            failure_code(frame_result(&frames[1])),
            "protocol_frame_too_large"
        );
        assert_eq!(frames[2].frame_id, "recovery-frame");
        assert_eq!(frame_result(&frames[2]).value, Some(serde_json::json!(42)));
    }

    #[test]
    fn framed_worker_rejects_versions_duplicates_cancel_and_reserved_frames() {
        let mut unsupported = invoke_frame("unsupported", "never-run", "1");
        unsupported.frame_version = SCRIPT_FRAME_VERSION + 1;
        let first = invoke_frame("first", "same-invocation", "1");
        let duplicate_invocation = invoke_frame("second", "same-invocation", "2");
        let duplicate_frame = invoke_frame("first", "another-invocation", "3");
        let cancel = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "cancel".to_owned(),
            payload: ScriptFramePayload::Cancel {
                invocation_id: "same-invocation".to_owned(),
            },
        };
        let broker = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "broker".to_owned(),
            payload: ScriptFramePayload::BrokerRequest {
                invocation_id: "same-invocation".to_owned(),
                request_id: "request-one".to_owned(),
                request: ScriptBrokerRequest {
                    operation: "ui.snapshot".to_owned(),
                    arguments: serde_json::json!({}),
                },
            },
        };
        let mut input = Vec::new();
        for frame in [
            unsupported,
            first,
            duplicate_invocation,
            duplicate_frame,
            cancel,
            broker,
        ] {
            input.extend(encoded_frame(&frame));
        }
        let mut output = Vec::new();
        process_framed_stream(Cursor::new(input), &mut output).expect("framed stream");

        let frames = decoded_frames(&output);
        let codes: Vec<_> = frames
            .iter()
            .map(|frame| {
                frame_result(frame)
                    .failure
                    .as_ref()
                    .map(|failure| failure.code.as_str())
            })
            .collect();
        assert_eq!(
            codes,
            vec![
                Some("protocol_unsupported_frame_version"),
                None,
                Some("protocol_duplicate_invocation"),
                Some("protocol_duplicate_frame"),
                Some("protocol_cancel_too_late"),
                Some("protocol_broker_unavailable"),
            ]
        );
    }

    #[test]
    fn framed_worker_replaces_unencodable_large_result_with_typed_failure() {
        let mut result = execute(invocation(ScriptOperation::Eval, "42"));
        result.stdout = "\0".repeat(1024 * 1024);
        let frame = result_frame("large-result".to_owned(), result);
        let mut output = Vec::new();
        write_frame(&mut output, &frame).expect("bounded replacement frame");
        assert!(output.len() <= SCRIPT_FRAME_MAX_BYTES as usize + 4);
        let frames = decoded_frames(&output);
        assert_eq!(
            failure_code(frame_result(&frames[0])),
            "protocol_result_frame_too_large"
        );
    }
}
