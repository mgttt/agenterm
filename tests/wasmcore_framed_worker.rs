//! Whole-file gate: exercises the wasmcore engine, which defaults off.
#![cfg(feature = "script-wasmcore")]

//! Real, black-box, product-level test for `agenterm-wasmcore`'s wiring
//! into the product script path (`WasmcoreEngineBackend` in
//! `src/script_engine.rs`, dispatched from `execute_inner` in
//! `src/script_worker.rs`).
//!
//! Spawns the actual `agenterm` binary as
//! `__agenterm-internal-engine rh --framed-worker` -- the same shared,
//! engine-agnostic framed-worker subprocess every `AGENTERM_SCRIPT_BACKEND`
//! value routes through (`src/worker_supervisor/mod.rs`'s
//! `SCRIPT_WORKER_ENGINE_ARGS` is *always* the `"rh"` token; which backend
//! actually executes is decided inside `execute_inner`/
//! `ScriptBackend::from_env`, not by which CLI token launched the worker --
//! `tests/rh_backend.rs`/`tests/rh_framed_worker.rs` exercise the identical
//! subprocess entry point for the rh backend). Sets
//! `AGENTERM_SCRIPT_BACKEND=wasmcore` so the worker's real dispatch selects
//! `WasmcoreEngineBackend`, and sends a `ScriptInvocation` whose `source` is
//! the path to a REAL `wasm32-wasip1` binary -- compiled here via a real
//! `rustc --target wasm32-wasip1` invocation of `agenterm-wasmcore`'s own
//! verification guest (`crates/agenterm-wasmcore/guests/fleet_guest.rs`,
//! reused as-is, not duplicated or modified: that crate's own scope is not
//! expanded by this file).
//!
//! This is deliberately NOT a re-run of `agenterm-wasmcore`'s own
//! crate-internal tests (`crates/agenterm-wasmcore/tests/fleet_call_roundtrip.rs`
//! already proves the crate's mechanism in isolation with a hand-rolled
//! bridge closure in the same process). This file proves the PRODUCT wiring
//! around it end to end: `Cargo.toml`'s `script-wasmcore` feature and
//! workspace membership, `WasmcoreEngineBackend::check`/`execute`, the
//! `execute_inner` dispatch branch, and -- the part that cannot be faked --
//! a genuine `fleet_call` round trip carried over the real framed-worker
//! wire protocol (`ScriptFramePayload::BrokerRequest`/`BrokerResponse`),
//! with this test process itself acting as the broker exactly as a real
//! server-side broker would.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use agenterm::script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, SCRIPT_FRAME_VERSION, ScriptBrokerError,
    ScriptBrokerResponse, ScriptBudgets, ScriptFrame, ScriptFramePayload, ScriptFrameRead,
    ScriptInvocation, ScriptOperation, ScriptProfile, read_script_frame, write_script_frame,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Whether this toolchain can actually build `wasm32-wasip1` guests.
///
/// These tests compile a real guest with `rustc --target wasm32-wasip1`,
/// which needs that target's std installed (`rustup target add
/// wasm32-wasip1`). Unlike `crates/agenterm-wasmcore/tests/*` — a
/// different package, so a bare `cargo test` at the workspace root never
/// runs them — this file lives in the ROOT package's `tests/`, so it runs
/// in the Windows quality gate's full `cargo test --all-features`, on a
/// runner that does not install the target. Probing keeps that lane honest
/// (skip, loudly, on a toolchain that cannot express the test) instead of
/// reporting a wasmcore product failure that is really a missing target.
///
/// `rustc --print target-libdir` resolves the path for any *known* target
/// spec whether or not its std is installed, so the directory's existence
/// — not the command's exit status — is what actually answers the question.
fn wasip1_target_available() -> bool {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let Ok(output) = Command::new(&rustc)
        .args(["--print", "target-libdir", "--target", "wasm32-wasip1"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let libdir = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    !libdir.is_empty() && Path::new(&libdir).is_dir()
}

/// Emits the skip notice these tests share, so a skipped run says why.
fn skip_without_wasip1(test: &str) -> bool {
    if wasip1_target_available() {
        return false;
    }
    eprintln!(
        "SKIP {test}: this toolchain has no wasm32-wasip1 std \
         (`rustup target add wasm32-wasip1` enables it)"
    );
    true
}

/// Compiles `agenterm-wasmcore`'s own verification guest to a real `.wasm`
/// file exactly once per test binary run, reused across every `#[test]`
/// here. Mirrors `crates/agenterm-wasmcore/tests/fleet_call_roundtrip.rs`'s
/// own `compiled_guest_wasm` helper -- same source file, same compile
/// invocation shape -- just pointed at the root workspace's manifest dir
/// and its own scratch output directory so the two test binaries never
/// race on the same output path.
fn compiled_guest_wasm() -> &'static Path {
    static WASM_PATH: OnceLock<PathBuf> = OnceLock::new();
    WASM_PATH.get_or_init(|| {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let guest_src = manifest_dir
            .join("crates")
            .join("agenterm-wasmcore")
            .join("guests")
            .join("fleet_guest.rs");
        let out_dir = manifest_dir
            .join("target")
            .join("wasmcore-product-test-guests");
        std::fs::create_dir_all(&out_dir)
            .unwrap_or_else(|e| panic!("create {}: {e}", out_dir.display()));
        let out_wasm = out_dir.join("fleet_guest.wasm");

        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
        let status = Command::new(&rustc)
            .arg("--target")
            .arg("wasm32-wasip1")
            .arg("--edition")
            .arg("2021")
            .arg("-O")
            .arg(&guest_src)
            .arg("-o")
            .arg(&out_wasm)
            .status()
            .unwrap_or_else(|e| {
                panic!(
                    "failed to spawn `{rustc} --target wasm32-wasip1 {}`: {e}\n\
                     (requires the wasm32-wasip1 target: `rustup target add wasm32-wasip1`)",
                    guest_src.display()
                )
            });
        assert!(
            status.success(),
            "compiling {} to wasm32-wasip1 failed with {status}",
            guest_src.display()
        );
        assert!(
            out_wasm.is_file(),
            "expected {} to exist after a successful rustc run",
            out_wasm.display()
        );
        out_wasm
    })
}

fn with_wasmcore_backend<T>(run: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
    unsafe {
        std::env::set_var("AGENTERM_SCRIPT_BACKEND", "wasmcore");
    }
    let out = run();
    unsafe {
        match &prior {
            Some(value) => std::env::set_var("AGENTERM_SCRIPT_BACKEND", value),
            None => std::env::remove_var("AGENTERM_SCRIPT_BACKEND"),
        }
    }
    out
}

/// Real broker reply for the guest's `wasmcore.echo` call: echoes the
/// guest's exact operation id and params back inside a small JSON envelope,
/// proving this test process (standing in for a real server-side broker)
/// received the guest's exact bytes after they crossed guest -> wasmtime
/// host -> `fleet_call` bridge closure -> `BrokerRequest` frame -> here.
fn echo_response(arguments: &serde_json::Value) -> ScriptBrokerResponse {
    // `arguments` is `{"operation_id": ..., "parameters": ...}` -- the exact
    // envelope `src/script_worker.rs`'s `execute_inner` builds around every
    // fleet bridge call, regardless of which engine made it.
    ScriptBrokerResponse {
        ok: true,
        value: Some(serde_json::json!({
            "echoed_op": arguments.get("operation_id").cloned().unwrap_or(serde_json::Value::Null),
            "echoed_params": arguments.get("parameters").cloned().unwrap_or(serde_json::Value::Null),
        })),
        error: None,
    }
}

/// Real broker reply for the guest's second, deliberately-unrecognized
/// call -- proves the error path (not just the happy path) round-trips
/// correctly back through the same real wire protocol.
fn unknown_op_response() -> ScriptBrokerResponse {
    ScriptBrokerResponse {
        ok: false,
        value: None,
        error: Some(ScriptBrokerError {
            code: "unknown_op".to_owned(),
            message: "wasmcore.unknown_op".to_owned(),
            details: None,
        }),
    }
}

#[test]
fn framed_worker_wasmcore_runs_real_wasm_guest_with_fleet_bridge_round_trip() {
    if skip_without_wasip1(
        "framed_worker_wasmcore_runs_real_wasm_guest_with_fleet_bridge_round_trip",
    ) {
        return;
    }
    with_wasmcore_backend(|| {
        let wasm_path = compiled_guest_wasm();

        let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm"))
            .args(["__agenterm-internal-engine", "rh"])
            .arg("--framed-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn framed worker");

        let invocation = ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: "wasmcore-framed-echo".into(),
            api_version: SCRIPT_API_VERSION,
            operation: ScriptOperation::Run,
            profile: ScriptProfile::Local,
            source_label: "fleet_guest.wasm".into(),
            source: wasm_path.display().to_string(),
            project_root: Some(env!("CARGO_MANIFEST_DIR").into()),
            invocation_temp_root: None,
            arguments: vec![],
            budgets: ScriptBudgets::default(),
            observation: None,
        };
        let frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "invoke-wasmcore-framed-echo".into(),
            payload: ScriptFramePayload::Invoke(invocation),
        };

        let mut stdin = child.stdin.take().expect("stdin");
        write_script_frame(&mut stdin, &frame).expect("write invoke frame");

        let mut stdout = child.stdout.take().expect("stdout");
        let result = loop {
            match read_script_frame(&mut stdout).expect("read frame") {
                ScriptFrameRead::Frame(frame) => match frame.payload {
                    ScriptFramePayload::BrokerRequest {
                        invocation_id,
                        request_id,
                        request,
                    } => {
                        assert_eq!(request.operation, "fleet.call");
                        let operation_id = request
                            .arguments
                            .get("operation_id")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_owned();
                        let response = if operation_id == "wasmcore.echo" {
                            echo_response(&request.arguments)
                        } else {
                            unknown_op_response()
                        };
                        let response_frame = ScriptFrame {
                            frame_version: SCRIPT_FRAME_VERSION,
                            frame_id: format!("broker-response-{request_id}"),
                            payload: ScriptFramePayload::BrokerResponse {
                                invocation_id,
                                request_id,
                                response,
                            },
                        };
                        write_script_frame(&mut stdin, &response_frame)
                            .expect("write broker response frame");
                    }
                    ScriptFramePayload::Result(result) => break result,
                    other => panic!("unexpected frame payload: {other:?}"),
                },
                ScriptFrameRead::Eof => {
                    let stderr = child.stderr.take().map(|mut stderr| {
                        let mut text = String::new();
                        let _ = stderr.read_to_string(&mut text);
                        text
                    });
                    panic!("worker EOF before result: stderr={stderr:?}");
                }
                ScriptFrameRead::Rejected(rejection) => {
                    panic!("frame rejected: {rejection:?}");
                }
            }
        };
        drop(stdin);
        let _ = child.wait();

        assert!(result.ok, "expected ok result, got {:?}", result.failure);

        // Proves a genuine wasmtime-executed guest: the guest's real
        // `std::process::exit(7)` call must surface as a clean success at
        // the product level (WasmcoreEngineBackend::execute treats
        // GuestExit::Exited as a normal outcome, not an error), and its
        // real stdout output must be captured verbatim.
        assert!(
            result.stdout.contains("ECHO status=0"),
            "expected a successful (status=0) echo call in guest stdout via the real fleet \
             bridge round trip:\n{}",
            result.stdout
        );
        assert!(
            result.stdout.contains("héllo wörld 🎉"),
            "expected the guest's original unicode params to round-trip guest -> wasmtime host \
             -> BrokerRequest frame -> this test's broker -> BrokerResponse frame -> wasmtime \
             host -> guest -> stdout, byte for byte:\n{}",
            result.stdout
        );
        assert!(
            result.stdout.contains("UNKNOWN status=1"),
            "expected the broker's Err to surface as status=1 in guest stdout:\n{}",
            result.stdout
        );
        assert!(
            result.stdout.contains("unknown_op: wasmcore.unknown_op"),
            "expected the broker's exact error message to round-trip back to the guest:\n{}",
            result.stdout
        );
    });
}

#[test]
fn framed_worker_wasmcore_check_validates_real_binary_and_rejects_garbage() {
    if skip_without_wasip1("framed_worker_wasmcore_check_validates_real_binary_and_rejects_garbage")
    {
        return;
    }
    with_wasmcore_backend(|| {
        let wasm_path = compiled_guest_wasm();

        // A garbage "wasm" file: wrong magic bytes entirely, not just a
        // truncated real module -- proves `check()` really calls
        // `wasmtime::Module::validate` (via `WasmCoreHost::validate_binary`)
        // rather than, say, only checking the file exists.
        let broken_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("wasmcore-product-test-guests");
        std::fs::create_dir_all(&broken_dir).expect("create scratch dir");
        let broken_wasm = broken_dir.join("not_actually_wasm.wasm");
        std::fs::write(&broken_wasm, b"this is not a wasm module")
            .expect("write garbage wasm fixture");

        for (source_label, source, expect_ok) in [
            ("fleet_guest.wasm", wasm_path.display().to_string(), true),
            (
                "not_actually_wasm.wasm",
                broken_wasm.display().to_string(),
                false,
            ),
        ] {
            let mut child = Command::new(env!("CARGO_BIN_EXE_agenterm"))
                .args(["__agenterm-internal-engine", "rh"])
                .arg("--framed-worker")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn framed worker");

            let invocation = ScriptInvocation {
                envelope_version: SCRIPT_ENVELOPE_VERSION,
                invocation_id: format!("wasmcore-framed-check-{source_label}"),
                api_version: SCRIPT_API_VERSION,
                operation: ScriptOperation::Check,
                profile: ScriptProfile::Local,
                source_label: source_label.to_owned(),
                source,
                project_root: Some(env!("CARGO_MANIFEST_DIR").into()),
                invocation_temp_root: None,
                arguments: vec![],
                budgets: ScriptBudgets::default(),
                observation: None,
            };
            let frame = ScriptFrame {
                frame_version: SCRIPT_FRAME_VERSION,
                frame_id: format!("invoke-wasmcore-framed-check-{source_label}"),
                payload: ScriptFramePayload::Invoke(invocation),
            };

            {
                let mut stdin = child.stdin.take().expect("stdin");
                write_script_frame(&mut stdin, &frame).expect("write invoke frame");
            }

            let mut stdout = child.stdout.take().expect("stdout");
            let result = loop {
                match read_script_frame(&mut stdout).expect("read frame") {
                    ScriptFrameRead::Frame(frame) => {
                        if let ScriptFramePayload::Result(result) = frame.payload {
                            break result;
                        }
                    }
                    ScriptFrameRead::Eof => panic!("worker EOF before result ({source_label})"),
                    ScriptFrameRead::Rejected(rejection) => {
                        panic!("frame rejected ({source_label}): {rejection:?}")
                    }
                }
            };
            let _ = child.wait();

            assert_eq!(
                result.ok, expect_ok,
                "{source_label}: expected ok={expect_ok}, got {:?}",
                result.failure
            );
        }
    });
}
