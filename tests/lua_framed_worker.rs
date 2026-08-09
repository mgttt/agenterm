//! Black-box framed-worker test for the lua script engine.
//! Spawns `agenterm __agenterm-internal-engine lua --framed-worker`
//! (the standalone `agenterm-lua` binary is retired; this is the internal
//! dispatch path the main `agenterm` PE uses in-process/via re-exec), sends
//! `ScriptInvocation` frames, and validates `ScriptResult` responses.

use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

fn send_invocation(
    mut child: std::process::Child,
    invocation: &Value,
) -> Result<Value, String> {
    {
        let stdin = child.stdin.as_mut().ok_or("stdin unavailable")?;
        serde_json::to_writer(&mut *stdin, invocation).map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
    }

    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value =
        serde_json::from_str(stdout.trim()).map_err(|e| format!("json_parse: {e}: {stdout}"))?;
    Ok(parsed)
}

fn spawn_lua() -> Result<std::process::Child, String> {
    Command::new(env!("CARGO_BIN_EXE_agenterm"))
        .args(["__agenterm-internal-engine", "lua"])
        .arg("--framed-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())
}

fn eval_invocation(source: &str) -> Value {
    serde_json::json!({
        "envelope_version": 2,
        "invocation_id": "test-lua-fw",
        "api_version": 2,
        "operation": "eval",
        "profile": "local",
        "source_label": "test",
        "source": source,
        "arguments": [],
        "budgets": {
            "source_bytes": 65536,
            "operations": 1000000,
            "call_depth": 64,
            "expression_depth": 64,
            "collection_items": 10000,
            "string_bytes": 262144,
            "output_bytes": 65536,
            "wall_time_ms": 5000
        }
    })
}

fn check_invocation(source: &str) -> Value {
    serde_json::json!({
        "envelope_version": 2,
        "invocation_id": "test-lua-fw-check",
        "api_version": 2,
        "operation": "check",
        "profile": "local",
        "source_label": "test",
        "source": source,
        "arguments": [],
        "budgets": {
            "source_bytes": 65536,
            "operations": 1000000,
            "call_depth": 64,
            "expression_depth": 64,
            "collection_items": 10000,
            "string_bytes": 262144,
            "output_bytes": 65536,
            "wall_time_ms": 5000
        }
    })
}

#[test]
fn framed_worker_eval_returns_42() {
    let child = spawn_lua().expect("spawn lua");
    let result = send_invocation(child, &eval_invocation("return 42"))
        .expect("send invocation");
    assert_eq!(result["ok"], true);
    assert_eq!(result["value"], 42);
    assert_eq!(result["exit_class"], "success");
}

#[test]
fn framed_worker_eval_with_print() {
    let child = spawn_lua().expect("spawn lua");
    let result = send_invocation(
        child,
        &eval_invocation("print('hello framed') return 7"),
    )
    .expect("send invocation");
    assert_eq!(result["ok"], true);
    assert_eq!(result["value"], 7);
    assert!(result["stdout"].as_str().unwrap_or("").contains("hello framed"));
}

#[test]
fn framed_worker_check_valid() {
    let child = spawn_lua().expect("spawn lua");
    let result =
        send_invocation(child, &check_invocation("return 0")).expect("send invocation");
    assert_eq!(result["ok"], true);
    assert_eq!(result["exit_class"], "success");
}

#[test]
fn framed_worker_check_invalid_syntax() {
    let child = spawn_lua().expect("spawn lua");
    let result =
        send_invocation(child, &check_invocation("return !!")).expect("send invocation");
    assert_eq!(result["ok"], false);
    assert_eq!(result["exit_class"], "script");
}
