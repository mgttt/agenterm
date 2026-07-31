use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn binary() -> String {
    std::env::var("CARGO_BIN_EXE_agenterm-net").expect("Cargo exposes research binary")
}

fn json_output(arguments: &[&str]) -> (std::process::ExitStatus, Value) {
    let mut child = Command::new(binary())
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("run research binary");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("captured stdout"))
        .read_line(&mut line)
        .expect("read typed JSON line");
    let status = child.wait().expect("reap research binary");
    let value = serde_json::from_str(&line).expect("typed JSON stdout");
    (status, value)
}

fn test_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "agenterm-net-cli-{label}-{}-{nonce:x}",
        std::process::id()
    ))
}

fn path_text(path: &std::path::Path) -> &str {
    path.to_str().expect("test path is UTF-8")
}

#[test]
fn public_self_test_uses_distinct_processes_and_verifies_blocks() {
    let (status, value) = json_output(&["self-test", "--json"]);
    assert!(status.success(), "{value}");
    let process = &value["result"]["process"];
    assert_ne!(process["listener_pid"], process["connector_pid"]);
    assert_eq!(process["handshake"], true);
    assert_eq!(process["bounded_ping"], true);
    assert_eq!(process["child_exit_clean"], true);
    assert_eq!(process["orphan_cleanup_armed"], true);
    assert_eq!(process["forced_cleanup_reaped"], true);
    assert!(process["forced_cleanup_pid"].as_u64().unwrap() > 0);
    let resources = &value["result"]["resources"];
    assert_eq!(resources["measurement_complete"], true);
    assert!(resources["peak_child_rss_bytes"].as_u64().unwrap() > 0);
    assert!(resources["max_observed_child_threads"].as_u64().unwrap() > 0);
    assert_eq!(resources["process_samples"].as_array().unwrap().len(), 2);
    assert_eq!(value["result"]["block"]["round_trip_verified"], true);
    assert_eq!(value["result"]["block"]["corruption_rejected"], true);
    assert_eq!(value["result"]["block"]["store_removed"], true);
}

#[test]
fn peer_loss_is_typed_and_bounded() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve loopback port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let address = format!("/ip4/127.0.0.1/tcp/{port}");
    let started = Instant::now();
    let (status, value) = json_output(&[
        "__connect",
        &address,
        "12D3KooWGpeStS63KGmG8n6YcUDomaRh9hAPLUbvouHWu4f6xo6c",
        "500",
    ]);
    assert!(!status.success(), "{value}");
    assert_eq!(value["state"], "failed");
    assert_eq!(value["code"], "connector_failed");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn listener_can_be_cancelled_after_ready_without_an_orphan() {
    let mut child = Command::new(binary())
        .args(["__listen", "10000"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn listener worker");
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .expect("read ready event");
    let ready: Value = serde_json::from_str(&ready).expect("typed ready event");
    assert_eq!(ready["event"], "ready");
    child.kill().expect("cancel worker");
    let status = child.wait().expect("reap cancelled worker");
    assert!(!status.success());
    assert!(child.try_wait().unwrap().is_some());
}

#[test]
fn durable_node_lifecycle_is_explicit_and_identity_survives_restart() {
    let state = test_path("durable-node");
    let (start_status, started) = json_output(&[
        "node",
        "start",
        "--state-dir",
        path_text(&state),
        "--identity",
        "durable",
        "--json",
    ]);
    assert!(start_status.success(), "{started}");
    assert_eq!(started["result"]["lifecycle"], "running");
    assert_eq!(started["result"]["descriptor"]["identity"], "durable");
    assert_eq!(started["result"]["descriptor"]["public_bootstrap"], false);
    assert_eq!(started["result"]["descriptor"]["nat_traversal"], false);
    assert_eq!(started["result"]["descriptor"]["relay_server"], false);
    assert_eq!(started["result"]["descriptor"]["remote_control"], false);
    assert_eq!(started["receipt"]["schema"], "agenterm-net/receipt/v1");
    let peer_id = started["result"]["descriptor"]["peer_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status_status, observed) =
        json_output(&["node", "status", "--state-dir", path_text(&state), "--json"]);
    assert!(status_status.success(), "{observed}");
    assert_eq!(observed["result"]["descriptor"]["peer_id"], peer_id);
    assert_eq!(observed["result"]["store"]["corrupt_blocks"], 0);
    assert!(
        observed["result"]["resources"]["peak_rss_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );

    let (stop_status, stopped) =
        json_output(&["node", "stop", "--state-dir", path_text(&state), "--json"]);
    assert!(stop_status.success(), "{stopped}");
    assert_eq!(stopped["result"]["lifecycle"], "stopped");
    assert!(!state.join("node.json").exists());

    let (restart_status, restarted) = json_output(&[
        "node",
        "start",
        "--state-dir",
        path_text(&state),
        "--identity",
        "durable",
        "--json",
    ]);
    assert!(restart_status.success(), "{restarted}");
    assert_eq!(restarted["result"]["descriptor"]["peer_id"], peer_id);
    assert_eq!(restarted["result"]["descriptor"]["identity_created"], false);
    let (stop_status, stopped) =
        json_output(&["node", "stop", "--state-dir", path_text(&state), "--json"]);
    assert!(stop_status.success(), "{stopped}");
    fs::remove_dir_all(state).unwrap();
}

#[test]
fn ephemeral_node_restart_rotates_identity_without_a_key_file() {
    let state = test_path("ephemeral-node");
    let start = || {
        json_output(&[
            "node",
            "start",
            "--state-dir",
            path_text(&state),
            "--identity",
            "ephemeral",
            "--json",
        ])
    };
    let stop = || json_output(&["node", "stop", "--state-dir", path_text(&state), "--json"]);
    let (first_status, first) = start();
    assert!(first_status.success(), "{first}");
    let first_peer = first["result"]["descriptor"]["peer_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(stop().0.success());
    let (second_status, second) = start();
    assert!(second_status.success(), "{second}");
    let second_peer = second["result"]["descriptor"]["peer_id"].as_str().unwrap();
    assert_ne!(first_peer, second_peer);
    assert!(stop().0.success());
    assert!(!state.join("identity.key").exists());
    fs::remove_dir_all(state).unwrap();
}

#[test]
fn persistent_store_cli_round_trips_pins_and_gc() {
    let root = test_path("store");
    let store = root.join("blocks");
    let input = root.join("input.bin");
    let output = root.join("output.bin");
    fs::create_dir_all(&root).unwrap();
    fs::write(&input, b"persistent verified payload").unwrap();
    let (put_status, put) = json_output(&[
        "store",
        "put",
        "--store",
        path_text(&store),
        "--input",
        path_text(&input),
        "--pin",
        "--json",
    ]);
    assert!(put_status.success(), "{put}");
    assert_eq!(put["result"]["pinned"], true);
    let cid = put["result"]["cid"].as_str().unwrap();
    let (get_status, get) = json_output(&[
        "store",
        "get",
        "--store",
        path_text(&store),
        "--cid",
        cid,
        "--output",
        path_text(&output),
        "--json",
    ]);
    assert!(get_status.success(), "{get}");
    assert_eq!(get["result"]["verified"], true);
    assert_eq!(fs::read(&output).unwrap(), fs::read(&input).unwrap());

    let (unpin_status, unpin) = json_output(&[
        "store",
        "unpin",
        "--store",
        path_text(&store),
        "--cid",
        cid,
        "--json",
    ]);
    assert!(unpin_status.success(), "{unpin}");
    let (gc_status, gc) = json_output(&["store", "gc", "--store", path_text(&store), "--json"]);
    assert!(gc_status.success(), "{gc}");
    assert_eq!(gc["result"]["removed_blocks"], 1);
    assert_eq!(gc["result"]["snapshot"]["block_count"], 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_node_descriptor_is_preserved_and_second_owner_is_refused() {
    let state = test_path("stale-node");
    fs::create_dir_all(&state).unwrap();
    let descriptor = serde_json::json!({
        "schema": "agenterm-net/node-descriptor/v1",
        "pid": 4294967295_u32,
        "peer_id": "stale-peer",
        "identity": "ephemeral",
        "identity_created": true,
        "identity_key_path": null,
        "control_address": "127.0.0.1:1",
        "nonce": "stale-nonce",
        "started_unix_ms": 0,
        "state_dir": path_text(&state),
        "store_dir": path_text(&state.join("store")),
        "network_scope": "private-mesh only",
        "public_bootstrap": false,
        "nat_traversal": false,
        "relay_server": false,
        "remote_control": false
    });
    fs::write(
        state.join("node.json"),
        serde_json::to_vec(&descriptor).unwrap(),
    )
    .unwrap();
    let (status, result) = json_output(&[
        "node",
        "start",
        "--state-dir",
        path_text(&state),
        "--identity",
        "ephemeral",
        "--json",
    ]);
    assert!(!status.success(), "{result}");
    assert_eq!(result["code"], "node_start_failed");
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("refusing to erase crash evidence")
    );
    assert!(state.join("node.json").exists());
    fs::remove_dir_all(state).unwrap();
}

#[test]
fn oversized_store_put_has_a_typed_budget_failure() {
    let root = test_path("store-budget");
    let store = root.join("store");
    let input = root.join("oversized.bin");
    fs::create_dir_all(&root).unwrap();
    fs::write(&input, vec![0_u8; 4 * 1024 * 1024 + 1]).unwrap();
    let (status, result) = json_output(&[
        "store",
        "put",
        "--store",
        path_text(&store),
        "--input",
        path_text(&input),
        "--json",
    ]);
    assert!(!status.success(), "{result}");
    assert_eq!(result["code"], "store_put_failed");
    assert!(result["message"].as_str().unwrap().contains("exceeds"));
    fs::remove_dir_all(root).unwrap();
}
