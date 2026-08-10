use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

#[test]
fn public_stdio_lifecycle_keeps_stdout_machine_only() {
    let mut child = mcp_command(&["serve", "--stdio"])
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
fn public_stdio_rejects_an_unsupported_revision_and_recovers() {
    let mut child = mcp_command(&["serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start public agenterm-mcp executable");
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":\"future\",\"method\":\"initialize\",\"params\":",
        "{\"protocolVersion\":\"future\",\"capabilities\":{},",
        "\"clientInfo\":{\"name\":\"black-box\",\"version\":\"1\"}}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":\"supported\",\"method\":\"initialize\",\"params\":",
        "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
        "\"clientInfo\":{\"name\":\"black-box\",\"version\":\"1\"}}}\n"
    );
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input.as_bytes())
        .expect("write MCP revision negotiation");
    let output = child.wait_with_output().expect("wait for EOF shutdown");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let responses = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("stdout line is JSON-RPC"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], "future");
    assert_eq!(responses[0]["error"]["code"], -32005);
    assert_eq!(
        responses[0]["error"]["data"]["code"],
        "mcp_protocol_version"
    );
    assert_eq!(
        responses[0]["error"]["data"]["supported"],
        json!(["2025-11-25"])
    );
    assert_eq!(responses[1]["id"], "supported");
    assert_eq!(responses[1]["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn public_stdio_recovers_after_malformed_and_oversized_frames() {
    let mut child = mcp_command(&["serve", "--stdio"])
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
            },
            "pane_text": "private-pane",
            "selection_text": "private-selection",
            "environment": {"SECRET": "private-environment"},
            "clipboard": "private-clipboard",
            "credential": "private-credential",
            "pat": "private-pat",
            "ipc_secret": "private-ipc-secret"
        }]
    });
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind same-source fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let fixture_snapshot = snapshot.clone();
    let fixture = thread::spawn(move || {
        for _ in 0..2 {
            let stream = accept_fixture(&listener, "same-source request");
            reply_snapshot_backend(stream, &fixture_snapshot);
        }
    });

    let cli_output = Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["cli", "--address", &address, "ui-snapshot"])
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
    let complete_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for private in [
        "private-title",
        "private-draft",
        "private-cwd",
        "private-proxy",
        "private-pane",
        "private-selection",
        "private-environment",
        "private-clipboard",
        "private-credential",
        "private-pat",
        "private-ipc-secret",
    ] {
        assert!(
            !complete_output.contains(private),
            "MCP output leaked {private}"
        );
    }
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

    assert_eq!(mcp["server"]["address"], format!("tcp:{address}"));
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

#[test]
fn backend_error_diagnostics_do_not_echo_private_values() {
    let private_values = [
        "error-private-pane",
        "error-private-selection",
        "error-private-environment",
        "error-private-clipboard",
        "error-private-credential",
        "error-private-pat",
        "error-private-ipc-secret",
    ];
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind private-error fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let backend_error = private_values.join("|");
    let fixture = thread::spawn(move || {
        let mut stream = accept_fixture(&listener, "private-error request");
        let request = read_backend_request(&stream);
        assert_eq!(request["args"][0], "ui-snapshot");
        let response = json!({
            "ok": false,
            "output": "",
            "error": backend_error,
            "error_code": "snapshot_private_failure",
            "error_category": "fixture",
            "retryable": false
        });
        writeln!(stream, "{response}").expect("write private backend error");
    });
    let mut child = start_mcp(&address);
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_initialize(&mut stdin);
    writeln!(
        stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": "private-error",
            "method": "resources/read",
            "params": {"uri": "agenterm://fleet/snapshot"}
        })
    )
    .expect("write private-error read");
    drop(stdin);
    let output = child
        .wait_with_output()
        .expect("wait private-error session");
    fixture.join().expect("join private-error fixture");
    assert!(output.status.success(), "{output:?}");
    let complete_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for private in private_values {
        assert!(
            !complete_output.contains(private),
            "MCP diagnostics leaked {private}"
        );
    }
    let responses = String::from_utf8(output.stdout)
        .expect("MCP stdout UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("MCP response JSON"))
        .collect::<Vec<_>>();
    let error = response_by_id(&responses, "private-error");
    assert_eq!(error["error"]["data"]["class"], "snapshot_failed");
    assert_eq!(
        error["error"]["data"]["backend_code"],
        "snapshot_private_failure"
    );
}

#[test]
fn public_workspace_tabs_and_snapshot_preserve_detached_tree_and_dead_shapes() {
    let snapshot = json!({
        "server_pid": 4242,
        "protocol_version": 1,
        "event_position": {"epoch": "shape-epoch", "sequence": 44},
        "focus": {"window_id": "@3"},
        "window": {
            "visible": false,
            "detached": true,
            "minimized": false,
            "state": "detached"
        },
        "tabs": [
            {
                "id": "@1", "parent_id": null, "name": "renamed-root",
                "note": "leader", "state": "running", "pid": 5101,
                "exit_code": null, "active": false, "has_children": true,
                "collapsed": true, "depth": 0
            },
            {
                "id": "@2", "parent_id": "@1", "name": "tree-child",
                "note": "worker", "state": "dead", "pid": null,
                "exit_code": 17, "active": false, "has_children": false,
                "collapsed": false, "depth": 1
            },
            {
                "id": "@3", "parent_id": null, "name": "active-peer",
                "note": "", "state": "running", "pid": 5103,
                "exit_code": null, "active": true, "has_children": false,
                "collapsed": false, "depth": 0
            }
        ]
    });
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind resource-shape fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let fixture_snapshot = snapshot.clone();
    let fixture = thread::spawn(move || {
        for _ in 0..3 {
            let stream = accept_fixture(&listener, "resource-shape request");
            reply_snapshot_backend(stream, &fixture_snapshot);
        }
    });

    let mut child = start_mcp(&address);
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_initialize(&mut stdin);
    for (id, uri) in [
        ("workspace", "agenterm://fleet/workspace"),
        ("tabs", "agenterm://fleet/tabs"),
        ("snapshot", "agenterm://fleet/snapshot"),
    ] {
        writeln!(
            stdin,
            "{}",
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "resources/read",
                "params": {"uri": uri}
            })
        )
        .expect("write resource-shape read");
    }
    drop(stdin);
    let output = child
        .wait_with_output()
        .expect("wait resource-shape session");
    fixture.join().expect("join resource-shape fixture");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let responses = String::from_utf8(output.stdout)
        .expect("MCP stdout UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("MCP response JSON"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 4);
    let resource = |id: &str| {
        let response = responses
            .iter()
            .find(|response| response["id"] == id)
            .expect("resource response");
        serde_json::from_str::<Value>(
            response["result"]["contents"][0]["text"]
                .as_str()
                .expect("resource text"),
        )
        .expect("resource JSON")
    };
    let workspace = resource("workspace");
    assert_eq!(workspace["event_position"], snapshot["event_position"]);
    assert_eq!(workspace["active_tab_id"], "@3");
    assert_eq!(workspace["tab_count"], 3);
    assert_eq!(workspace["window"]["detached"], true);
    assert_eq!(workspace["window"]["state"], "detached");

    let tabs = resource("tabs");
    assert_eq!(tabs["event_position"], snapshot["event_position"]);
    assert_eq!(tabs["tabs"].as_array().map(Vec::len), Some(3));
    assert_eq!(tabs["tabs"][0]["name"], "renamed-root");
    assert_eq!(tabs["tabs"][1]["parent_id"], "@1");
    assert_eq!(tabs["tabs"][1]["state"], "dead");
    assert_eq!(tabs["tabs"][1]["exit_code"], 17);

    let fleet = resource("snapshot");
    assert_eq!(fleet["event_position"], snapshot["event_position"]);
    assert_eq!(fleet["workspace"]["window_detached"], true);
    assert_eq!(fleet["workspace"]["active_tab_id"], "@3");
    assert_eq!(fleet["tabs"], tabs["tabs"]);
}

#[test]
fn public_workspace_tabs_and_snapshot_limits_fail_closed_with_typed_budget_facts() {
    let snapshot = json!({
        "server_pid": 4242,
        "protocol_version": 7,
        "event_position": {"epoch": "resource-limit-epoch", "sequence": 1},
        "focus": {"window_id": "@1"},
        "window": {
            "visible": true,
            "detached": false,
            "minimized": false,
            "state": "x".repeat(786_432)
        },
        "tabs": [{
            "id": "@1",
            "parent_id": null,
            "name": "resource-limit",
            "note": "x".repeat(786_432),
            "state": "running",
            "pid": 5151,
            "exit_code": null,
            "active": true,
            "has_children": false,
            "collapsed": false,
            "depth": 0
        }]
    });
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind resource-limit fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let fixture = thread::spawn(move || {
        for _ in 0..3 {
            let stream = accept_fixture(&listener, "resource-limit request");
            reply_snapshot_backend(stream, &snapshot);
        }
    });

    let mut child = start_mcp(&address);
    let mut stdin = child.stdin.take().expect("piped stdin");
    stdin
        .write_all(
            concat!(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":",
                "{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},",
                "\"clientInfo\":{\"name\":\"resource-limit\",\"version\":\"1\"}}}\n",
                "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
                "{\"jsonrpc\":\"2.0\",\"id\":\"workspace\",\"method\":\"resources/read\",",
                "\"params\":{\"uri\":\"agenterm://fleet/workspace\"}}\n",
                "{\"jsonrpc\":\"2.0\",\"id\":\"tabs\",\"method\":\"resources/read\",",
                "\"params\":{\"uri\":\"agenterm://fleet/tabs\"}}\n",
                "{\"jsonrpc\":\"2.0\",\"id\":\"snapshot\",\"method\":\"resources/read\",",
                "\"params\":{\"uri\":\"agenterm://fleet/snapshot\"}}\n"
            )
            .as_bytes(),
        )
        .expect("write resource-limit session");
    drop(stdin);
    let output = child
        .wait_with_output()
        .expect("wait resource-limit session");
    fixture.join().expect("join resource-limit fixture");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let responses = String::from_utf8(output.stdout)
        .expect("MCP stdout UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("MCP response JSON"))
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 4);
    for id in ["workspace", "tabs", "snapshot"] {
        let response = response_by_id(&responses, id);
        assert_eq!(response["error"]["code"], -32004);
        assert_eq!(
            response["error"]["data"]["code"],
            "agenterm_response_too_large"
        );
        assert_eq!(response["error"]["data"]["maximum_bytes"], 786_432);
    }
}

#[test]
fn public_instances_resource_reports_healthy_and_dead_registrations() {
    let root = unique_test_root("mcp-instance-resource");
    let instances = root.join("instances");
    fs::create_dir_all(&instances).expect("create instance fixture directory");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind healthy instance fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    write_instance_registration(
        &instances,
        "healthy.json",
        std::process::id(),
        &address,
        "healthy-session",
    );
    write_instance_registration(
        &instances,
        "dead.json",
        u32::MAX,
        "127.0.0.1:1",
        "dead-session",
    );
    let handshake_address = address.clone();
    let fixture = thread::spawn(move || {
        let stream = accept_fixture(&listener, "healthy handshake");
        reply_protocol_backend(stream, std::process::id(), &handshake_address, 1);
    });

    let responses = run_discovery_resource(&instances, "agenterm://fleet/instances");
    fixture.join().expect("join healthy handshake");
    let body = resource_body(&responses, "resource");
    let inventory = body["instances"].as_array().expect("instance inventory");
    assert_eq!(inventory.len(), 2);
    let healthy = inventory
        .iter()
        .find(|instance| instance["session"] == "healthy-session")
        .expect("healthy instance");
    assert_eq!(healthy["address"], address);
    assert_eq!(healthy["status"], "healthy");
    assert_eq!(healthy["compatible"], true);
    let dead = inventory
        .iter()
        .find(|instance| instance["session"] == "dead-session")
        .expect("dead instance");
    assert_eq!(dead["status"], "stale");
    assert_eq!(dead["compatible"], false);

    fs::remove_dir_all(&root).expect("remove instance fixture root");
}

#[test]
fn public_instances_item_limit_fails_before_health_allocation() {
    let root = unique_test_root("mcp-instance-limit");
    let instances = root.join("instances");
    fs::create_dir_all(&instances).expect("create instance-limit directory");
    for index in 0..1_025 {
        write_instance_registration(
            &instances,
            &format!("{index:04}.json"),
            u32::MAX,
            "127.0.0.1:1",
            &format!("limit-{index}"),
        );
    }
    let responses = run_discovery_resource(&instances, "agenterm://fleet/instances");
    let error = response_by_id(&responses, "resource");
    assert_eq!(error["error"]["code"], -32004);
    assert_eq!(error["error"]["data"]["item_kind"], "instances");
    assert_eq!(error["error"]["data"]["actual_items"], 1_025);
    assert_eq!(error["error"]["data"]["maximum_items"], 1_024);
    fs::remove_dir_all(&root).expect("remove instance-limit root");
}

#[test]
fn explicit_legacy_address_selects_the_requested_server() {
    let root = unique_test_root("mcp-selection-one");
    let instances = root.join("instances");
    fs::create_dir_all(&instances).expect("create selection fixture directory");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind selection fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    write_instance_registration(
        &instances,
        "one.json",
        std::process::id(),
        &address,
        "one-session",
    );
    let fixture = thread::spawn(move || {
        let stream = accept_fixture(&listener, "explicit selected snapshot");
        reply_snapshot_backend(stream, &minimal_snapshot("selection-one", 1));
    });

    let responses = run_selected_resource(
        &instances,
        &["--address", &address],
        "agenterm://fleet/workspace",
    );
    fixture.join().expect("join selection fixture");
    let body = resource_body(&responses, "resource");
    assert_eq!(body["server"]["address"], format!("tcp:{address}"));
    assert_eq!(body["event_position"]["epoch"], "selection-one");
    fs::remove_dir_all(&root).expect("remove selection fixture root");
}

#[test]
fn default_main_uses_native_endpoint_and_ignores_random_tcp_inventory() {
    let many_root = unique_test_root("mcp-selection-many");
    let many_instances = many_root.join("instances");
    fs::create_dir_all(&many_instances).expect("create many instance directory");
    let first = TcpListener::bind("127.0.0.1:0").expect("bind first healthy fixture");
    let second = TcpListener::bind("127.0.0.1:0").expect("bind second healthy fixture");
    let first_address = first.local_addr().expect("first address").to_string();
    let second_address = second.local_addr().expect("second address").to_string();
    write_instance_registration(
        &many_instances,
        "first.json",
        std::process::id(),
        &first_address,
        "first-session",
    );
    write_instance_registration(
        &many_instances,
        "second.json",
        std::process::id(),
        &second_address,
        "second-session",
    );
    let many = run_discovery_resource(&many_instances, "agenterm://fleet/workspace");
    let selected = selected_resource_endpoint(&many);
    assert_ne!(selected, first_address);
    assert_ne!(selected, second_address);
    #[cfg(windows)]
    assert!(selected.starts_with(r"pipe:\\.\pipe\agenterm-"));
    #[cfg(unix)]
    assert!(selected.starts_with("unix:"));
    drop(first);
    drop(second);
    fs::remove_dir_all(&many_root).expect("remove many fixture root");
}

#[test]
fn explicit_instance_dev_selects_a_distinct_native_endpoint() {
    let root = unique_test_root("mcp-selection-unhealthy");
    let instances = root.join("instances");
    fs::create_dir_all(&instances).expect("create unhealthy instance directory");
    let main = run_selected_resource(
        &instances,
        &["--instance", "main"],
        "agenterm://fleet/workspace",
    );
    let dev = run_selected_resource(
        &instances,
        &["--instance", "dev"],
        "agenterm://fleet/workspace",
    );
    let main_endpoint = selected_resource_endpoint(&main);
    let dev_endpoint = selected_resource_endpoint(&dev);
    assert_ne!(main_endpoint, dev_endpoint);
    #[cfg(windows)]
    assert!(main_endpoint.starts_with(r"pipe:\\.\pipe\agenterm-"));
    #[cfg(windows)]
    assert!(dev_endpoint.starts_with(r"pipe:\\.\pipe\agenterm-"));
    #[cfg(unix)]
    assert!(main_endpoint.starts_with("unix:"));
    #[cfg(unix)]
    assert!(dev_endpoint.starts_with("unix:"));
    fs::remove_dir_all(&root).expect("remove unhealthy fixture root");
}

#[test]
fn instance_discovery_has_one_total_deadline_for_many_hanging_records() {
    let root = unique_test_root("mcp-discovery-deadline");
    let instances = root.join("instances");
    fs::create_dir_all(&instances).expect("create deadline fixture directory");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind hanging fixture");
    listener
        .set_nonblocking(true)
        .expect("set hanging fixture nonblocking");
    let address = listener.local_addr().expect("hanging address").to_string();
    for index in 0..256 {
        write_instance_registration(
            &instances,
            &format!("hanging-{index}.json"),
            std::process::id(),
            &address,
            &format!("hanging-{index}"),
        );
    }
    let stop = Arc::new(AtomicBool::new(false));
    let fixture = thread::spawn({
        let stop = Arc::clone(&stop);
        move || {
            let mut held = Vec::new();
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => held.push(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("accept hanging fixture: {error}"),
                }
            }
        }
    });
    let started = Instant::now();
    let responses = run_discovery_resource(&instances, "agenterm://fleet/instances");
    let elapsed = started.elapsed();
    stop.store(true, Ordering::Release);
    fixture.join().expect("join hanging fixture");
    let inventory = resource_body(&responses, "resource");
    assert_eq!(inventory["instances"].as_array().map(Vec::len), Some(256));
    assert!(
        elapsed < Duration::from_secs(4),
        "discovery exceeded its published total deadline: {elapsed:?}"
    );
    assert!(
        inventory["instances"]
            .as_array()
            .expect("instance inventory")
            .iter()
            .all(|instance| instance["status"] == "unreachable")
    );
    fs::remove_dir_all(&root).expect("remove deadline fixture root");
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
            env!("CARGO_BIN_EXE_agenterm"),
            &address,
            &workspace,
            &settings,
            &instances,
        )
        .args(["server", "--address", &address])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start isolated server"),
    );
    // OS-enforced backstop for the manual kill/wait loop below the
    // `catch_unwind`: a kill-on-close containment handle that the OS closes
    // during this test process's teardown no matter how that teardown
    // happens (normal return, panic, or this binary being force-killed from
    // the outside by a CI timeout) — unlike the manual cleanup, it does not
    // depend on any Rust code running.
    let _server_tree_guard = agenterm_platform::process::ProcessTreeGuard::attach(
        server.as_ref().expect("server just spawned"),
    )
    .expect("attach kill-on-close containment to isolated server");
    let mut gui: Option<std::process::Child> = None;
    let mut _gui_tree_guard: Option<agenterm_platform::process::ProcessTreeGuard> = None;
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
        _gui_tree_guard = Some(
            agenterm_platform::process::ProcessTreeGuard::attach(
                gui.as_ref().expect("GUI just spawned"),
            )
            .expect("attach kill-on-close containment to replaceable GUI"),
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
        let instance_files_before = directory_file_bytes(&instances);
        let root_entries_before = directory_entry_names(&root);
        let settings_before = fs::read(&settings).ok();

        let mut sidecar =
            configured_command(mcp_cli(), &address, &workspace, &settings, &instances)
                .args(["cli", "mcp", "--address", &address, "serve", "--stdio"])
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

        let post_kill_snapshot = run_cli(
            &address,
            &workspace,
            &settings,
            &instances,
            &["ui-snapshot"],
        );
        assert!(
            post_kill_snapshot.status.success(),
            "{post_kill_snapshot:?}"
        );
        let post_kill_snapshot: Value =
            serde_json::from_slice(&post_kill_snapshot.stdout).expect("post-kill snapshot JSON");
        assert_eq!(
            post_kill_snapshot["server_pid"], snapshot["server_pid"],
            "sidecar kill replaced the server authority"
        );
        assert_eq!(
            post_kill_snapshot["tabs"]
                .as_array()
                .expect("post-kill tabs")
                .iter()
                .map(|tab| (&tab["id"], &tab["pid"]))
                .collect::<Vec<_>>(),
            snapshot["tabs"]
                .as_array()
                .expect("pre-kill tabs")
                .iter()
                .map(|tab| (&tab["id"], &tab["pid"]))
                .collect::<Vec<_>>(),
            "sidecar lifecycle changed PTY ownership"
        );
        assert_eq!(
            directory_file_bytes(&instances),
            instance_files_before,
            "sidecar lifecycle changed instance registrations"
        );
        assert_eq!(
            directory_entry_names(&root),
            root_entries_before,
            "sidecar lifecycle left a temp/settings artifact"
        );
        assert_eq!(
            fs::read(&settings).ok(),
            settings_before,
            "sidecar lifecycle changed settings"
        );

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
            let stream = accept_fixture(&listener, "MCP backend request");
            reply_to_backend(stream);
        }
    });
    let mut child = mcp_command(&["--address", &address, "serve", "--stdio"])
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
            let mut stream = accept_fixture(&listener, "MCP backend request");
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
        let stream = accept_fixture(&listener, "MCP backend request");
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
fn cancellation_wins_over_a_late_backend_completion() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind cancellation-race fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let (accepted_sender, accepted_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let fixture = thread::spawn(move || {
        let mut stream = accept_fixture(&listener, "cancellation-race request");
        read_backend_request(&stream);
        accepted_sender
            .send(())
            .expect("report cancellation-race request");
        release_receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("release late backend completion");
        let response = json!({
            "ok": false,
            "output": "",
            "error": "late fixture completion",
            "error_code": "server_restart",
            "error_category": "event_position",
            "retryable": false
        });
        writeln!(stream, "{response}").expect("write late backend completion");
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
        .expect("wait became active");
    write_cancel(&mut stdin, "wait");
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": "cancel-barrier", "method": "ping"})
    )
    .expect("write cancellation barrier");
    stdin.flush().expect("flush cancellation barrier");
    assert_eq!(
        receive_response(&receiver, &mut child)["id"],
        "cancel-barrier"
    );
    release_sender
        .send(())
        .expect("release late backend completion");
    let cancelled = receive_response(&receiver, &mut child);
    assert_eq!(cancelled["id"], "wait");
    assert_eq!(
        cancelled["result"]["structuredContent"]["outcome"],
        "cancelled"
    );

    drop(stdin);
    let output = child.wait_with_output().expect("wait race session");
    reader.join().expect("join race reader");
    fixture.join().expect("join cancellation-race fixture");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn force_killed_client_closes_its_active_backend_wait() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind isolated IPC fixture");
    let address = listener.local_addr().expect("fixture address").to_string();
    let (accepted_sender, accepted_receiver) = mpsc::channel();
    let (closed_sender, closed_receiver) = mpsc::channel();
    let fixture = thread::spawn(move || {
        let mut stream = accept_fixture(&listener, "MCP backend request");
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
    let mut child_tree = agenterm_platform::process::ProcessTreeGuard::attach(&child)
        .expect("attach containment to isolated MCP client");
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    let reader = spawn_stdout_reader(stdout, sender);
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_wait_session(&mut stdin, "tab.note", None, 5_000);
    assert_eq!(receive_response(&receiver, &mut child)["id"], 1);
    accepted_receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("wait became active before client kill");
    child_tree
        .terminate()
        .expect("force-kill isolated MCP client tree");
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

fn run_discovery_resource(instances: &Path, uri: &str) -> Vec<Value> {
    run_selected_resource(instances, &[], uri)
}

fn run_selected_resource(instances: &Path, selectors: &[&str], uri: &str) -> Vec<Value> {
    let mut child = Command::new(mcp_cli())
        .args(["cli", "mcp"])
        .args(selectors)
        .args(["serve", "--stdio"])
        .env("AGENTERM_INSTANCE_DIR", instances)
        .env("AGENTERM_NO_ACTIVATE", "1")
        .env_remove("AGENTERM_IPC_ENDPOINT")
        .env_remove("AGENTERM_IPC_ADDRESS")
        .env_remove("AGENTERM_INSTANCE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start discovery MCP sidecar");
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_initialize(&mut stdin);
    writeln!(
        stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": "resource",
            "method": "resources/read",
            "params": {"uri": uri}
        })
    )
    .expect("write discovery resource read");
    drop(stdin);
    let output = child.wait_with_output().expect("wait discovery session");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("MCP stdout UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("MCP response JSON"))
        .collect()
}

fn accept_fixture(listener: &TcpListener, context: &str) -> TcpStream {
    listener
        .set_nonblocking(true)
        .expect("set fixture listener nonblocking");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("restore fixture stream blocking mode");
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "{context} was not contacted before the fixture deadline"
                );
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("{context} accept failed: {error}"),
        }
    }
}

fn response_by_id<'a>(responses: &'a [Value], id: &str) -> &'a Value {
    responses
        .iter()
        .find(|response| response["id"] == id)
        .expect("MCP response ID")
}

fn resource_body(responses: &[Value], id: &str) -> Value {
    serde_json::from_str(
        response_by_id(responses, id)["result"]["contents"][0]["text"]
            .as_str()
            .expect("resource text"),
    )
    .expect("resource JSON")
}

fn selected_resource_endpoint(responses: &[Value]) -> String {
    let response = response_by_id(responses, "resource");
    if let Some(endpoint) = response["error"]["data"]["address"].as_str() {
        return endpoint.to_owned();
    }
    resource_body(responses, "resource")["server"]["address"]
        .as_str()
        .expect("resource server endpoint")
        .to_owned()
}

fn unique_test_root(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ))
}

fn write_instance_registration(
    directory: &Path,
    file_name: &str,
    pid: u32,
    address: &str,
    session: &str,
) {
    let record = json!({
        "schema_version": 1,
        "pid": pid,
        "address": address,
        "version": "0.1.10",
        "session": session,
        "workspace_path": format!("{session}-workspace.json"),
        "started_at_unix_ms": 1
    });
    fs::write(
        directory.join(file_name),
        serde_json::to_vec(&record).expect("registration JSON"),
    )
    .expect("write instance registration");
}

fn minimal_snapshot(epoch: &str, sequence: u64) -> Value {
    json!({
        "server_pid": std::process::id(),
        "protocol_version": 1,
        "event_position": {"epoch": epoch, "sequence": sequence},
        "focus": {"window_id": null},
        "window": {
            "visible": false,
            "detached": true,
            "minimized": false,
            "state": "detached"
        },
        "tabs": []
    })
}

#[cfg(windows)]
fn directory_file_bytes(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = fs::read_dir(directory)
        .expect("read fixture directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).expect("read fixture file"),
            )
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[cfg(windows)]
fn directory_entry_names(directory: &Path) -> Vec<String> {
    let mut entries = fs::read_dir(directory)
        .expect("read fixture root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn start_mcp(address: &str) -> std::process::Child {
    let mut command = mcp_command(&["--address", address, "serve", "--stdio"]);
    agenterm_platform::process::configure_owned_command(&mut command)
        .expect("configure owned MCP client process tree");
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start public agenterm-mcp executable")
}

#[cfg(windows)]
fn configured_command(
    program: impl AsRef<std::ffi::OsStr>,
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

fn mcp_cli() -> PathBuf {
    // MCP is hosted under `agenterm cli mcp` (standalone PE removed).
    std::env::var_os("AGENTERM_MCP_CLI")
        .or_else(|| std::env::var_os("AGENTERM_MCP_EXE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_agenterm")))
}

fn mcp_command(args: &[&str]) -> Command {
    let mut command = Command::new(mcp_cli());
    command.args(["cli", "mcp"]);
    command.args(args);
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
        env!("CARGO_BIN_EXE_agenterm"),
        address,
        workspace,
        settings,
        instances,
    )
    .args(["cli", "--address", address])
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
    let _ = writeln!(stream, "{response}");
    let _ = stream.flush();
    let mut trailing = Vec::new();
    let _ = stream.read_to_end(&mut trailing);
}

fn reply_protocol_backend(mut stream: TcpStream, pid: u32, address: &str, protocol_version: u64) {
    let request = read_backend_request(&stream);
    assert_eq!(request["args"][0], "protocol-info");
    let response = json!({
        "ok": true,
        "output": json!({
            "protocol_version": protocol_version,
            "agenterm_version": "0.1.10",
            "identity_scope": "fixture",
            "pid": pid,
            "address": address,
            "build_identity": {},
            "build_identity_complete": false
        }).to_string(),
        "error": "",
        "error_code": "",
        "error_category": "",
        "retryable": false
    });
    writeln!(stream, "{response}").expect("write protocol response");
    stream.flush().expect("flush protocol response");
}

fn read_backend_request(stream: &TcpStream) -> Value {
    let mut request_line = String::new();
    BufReader::new(stream.try_clone().expect("clone fixture stream"))
        .read_line(&mut request_line)
        .expect("read backend request");
    serde_json::from_str(&request_line).expect("typed backend request")
}
