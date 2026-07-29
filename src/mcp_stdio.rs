use std::io::{self, BufRead, Write};

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

#[derive(Clone, Debug, Default)]
pub struct McpStdioConfig {
    pub address: Option<String>,
}

pub fn serve_stdio<R: BufRead, W: Write>(input: R, output: W) -> io::Result<()> {
    serve_stdio_with_config(input, output, McpStdioConfig::default())
}

pub fn serve_stdio_with_config<R: BufRead, W: Write>(
    mut input: R,
    mut output: W,
    config: McpStdioConfig,
) -> io::Result<()> {
    let limit = capabilities().limits.frame_bytes as usize;
    let mut state = SessionState::New;
    loop {
        let line = match read_bounded_line(&mut input, limit)? {
            BoundedLine::Eof => return Ok(()),
            BoundedLine::Oversized => {
                write_message(
                    &mut output,
                    &error_response(
                        Value::Null,
                        ERROR_INVALID_REQUEST,
                        "MCP message exceeds the frame limit",
                        Some(json!({"maximum_bytes": limit})),
                    ),
                )?;
                continue;
            }
            BoundedLine::Line(line) => line,
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            write_message(
                &mut output,
                &error_response(
                    Value::Null,
                    ERROR_INVALID_REQUEST,
                    "MCP message must not be empty",
                    None,
                ),
            )?;
            continue;
        }
        let message = match serde_json::from_slice::<Value>(&line) {
            Ok(message) => message,
            Err(error) => {
                write_message(
                    &mut output,
                    &error_response(
                        Value::Null,
                        ERROR_PARSE,
                        "Parse error",
                        Some(json!({"detail": bounded_detail(&error.to_string())})),
                    ),
                )?;
                continue;
            }
        };
        if let Some(response) = process_message(message, &mut state, &config) {
            write_message(&mut output, &response)?;
        }
    }
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
                        }
                    },
                    "serverInfo": {
                        "name": "agenterm-mcp",
                        "title": "AgenTerm MCP",
                        "version": env!("CARGO_PKG_VERSION"),
                        "description": "Read-only AgenTerm Fleet bridge"
                    },
                    "instructions": "This implementation slice supports lifecycle and ping only."
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
        serve_stdio(BufReader::new(Cursor::new(input.as_bytes())), &mut output).unwrap();
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
            json!({"resources": {"subscribe": false, "listChanged": false}})
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
