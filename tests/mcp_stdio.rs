use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use serde_json::{Value, json};

#[test]
fn public_stdio_lifecycle_keeps_stdout_machine_only() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-mcp"))
        .args(["serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start public agenterm-mcp executable");
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
        "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
        "\"clientInfo\":{\"name\":\"black-box\",\"version\":\"1\"}}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"ping\",\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"resources\",\"method\":\"resources/list\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"tools\",\"method\":\"tools/list\"}\n"
    );
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input.as_bytes())
        .expect("write MCP lifecycle");
    let output = child.wait_with_output().expect("wait for EOF shutdown");
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "successful lifecycle wrote diagnostics: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("stdout line is JSON-RPC"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        responses[0]["result"]["capabilities"],
        json!({
            "resources": {"subscribe": false, "listChanged": false},
            "tools": {"listChanged": false}
        })
    );
    assert_eq!(
        responses[1],
        json!({"jsonrpc": "2.0", "id": "ping", "result": {}})
    );
    assert_eq!(
        responses[2]["result"]["resources"]
            .as_array()
            .expect("resource list"),
        &[
            json!({
                "uri": "agenterm://fleet/instances",
                "name": "fleet.instances",
                "title": "AgenTerm Instances",
                "description": "Registered local AgenTerm server metadata",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "agenterm://fleet/workspace",
                "name": "fleet.workspace",
                "title": "AgenTerm Workspace",
                "description": "Selected workspace identity and event baseline",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "agenterm://fleet/tabs",
                "name": "fleet.tabs",
                "title": "AgenTerm Tabs",
                "description": "Metadata-only stable tab inventory",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "agenterm://fleet/snapshot",
                "name": "fleet.snapshot",
                "title": "AgenTerm Fleet Snapshot",
                "description": "One causal metadata-only Fleet snapshot",
                "mimeType": "application/json"
            })
        ]
    );
    let tools = responses[3]["result"]["tools"]
        .as_array()
        .expect("tool list");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "agenterm_wait");
    assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
}

#[test]
fn public_wait_returns_one_projected_event_and_verified_post_state() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind isolated IPC fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let fixture = thread::spawn(move || {
        for _ in 0..2 {
            let (stream, _) = listener.accept().expect("accept MCP backend request");
            reply_to_backend(stream);
        }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-mcp"))
        .args(["--address", &address, "serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start public agenterm-mcp executable");
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut stdin = child.stdin.take().expect("piped stdin");
    for message in [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "wait-fixture", "version": "1"}
            }
        }),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({
            "jsonrpc": "2.0",
            "id": "wait",
            "method": "tools/call",
            "params": {
                "name": "agenterm_wait",
                "arguments": {
                    "epoch": "epoch-a",
                    "after_sequence": 7,
                    "event_kind": "tab.note",
                    "tab_id": "@2",
                    "timeout_ms": 1_000
                }
            }
        }),
    ] {
        writeln!(stdin, "{message}").expect("write MCP request");
    }
    stdin.flush().expect("flush MCP requests");
    let initialized = receive_response(&receiver, &mut child);
    let waited = receive_response(&receiver, &mut child);
    drop(stdin);
    let output = child.wait_with_output().expect("wait for EOF shutdown");
    reader.join().expect("join stdout reader");
    fixture.join().expect("join backend fixture");

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(initialized["id"], 1);
    assert_eq!(waited["id"], "wait");
    assert_eq!(waited["result"]["isError"], false);
    let result = &waited["result"]["structuredContent"];
    assert_eq!(result["outcome"], "matched");
    assert_eq!(result["event"]["sequence"], 8);
    assert_eq!(result["event"]["tab_id"], "@2");
    assert_eq!(result["detail"]["event_position"]["sequence"], 8);
    assert_eq!(result["detail"]["active_tab_id"], "@2");
    assert!(
        !serde_json::to_string(result)
            .expect("serialize result")
            .contains("private payload")
    );
}

fn receive_response(
    receiver: &mpsc::Receiver<Result<String, std::io::Error>>,
    child: &mut std::process::Child,
) -> Value {
    match receiver.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(line)) => serde_json::from_str(&line).expect("stdout line is JSON-RPC"),
        Ok(Err(error)) => {
            let _ = child.kill();
            panic!("could not read MCP stdout: {error}");
        }
        Err(error) => {
            let _ = child.kill();
            panic!("timed out waiting for MCP response: {error}");
        }
    }
}

fn reply_to_backend(mut stream: TcpStream) {
    let mut request_line = String::new();
    BufReader::new(stream.try_clone().expect("clone fixture stream"))
        .read_line(&mut request_line)
        .expect("read backend request");
    let request: Value = serde_json::from_str(&request_line).expect("typed backend request");
    let output = match request["args"][0].as_str() {
        Some("read-events") => json!({
            "events": [{
                "epoch": "epoch-a",
                "sequence": 8,
                "kind": "tab.note",
                "tab_id": 2,
                "payload": {"note": "private payload"}
            }],
            "position": {"epoch": "epoch-a", "sequence": 8}
        }),
        Some("ui-snapshot") => json!({
            "server_pid": 42,
            "protocol_version": 1,
            "event_position": {"epoch": "epoch-a", "sequence": 8},
            "focus": {"window_id": "@2"},
            "tabs": [{"id": "@2"}]
        }),
        command => panic!("unexpected backend command: {command:?}"),
    };
    let response = json!({
        "ok": true,
        "output": output.to_string(),
        "error": "",
        "error_code": "",
        "error_category": "",
        "retryable": false
    });
    writeln!(stream, "{response}").expect("write backend response");
    stream.flush().expect("flush backend response");
    let mut trailing = Vec::new();
    let _ = stream.read_to_end(&mut trailing);
}
