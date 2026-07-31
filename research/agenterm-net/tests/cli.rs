use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn binary() -> String {
    std::env::var("CARGO_BIN_EXE_agenterm-net").expect("Cargo exposes research binary")
}

fn json_output(arguments: &[&str]) -> (std::process::ExitStatus, Value) {
    let output = Command::new(binary())
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("run research binary");
    let value = serde_json::from_slice(&output.stdout).expect("typed JSON stdout");
    (output.status, value)
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
