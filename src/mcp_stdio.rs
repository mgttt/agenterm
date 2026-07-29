use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use serde_json::{Value, json};

use crate::{
    mcp_catalog::{MCP_PROTOCOL_REVISION, capabilities},
    mcp_fleet,
};

const JSON_RPC_VERSION: &str = "2.0";
const ERROR_PARSE: i64 = -32700;
const ERROR_INVALID_REQUEST: i64 = -32600;
const ERROR_METHOD_NOT_FOUND: i64 = -32601;
const ERROR_INVALID_PARAMS: i64 = -32602;
const ERROR_NOT_INITIALIZED: i64 = -32002;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionState {
    New,
    InitializeResponded,
    Ready,
}

enum BoundedLine {
    Eof,
    Line(Vec<u8>),
    Oversized,
}

enum ServerEvent {
    Input(BoundedLine),
    InputError(io::Error),
    WaitComplete {
        key: String,
        id: Value,
        result: Result<Value, mcp_fleet::McpFleetError>,
    },
}

struct ActiveWait {
    cancelled: Arc<AtomicBool>,
    worker: thread::JoinHandle<()>,
}

#[derive(Clone, Debug, Default)]
pub struct McpStdioConfig {
    pub address: Option<String>,
}

pub fn serve_stdio<R: BufRead + Send + 'static, W: Write>(input: R, output: W) -> io::Result<()> {
    serve_stdio_with_config(input, output, McpStdioConfig::default())
}

pub fn serve_stdio_with_config<R: BufRead + Send + 'static, W: Write>(
    input: R,
    mut output: W,
    config: McpStdioConfig,
) -> io::Result<()> {
    let limit = capabilities().limits.frame_bytes as usize;
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn({
        let sender = sender.clone();
        move || read_input(input, limit, sender)
    });
    let mut state = SessionState::New;
    let mut active = HashMap::<String, ActiveWait>::new();
    loop {
        match receiver.recv() {
            Ok(ServerEvent::Input(BoundedLine::Eof)) | Err(_) => {
                for wait in active.values() {
                    wait.cancelled.store(true, Ordering::Release);
                }
                for (_, wait) in active {
                    let _ = wait.worker.join();
                }
                let _ = reader.join();
                return Ok(());
            }
            Ok(ServerEvent::InputError(error)) => {
                for wait in active.values() {
                    wait.cancelled.store(true, Ordering::Release);
                }
                for (_, wait) in active {
                    let _ = wait.worker.join();
                }
                let _ = reader.join();
                return Err(error);
            }
            Ok(ServerEvent::Input(BoundedLine::Oversized)) => {
                write_message(
                    &mut output,
                    &error_response(
                        Value::Null,
                        ERROR_INVALID_REQUEST,
                        "MCP message exceeds the frame limit",
                        Some(json!({"maximum_bytes": limit})),
                    ),
                )?;
            }
            Ok(ServerEvent::Input(BoundedLine::Line(line))) => {
                let message = match decode_line(&line) {
                    Ok(message) => message,
                    Err(response) => {
                        write_message(&mut output, &response)?;
                        continue;
                    }
                };
                if handle_cancel_notification(&message, &active) {
                    continue;
                }
                if state == SessionState::Ready && is_tool_call(&message) {
                    match start_wait(
                        &message,
                        &config,
                        &sender,
                        &mut active,
                        capabilities().limits.waiter_concurrency as usize,
                    ) {
                        Ok(()) => {}
                        Err(response) => write_message(&mut output, &response)?,
                    }
                    continue;
                }
                if let Some(response) = process_message(message, &mut state, &config) {
                    write_message(&mut output, &response)?;
                }
            }
            Ok(ServerEvent::WaitComplete { key, id, result }) => {
                let Some(wait) = active.remove(&key) else {
                    continue;
                };
                let _ = wait.worker.join();
                let response = match result {
                    Ok(result) => {
                        let is_error = result["outcome"].as_str() != Some("matched");
                        success_response(
                            id,
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": serde_json::to_string(&result)
                                        .expect("wait result serializes")
                                }],
                                "structuredContent": result,
                                "isError": is_error
                            }),
                        )
                    }
                    Err(error) => error_response(id, error.code, error.message, Some(error.data)),
                };
                write_message(&mut output, &response)?;
            }
        }
    }
}

fn read_input<R: BufRead>(mut input: R, limit: usize, sender: mpsc::Sender<ServerEvent>) {
    loop {
        match read_bounded_line(&mut input, limit) {
            Ok(line) => {
                let eof = matches!(line, BoundedLine::Eof);
                if sender.send(ServerEvent::Input(line)).is_err() || eof {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(ServerEvent::InputError(error));
                return;
            }
        }
    }
}

fn decode_line(line: &[u8]) -> Result<Value, Value> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Err(error_response(
            Value::Null,
            ERROR_INVALID_REQUEST,
            "MCP message must not be empty",
            None,
        ));
    }
    serde_json::from_slice::<Value>(line).map_err(|error| {
        error_response(
            Value::Null,
            ERROR_PARSE,
            "Parse error",
            Some(json!({"detail": bounded_detail(&error.to_string())})),
        )
    })
}

fn handle_cancel_notification(message: &Value, active: &HashMap<String, ActiveWait>) -> bool {
    let Some(object) = message.as_object() else {
        return false;
    };
    if object.get("method").and_then(Value::as_str) != Some("notifications/cancelled")
        || object.get("id").is_some()
    {
        return false;
    }
    if let Some(request_id) = object
        .get("params")
        .and_then(Value::as_object)
        .and_then(|params| params.get("requestId"))
        .filter(|id| valid_request_id(id))
        && let Some(wait) = active.get(&request_id_key(request_id))
    {
        wait.cancelled.store(true, Ordering::Release);
    }
    true
}

fn is_tool_call(message: &Value) -> bool {
    message
        .as_object()
        .and_then(|object| object.get("method"))
        .and_then(Value::as_str)
        == Some("tools/call")
}

fn start_wait(
    message: &Value,
    config: &McpStdioConfig,
    sender: &mpsc::Sender<ServerEvent>,
    active: &mut HashMap<String, ActiveWait>,
    maximum_waiters: usize,
) -> Result<(), Value> {
    let object = message.as_object().ok_or_else(|| {
        error_response(
            Value::Null,
            ERROR_INVALID_REQUEST,
            "Invalid JSON-RPC request",
            None,
        )
    })?;
    let Some(id) = object.get("id").cloned() else {
        return Ok(());
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some(JSON_RPC_VERSION)
        || !valid_request_id(&id)
    {
        return Err(error_response(
            Value::Null,
            ERROR_INVALID_REQUEST,
            "Invalid JSON-RPC request",
            None,
        ));
    }
    let key = request_id_key(&id);
    if active.contains_key(&key) {
        return Err(error_response(
            id,
            ERROR_INVALID_REQUEST,
            "A request with this id is already active",
            None,
        ));
    }
    if active.len() >= maximum_waiters {
        return Err(error_response(
            id,
            -32003,
            "MCP waiter capacity is exhausted",
            Some(json!({"maximum_waiters": maximum_waiters})),
        ));
    }
    let Some(params) = object.get("params").and_then(Value::as_object) else {
        return Err(error_response(
            id,
            ERROR_INVALID_PARAMS,
            "tools/call params must be an object",
            None,
        ));
    };
    if params.get("name").and_then(Value::as_str) != Some("agenterm_wait") {
        return Err(error_response(
            id,
            ERROR_INVALID_PARAMS,
            "Unknown tool",
            Some(json!({"name": params.get("name")})),
        ));
    }
    let arguments = params
        .get("arguments")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            error_response(
                id.clone(),
                ERROR_INVALID_PARAMS,
                "agenterm_wait arguments must be an object",
                None,
            )
        })?;
    let allowed = [
        "epoch",
        "after_sequence",
        "event_kind",
        "tab_id",
        "timeout_ms",
    ];
    if arguments.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(error_response(
            id,
            ERROR_INVALID_PARAMS,
            "agenterm_wait contains an unknown argument",
            None,
        ));
    }
    let epoch = arguments
        .get("epoch")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .ok_or_else(|| {
            error_response(
                id.clone(),
                ERROR_INVALID_PARAMS,
                "agenterm_wait requires a bounded epoch",
                None,
            )
        })?;
    let after_sequence = arguments
        .get("after_sequence")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            error_response(
                id.clone(),
                ERROR_INVALID_PARAMS,
                "agenterm_wait requires after_sequence",
                None,
            )
        })?;
    let event_kind = arguments
        .get("event_kind")
        .and_then(Value::as_str)
        .filter(|kind| mcp_fleet::WAIT_EVENT_KINDS.contains(kind))
        .ok_or_else(|| {
            error_response(
                id.clone(),
                ERROR_INVALID_PARAMS,
                "agenterm_wait event_kind is not allowlisted",
                Some(json!({"allowed": mcp_fleet::WAIT_EVENT_KINDS})),
            )
        })?;
    let tab_id = match arguments.get("tab_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if valid_tab_id(value) => Some(value.clone()),
        _ => {
            return Err(error_response(
                id,
                ERROR_INVALID_PARAMS,
                "agenterm_wait tab_id must be a stable @ID",
                None,
            ));
        }
    };
    let timeout_ms = arguments
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .filter(|value| {
            *value >= 1 && *value <= u64::from(capabilities().limits.wait_timeout_ms_maximum)
        })
        .ok_or_else(|| {
            error_response(
                id.clone(),
                ERROR_INVALID_PARAMS,
                "agenterm_wait timeout_ms is outside the published limit",
                None,
            )
        })?;
    let request = mcp_fleet::McpWaitRequest {
        epoch: epoch.to_owned(),
        after_sequence,
        event_kind: event_kind.to_owned(),
        tab_id,
        timeout_ms,
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let worker_sender = sender.clone();
    let worker_key = key.clone();
    let worker_id = id.clone();
    let address = config.address.clone();
    let worker = thread::spawn(move || {
        let result = mcp_fleet::wait_event(address.as_deref(), request, worker_cancelled);
        let _ = worker_sender.send(ServerEvent::WaitComplete {
            key: worker_key,
            id: worker_id,
            result,
        });
    });
    active.insert(key, ActiveWait { cancelled, worker });
    Ok(())
}

fn request_id_key(id: &Value) -> String {
    serde_json::to_string(id).expect("valid request id serializes")
}

fn valid_tab_id(value: &str) -> bool {
    value.strip_prefix('@').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn process_message(
    message: Value,
    state: &mut SessionState,
    config: &McpStdioConfig,
) -> Option<Value> {
    let Some(object) = message.as_object() else {
        return Some(error_response(
            Value::Null,
            ERROR_INVALID_REQUEST,
            "JSON-RPC message must be an object",
            None,
        ));
    };
    let id = object.get("id").cloned();
    let notification = id.is_none();
    let response_id = id.clone().filter(valid_request_id).unwrap_or(Value::Null);
    if object.get("jsonrpc").and_then(Value::as_str) != Some(JSON_RPC_VERSION)
        || object.get("method").and_then(Value::as_str).is_none()
        || id.as_ref().is_some_and(|id| !valid_request_id(id))
    {
        return (!notification).then(|| {
            error_response(
                response_id,
                ERROR_INVALID_REQUEST,
                "Invalid JSON-RPC request",
                None,
            )
        });
    }
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .expect("validated method");
    let params = object.get("params");

    match method {
        "initialize" if notification => None,
        "initialize" if *state != SessionState::New => Some(error_response(
            response_id,
            ERROR_INVALID_REQUEST,
            "MCP session is already initialized",
            None,
        )),
        "initialize" => {
            let Some(params) = params.and_then(Value::as_object) else {
                return Some(error_response(
                    response_id,
                    ERROR_INVALID_PARAMS,
                    "initialize params must be an object",
                    None,
                ));
            };
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            let capabilities_valid = params.get("capabilities").is_some_and(Value::is_object);
            let client_valid = params
                .get("clientInfo")
                .and_then(Value::as_object)
                .is_some_and(|client| {
                    client.get("name").is_some_and(Value::is_string)
                        && client.get("version").is_some_and(Value::is_string)
                });
            if requested.is_none() || !capabilities_valid || !client_valid {
                return Some(error_response(
                    response_id,
                    ERROR_INVALID_PARAMS,
                    "initialize requires protocolVersion, capabilities, and clientInfo",
                    Some(json!({"supported": [MCP_PROTOCOL_REVISION]})),
                ));
            }
            *state = SessionState::InitializeResponded;
            Some(success_response(
                response_id,
                json!({
                    "protocolVersion": MCP_PROTOCOL_REVISION,
                    "capabilities": {
                        "resources": {
                            "subscribe": false,
                            "listChanged": false
                        },
                        "tools": {
                            "listChanged": false
                        }
                    },
                    "serverInfo": {
                        "name": "agenterm-mcp",
                        "title": "AgenTerm MCP",
                        "version": env!("CARGO_PKG_VERSION"),
                        "description": "Read-only AgenTerm Fleet bridge"
                    },
                    "instructions": "Read metadata-safe Fleet resources or use agenterm_wait for one bounded, cancellable event wait."
                }),
            ))
        }
        "notifications/initialized" if !notification => Some(error_response(
            response_id,
            ERROR_INVALID_REQUEST,
            "notifications/initialized must not contain an id",
            None,
        )),
        "notifications/initialized" if *state == SessionState::InitializeResponded => {
            *state = SessionState::Ready;
            None
        }
        "notifications/initialized" => None,
        "ping" if notification => None,
        "ping" => Some(success_response(response_id, json!({}))),
        _ if notification => None,
        _ if *state != SessionState::Ready => Some(error_response(
            response_id,
            ERROR_NOT_INITIALIZED,
            "MCP session is not initialized",
            None,
        )),
        "resources/list" => Some(success_response(
            response_id,
            json!({
                "resources": capabilities()
                    .resources
                    .into_iter()
                    .map(|resource| json!({
                        "uri": resource.uri,
                        "name": resource.stable_id,
                        "title": resource_title(resource.stable_id),
                        "description": resource_description(resource.stable_id),
                        "mimeType": "application/json"
                    }))
                    .collect::<Vec<_>>()
            }),
        )),
        "resources/read" => {
            let Some(uri) = params
                .and_then(Value::as_object)
                .and_then(|params| params.get("uri"))
                .and_then(Value::as_str)
            else {
                return Some(error_response(
                    response_id,
                    ERROR_INVALID_PARAMS,
                    "resources/read requires a string uri",
                    None,
                ));
            };
            match mcp_fleet::read_resource(uri, config.address.as_deref()) {
                Ok(resource) => Some(success_response(
                    response_id,
                    json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": serde_json::to_string(&resource)
                                .expect("resource projection serializes")
                        }]
                    }),
                )),
                Err(error) => Some(error_response(
                    response_id,
                    error.code,
                    error.message,
                    Some(error.data),
                )),
            }
        }
        "tools/list" => Some(success_response(
            response_id,
            json!({
                "tools": [{
                    "name": "agenterm_wait",
                    "title": "Wait for an AgenTerm Fleet event",
                    "description": "Read-only bounded wait from a verified epoch and sequence.",
                    "inputSchema": {
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "epoch": {"type": "string", "minLength": 1, "maxLength": 256},
                            "after_sequence": {"type": "integer", "minimum": 0},
                            "event_kind": {
                                "type": "string",
                                "enum": mcp_fleet::WAIT_EVENT_KINDS
                            },
                            "tab_id": {"type": ["string", "null"], "pattern": "^@[0-9]+$"},
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": capabilities().limits.wait_timeout_ms_maximum
                            }
                        },
                        "required": ["epoch", "after_sequence", "event_kind", "timeout_ms"]
                    },
                    "outputSchema": {
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": "object",
                        "properties": {
                            "schema_id": {"type": "string"},
                            "outcome": {
                                "type": "string",
                                "enum": [
                                    "matched", "timeout", "cancelled", "server_restart",
                                    "journal_gap", "future_sequence", "target_closed",
                                    "event_read_failed"
                                ]
                            },
                            "position": {"type": "object"}
                        },
                        "required": ["schema_id", "outcome", "position"]
                    },
                    "annotations": {
                        "readOnlyHint": true,
                        "destructiveHint": false,
                        "idempotentHint": false,
                        "openWorldHint": false
                    }
                }]
            }),
        )),
        _ => Some(error_response(
            response_id,
            ERROR_METHOD_NOT_FOUND,
            "Method not found",
            Some(json!({"method": method})),
        )),
    }
}

fn resource_title(stable_id: &str) -> &'static str {
    match stable_id {
        "fleet.instances" => "AgenTerm Instances",
        "fleet.workspace" => "AgenTerm Workspace",
        "fleet.tabs" => "AgenTerm Tabs",
        "fleet.snapshot" => "AgenTerm Fleet Snapshot",
        _ => "AgenTerm Resource",
    }
}

fn resource_description(stable_id: &str) -> &'static str {
    match stable_id {
        "fleet.instances" => "Registered local AgenTerm server metadata",
        "fleet.workspace" => "Selected workspace identity and event baseline",
        "fleet.tabs" => "Metadata-only stable tab inventory",
        "fleet.snapshot" => "One causal metadata-only Fleet snapshot",
        _ => "AgenTerm metadata",
    }
}

fn valid_request_id(id: &Value) -> bool {
    id.is_string() || id.is_i64() || id.is_u64()
}

fn success_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": JSON_RPC_VERSION, "id": id, "result": result})
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = serde_json::Map::from_iter([
        ("code".to_owned(), Value::from(code)),
        ("message".to_owned(), Value::from(message)),
    ]);
    if let Some(data) = data {
        error.insert("data".to_owned(), data);
    }
    json!({"jsonrpc": JSON_RPC_VERSION, "id": id, "error": error})
}

fn bounded_detail(detail: &str) -> String {
    let maximum = capabilities().limits.error_detail_bytes as usize;
    detail.chars().take(maximum).collect()
}

fn write_message<W: Write>(output: &mut W, message: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut *output, message)?;
    output.write_all(b"\n")?;
    output.flush()
}

fn read_bounded_line<R: BufRead>(input: &mut R, maximum: usize) -> io::Result<BoundedLine> {
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() && !oversized {
                Ok(BoundedLine::Eof)
            } else if oversized {
                Ok(BoundedLine::Oversized)
            } else {
                Ok(BoundedLine::Line(line))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !oversized {
            let payload = if newline.is_some() {
                &available[..consumed - 1]
            } else {
                &available[..consumed]
            };
            if line.len().saturating_add(payload.len()) > maximum {
                oversized = true;
                line.clear();
            } else {
                line.extend_from_slice(payload);
            }
        }
        input.consume(consumed);
        if newline.is_some() {
            return if oversized {
                Ok(BoundedLine::Oversized)
            } else {
                Ok(BoundedLine::Line(line))
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    fn exchange(input: &str) -> Vec<Value> {
        let mut output = Vec::new();
        serve_stdio(
            BufReader::new(Cursor::new(input.as_bytes().to_vec())),
            &mut output,
        )
        .unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn initialize_notification_and_ping_follow_the_lifecycle() {
        let responses = exchange(concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
            "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
            "\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":\"ping-1\",\"method\":\"ping\"}\n"
        ));
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["id"], 1);
        assert_eq!(
            responses[0]["result"]["protocolVersion"],
            MCP_PROTOCOL_REVISION
        );
        assert_eq!(
            responses[0]["result"]["capabilities"],
            json!({
                "resources": {"subscribe": false, "listChanged": false},
                "tools": {"listChanged": false}
            })
        );
        assert_eq!(responses[1], success_response(json!("ping-1"), json!({})));
    }

    #[test]
    fn non_ping_request_is_rejected_before_initialized_notification() {
        let responses = exchange(concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
            "{\"protocolVersion\":\"future\",\"capabilities\":{},",
            "\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"resources/list\"}\n"
        ));
        assert_eq!(
            responses[0]["result"]["protocolVersion"],
            MCP_PROTOCOL_REVISION
        );
        assert_eq!(responses[1]["error"]["code"], ERROR_NOT_INITIALIZED);
    }

    #[test]
    fn wait_tool_cannot_start_before_initialized_notification() {
        let responses = exchange(concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
            "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
            "\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":",
            "{\"name\":\"agenterm_wait\",\"arguments\":{\"epoch\":\"e\",",
            "\"after_sequence\":0,\"event_kind\":\"tab.selected\",\"timeout_ms\":10}}}\n"
        ));
        assert_eq!(responses[1]["error"]["code"], ERROR_NOT_INITIALIZED);
    }

    #[test]
    fn ready_session_lists_exactly_four_metadata_resources() {
        let responses = exchange(concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
            "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
            "\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"resources/list\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"resources/read\",",
            "\"params\":{\"uri\":\"agenterm://fleet/unknown\"}}\n"
        ));
        assert_eq!(
            responses[1]["result"]["resources"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(responses[2]["error"]["code"], -32002);
    }

    #[test]
    fn ready_session_lists_one_bounded_read_only_wait_tool() {
        let responses = exchange(concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
            "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
            "\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n"
        ));
        let tools = responses[1]["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "agenterm_wait");
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
        assert_eq!(
            tools[0]["inputSchema"]["properties"]["timeout_ms"]["maximum"],
            capabilities().limits.wait_timeout_ms_maximum
        );
    }

    #[test]
    fn waiter_capacity_fails_closed_before_backend_allocation() {
        let message = json!({
            "jsonrpc": "2.0",
            "id": "wait-over-capacity",
            "method": "tools/call",
            "params": {
                "name": "agenterm_wait",
                "arguments": {
                    "epoch": "epoch-a",
                    "after_sequence": 7,
                    "event_kind": "tab.note",
                    "timeout_ms": 100
                }
            }
        });
        let (sender, _receiver) = mpsc::channel();
        let mut active = HashMap::new();
        let error = start_wait(
            &message,
            &McpStdioConfig::default(),
            &sender,
            &mut active,
            0,
        )
        .unwrap_err();
        assert_eq!(error["error"]["code"], -32003);
        assert_eq!(error["error"]["data"]["maximum_waiters"], 0);
        assert!(active.is_empty());
    }

    #[test]
    fn malformed_unknown_and_duplicate_initialize_are_typed() {
        let responses = exchange(concat!(
            "{bad json}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
            "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
            "\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"unknown\"}\n"
        ));
        assert_eq!(responses[0]["error"]["code"], ERROR_PARSE);
        assert_eq!(responses[2]["error"]["code"], ERROR_INVALID_REQUEST);
        assert_eq!(responses[3]["error"]["code"], ERROR_METHOD_NOT_FOUND);
    }

    #[test]
    fn oversized_message_is_drained_and_the_next_message_survives() {
        let maximum = capabilities().limits.frame_bytes as usize;
        let mut input = vec![b'x'; maximum + 1];
        input.extend_from_slice(b"\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n");
        let mut output = Vec::new();
        serve_stdio(BufReader::new(Cursor::new(input)), &mut output).unwrap();
        let responses = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(responses[0]["error"]["code"], ERROR_INVALID_REQUEST);
        assert_eq!(responses[1], success_response(json!(7), json!({})));
    }

    #[test]
    fn notifications_never_receive_responses() {
        assert!(exchange("{\"jsonrpc\":\"2.0\",\"method\":\"unknown\"}\n").is_empty());
        assert!(
            exchange("{\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"params\":{}}\n").is_empty()
        );
    }
}
