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

#[cfg(windows)]
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

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
fn public_stdio_recovers_after_malformed_and_oversized_frames() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm-mcp"))
        .args(["serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start public agenterm-mcp executable");
    let mut stdin = child.stdin.take().expect("piped stdin");
    writeln!(stdin, "{{bad json}}").expect("write malformed frame");
    writeln!(stdin, "{}", "x".repeat(4 * 1024 * 1024)).expect("write oversized frame");
    writeln!(
        stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": "recovered",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "recovery", "version": "1"}
            }
        })
    )
    .expect("write recovery initialize");
    drop(stdin);
    let output = child.wait_with_output().expect("wait for recovery session");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let responses = String::from_utf8(output.stdout)
        .expect("MCP stdout UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("MCP response JSON"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["error"]["code"], -32600);
    assert!(
        responses[1]["error"]["data"]["maximum_bytes"]
            .as_u64()
            .is_some_and(|maximum| maximum < 4 * 1024 * 1024)
    );
    assert_eq!(responses[2]["id"], "recovered");
    assert_eq!(responses[2]["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn public_resource_matches_cli_snapshot_field_for_field() {
    let snapshot = json!({
        "server_pid": 4242,
        "protocol_version": 7,
        "event_position": {"epoch": "same-source-epoch", "sequence": 91},
        "focus": {"window_id": "@12"},
        "window": {
            "visible": true,
            "detached": false,
            "minimized": false,
            "state": "normal"
        },
        "tabs": [{
            "id": "@12",
            "parent_id": null,
            "name": "same-source",
            "note": "metadata-note",
            "state": "running",
            "pid": 5151,
            "exit_code": null,
            "active": true,
            "has_children": false,
            "collapsed": false,
            "depth": 0,
            "terminal_title": "private-title",
            "composer": "private-draft",
            "working_context": {
                "cwd": {"path": "private-cwd"},
                "proxy": {"endpoint": "private-proxy"}
            }
        }]
    });
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind same-source fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let fixture_snapshot = snapshot.clone();
    let fixture = thread::spawn(move || {
        for _ in 0..2 {
            let (stream, _) = listener.accept().expect("accept same-source request");
            reply_snapshot_backend(stream, &fixture_snapshot);
        }
    });

    let cli_output = Command::new(env!("CARGO_BIN_EXE_agenterm-cli"))
        .args(["--address", &address, "ui-snapshot"])
        .output()
        .expect("run public CLI snapshot");
    assert!(cli_output.status.success(), "{cli_output:?}");
    assert!(cli_output.stderr.is_empty(), "{cli_output:?}");
    let cli: Value = serde_json::from_slice(&cli_output.stdout).expect("CLI snapshot JSON");

    let mut child = start_mcp(&address);
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
        "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
        "\"clientInfo\":{\"name\":\"same-source\",\"version\":\"1\"}}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"snapshot\",\"method\":\"resources/read\",",
        "\"params\":{\"uri\":\"agenterm://fleet/snapshot\"}}\n"
    );
    stdin.write_all(input.as_bytes()).expect("write MCP reads");
    drop(stdin);
    let output = child.wait_with_output().expect("wait for MCP snapshot");
    fixture.join().expect("join same-source fixture");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let responses = String::from_utf8(output.stdout)
        .expect("MCP stdout UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("MCP response JSON"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    let mcp_text = responses[1]["result"]["contents"][0]["text"]
        .as_str()
        .expect("MCP resource text");
    let mcp: Value = serde_json::from_str(mcp_text).expect("MCP resource JSON");

    assert_eq!(mcp["server"]["address"], address);
    assert_eq!(mcp["server"]["pid"], cli["server_pid"]);
    assert_eq!(mcp["server"]["protocol_version"], cli["protocol_version"]);
    assert_eq!(mcp["event_position"], cli["event_position"]);
    assert_eq!(mcp["workspace"]["active_tab_id"], cli["focus"]["window_id"]);
    assert_eq!(mcp["workspace"]["tab_count"], 1);
    assert_eq!(mcp["workspace"]["window_state"], cli["window"]["state"]);
    assert_eq!(mcp["workspace"]["window_visible"], cli["window"]["visible"]);
    assert_eq!(
        mcp["workspace"]["window_detached"],
        cli["window"]["detached"]
    );
    for field in [
        "id",
        "parent_id",
        "name",
        "note",
        "state",
        "pid",
        "exit_code",
        "active",
        "has_children",
        "collapsed",
        "depth",
    ] {
        assert_eq!(mcp["tabs"][0][field], cli["tabs"][0][field], "{field}");
    }
    let encoded = serde_json::to_string(&mcp).expect("encode MCP projection");
    for private in [
        "private-title",
        "private-draft",
        "private-cwd",
        "private-proxy",
    ] {
        assert!(!encoded.contains(private), "MCP leaked {private}");
    }
}

#[cfg(windows)]
#[test]
fn killed_sidecar_cannot_interrupt_live_gui_server_or_pty() {
    let root = std::env::temp_dir().join(format!(
        "agenterm-mcp-isolation-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_millis()
    ));
    fs::create_dir_all(&root).expect("create isolation root");
    let address = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve isolation address");
        listener
            .local_addr()
            .expect("isolation address")
            .to_string()
    };
    let workspace = root.join("workspace.json");
    let settings = root.join("settings.json");
    let instances = root.join("instances");
    fs::create_dir_all(&instances).expect("create instance directory");

    let mut server = Some(
        configured_command(
            env!("CARGO_BIN_EXE_agenterm-server"),
            &address,
            &workspace,
            &settings,
            &instances,
        )
        .args(["--address", &address])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start isolated server"),
    );
    let mut gui: Option<std::process::Child> = None;
    let mut mcp: Option<std::process::Child> = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for_cli(
            &address,
            &workspace,
            &settings,
            &instances,
            &["protocol-info", "--running"],
            Duration::from_secs(5),
        );
        gui = Some(
            configured_command(
                env!("CARGO_BIN_EXE_agenterm"),
                &address,
                &workspace,
                &settings,
                &instances,
            )
            .arg("--no-activate")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start replaceable GUI"),
        );
        wait_for_cli(
            &address,
            &workspace,
            &settings,
            &instances,
            &["ui-lease", "status"],
            Duration::from_secs(5),
        );

        let created = run_cli(
            &address,
            &workspace,
            &settings,
            &instances,
            &[
                "new-window",
                "-d",
                "-P",
                "-F",
                "#{window_id}",
                "-n",
                "mcp-isolation",
                "--",
                "cmd.exe",
                "/d",
                "/c",
                "echo AGENTERM_MCP_ISOLATION & ping -n 30 127.0.0.1 >nul",
            ],
        );
        assert!(created.status.success(), "{created:?}");
        let tab_id = String::from_utf8(created.stdout)
            .expect("tab ID UTF-8")
            .trim()
            .to_owned();
        assert!(tab_id.starts_with('@'), "{tab_id}");
        let waited = run_cli(
            &address,
            &workspace,
            &settings,
            &instances,
            &[
                "wait-pane",
                "-t",
                &tab_id,
                "--contains",
                "AGENTERM_MCP_ISOLATION",
                "--timeout-ms",
                "5000",
            ],
        );
        assert!(waited.status.success(), "{waited:?}");
        let snapshot = run_cli(
            &address,
            &workspace,
            &settings,
            &instances,
            &["ui-snapshot"],
        );
        assert!(snapshot.status.success(), "{snapshot:?}");
        let snapshot: Value = serde_json::from_slice(&snapshot.stdout).expect("snapshot JSON");

        let mut sidecar = configured_command(
            env!("CARGO_BIN_EXE_agenterm-mcp"),
            &address,
            &workspace,
            &settings,
            &instances,
        )
        .args(["--address", &address, "serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start isolation sidecar");
        let stdout = sidecar.stdout.take().expect("sidecar stdout");
        let (sender, receiver) = mpsc::channel();
        let reader = spawn_stdout_reader(stdout, sender);
        let mut stdin = sidecar.stdin.take().expect("sidecar stdin");
        write_initialize(&mut stdin);
        writeln!(
            stdin,
            "{}",
            json!({
                "jsonrpc": "2.0",
                "id": "isolation-wait",
                "method": "tools/call",
                "params": {
                    "name": "agenterm_wait",
                    "arguments": {
                        "epoch": snapshot["event_position"]["epoch"],
                        "after_sequence": snapshot["event_position"]["sequence"],
                        "event_kind": "workspace.shutdown",
                        "timeout_ms": 30_000
                    }
                }
            })
        )
        .expect("write isolation wait");
        writeln!(
            stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": "live-ping", "method": "ping"})
        )
        .expect("write concurrent ping");
        stdin.flush().expect("flush isolation session");
        assert_eq!(receive_response(&receiver, &mut sidecar)["id"], 1);
        assert_eq!(receive_response(&receiver, &mut sidecar)["id"], "live-ping");
        sidecar.kill().expect("force kill sidecar");
        let status = sidecar.wait().expect("wait killed sidecar");
        assert!(!status.success());
        drop(stdin);
        reader.join().expect("join sidecar reader");
        mcp = Some(sidecar);

        assert!(
            server
                .as_mut()
                .expect("server")
                .try_wait()
                .expect("poll server")
                .is_none(),
            "server exited with sidecar"
        );
        assert!(
            gui.as_mut()
                .expect("GUI")
                .try_wait()
                .expect("poll GUI")
                .is_none(),
            "GUI exited with sidecar"
        );
        let capture = run_cli(
            &address,
            &workspace,
            &settings,
            &instances,
            &["capture-pane", "-p", "-t", &tab_id, "--max-bytes", "16384"],
        );
        assert!(capture.status.success(), "{capture:?}");
        assert!(String::from_utf8_lossy(&capture.stdout).contains("AGENTERM_MCP_ISOLATION"));
        let note = run_cli(
            &address,
            &workspace,
            &settings,
            &instances,
            &["set-tab-note", "-t", &tab_id, "sidecar-isolated"],
        );
        assert!(note.status.success(), "{note:?}");
    }));

    let _ = run_cli(&address, &workspace, &settings, &instances, &["shutdown"]);
    for child in [&mut mcp, &mut gui, &mut server] {
        if let Some(child) = child.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
    let _ = fs::remove_dir_all(&root);
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
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
                        // Windows can inherit the listener's nonblocking mode on
                        // accepted sockets. The listener polls only so this
                        // fixture can stop; each request stream must block until
                        // its complete JSON line arrives.
                        stream
                            .set_nonblocking(false)
                            .expect("make accepted fixture stream blocking");
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

#[cfg(windows)]
fn configured_command(
    program: &str,
    address: &str,
    workspace: &Path,
    settings: &Path,
    instances: &Path,
) -> Command {
    let mut command = Command::new(program);
    command
        .env("AGENTERM_IPC_ADDRESS", address)
        .env("AGENTERM_WORKSPACE_PATH", workspace)
        .env("AGENTERM_SETTINGS_PATH", settings)
        .env("AGENTERM_INSTANCE_DIR", instances)
        .env("AGENTERM_NO_ACTIVATE", "1");
    command
}

#[cfg(windows)]
fn run_cli(
    address: &str,
    workspace: &Path,
    settings: &Path,
    instances: &Path,
    arguments: &[&str],
) -> std::process::Output {
    configured_command(
        env!("CARGO_BIN_EXE_agenterm-cli"),
        address,
        workspace,
        settings,
        instances,
    )
    .args(["--address", address])
    .args(arguments)
    .output()
    .expect("run isolated CLI")
}

#[cfg(windows)]
fn wait_for_cli(
    address: &str,
    workspace: &Path,
    settings: &Path,
    instances: &Path,
    arguments: &[&str],
    timeout: Duration,
) -> std::process::Output {
    let deadline = Instant::now() + timeout;
    loop {
        let output = run_cli(address, workspace, settings, instances, arguments);
        if output.status.success() {
            return output;
        }
        assert!(Instant::now() < deadline, "{output:?}");
        thread::sleep(Duration::from_millis(25));
    }
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

fn reply_snapshot_backend(mut stream: TcpStream, snapshot: &Value) {
    let request = read_backend_request(&stream);
    assert_eq!(request["args"][0], "ui-snapshot");
    let response = json!({
        "ok": true,
        "output": snapshot.to_string(),
        "error": "",
        "error_code": "",
        "error_category": "",
        "retryable": false
    });
    writeln!(stream, "{response}").expect("write snapshot backend response");
    stream.flush().expect("flush snapshot backend response");
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
