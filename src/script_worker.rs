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

    // dynacore pack: opt-in via AGENTERM_DYNACORE_PACK_STORE +
    // AGENTERM_DYNACORE_PACK_HASH, independent of AGENTERM_SCRIPT_BACKEND
    // (which only selects among rh/lua/qjs/sql and defaults to rh) — this
    // only fires when a caller has explicitly pointed at a dynacore pack
    // store+hash, so it cannot change behavior for any existing invocation
    // that doesn't set those two env vars. Checked before the rh/lua/qjs/sql
    // branches below for that reason.
    //
    // Reuses the SAME `fleet_bridge` built above — in-process, one function
    // call per fleet.call, no new bridge and no subprocess — via a direct
    // pass-through: `crate::script_engine::ScriptFleetBridgeFn` and
    // `crate::script_dynacore_host::DynacoreFleetBridgeFn` are both exactly
    // `Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>` (the
    // same type under two names — a type alias does not create a new
    // nominal type), so no wrapper closure is needed here, unlike rh's
    // Arc->Box bridge in `RhEngineBackend::execute`.
    //
    // Does NOT go through `ScriptEngineBackend`/`ScriptEngine` — dynacore
    // packs are binary content-addressed artifacts, not human-authored text
    // source, so this is not a fifth `ScriptEngineBackend` impl; no new
    // `ScriptBackend`/`ScriptOperation` variant either. See
    // `script_backend::try_execute_dynacore_pack_invocation`'s doc for the
    // full rationale (mirrors rh's own native-pack path, `script_rh_pack.rs`).
    match crate::script_backend::try_execute_dynacore_pack_invocation(
        invocation.operation,
        fleet_bridge.clone(),
    ) {
        Ok(Some(result)) => return Ok((result.stdout, result.value)),
        Ok(None) => {}
        Err(error) => return Err(configuration_error("dynacore_backend", error)),
    }

    // nativecore pack: opt-in via AGENTERM_NATIVECORE_PACK_STORE +
    // AGENTERM_NATIVECORE_PACK_HASH, independent of the dynacore pack pair
    // above and of AGENTERM_SCRIPT_BACKEND — only fires when a caller has
    // explicitly pointed at a nativecore pack store+hash, so it cannot
    // change behavior for any existing invocation that doesn't set those
    // two env vars.
    //
    // No `fleet_bridge` to pass through here (unlike the dynacore branch
    // above): nativecore intents call `seam.rs::do_intent` directly onto
    // real Win32 APIs, not through a fleet broker — see
    // `script_backend::try_execute_nativecore_pack_invocation`'s doc and
    // `plan/design-dynacore-native-core.md` §7.1.
    match crate::script_backend::try_execute_nativecore_pack_invocation(invocation.operation) {
        Ok(Some(result)) => return Ok((result.stdout, result.value)),
        Ok(None) => {}
        Err(error) => return Err(configuration_error("nativecore_backend", error)),
    }

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

    // sql backend: enabled via AGENTERM_SCRIPT_BACKEND=sql or `.sql` entry.
    // Same #[cfg(not(test))] gate as lua/qjs above (see script_engine.rs's
    // module doc for why: the M3 phase's per-backend gates are conservative
    // and independent per call site, not a single shared flag). `execute`
    // will surface `SqlEngineBackend`'s honest not-implemented error through
    // `dispatch_via_engine`'s `configuration_error` mapping — correct
    // fail-closed behavior for a placeholder backend, not a bug: a `check`
    // operation still works (sql's check is real), only `run`/`eval` fail.
    #[cfg(not(test))]
    if crate::script_engine::SqlEngineBackend.enabled() {
        return dispatch_via_engine(
            &crate::script_engine::SqlEngineBackend,
            invocation.operation,
            &invocation.source,
            &options,
            fleet_bridge,
        );
    }

    Err(configuration_error(
        "script_backend_unavailable",
        "no script backend handled this invocation; set AGENTERM_SCRIPT_BACKEND to rh, lua, qjs, or sql with matching source",
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

/// Black-box proof that the dynacore integration's fleet_call path is a real
/// in-process function call, not a subprocess — the thing
/// `tests/dynacore_live_server.rs`/`examples/dynacore_run.rs`'s old
/// subprocess-per-call bridge could NOT prove (that file's own header
/// explains why it had to shell out to `agenterm cli` for every call:
/// `script_worker.rs`/`script_backend.rs` were mid-flight-modified by a
/// concurrent session at the time). That constraint is gone; this module
/// exercises `crate::script_dynacore_host::verify_pack`/`run_pack` — the
/// SAME two calls `crate::script_backend::try_execute_dynacore_pack_invocation`
/// makes (see that function's doc) — through a bridge that reaches a REAL,
/// separately-spawned `agenterm` server via `crate::client::send_ipc_request_to_timeout`,
/// a direct function call that opens one real IPC socket per call and never
/// spawns a subprocess.
///
/// Lives in a separate `#[cfg(test)] mod` (not folded into `mod tests`
/// above) because it needs its own `Harness`/bridge helpers and does not
/// touch `script_worker.rs`'s frame/invocation machinery at all — unlike
/// `mod tests`, nothing here calls `execute`/`execute_inner`, so it is not
/// exposed to that OnceLock's process-wide "first caller wins" caching
/// behavior (`script_dynacore_pack::cached_dynacore_pack()` is itself
/// exercised by `script_dynacore_pack.rs`'s own env-free unit tests instead
/// — see that file's header for why the *env-var-triggered* path specifically
/// cannot be unit-tested reliably in `--lib`'s shared test binary, the exact
/// same reason `script_rh_pack.rs`'s own `cached_rh_pack()` is only
/// exercised through subprocess-spawning integration tests like
/// `tests/rh_framed_worker.rs`, never a `--lib` unit test).
#[cfg(test)]
mod dynacore_in_process_broker_tests {
    use std::{
        fs,
        net::TcpListener,
        path::PathBuf,
        process::{Child, Command, Stdio},
        sync::Arc,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use agenterm_dynacore::eval_core::Termination;

    use crate::script_dynacore_host::{self, DynacoreFleetBridgeFn};

    /// Resolve the `agenterm` server executable built alongside this test
    /// binary at RUNTIME. `CARGO_BIN_EXE_agenterm` is only defined at
    /// compile time for integration tests/benchmarks — NOT for `--lib` unit
    /// tests (probed directly: `env!("CARGO_BIN_EXE_agenterm")` fails to
    /// compile in this module with "may not be available for the current
    /// Cargo target") — so this mirrors `examples/dynacore_run.rs`'s own
    /// `locate_agenterm_cli` sibling-path strategy (that file hit the exact
    /// same macro restriction for `[[example]]` targets) instead. A `--lib`
    /// unit test binary builds to `target/<profile>/deps/agenterm-<hash>
    /// (.exe)`; the `agenterm` bin target builds to
    /// `target/<profile>/agenterm(.exe)` — one directory up from `deps/`.
    fn locate_agenterm_server_exe() -> PathBuf {
        locate_sibling_exe("agenterm")
    }

    fn locate_sibling_exe(bin_name: &str) -> PathBuf {
        let exe_name = format!("{bin_name}{}", std::env::consts::EXE_SUFFIX);
        if let Ok(current) = std::env::current_exe()
            && let Some(deps_dir) = current.parent()
            && let Some(profile_dir) = deps_dir.parent()
        {
            let candidate = profile_dir.join(&exe_name);
            if candidate.is_file() {
                return candidate;
            }
        }
        PathBuf::from(exe_name)
    }

    /// A real, isolated headless `agenterm` server process this test owns
    /// end to end — trimmed from `tests/dynacore_live_server.rs`'s own
    /// `Harness`. Unlike that file, nothing here shells out to
    /// `agenterm cli` per call: `wait_ready` and every fleet bridge call
    /// below use `crate::client::send_ipc_request_to_timeout` directly, an
    /// in-process function call over a real IPC socket.
    struct Harness {
        root: PathBuf,
        address: String,
        workspace: PathBuf,
        settings: PathBuf,
        instances: PathBuf,
        server: Option<Child>,
    }

    impl Harness {
        fn start(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "agenterm-dynacore-in-process-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time")
                    .as_nanos()
            ));
            fs::create_dir_all(&root).expect("create isolation root");
            let address = TcpListener::bind("127.0.0.1:0")
                .expect("reserve loopback address")
                .local_addr()
                .expect("loopback address")
                .to_string();
            let instances = root.join("instances");
            fs::create_dir_all(&instances).expect("create instance directory");
            let mut harness = Self {
                workspace: root.join("workspace.json"),
                settings: root.join("settings.json"),
                instances,
                address,
                root,
                server: None,
            };
            harness.server = Some(
                Command::new(locate_agenterm_server_exe())
                    .args(["server", "--address", &harness.address])
                    .env("AGENTERM_IPC_ADDRESS", &harness.address)
                    .env("AGENTERM_WORKSPACE_PATH", &harness.workspace)
                    .env("AGENTERM_SETTINGS_PATH", &harness.settings)
                    .env("AGENTERM_INSTANCE_DIR", &harness.instances)
                    .env("AGENTERM_NO_ACTIVATE", "1")
                    .env("AGENTERM_HEADLESS_VIEWPORT", "1400x900")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("start isolated headless server"),
            );
            harness.wait_ready();
            harness
        }

        fn wait_ready(&self) {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if crate::client::send_ipc_request_to_timeout(
                    &self.address,
                    vec!["protocol-info".to_owned()],
                    Duration::from_millis(500),
                )
                .is_ok_and(|response| response.ok)
                {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "server never became reachable"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            if let Some(mut server) = self.server.take() {
                let _ = crate::client::send_ipc_request_to_timeout(
                    &self.address,
                    vec!["kill-server".to_owned()],
                    Duration::from_secs(5),
                );
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    if matches!(server.try_wait(), Ok(Some(_))) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = server.kill();
                let _ = server.wait();
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// In-process fleet bridge for the proofs below: routes `tabs.list` to a
    /// real, already-running server via `crate::client::send_ipc_request_to_timeout`
    /// (a direct function call, one real IPC socket per call, no subprocess
    /// spawn) and projects the real `tabs` field — the exact translation
    /// `client::handle_script_broker` performs for the `tabs.list` broker
    /// operation in production.
    fn real_tabs_list_bridge(address: String) -> DynacoreFleetBridgeFn {
        Arc::new(move |operation_id: &str, _params_json: &str| -> Result<String, String> {
            if operation_id != "tabs.list" {
                return Err(format!("test bridge has no mapping for {operation_id}"));
            }
            let response = crate::client::send_ipc_request_to_timeout(
                &address,
                vec!["ui-snapshot".to_owned()],
                Duration::from_secs(5),
            )
            .map_err(|error| format!("send_ipc_request_to_timeout failed: {error:#}"))?;
            if !response.ok {
                return Err(format!("ui-snapshot failed: {}", response.error));
            }
            let snapshot: serde_json::Value = serde_json::from_str(&response.output)
                .map_err(|error| format!("ui-snapshot did not print JSON: {error}"))?;
            let tabs = snapshot
                .get("tabs")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            Ok(tabs.to_string())
        })
    }

    /// Acceptance: N dynacore `run_pack` invocations — each making TWO real
    /// `fleet.tabs.list` round trips against a live server — complete
    /// entirely within THIS test process. No `agenterm cli` (or any other)
    /// subprocess is spawned for any of the fleet calls, in contrast to the
    /// ONE reference subprocess spawn this same test also times, for scale.
    #[test]
    fn n_dynacore_fleet_calls_run_in_process_without_any_subprocess() {
        let harness = Harness::start("cheap");
        let bridge = real_tabs_list_bridge(harness.address.clone());

        let module = script_dynacore_host::demo_pack_module();
        let verified = script_dynacore_host::verify_pack(&module)
            .expect("the demo pack must verify against the real OPERATION_CATALOG");

        const ITERATIONS: usize = 100;
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            let outcome = script_dynacore_host::run_pack(&verified, &bridge);
            assert_eq!(
                outcome.termination,
                Termination::Exited(1),
                "against a reachable server tabs.list succeeds every time"
            );
            assert_eq!(outcome.calls.len(), 2);
        }
        let in_process_elapsed = started.elapsed();
        let calls_made = ITERATIONS * 2;
        let avg_in_process_call = in_process_elapsed / calls_made as u32;

        // Reference point: ONE `agenterm cli ui-snapshot` subprocess call —
        // the EXACT SAME real work (`ui-snapshot` against this same live
        // server) the old `dynacore_live_server.rs`/`dynacore_run.rs`
        // bridge paid a fresh subprocess spawn for, on EVERY SINGLE fleet
        // call. Doing the identical operation isolates the subprocess-spawn
        // tax as the only difference from this test's in-process bridge
        // (which reaches the same `ui-snapshot` IPC path, just via a direct
        // function call instead of `Command::spawn`).
        let spawn_started = Instant::now();
        let output = Command::new(locate_agenterm_server_exe())
            .args(["cli", "--address", &harness.address, "ui-snapshot"])
            .output()
            .expect("spawn a reference agenterm cli subprocess");
        let single_subprocess_elapsed = spawn_started.elapsed();
        assert!(
            output.status.success(),
            "reference agenterm cli ui-snapshot subprocess must itself succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        println!(
            "dynacore in-process: {calls_made} real fleet_calls in {in_process_elapsed:?} \
             (avg {avg_in_process_call:?}/call); one reference `agenterm cli ui-snapshot` \
             subprocess doing the SAME work took {single_subprocess_elapsed:?}"
        );

        assert!(
            avg_in_process_call < single_subprocess_elapsed,
            "average in-process fleet_call ({avg_in_process_call:?}) should be cheaper than \
             paying a subprocess spawn ON TOP OF the same ui-snapshot IPC work \
             ({single_subprocess_elapsed:?}) — the old per-call subprocess architecture paid \
             that subprocess tax for every one of the {calls_made} calls this test made \
             in-process instead"
        );
    }

    /// Acceptance: real fleet_call semantics, real data — not a mocked
    /// bridge. The demo pack's Ok arm branches on a REAL round trip's Ok/Err
    /// outcome and makes its second call using that real result, proving
    /// this in-process path produces the same correctness the old
    /// subprocess-bridged `tests/dynacore_live_server.rs` proved, without
    /// paying a subprocess per call.
    #[test]
    fn dynacore_pack_reflects_real_server_data_through_the_in_process_bridge() {
        let harness = Harness::start("correctness");
        let bridge = real_tabs_list_bridge(harness.address.clone());

        let module = script_dynacore_host::demo_pack_module();
        let verified = script_dynacore_host::verify_pack(&module).expect("verify");

        let outcome = script_dynacore_host::run_pack(&verified, &bridge);
        assert_eq!(
            outcome.termination,
            Termination::Exited(1),
            "a reachable server takes the Ok arm and exits with the second call's dest word"
        );
        assert_eq!(outcome.calls.len(), 2, "the Ok arm makes exactly two real round trips");
        for call in &outcome.calls {
            assert_eq!(call.operation_id, "tabs.list");
            assert_eq!(call.params_json, "{}");
            let tabs: serde_json::Value = serde_json::from_str(
                call.result
                    .as_ref()
                    .expect("a real, reachable server answers tabs.list with Ok"),
            )
            .expect("a real server's tabs.list result is JSON");
            assert!(
                tabs.is_array(),
                "a real ui-snapshot's tabs field is a JSON array, not a hand-written mock \
                 payload: {tabs}"
            );
        }
    }
}
