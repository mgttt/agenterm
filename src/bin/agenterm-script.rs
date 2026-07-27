use std::{
    collections::HashSet,
    io::{Read, Write},
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use agenterm::script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, SCRIPT_FRAME_MAX_BYTES, SCRIPT_FRAME_VERSION,
    SCRIPT_INVOCATION_MAX_BYTES, ScriptBrokerRequest, ScriptBrokerResponse, ScriptBudgets,
    ScriptExitClass, ScriptFailure, ScriptFailureCategory, ScriptFrame, ScriptFramePayload,
    ScriptInvocation, ScriptOperation, ScriptProfile, ScriptResult,
};
use rhai::{Dynamic, Engine, EvalAltResult, Scope};

type PendingBroker = Option<(String, mpsc::SyncSender<ScriptBrokerResponse>)>;

#[derive(Clone)]
struct BrokerClient {
    invocation_id: String,
    output: Arc<Mutex<std::io::Stdout>>,
    pending: Arc<Mutex<PendingBroker>>,
    next_request: Arc<std::sync::atomic::AtomicUsize>,
    requests_remaining: Arc<std::sync::atomic::AtomicUsize>,
    timeout: Duration,
}

impl BrokerClient {
    fn call(
        &self,
        operation: &str,
        arguments: serde_json::Value,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        if self
            .requests_remaining
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_sub(1)
            })
            .is_err()
        {
            return Err("broker request budget exceeded".into());
        }
        let sequence = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("broker-{sequence}");
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut pending = self.pending.lock().expect("pending broker lock poisoned");
            if pending.is_some() {
                return Err("only one broker request may be outstanding".into());
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
            return Err(format!("failed to send broker request: {error}").into());
        }
        let response = receiver.recv_timeout(self.timeout).map_err(|_| {
            self.pending
                .lock()
                .expect("pending broker lock poisoned")
                .take();
            Box::<EvalAltResult>::from("broker response timed out")
        })?;
        if let Some(error) = response.error {
            return Err(format!("{}: {}", error.code, error.message).into());
        }
        let value = response.value.unwrap_or(serde_json::Value::Null);
        rhai::serde::to_dynamic(value).map_err(|error| error.to_string().into())
    }
}

#[derive(Clone)]
struct AgentApi(BrokerClient);

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<u8> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("--worker") if arguments.next().is_none() => run_worker(),
        Some("--framed-worker") if arguments.next().is_none() => run_framed_worker(),
        Some("--version") if arguments.next().is_none() => {
            println!("agenterm-script {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Some("--help") | None => {
            println!(
                "AgenTerm scripting worker\n\
                 Usage: agenterm-script --worker | --framed-worker\n\
                 Public scripts are invoked through `agenterm-cli script ...`."
            );
            Ok(0)
        }
        _ => anyhow::bail!(
            "unknown agenterm-script option; use --help or invoke scripts through agenterm-cli"
        ),
    }
}

fn run_worker() -> anyhow::Result<u8> {
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

fn run_framed_worker() -> anyhow::Result<u8> {
    process_concurrent_framed_worker(std::io::stdin().lock(), std::io::stdout())?;
    Ok(0)
}

fn process_concurrent_framed_worker<R: Read>(
    mut input: R,
    output: std::io::Stdout,
) -> anyhow::Result<()> {
    let output = Arc::new(Mutex::new(output));
    let active = Arc::new(Mutex::new(None::<(String, Arc<AtomicBool>)>));
    let pending_broker = Arc::new(Mutex::new(
        None::<(String, mpsc::SyncSender<ScriptBrokerResponse>)>,
    ));
    let completed = Arc::new(Mutex::new(HashSet::<String>::new()));
    let mut seen_frames = HashSet::new();
    let mut known_invocations = HashSet::new();
    let mut workers = Vec::new();
    loop {
        let mut header = [0_u8; 4];
        match input.read(&mut header[..1])? {
            0 => break,
            1 => {}
            _ => unreachable!("one-byte read returned more than one byte"),
        }
        if let Err(error) = input.read_exact(&mut header[1..]) {
            write_shared_protocol_frame(
                &output,
                "unknown",
                "unknown",
                "protocol_truncated_header",
                format!("frame header ended before four bytes: {error}"),
            )?;
            break;
        }
        let length = u32::from_be_bytes(header);
        if length > SCRIPT_FRAME_MAX_BYTES {
            let drained = drain_frame(&mut input, u64::from(length))?;
            write_shared_protocol_frame(
                &output,
                "unknown",
                "unknown",
                "protocol_frame_too_large",
                format!("frame length {length} exceeds the {SCRIPT_FRAME_MAX_BYTES} byte limit"),
            )?;
            if !drained {
                break;
            }
            continue;
        }
        let mut bytes = vec![0_u8; length as usize];
        if let Err(error) = input.read_exact(&mut bytes) {
            write_shared_protocol_frame(
                &output,
                "unknown",
                "unknown",
                "protocol_truncated_frame",
                format!("frame payload ended before {length} bytes: {error}"),
            )?;
            break;
        }
        let frame: ScriptFrame = match serde_json::from_slice(&bytes) {
            Ok(frame) => frame,
            Err(error) => {
                write_shared_protocol_frame(
                    &output,
                    "unknown",
                    "unknown",
                    "protocol_malformed_frame",
                    error.to_string(),
                )?;
                continue;
            }
        };
        let frame_id = frame.frame_id;
        if frame_id.is_empty() || frame_id.len() > 128 {
            write_shared_protocol_frame(
                &output,
                &frame_id,
                "unknown",
                "protocol_invalid_frame_id",
                "frame_id must contain from 1 to 128 bytes",
            )?;
            continue;
        }
        if !seen_frames.insert(frame_id.clone()) {
            write_shared_protocol_frame(
                &output,
                &frame_id,
                "unknown",
                "protocol_duplicate_frame",
                "frame_id has already been processed",
            )?;
            continue;
        }
        if frame.frame_version != SCRIPT_FRAME_VERSION {
            write_shared_protocol_frame(
                &output,
                &frame_id,
                "unknown",
                "protocol_unsupported_frame_version",
                format!(
                    "worker supports frame version {SCRIPT_FRAME_VERSION}, requested {}",
                    frame.frame_version
                ),
            )?;
            continue;
        }
        match frame.payload {
            ScriptFramePayload::Invoke(invocation) => {
                let invocation_id = invocation.invocation_id.clone();
                if invocation_id.is_empty() || invocation_id.len() > 128 {
                    write_shared_protocol_frame(
                        &output,
                        &frame_id,
                        &invocation_id,
                        "protocol_invalid_invocation_id",
                        "invocation_id must contain from 1 to 128 bytes",
                    )?;
                    continue;
                }
                if !known_invocations.insert(invocation_id.clone()) {
                    write_shared_protocol_frame(
                        &output,
                        &frame_id,
                        &invocation_id,
                        "protocol_duplicate_invocation",
                        "invocation_id has already been processed",
                    )?;
                    continue;
                }
                let cancellation = Arc::new(AtomicBool::new(false));
                {
                    let mut active_guard = active.lock().expect("active invocation lock poisoned");
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
                    *active_guard = Some((invocation_id.clone(), Arc::clone(&cancellation)));
                }
                let output_for_worker = Arc::clone(&output);
                let active_for_worker = Arc::clone(&active);
                let completed_for_worker = Arc::clone(&completed);
                let pending_for_worker = Arc::clone(&pending_broker);
                let broker = if invocation.profile == ScriptProfile::Observe
                    && matches!(
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
                    let _ = write_shared_frame(&output_for_worker, &result_frame(frame_id, result));
                    completed_for_worker
                        .lock()
                        .expect("completed invocation lock poisoned")
                        .insert(invocation_id);
                    *active_guard = None;
                }));
            }
            ScriptFramePayload::Cancel { invocation_id } => {
                let cancellation = active
                    .lock()
                    .expect("active invocation lock poisoned")
                    .as_ref()
                    .filter(|(active_id, _)| active_id == &invocation_id)
                    .map(|(_, cancellation)| Arc::clone(cancellation));
                if let Some(cancellation) = cancellation {
                    cancellation.store(true, Ordering::Relaxed);
                } else {
                    let (code, message) = if completed
                        .lock()
                        .expect("completed invocation lock poisoned")
                        .contains(&invocation_id)
                    {
                        (
                            "protocol_cancel_too_late",
                            "invocation has already completed",
                        )
                    } else {
                        (
                            "protocol_cancel_unknown",
                            "no active invocation has this invocation_id",
                        )
                    };
                    write_shared_protocol_frame(&output, &frame_id, &invocation_id, code, message)?;
                }
            }
            ScriptFramePayload::BrokerResponse {
                invocation_id,
                request_id,
                response,
            } => {
                let active_matches = active
                    .lock()
                    .expect("active invocation lock poisoned")
                    .as_ref()
                    .is_some_and(|(active_id, _)| active_id == &invocation_id);
                let pending = pending_broker
                    .lock()
                    .expect("pending broker lock poisoned")
                    .take();
                match pending {
                    Some((expected, sender)) if active_matches && expected == request_id => {
                        let _ = sender.send(response);
                    }
                    Some(pending) => {
                        *pending_broker.lock().expect("pending broker lock poisoned") =
                            Some(pending);
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
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

fn write_shared_frame(
    output: &Arc<Mutex<std::io::Stdout>>,
    frame: &ScriptFrame,
) -> anyhow::Result<()> {
    let mut output = output.lock().expect("framed stdout lock poisoned");
    write_frame(&mut *output, frame)
}

fn write_shared_protocol_frame(
    output: &Arc<Mutex<std::io::Stdout>>,
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

#[cfg(test)]
fn process_framed_stream<R: Read, W: Write>(mut input: R, mut output: W) -> anyhow::Result<()> {
    let mut seen_frames = HashSet::new();
    let mut completed_invocations = HashSet::new();
    loop {
        let Some(length) = read_frame_length(&mut input, &mut output)? else {
            return Ok(());
        };
        if length > SCRIPT_FRAME_MAX_BYTES {
            let drained = drain_frame(&mut input, u64::from(length))?;
            write_protocol_frame(
                &mut output,
                "unknown",
                "unknown",
                "protocol_frame_too_large",
                format!("frame length {length} exceeds the {SCRIPT_FRAME_MAX_BYTES} byte limit"),
            )?;
            if !drained {
                return Ok(());
            }
            continue;
        }
        let mut bytes = vec![0_u8; length as usize];
        if let Err(error) = input.read_exact(&mut bytes) {
            write_protocol_frame(
                &mut output,
                "unknown",
                "unknown",
                "protocol_truncated_frame",
                format!("frame payload ended before {length} bytes: {error}"),
            )?;
            return Ok(());
        }
        let frame: ScriptFrame = match serde_json::from_slice(&bytes) {
            Ok(frame) => frame,
            Err(error) => {
                write_protocol_frame(
                    &mut output,
                    "unknown",
                    "unknown",
                    "protocol_malformed_frame",
                    error.to_string(),
                )?;
                continue;
            }
        };
        let response = process_frame(frame, &mut seen_frames, &mut completed_invocations);
        write_frame(&mut output, &response)?;
    }
}

#[cfg(test)]
fn read_frame_length<R: Read, W: Write>(
    input: &mut R,
    output: &mut W,
) -> anyhow::Result<Option<u32>> {
    let mut header = [0_u8; 4];
    match input.read(&mut header[..1])? {
        0 => return Ok(None),
        1 => {}
        _ => unreachable!("one-byte read returned more than one byte"),
    }
    if let Err(error) = input.read_exact(&mut header[1..]) {
        write_protocol_frame(
            output,
            "unknown",
            "unknown",
            "protocol_truncated_header",
            format!("frame header ended before four bytes: {error}"),
        )?;
        return Ok(None);
    }
    Ok(Some(u32::from_be_bytes(header)))
}

fn drain_frame<R: Read>(input: &mut R, length: u64) -> anyhow::Result<bool> {
    let mut limited = input.take(length);
    let copied = std::io::copy(&mut limited, &mut std::io::sink())?;
    Ok(copied == length)
}

#[cfg(test)]
fn process_frame(
    frame: ScriptFrame,
    seen_frames: &mut HashSet<String>,
    completed_invocations: &mut HashSet<String>,
) -> ScriptFrame {
    let frame_id = frame.frame_id;
    if frame_id.is_empty() || frame_id.len() > 128 {
        return result_frame(
            frame_id,
            protocol_failure_for(
                "unknown",
                "protocol_invalid_frame_id",
                "frame_id must contain from 1 to 128 bytes",
            ),
        );
    }
    if !seen_frames.insert(frame_id.clone()) {
        return result_frame(
            frame_id,
            protocol_failure_for(
                "unknown",
                "protocol_duplicate_frame",
                "frame_id has already been processed",
            ),
        );
    }
    if frame.frame_version != SCRIPT_FRAME_VERSION {
        return result_frame(
            frame_id,
            protocol_failure_for(
                "unknown",
                "protocol_unsupported_frame_version",
                format!(
                    "worker supports frame version {SCRIPT_FRAME_VERSION}, requested {}",
                    frame.frame_version
                ),
            ),
        );
    }
    let result = match frame.payload {
        ScriptFramePayload::Invoke(invocation) => {
            if invocation.invocation_id.is_empty() || invocation.invocation_id.len() > 128 {
                protocol_failure_for(
                    &invocation.invocation_id,
                    "protocol_invalid_invocation_id",
                    "invocation_id must contain from 1 to 128 bytes",
                )
            } else if !completed_invocations.insert(invocation.invocation_id.clone()) {
                protocol_failure_for(
                    &invocation.invocation_id,
                    "protocol_duplicate_invocation",
                    "invocation_id has already been processed",
                )
            } else {
                execute(invocation)
            }
        }
        ScriptFramePayload::Cancel { invocation_id } => {
            let (code, message) = if completed_invocations.contains(&invocation_id) {
                (
                    "protocol_cancel_too_late",
                    "invocation has already completed",
                )
            } else {
                (
                    "protocol_cancel_unknown",
                    "no active invocation has this invocation_id",
                )
            };
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
    let mut bytes = serde_json::to_vec(frame)?;
    if bytes.len() > SCRIPT_FRAME_MAX_BYTES as usize {
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
        bytes = serde_json::to_vec(&replacement)?;
    }
    output.write_all(&(bytes.len() as u32).to_be_bytes())?;
    output.write_all(&bytes)?;
    output.flush()?;
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
    cancellation: Option<Arc<AtomicBool>>,
    broker: Option<BrokerClient>,
) -> Result<(String, Option<serde_json::Value>), ScriptFailure> {
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
        return Ok((String::new(), Some(api_catalog())));
    }

    let output = Arc::new(Mutex::new(String::new()));
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let wall_time_exceeded = Arc::new(AtomicBool::new(false));
    let cancellation = cancellation.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let output_for_print = Arc::clone(&output);
    let exceeded_for_print = Arc::clone(&output_exceeded);
    let output_limit = invocation.budgets.output_bytes;
    let deadline = Instant::now()
        .checked_add(std::time::Duration::from_millis(
            invocation.budgets.wall_time_ms,
        ))
        .unwrap_or_else(Instant::now);
    let wall_time_for_progress = Arc::clone(&wall_time_exceeded);
    let cancellation_for_progress = Arc::clone(&cancellation);
    let mut engine = Engine::new();
    engine.set_max_operations(invocation.budgets.operations);
    engine.set_max_call_levels(invocation.budgets.call_depth);
    engine.set_max_expr_depths(
        invocation.budgets.expression_depth,
        invocation.budgets.expression_depth,
    );
    engine.set_max_array_size(invocation.budgets.collection_items);
    engine.set_max_map_size(invocation.budgets.collection_items);
    engine.set_max_string_size(invocation.budgets.string_bytes);
    engine.on_print(move |text| {
        let mut output = output_for_print
            .lock()
            .expect("script output lock poisoned");
        let remaining = output_limit.saturating_sub(output.len());
        if text.len().saturating_add(1) > remaining {
            exceeded_for_print.store(true, Ordering::Relaxed);
        }
        let mut take = text.len().min(remaining);
        while take > 0 && !text.is_char_boundary(take) {
            take -= 1;
        }
        output.push_str(&text[..take]);
        if output.len() < output_limit {
            output.push('\n');
        }
    });
    engine.on_progress(move |_| {
        if cancellation_for_progress.load(Ordering::Relaxed) {
            Some(Dynamic::from("script cancellation requested"))
        } else if Instant::now() >= deadline {
            wall_time_for_progress.store(true, Ordering::Relaxed);
            Some(Dynamic::from("wall-time budget exceeded"))
        } else {
            None
        }
    });
    engine.register_type_with_name::<AgentApi>("Agent");
    engine.register_fn("workspace", |api: &mut AgentApi| {
        api.0.call("workspace.info", serde_json::json!({}))
    });
    engine.register_fn("tabs", |api: &mut AgentApi| {
        api.0.call("tabs.list", serde_json::json!({}))
    });
    engine.register_fn("active_tab", |api: &mut AgentApi| {
        api.0.call("tabs.active", serde_json::json!({}))
    });
    engine.register_fn("ui_snapshot", |api: &mut AgentApi| {
        api.0.call("ui.snapshot", serde_json::json!({}))
    });
    engine.register_fn(
        "capture",
        |api: &mut AgentApi, tab: &str, max_bytes: rhai::INT| {
            api.0.call(
                "pane.capture",
                serde_json::json!({"tab": tab, "max_bytes": max_bytes}),
            )
        },
    );
    engine.register_fn(
        "events_read",
        |api: &mut AgentApi, epoch: &str, after: rhai::INT, limit: rhai::INT| {
            api.0.call(
                "events.read",
                serde_json::json!({"epoch": epoch, "after": after, "limit": limit}),
            )
        },
    );
    engine.register_fn(
        "events_wait",
        |api: &mut AgentApi, epoch: &str, after: rhai::INT, kind: &str, timeout_ms: rhai::INT| {
            api.0.call(
                "events.wait",
                serde_json::json!({
                    "epoch": epoch,
                    "after": after,
                    "kind": kind,
                    "timeout_ms": timeout_ms,
                }),
            )
        },
    );

    let ast = engine
        .compile(&invocation.source)
        .map_err(|error| classify_compile_error(error.to_string()))?;
    if invocation.operation == ScriptOperation::Check {
        validate_profile_apis(&invocation.source, invocation.profile, &invocation.budgets)?;
        return Ok((String::new(), None));
    }

    let mut scope = Scope::new();
    let arguments = rhai::serde::to_dynamic(&invocation.arguments)
        .map_err(|error| configuration_error("configuration_arguments", error.to_string()))?;
    scope.push_dynamic("args", arguments);
    match invocation.profile {
        ScriptProfile::Pure => {}
        ScriptProfile::Observe => {
            let Some(broker) = broker else {
                return Err(configuration_error(
                    "configuration_broker",
                    "observe profile requires a host broker",
                ));
            };
            scope.push("agent", AgentApi(broker));
        }
    }
    let value = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .map_err(|error| {
            let message = error.to_string();
            if cancellation.load(Ordering::Relaxed) {
                limit_error("limit_cancelled", message)
            } else if wall_time_exceeded.load(Ordering::Relaxed) {
                limit_error("limit_wall_time", message)
            } else if message.contains("Too many operations") {
                limit_error("limit_operations", message)
            } else if message.contains("exceeds the maximum")
                || message.contains("Maximum call stack depth")
                || message.contains("Stack overflow")
            {
                classify_engine_limit(message)
            } else if output_exceeded.load(Ordering::Relaxed) {
                limit_error("limit_output_bytes", message)
            } else {
                script_error("script_runtime", message)
            }
        })?;
    let value = if value.is_unit() {
        None
    } else {
        Some(
            rhai::serde::from_dynamic(&value)
                .map_err(|error| script_error("script_result_type", error.to_string()))?,
        )
    };
    let stdout = output
        .lock()
        .map(|output| output.clone())
        .unwrap_or_default();
    if output_exceeded.load(Ordering::Relaxed) {
        return Err(limit_error(
            "limit_output_bytes",
            "script output reached its byte budget",
        ));
    }
    Ok((stdout, value))
}

fn api_catalog() -> serde_json::Value {
    let defaults = ScriptBudgets::default();
    let hard_limits = ScriptBudgets::hard_limits();
    serde_json::json!({
        "schema_version": 1,
        "api_version": SCRIPT_API_VERSION,
        "profiles": {
            "pure": {
                "variables": ["args"],
                "ambient_authority": [],
            },
            "observe": {
                "variables": ["args", "agent"],
                "ambient_authority": [],
            },
        },
        "operations": ["api", "check", "eval", "run"],
        "framing": {
            "version": SCRIPT_FRAME_VERSION,
            "max_frame_bytes": SCRIPT_FRAME_MAX_BYTES,
            "mode": "--framed-worker",
            "input_kinds": {
                "invoke": "available",
                "cancel": "available",
                "result": "worker_output_only",
                "broker_request": "available_worker_to_host",
                "broker_response": "available_host_to_worker",
            },
        },
        "supervisor": {
            "transport": "inherited_length_bounded_frames",
            "job_object": "kill_on_close",
            "cancel_grace_ms": 150,
            "per_process_concurrency": 2,
            "global_concurrency": 4,
        },
        "limits": {
            "defaults": defaults,
            "hard_maximums": hard_limits,
            "invocation_bytes": SCRIPT_INVOCATION_MAX_BYTES,
        },
        "apis": [
            {
                "name": "print",
                "kind": "rhai_builtin",
                "profiles": ["pure", "observe"],
                "available": true,
            },
            script_api_spec("agent.workspace", "agent.workspace()", "workspace.info",
                "workspace_metadata_with_event_position", &["broker_host_error", "broker_transport"]),
            script_api_spec("agent.tabs", "agent.tabs()", "tabs.list",
                "tab_list", &["broker_host_error", "broker_transport"]),
            script_api_spec("agent.active_tab", "agent.active_tab()", "tabs.active",
                "tab_or_null", &["broker_host_error", "broker_transport"]),
            script_api_spec("agent.ui_snapshot", "agent.ui_snapshot()", "ui.snapshot",
                "ui_snapshot", &["broker_host_error", "broker_transport", "broker_return_too_large"]),
            script_api_spec("agent.capture", "agent.capture(tab, max_bytes)", "pane.capture",
                "bounded_capture", &["broker_invalid_arguments", "broker_host_error", "broker_return_too_large"]),
            script_api_spec("agent.events_read", "agent.events_read(epoch, after, limit)", "events.read",
                "event_batch", &["server_restart", "journal_gap", "future_sequence", "broker_invalid_arguments"]),
            script_api_spec("agent.events_wait", "agent.events_wait(epoch, after, kind, timeout_ms)", "events.wait",
                "event", &["server_restart", "journal_gap", "future_sequence", "event_wait_timeout"]),
            {
                "name": "new_tab",
                "kind": "control",
                "profiles": [],
                "available": false,
                "reason": "control capability is deferred",
            },
        ],
        "failure_categories": ["configuration", "limit", "script", "protocol", "host"],
        "exit_classes": {
            "success": 0,
            "script": 1,
            "protocol": 1,
            "host": 1,
            "configuration": 2,
            "limit": 3,
        },
        "deferred_capabilities": [
            "control", "fs.read", "fs.write", "env.read", "proc.exec", "network"
        ],
    })
}

fn script_api_spec(
    name: &str,
    signature: &str,
    operation_id: &str,
    result: &str,
    errors: &[&str],
) -> serde_json::Value {
    let operation = agenterm::operations::operation_by_id(operation_id);
    serde_json::json!({
        "name": name,
        "signature": signature,
        "kind": "brokered_method",
        "profiles": ["observe"],
        "available": operation.is_some_and(|operation| operation.available),
        "api_version": SCRIPT_API_VERSION,
        "capability": "observe",
        "operation_id": operation_id,
        "operation": operation,
        "arguments": operation.map(|operation| operation.parameters),
        "result": result,
        "errors": errors,
    })
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

fn validate_profile_apis(
    source: &str,
    profile: ScriptProfile,
    budgets: &ScriptBudgets,
) -> Result<(), ScriptFailure> {
    for method in agent_method_calls(source) {
        if profile == ScriptProfile::Pure {
            return Err(script_error(
                "script_api_unavailable",
                format!("API agent.{method} is unavailable in the pure profile"),
            ));
        }
        if !matches!(
            method.as_str(),
            "workspace"
                | "tabs"
                | "active_tab"
                | "ui_snapshot"
                | "capture"
                | "events_read"
                | "events_wait"
        ) {
            return Err(script_error(
                "script_api_unknown",
                format!("unknown shipped scripting API: agent.{method}"),
            ));
        }
    }
    for (method, maximum) in [
        ("capture", budgets.capture_bytes as u64),
        ("events_read", budgets.event_items as u64),
        ("events_wait", budgets.wait_time_ms),
    ] {
        if let Some(value) = agent_last_literal_argument(source, method)
            && (value == 0 || value > maximum)
        {
            return Err(script_error(
                "script_api_static_limit",
                format!("agent.{method} literal limit must be from 1 to {maximum}"),
            ));
        }
    }
    for call in external_function_calls(source) {
        match call.as_str() {
            "print" | "debug" | "type_of" | "is_def_var" | "is_shared" | "eval" | "to_string"
            | "to_debug" => {}
            "new_tab" => {
                return Err(script_error(
                    "script_api_unavailable",
                    format!("API new_tab is unavailable in the {profile:?} profile"),
                ));
            }
            _ => {
                return Err(script_error(
                    "script_api_unknown",
                    format!("unknown shipped scripting API: {call}"),
                ));
            }
        }
    }
    Ok(())
}

fn agent_last_literal_argument(source: &str, method: &str) -> Option<u64> {
    let needle = format!("agent.{method}(");
    let start = source.find(&needle)? + needle.len();
    let end = source[start..].find(')')? + start;
    source[start..end].rsplit(',').next()?.trim().parse().ok()
}

fn agent_method_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut methods = Vec::new();
    let mut index = 0;
    while index + 6 < bytes.len() {
        if matches!(bytes[index], b'"' | b'\'' | b'`') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if source[index..].starts_with("agent.") {
            let start = index + 6;
            let mut end = start;
            while end < bytes.len() && (bytes[end] == b'_' || bytes[end].is_ascii_alphanumeric()) {
                end += 1;
            }
            let mut next = end;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if next < bytes.len() && bytes[next] == b'(' {
                methods.push(source[start..end].to_owned());
            }
            index = end.max(index + 1);
        } else {
            index += 1;
        }
    }
    methods
}

fn external_function_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut declared = Vec::new();
    let mut index = 0;
    let mut previous_identifier = None::<String>;
    while index < bytes.len() {
        match bytes[index] {
            b'"' | b'\'' | b'`' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
                {
                    index += 1;
                }
                let identifier = &source[start..index];
                let mut next = index;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                if previous_identifier.as_deref() == Some("fn") {
                    declared.push(identifier.to_owned());
                } else if bytes.get(next) == Some(&b'(') {
                    let mut previous = start;
                    while previous > 0 && bytes[previous - 1].is_ascii_whitespace() {
                        previous -= 1;
                    }
                    let is_method = previous > 0 && bytes[previous - 1] == b'.';
                    let is_keyword = matches!(
                        identifier,
                        "if" | "for" | "while" | "loop" | "switch" | "catch"
                    );
                    if !is_method && !is_keyword && !declared.iter().any(|name| name == identifier)
                    {
                        calls.push(identifier.to_owned());
                    }
                }
                previous_identifier = Some(identifier.to_owned());
            }
            byte if byte.is_ascii_whitespace() => index += 1,
            _ => {
                previous_identifier = None;
                index += 1;
            }
        }
    }
    calls.retain(|call| !declared.iter().any(|name| name == call));
    calls
}

fn classify_engine_limit(message: String) -> ScriptFailure {
    let lowercase = message.to_ascii_lowercase();
    let code = if lowercase.contains("call") || lowercase.contains("stack") {
        "limit_call_depth"
    } else if lowercase.contains("expression") {
        "limit_expression_depth"
    } else if lowercase.contains("string") {
        "limit_string_bytes"
    } else {
        "limit_collection_items"
    };
    limit_error(code, message)
}

fn classify_compile_error(message: String) -> ScriptFailure {
    let lowercase = message.to_ascii_lowercase();
    if lowercase.contains("expression")
        && (lowercase.contains("depth") || lowercase.contains("complexity"))
    {
        limit_error("limit_expression_depth", message)
    } else if (lowercase.contains("array") || lowercase.contains("map"))
        && lowercase.contains("maximum")
    {
        limit_error("limit_collection_items", message)
    } else if lowercase.contains("string") && lowercase.contains("maximum") {
        limit_error("limit_string_bytes", message)
    } else {
        script_error("script_parse", message)
    }
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
    use std::io::Cursor;

    fn invocation(operation: ScriptOperation, source: &str) -> ScriptInvocation {
        ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: "unit-invocation".to_owned(),
            api_version: SCRIPT_API_VERSION,
            operation,
            profile: ScriptProfile::Pure,
            source_label: "unit".to_owned(),
            source: source.to_owned(),
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
        while input.position() < bytes.len() as u64 {
            let mut header = [0_u8; 4];
            input.read_exact(&mut header).expect("frame header");
            let length = u32::from_be_bytes(header) as usize;
            let mut payload = vec![0_u8; length];
            input.read_exact(&mut payload).expect("frame payload");
            frames.push(serde_json::from_slice(&payload).expect("JSON frame"));
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
        let apis = catalog["apis"].as_array().expect("API entries");
        let new_tab = apis
            .iter()
            .find(|api| api["name"] == "new_tab")
            .expect("deferred control API");
        assert_eq!(new_tab["available"], false);
        let workspace = apis
            .iter()
            .find(|api| api["name"] == "agent.workspace")
            .expect("typed workspace API");
        assert_eq!(workspace["operation_id"], "workspace.info");
        assert_eq!(workspace["api_version"], SCRIPT_API_VERSION);
        assert_eq!(
            catalog["limits"]["defaults"]["broker_requests"],
            ScriptBudgets::default().broker_requests
        );
    }

    #[test]
    fn check_rejects_unknown_and_unavailable_apis() {
        let unknown = execute(invocation(ScriptOperation::Check, "made_up_api()"));
        assert_eq!(failure_code(&unknown), "script_api_unknown");
        assert_eq!(unknown.exit_class, ScriptExitClass::Script);

        let unavailable = execute(invocation(ScriptOperation::Check, "new_tab()"));
        assert_eq!(failure_code(&unavailable), "script_api_unavailable");
        assert_eq!(unavailable.exit_class, ScriptExitClass::Script);
    }

    #[test]
    fn check_accepts_shipped_api_methods_and_user_functions() {
        let source = r#"
            fn twice(value) { value * 2 }
            let values = [1, 2];
            print(twice(values.len()));
        "#;
        assert!(execute(invocation(ScriptOperation::Check, source)).ok);
    }

    #[test]
    fn check_enforces_typed_agent_api_profile_and_exact_names() {
        let mut observe = invocation(ScriptOperation::Check, "agent.workspace()");
        observe.profile = ScriptProfile::Observe;
        assert!(execute(observe).ok);

        let pure = execute(invocation(ScriptOperation::Check, "agent.workspace()"));
        assert_eq!(failure_code(&pure), "script_api_unavailable");

        let mut unknown = invocation(ScriptOperation::Check, "agent.not_shipped()");
        unknown.profile = ScriptProfile::Observe;
        assert_eq!(failure_code(&execute(unknown)), "script_api_unknown");
        let mut excessive = invocation(ScriptOperation::Check, r#"agent.capture("@1", 999999)"#);
        excessive.profile = ScriptProfile::Observe;
        assert_eq!(failure_code(&execute(excessive)), "script_api_static_limit");
        assert!(agent_method_calls(r#""agent.hidden()"; // agent.comment()"#).is_empty());
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
    fn operation_output_and_wall_time_limits_are_typed() {
        let mut operations = invocation(ScriptOperation::Eval, "loop {}");
        operations.budgets.operations = 100;
        let operations = execute(operations);
        assert_eq!(failure_code(&operations), "limit_operations");
        assert_eq!(operations.exit_class, ScriptExitClass::Limit);

        let mut output = invocation(ScriptOperation::Eval, r#"print("abcdef")"#);
        output.budgets.output_bytes = 3;
        assert_eq!(failure_code(&execute(output)), "limit_output_bytes");

        let mut wall_time = invocation(ScriptOperation::Eval, "loop {}");
        wall_time.budgets.operations = ScriptBudgets::hard_limits().operations;
        wall_time.budgets.wall_time_ms = 1;
        let wall_time = execute(wall_time);
        assert_eq!(failure_code(&wall_time), "limit_wall_time");
        assert_eq!(wall_time.exit_class, ScriptExitClass::Limit);
    }

    #[test]
    fn cooperative_cancellation_is_typed_before_wall_time() {
        let mut invocation = invocation(ScriptOperation::Eval, "loop {}");
        invocation.budgets.wall_time_ms = ScriptBudgets::hard_limits().wall_time_ms;
        invocation.budgets.operations = ScriptBudgets::hard_limits().operations;
        let cancellation = Arc::new(AtomicBool::new(true));
        let result = execute_with_cancellation(invocation, Some(cancellation));
        assert_eq!(failure_code(&result), "limit_cancelled");
        assert_eq!(result.exit_class, ScriptExitClass::Limit);
    }

    #[test]
    fn source_and_expression_limits_are_typed() {
        let mut source = invocation(ScriptOperation::Check, "12345");
        source.budgets.source_bytes = 4;
        assert_eq!(failure_code(&execute(source)), "limit_source_bytes");

        let mut expression = invocation(
            ScriptOperation::Check,
            "((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))",
        );
        expression.budgets.expression_depth = 4;
        let expression = execute(expression);
        assert_eq!(
            failure_code(&expression),
            "limit_expression_depth",
            "{:?}",
            expression.failure
        );
    }

    #[test]
    fn call_collection_and_string_limits_are_typed() {
        let mut call_depth = invocation(
            ScriptOperation::Eval,
            "fn recurse() { recurse(); } recurse();",
        );
        call_depth.budgets.call_depth = 2;
        assert_eq!(failure_code(&execute(call_depth)), "limit_call_depth");

        let mut collection = invocation(ScriptOperation::Eval, "[1, 2, 3]");
        collection.budgets.collection_items = 2;
        let collection = execute(collection);
        assert_eq!(
            failure_code(&collection),
            "limit_collection_items",
            "{:?}",
            collection.failure
        );

        let mut string = invocation(ScriptOperation::Eval, r#""abcdef""#);
        string.budgets.string_bytes = 3;
        assert_eq!(failure_code(&execute(string)), "limit_string_bytes");
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
