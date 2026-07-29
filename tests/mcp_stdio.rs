use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
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

#[test]
fn public_wait_preserves_journal_position_failure_classes() {
    for expected in ["server_restart", "journal_gap", "future_sequence"] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind isolated IPC fixture");
        let address = listener.local_addr().expect("fixture address").to_string();
        let fixture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept MCP backend request");
            read_backend_request(&stream);
            let response = json!({
                "ok": false,
                "output": "",
                "error": "fixture failure",
                "error_code": expected,
                "error_category": "event_position",
                "retryable": false
            });
            writeln!(stream, "{response}").expect("write backend failure");
        });
        let (result, stderr) = run_public_wait(&address, "tab.note", None, 1_000);
        fixture.join().expect("join backend fixture");
        assert_eq!(result["outcome"], expected);
        assert_eq!(result["detail"]["backend_code"], expected);
        assert!(stderr.is_empty());
    }
}

#[test]
fn public_disconnect_cancels_an_active_wait_within_bounded_grace() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind isolated IPC fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let (accepted_sender, accepted_receiver) = mpsc::channel();
    let fixture = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept MCP backend request");
        read_backend_request(&stream);
        accepted_sender
            .send(())
            .expect("report active backend request");
        thread::sleep(Duration::from_millis(1_000));
    });
    let mut child = start_mcp(&address);
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    let reader = spawn_stdout_reader(stdout, sender);
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_wait_session(&mut stdin, "tab.note", None, 5_000);
    assert_eq!(receive_response(&receiver, &mut child)["id"], 1);
    accepted_receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("wait became active before disconnect");
    let started = Instant::now();
    drop(stdin);
    let output = child.wait_with_output().expect("wait for EOF shutdown");
    let elapsed = started.elapsed();
    reader.join().expect("join stdout reader");
    fixture.join().expect("join backend fixture");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(
        elapsed < Duration::from_millis(1_500),
        "disconnect cleanup took {elapsed:?}"
    );
}

#[test]
fn force_killed_client_closes_its_active_backend_wait() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind isolated IPC fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let (accepted_sender, accepted_receiver) = mpsc::channel();
    let (closed_sender, closed_receiver) = mpsc::channel();
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept MCP backend request");
        read_backend_request(&stream);
        accepted_sender
            .send(())
            .expect("report active backend request");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("bound fixture read");
        let mut byte = [0_u8; 1];
        let closed = match stream.read(&mut byte) {
            Ok(0) => true,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                true
            }
            _ => false,
        };
        closed_sender
            .send(closed)
            .expect("report backend connection state");
    });
    let mut child = start_mcp(&address);
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    let reader = spawn_stdout_reader(stdout, sender);
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_wait_session(&mut stdin, "tab.note", None, 5_000);
    assert_eq!(receive_response(&receiver, &mut child)["id"], 1);
    accepted_receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("wait became active before client kill");
    child.kill().expect("force-kill isolated MCP client");
    let output = child.wait_with_output().expect("reap killed MCP client");
    drop(stdin);
    reader.join().expect("join stdout reader");
    assert!(!output.status.success(), "{output:?}");
    assert!(
        closed_receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("observe backend connection close")
    );
    fixture.join().expect("join backend fixture");
}

#[test]
fn waiter_capacity_recovers_after_cancellation() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind isolated IPC fixture");
    listener
        .set_nonblocking(true)
        .expect("make polling fixture stoppable");
    let address = listener.local_addr().expect("fixture address").to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let fixture = thread::spawn({
        let stop = Arc::clone(&stop);
        move || {
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        read_backend_request(&stream);
                        let response = json!({
                            "ok": true,
                            "output": json!({
                                "events": [],
                                "position": {"epoch": "epoch-a", "sequence": 7}
                            })
                            .to_string(),
                            "error": "",
                            "error_code": "",
                            "error_category": "",
                            "retryable": false
                        });
                        writeln!(stream, "{response}").expect("write empty event batch");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("polling fixture accept failed: {error}"),
                }
            }
        }
    });
    let mut child = start_mcp(&address);
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    let reader = spawn_stdout_reader(stdout, sender);
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_initialize(&mut stdin);
    for index in 0..8 {
        write_wait_call(&mut stdin, &format!("wait-{index}"), 5_000);
    }
    write_wait_call(&mut stdin, "over-capacity", 5_000);
    stdin.flush().expect("flush waiter burst");
    assert_eq!(receive_response(&receiver, &mut child)["id"], 1);
    let rejected = receive_response(&receiver, &mut child);
    assert_eq!(rejected["id"], "over-capacity");
    assert_eq!(rejected["error"]["code"], -32003);

    write_cancel(&mut stdin, "wait-0");
    stdin.flush().expect("flush first cancellation");
    let cancelled = receive_response(&receiver, &mut child);
    assert_eq!(cancelled["id"], "wait-0");
    assert_eq!(
        cancelled["result"]["structuredContent"]["outcome"],
        "cancelled"
    );

    write_wait_call(&mut stdin, "replacement", 5_000);
    write_cancel(&mut stdin, "replacement");
    stdin.flush().expect("flush replacement wait");
    let replacement = receive_response(&receiver, &mut child);
    assert_eq!(replacement["id"], "replacement");
    assert_eq!(
        replacement["result"]["structuredContent"]["outcome"],
        "cancelled"
    );

    drop(stdin);
    let output = child.wait_with_output().expect("wait for EOF shutdown");
    reader.join().expect("join stdout reader");
    stop.store(true, Ordering::Release);
    fixture.join().expect("join polling fixture");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

fn run_public_wait(
    address: &str,
    event_kind: &str,
    tab_id: Option<&str>,
    timeout_ms: u64,
) -> (Value, Vec<u8>) {
    let mut child = start_mcp(address);
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    let reader = spawn_stdout_reader(stdout, sender);
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_wait_session(&mut stdin, event_kind, tab_id, timeout_ms);
    assert_eq!(receive_response(&receiver, &mut child)["id"], 1);
    let response = receive_response(&receiver, &mut child);
    drop(stdin);
    let output = child.wait_with_output().expect("wait for EOF shutdown");
    reader.join().expect("join stdout reader");
    assert!(output.status.success(), "{output:?}");
    (
        response["result"]["structuredContent"].clone(),
        output.stderr,
    )
}

fn start_mcp(address: &str) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_agenterm-mcp"))
        .args(["--address", address, "serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start public agenterm-mcp executable")
}

fn spawn_stdout_reader(
    stdout: std::process::ChildStdout,
    sender: mpsc::Sender<Result<String, std::io::Error>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    })
}

fn write_wait_session(
    stdin: &mut std::process::ChildStdin,
    event_kind: &str,
    tab_id: Option<&str>,
    timeout_ms: u64,
) {
    write_initialize(stdin);
    let mut message = wait_call("wait", timeout_ms);
    message["params"]["arguments"]["event_kind"] = json!(event_kind);
    if let Some(tab_id) = tab_id {
        message["params"]["arguments"]["tab_id"] = json!(tab_id);
    }
    writeln!(stdin, "{message}").expect("write MCP wait");
    stdin.flush().expect("flush MCP requests");
}

fn write_initialize(stdin: &mut std::process::ChildStdin) {
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
    ] {
        writeln!(stdin, "{message}").expect("write MCP request");
    }
}

fn write_wait_call(stdin: &mut std::process::ChildStdin, id: &str, timeout_ms: u64) {
    writeln!(stdin, "{}", wait_call(id, timeout_ms)).expect("write MCP wait");
}

fn wait_call(id: &str, timeout_ms: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "agenterm_wait",
            "arguments": {
                "epoch": "epoch-a",
                "after_sequence": 7,
                "event_kind": "tab.note",
                "timeout_ms": timeout_ms
            }
        }
    })
}

fn write_cancel(stdin: &mut std::process::ChildStdin, request_id: &str) {
    let message = json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": {"requestId": request_id}
    });
    writeln!(stdin, "{message}").expect("write MCP cancellation");
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
    let request = read_backend_request(&stream);
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

fn read_backend_request(stream: &TcpStream) -> Value {
    let mut request_line = String::new();
    BufReader::new(stream.try_clone().expect("clone fixture stream"))
        .read_line(&mut request_line)
        .expect("read backend request");
    serde_json::from_str(&request_line).expect("typed backend request")
}
