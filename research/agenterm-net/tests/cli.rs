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

fn force_kill_process(pid: u64) {
    #[cfg(windows)]
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run taskkill");
    #[cfg(unix)]
    let status = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run kill");
    assert!(status.success(), "force-kill node pid {pid}");
}

#[test]
fn public_self_test_uses_distinct_processes_and_verifies_blocks() {
    let (status, value) = json_output(&["self-test", "--json"]);
    assert!(status.success(), "{value}");
    assert_eq!(value["deadline_ms"], 30_000);
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
fn hidden_listener_reports_typed_ping_deadline_after_ready() {
    let output = Command::new(binary())
        .args(["__listen", "100"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .expect("run hidden listener");
    assert!(!output.status.success());
    let receipts = String::from_utf8(output.stdout)
        .expect("listener stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("typed listener JSON"))
        .collect::<Vec<_>>();
    assert_eq!(receipts.len(), 2, "{receipts:?}");
    assert_eq!(receipts[0]["event"], "ready");
    assert_eq!(receipts[1]["schema"], "agenterm-net/error/v1");
    assert_eq!(receipts[1]["state"], "failed");
    assert_eq!(receipts[1]["code"], "listener_failed");
    assert_eq!(receipts[1]["message"], "listener deadline exceeded");
}

#[test]
fn private_mesh_proves_dht_pubsub_and_relay_without_public_authority() {
    let (capability_status, capabilities) = json_output(&["capabilities", "--json"]);
    assert!(capability_status.success(), "{capabilities}");
    let advertised = capabilities["result"]["capabilities"]
        .as_array()
        .expect("capability list");
    for name in [
        "private-mesh.dht",
        "private-mesh.pubsub",
        "private-mesh.relay",
    ] {
        let capability = advertised
            .iter()
            .find(|capability| capability["name"] == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(capability["state"], "prototype");
    }

    let started = Instant::now();
    let (status, value) = json_output(&["mesh-self-test", "--json"]);
    assert!(status.success(), "{value}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let result = &value["result"];
    assert_eq!(result["status"], "proven-in-deterministic-fixture");
    assert_eq!(result["public_bootstrap"], false);
    assert_eq!(result["nat_traversal"], false);
    assert_eq!(result["relay_serving_default"], false);
    assert_eq!(result["remote_fleet_control"], false);
    assert_eq!(result["dht"]["provider_record_published"], true);
    assert_eq!(result["dht"]["provider_found_via_hub"], true);
    assert_eq!(result["dht"]["public_bootstrap_attempts"], 0);
    assert_eq!(result["pubsub"]["signed"], true);
    assert_eq!(result["pubsub"]["payload_verified"], true);
    assert_eq!(result["relay"]["reservation_accepted"], true);
    assert_eq!(result["relay"]["circuit_accepted"], true);
    assert_eq!(result["relay"]["source_connected_to_destination"], true);
    assert_eq!(result["relay"]["destination_connected_to_source"], true);
    assert_eq!(result["relay"]["relay_serving_is_fixture_only"], true);
    assert_eq!(value["receipt"]["schema"], "agenterm-net/receipt/v1");
}

#[test]
fn remote_fleet_attach_is_explicit_paired_and_read_only() {
    let (capability_status, capabilities) = json_output(&["capabilities", "--json"]);
    assert!(capability_status.success(), "{capabilities}");
    let advertised = capabilities["result"]["capabilities"]
        .as_array()
        .expect("capability list")
        .iter()
        .find(|capability| capability["name"] == "remote-fleet.read-only-attach")
        .expect("Remote Fleet attach capability");
    assert_eq!(advertised["state"], "prototype");

    let started = Instant::now();
    let (status, value) = json_output(&["attach-self-test", "--json"]);
    assert!(status.success(), "{value}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let result = &value["result"];
    assert_eq!(result["status"], "prototype-proven");
    assert_eq!(result["authenticated_peer_identity"], true);
    assert_eq!(result["accepted"]["state"], "complete");
    assert!(result["accepted"]["snapshot_bytes"].as_u64().unwrap() <= 16 * 1024);
    assert!(result["accepted"]["server_count"].as_u64().unwrap() <= 8);
    assert!(result["accepted"]["event_digest_count"].as_u64().unwrap() <= 16);
    assert_eq!(
        result["accepted"]["event_digest_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(result["rejections"]["replay"], "replay");
    assert_eq!(result["rejections"]["wrong_peer"], "wrong_peer");
    assert_eq!(result["rejections"]["expired"], "expired");
    assert_eq!(result["authority"]["read_only_projection"], true);
    for excluded in [
        "shell",
        "command_execution",
        "pty_control",
        "terminal_input",
        "server_control",
    ] {
        assert_eq!(result["authority"][excluded], false, "{excluded}");
    }
    assert_eq!(result["public_bootstrap"], false);
    assert_eq!(result["nat_traversal"], false);
    assert_eq!(result["relay_serving_default"], false);
    assert_eq!(value["receipt"]["schema"], "agenterm-net/receipt/v1");
}

#[test]
fn cross_process_tcp_fixture_is_private_bounded_and_reaped() {
    let started = Instant::now();
    let (status, value) = json_output(&["tcp-self-test", "--json"]);
    assert!(status.success(), "{value}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let result = &value["result"];
    assert_eq!(result["status"], "prototype-proven");
    assert_eq!(result["process"]["child_processes"], 8);
    assert_eq!(result["process"]["maximum_concurrent_children"], 3);
    assert_eq!(result["process"]["graceful_child_exits"], 6);
    assert_eq!(result["process"]["forced_cleanup_children"], 2);
    assert_eq!(result["process"]["reaped_children"], 8);
    assert_eq!(result["process"]["residual_children"], 0);
    assert_eq!(result["process"]["no_fixed_sleep"], true);

    let attach = &result["attach"];
    assert_eq!(attach["authenticated_peer_identity"], true);
    assert_eq!(attach["explicit_pairing"], true);
    assert!(attach["snapshot_bytes"].as_u64().unwrap() <= 16 * 1024);
    assert!(attach["server_count"].as_u64().unwrap() <= 8);
    assert!(attach["event_digest_count"].as_u64().unwrap() <= 16);
    assert_eq!(attach["replay_rejection"], "replay");
    assert_eq!(attach["wrong_peer_rejection"], "wrong_peer");
    assert_eq!(attach["expired_rejection"], "expired");
    assert_ne!(attach["paired_peer_id"], attach["wrong_peer_id"]);

    let dht = &result["dht"];
    assert_eq!(dht["record_published"], true);
    assert_eq!(dht["record_found_via_hub"], true);
    assert_eq!(dht["publisher_forced_cleanup_reaped"], true);
    assert_eq!(dht["hub_forced_cleanup_reaped"], true);
    assert_eq!(dht["public_bootstrap_attempts"], 0);
    assert_eq!(dht["record_sha256"].as_str().unwrap().len(), 64);

    for excluded in [
        "shell",
        "command_execution",
        "pty_control",
        "terminal_input",
        "server_control",
    ] {
        assert_eq!(result["authority"][excluded], false, "{excluded}");
    }
    assert_eq!(result["public_bootstrap"], false);
    assert_eq!(result["nat_traversal"], false);
    assert_eq!(result["relay_serving_default"], false);
    assert_eq!(value["receipt"]["schema"], "agenterm-net/receipt/v1");
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
fn crashed_durable_node_requires_explicit_recovery_and_reconnects_control() {
    let state = test_path("durable-node-crash-recovery");
    let (capability_status, capabilities) = json_output(&["capabilities", "--json"]);
    assert!(capability_status.success(), "{capabilities}");
    let advertised = capabilities["result"]["capabilities"]
        .as_array()
        .expect("capability list")
        .iter()
        .find(|capability| capability["name"] == "node.explicit-crash-recovery")
        .expect("crash recovery capability");
    assert_eq!(advertised["state"], "prototype");

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
    let prior_pid = started["result"]["descriptor"]["pid"].as_u64().unwrap();
    let peer_id = started["result"]["descriptor"]["peer_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (live_recovery_status, live_recovery) = json_output(&[
        "node",
        "recover",
        "--state-dir",
        path_text(&state),
        "--json",
    ]);
    assert!(!live_recovery_status.success(), "{live_recovery}");
    assert_eq!(live_recovery["code"], "node_recover_failed");
    assert!(
        live_recovery["message"]
            .as_str()
            .unwrap()
            .contains("still reachable")
    );

    force_kill_process(prior_pid);
    let (status_status, unavailable) =
        json_output(&["node", "status", "--state-dir", path_text(&state), "--json"]);
    assert!(!status_status.success(), "{unavailable}");
    assert_eq!(unavailable["code"], "node_status_failed");

    let (start_again_status, refused) = json_output(&[
        "node",
        "start",
        "--state-dir",
        path_text(&state),
        "--identity",
        "durable",
        "--json",
    ]);
    assert!(!start_again_status.success(), "{refused}");
    assert_eq!(refused["code"], "node_start_failed");
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("refusing to erase crash evidence")
    );

    let (recovery_status, recovered) = json_output(&[
        "node",
        "recover",
        "--state-dir",
        path_text(&state),
        "--json",
    ]);
    assert!(recovery_status.success(), "{recovered}");
    let result = &recovered["result"];
    assert_eq!(result["schema"], "agenterm-net/node-recovery/v1");
    assert_eq!(result["lifecycle"], "recovered");
    assert_eq!(result["prior_descriptor"]["pid"], prior_pid);
    assert_eq!(result["prior_descriptor"]["peer_id"], peer_id);
    assert_ne!(result["replacement"]["descriptor"]["pid"], prior_pid);
    assert_eq!(result["replacement"]["descriptor"]["peer_id"], peer_id);
    assert_eq!(result["owner_absence_confirmed"], true);
    assert_eq!(result["control_reconnected"], true);
    assert_eq!(result["identity_continuity"], true);
    assert_eq!(result["store_continuity"], true);
    assert_eq!(result["explicit_operator_action"], true);
    assert!(std::path::Path::new(result["crash_evidence_path"].as_str().unwrap()).exists());

    let (status_status, observed) =
        json_output(&["node", "status", "--state-dir", path_text(&state), "--json"]);
    assert!(status_status.success(), "{observed}");
    assert_eq!(observed["result"]["descriptor"]["peer_id"], peer_id);
    assert!(
        json_output(&["node", "stop", "--state-dir", path_text(&state), "--json"])
            .0
            .success()
    );
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

#[test]
fn durable_identity_backup_rotation_and_loss_recovery_are_explicit() {
    let state = test_path("identity-lifecycle");
    let original_backup = state.join("backups/original.json");
    let rotation_backup = state.join("backups/rotation.json");
    let current_backup = state.join("backups/current.json");

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
    let original_peer = started["result"]["descriptor"]["peer_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (live_backup_status, live_backup) = json_output(&[
        "identity",
        "backup",
        "--state-dir",
        path_text(&state),
        "--output",
        path_text(&original_backup),
        "--json",
    ]);
    assert!(!live_backup_status.success(), "{live_backup}");
    assert_eq!(live_backup["code"], "identity_backup_failed");
    assert!(live_backup["message"].as_str().unwrap().contains("stopped"));
    assert!(
        json_output(&["node", "stop", "--state-dir", path_text(&state), "--json"])
            .0
            .success()
    );

    let (backup_status, backed_up) = json_output(&[
        "identity",
        "backup",
        "--state-dir",
        path_text(&state),
        "--output",
        path_text(&original_backup),
        "--json",
    ]);
    assert!(backup_status.success(), "{backed_up}");
    assert_eq!(backed_up["result"]["peer_id"], original_peer);
    assert_eq!(
        backed_up["result"]["schema"],
        "agenterm-net/identity-lifecycle/v1"
    );

    let (rotate_status, rotated) = json_output(&[
        "identity",
        "rotate",
        "--state-dir",
        path_text(&state),
        "--backup",
        path_text(&rotation_backup),
        "--json",
    ]);
    assert!(rotate_status.success(), "{rotated}");
    assert_eq!(rotated["result"]["previous_peer_id"], original_peer);
    let rotated_peer = rotated["result"]["peer_id"].as_str().unwrap().to_string();
    assert_ne!(rotated_peer, original_peer);

    let (current_backup_status, current) = json_output(&[
        "identity",
        "backup",
        "--state-dir",
        path_text(&state),
        "--output",
        path_text(&current_backup),
        "--json",
    ]);
    assert!(current_backup_status.success(), "{current}");
    assert_eq!(current["result"]["peer_id"], rotated_peer);

    fs::remove_file(state.join("identity.key")).unwrap();
    let (lost_status, lost) = json_output(&[
        "node",
        "start",
        "--state-dir",
        path_text(&state),
        "--identity",
        "durable",
        "--json",
    ]);
    assert!(!lost_status.success(), "{lost}");
    assert_eq!(lost["code"], "node_start_failed");
    assert!(lost["message"].as_str().unwrap().contains("missing"));

    let (wrong_status, wrong) = json_output(&[
        "identity",
        "restore",
        "--state-dir",
        path_text(&state),
        "--input",
        path_text(&original_backup),
        "--json",
    ]);
    assert!(!wrong_status.success(), "{wrong}");
    assert_eq!(wrong["code"], "identity_restore_failed");
    assert!(
        wrong["message"]
            .as_str()
            .unwrap()
            .contains("does not match")
    );

    let (restore_status, restored) = json_output(&[
        "identity",
        "restore",
        "--state-dir",
        path_text(&state),
        "--input",
        path_text(&current_backup),
        "--json",
    ]);
    assert!(restore_status.success(), "{restored}");
    assert_eq!(restored["result"]["peer_id"], rotated_peer);

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
    assert_eq!(restarted["result"]["descriptor"]["peer_id"], rotated_peer);
    assert!(
        json_output(&["node", "stop", "--state-dir", path_text(&state), "--json"])
            .0
            .success()
    );
    fs::remove_dir_all(state).unwrap();
}
