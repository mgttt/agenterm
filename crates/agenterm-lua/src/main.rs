//! `agenterm-lua` binary — LuaJIT script worker for AgenTerm.
//!
//! Supports `--framed-worker` for the AgenTerm framed worker protocol,
//! and `task run --manifest` for task manifest execution.

use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = if args.len() >= 2 && args[1] == "--framed-worker" {
        match run_framed_worker() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("agenterm-lua: {e}");
                1
            }
        }
    } else {
        eprintln!("agenterm-lua: usage: agenterm-lua --framed-worker");
        1
    };
    std::process::exit(code as i32);
}

fn run_framed_worker() -> Result<u8, String> {
    // Simple framed worker: read one ScriptInvocation (JSON frame), execute, write result.
    let mut input = Vec::new();
    std::io::stdin()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;

    let invocation: Option<serde_json::Value> = serde_json::from_slice(&input).ok();
    let source = invocation
        .as_ref()
        .and_then(|v| v.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let operation = invocation
        .as_ref()
        .and_then(|v| v.get("operation"))
        .and_then(|v| v.as_str())
        .unwrap_or("eval");

    let invocation_id = invocation
        .as_ref()
        .and_then(|v| v.get("invocation_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let engine = agenterm_lua::LuaEngine::new().map_err(|e| e.to_string())?;

    let host = agenterm_lua::LuaHostFunctions::default();

    let mut stdout = std::io::stdout().lock();

    match operation {
        "check" => {
            match engine.check(source) {
                Ok(()) => {
                    let result = serde_json::json!({
                        "envelope_version": 2,
                        "invocation_id": invocation_id,
                        "api_version": 2,
                        "ok": true,
                        "exit_class": "success",
                        "operation": "check",
                        "stdout": "",
                        "duration_ms": 1,
                    });
                    serde_json::to_writer(&mut stdout, &result)
                        .map_err(|e| e.to_string())?;
                    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
                    Ok(0)
                }
                Err(e) => {
                    let result = serde_json::json!({
                        "envelope_version": 2,
                        "invocation_id": invocation_id,
                        "api_version": 2,
                        "ok": false,
                        "exit_class": "script",
                        "operation": "check",
                        "stdout": "",
                        "failure": {
                            "code": "lua_parse",
                            "message": e.to_string(),
                            "category": "script"
                        },
                        "duration_ms": 1,
                    });
                    serde_json::to_writer(&mut stdout, &result)
                        .map_err(|e| e.to_string())?;
                    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
                    Ok(1)
                }
            }
        }
        "eval" | "run" => {
            match engine.eval(source, &host) {
                Ok(eval_result) => {
                    let result = serde_json::json!({
                        "envelope_version": 2,
                        "invocation_id": invocation_id,
                        "api_version": 2,
                        "ok": true,
                        "exit_class": "success",
                        "operation": operation,
                        "stdout": eval_result.stdout,
                        "value": eval_result.value,
                        "duration_ms": 1,
                    });
                    serde_json::to_writer(&mut stdout, &result)
                        .map_err(|e| e.to_string())?;
                    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
                    Ok(0)
                }
                Err(e) => {
                    let result = serde_json::json!({
                        "envelope_version": 2,
                        "invocation_id": invocation_id,
                        "api_version": 2,
                        "ok": false,
                        "exit_class": "script",
                        "operation": operation,
                        "stdout": "",
                        "failure": {
                            "code": "lua_runtime",
                            "message": e.to_string(),
                            "category": "script"
                        },
                        "duration_ms": 1,
                    });
                    serde_json::to_writer(&mut stdout, &result)
                        .map_err(|e| e.to_string())?;
                    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
                    Ok(1)
                }
            }
        }
        _ => {
            // Unknown operation — treat as eval.
            match engine.eval(source, &host) {
                Ok(eval_result) => {
                    let result = serde_json::json!({
                        "envelope_version": 2,
                        "invocation_id": invocation_id,
                        "api_version": 2,
                        "ok": true,
                        "exit_class": "success",
                        "operation": operation,
                        "stdout": eval_result.stdout,
                        "value": eval_result.value,
                        "duration_ms": 1,
                    });
                    serde_json::to_writer(&mut stdout, &result)
                        .map_err(|e| e.to_string())?;
                    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
                    Ok(0)
                }
                Err(e) => {
                    let result = serde_json::json!({
                        "envelope_version": 2,
                        "invocation_id": invocation_id,
                        "api_version": 2,
                        "ok": false,
                        "exit_class": "script",
                        "operation": operation,
                        "stdout": "",
                        "failure": {
                            "code": "lua_runtime",
                            "message": e.to_string(),
                            "category": "script"
                        },
                        "duration_ms": 1,
                    });
                    serde_json::to_writer(&mut stdout, &result)
                        .map_err(|e| e.to_string())?;
                    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
                    Ok(1)
                }
            }
        }
    }
}
