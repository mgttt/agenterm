//! Real, end-to-end verification: compiles the actual Rust WASI guest
//! program at `guests/fleet_guest.rs` with a real `rustc --target
//! wasm32-wasip1` invocation (no fake/mocked bytes), runs it through
//! `agenterm_wasmcore::WasmCoreHost` with a real (test-double, clearly
//! labeled) bridge closure, and asserts the string marshalling actually
//! round-trips real non-ASCII content in both directions, that a bridge
//! `Err` propagates back to the guest correctly, and that the guest's
//! real `exit(7)` is observed as `GuestExit::Exited(7)`, not a crash.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

use agenterm_wasmcore::{GuestExit, WasmCoreHost, WasmFleetBridgeFn};

/// Compiles `guests/fleet_guest.rs` to a real `.wasm` file exactly once
/// per test binary run (tests execute concurrently on separate threads by
/// default; `OnceLock::get_or_init` serializes the one real compile
/// instead of racing N copies of `rustc`), and reuses the result across
/// every `#[test]` in this file.
fn compiled_guest_wasm() -> &'static Path {
    static WASM_PATH: OnceLock<PathBuf> = OnceLock::new();
    WASM_PATH.get_or_init(|| {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let guest_src = manifest_dir.join("guests").join("fleet_guest.rs");
        let out_dir = manifest_dir.join("target").join("wasmcore-test-guests");
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

/// A real (test-double) bridge: recognizes exactly one operation id and
/// echoes its params back inside a small JSON envelope (proving the host
/// received the guest's exact bytes), and returns `Err` for anything else
/// (proving the error path, not just the happy path).
fn echo_and_reject_bridge() -> WasmFleetBridgeFn {
    Arc::new(|op_id: &str, params_json: &str| -> Result<String, String> {
        match op_id {
            "wasmcore.echo" => Ok(format!(
                "{{\"echoed_op\":\"{op_id}\",\"echoed_params\":{params_json}}}"
            )),
            other => Err(format!("unknown_op: {other}")),
        }
    })
}

#[test]
fn fleet_call_round_trips_real_unicode_and_propagates_bridge_errors() {
    let wasm_path = compiled_guest_wasm();
    let host = WasmCoreHost::new();

    let result = host
        .run_module(wasm_path, Some(echo_and_reject_bridge()))
        .expect("guest run should complete (not trap)");

    eprintln!("[test] guest stdout:\n{}", result.stdout);

    assert_eq!(
        result.exit,
        GuestExit::Exited(7),
        "guest calls std::process::exit(7) explicitly -- this must surface as a real \
         I32Exit(7), not a trap/crash"
    );

    // Happy path: the bridge saw the guest's exact operation id and JSON
    // params (proving guest -> host marshalling), and the guest printed
    // back exactly what the host's `fleet_call` wrote into its memory
    // (proving host -> guest marshalling) -- including real multi-byte
    // UTF-8 content, not just ASCII.
    assert!(
        result.stdout.contains("ECHO status=0"),
        "expected a successful (status=0) echo call in guest stdout:\n{}",
        result.stdout
    );
    assert!(
        result.stdout.contains("héllo wörld 🎉"),
        "expected the exact unicode greeting to round-trip host -> guest -> stdout:\n{}",
        result.stdout
    );
    assert!(
        result.stdout.contains(r#""echoed_op":"wasmcore.echo""#),
        "expected the host's echoed JSON envelope in guest stdout:\n{}",
        result.stdout
    );

    // Error path: the bridge's `Err(..)` for the unrecognized operation
    // must propagate back to the guest as status=1 with the exact message.
    assert!(
        result.stdout.contains("UNKNOWN status=1"),
        "expected the bridge's Err to surface as status=1 in guest stdout:\n{}",
        result.stdout
    );
    assert!(
        result.stdout.contains("unknown_op: wasmcore.unknown_op"),
        "expected the bridge's exact error message to round-trip back to the guest:\n{}",
        result.stdout
    );
}

#[test]
fn fleet_call_reports_no_bridge_when_none_is_configured() {
    let wasm_path = compiled_guest_wasm();
    let host = WasmCoreHost::new();

    let result = host
        .run_module(wasm_path, None)
        .expect("guest run should complete (not trap) even with no bridge configured");

    eprintln!("[test] guest stdout (no bridge):\n{}", result.stdout);

    assert_eq!(result.exit, GuestExit::Exited(7));
    // Both calls the guest makes should come back as status=2 (no bridge
    // configured for this host run at all) -- distinct from status=1
    // (the bridge ran and rejected the operation).
    assert!(
        result.stdout.contains("ECHO status=2"),
        "expected NO_BRIDGE (status=2) with no bridge configured:\n{}",
        result.stdout
    );
    assert!(
        result.stdout.contains("UNKNOWN status=2"),
        "expected NO_BRIDGE (status=2) with no bridge configured:\n{}",
        result.stdout
    );
}
