//! `wasmcore_run` -- the CLI-triggerable entry point for
//! `agenterm_wasmcore::WasmCoreHost`: loads (or, with no arguments, builds)
//! a real `wasm32-wasip1` guest module, runs it against a real host with a
//! small built-in demo `fleet_call` bridge, and prints the guest's real
//! stdout and exit status.
//!
//! Lives under `examples/` -- Cargo auto-discovers `examples/*.rs` as
//! independently runnable targets (this crate's `Cargo.toml` does not
//! disable `autoexamples`, unlike `autobins`), so this needs no
//! `Cargo.toml` edit at all.
//!
//! Usage:
//!   cargo run --example wasmcore_run -- path/to/guest.wasm
//!   cargo run --example wasmcore_run                      (zero-setup demo)
//!
//! Without an explicit path, this compiles a small built-in demo guest at
//! run time via a real `rustc --target wasm32-wasip1` invocation --
//! mirroring `tests/fleet_call_roundtrip.rs`'s `compiled_guest_wasm` (same
//! mechanism, not shared code: an example and a test are separate
//! compilation units) -- so the command is usable out of the box with zero
//! setup beyond `rustup target add wasm32-wasip1`.
//!
//! Either way, the guest runs against a demo `fleet_call` bridge that
//! recognizes *every* operation_id and echoes its params back inside
//! `{"received": <params_json>}`. This crate has no dependency on the
//! `agenterm` product crate (see `README.md` and `src/lib.rs`'s module
//! docs), so this deliberately does not pretend to reach any real product
//! operation -- it is an honest demonstration of the ABI round trip, not a
//! product integration.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Arc;

use agenterm_wasmcore::{GuestExit, WasmCoreHost, WasmFleetBridgeFn};

/// Real wasm32-wasip1 WASI guest source, compiled at run time by
/// [`build_demo_guest`] when no explicit guest path is given. Deliberately
/// self-contained (no dependency on `guests/fleet_guest.rs`) so this
/// example file alone documents the whole guest-side calling convention;
/// see `../README.md` "fleet_call calling convention" for the full spec.
const DEMO_GUEST_SOURCE: &str = r###"
use std::alloc::{Layout, alloc};

#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(
        op_ptr: *const u8,
        op_len: i32,
        params_ptr: *const u8,
        params_len: i32,
        out_ptr_ptr: *mut i32,
        out_len_ptr: *mut i32,
    ) -> i32;
}

/// Host-callable allocator -- see ../README.md for why the host calls back
/// into a guest-exported allocator rather than guessing a buffer size.
/// Never freed: this is a short-lived demo program that exits right after
/// printing its results.
#[no_mangle]
pub extern "C" fn wasmcore_alloc(len: i32) -> i32 {
    let size = if len < 0 { 1usize } else { (len as usize).max(1) };
    let layout = Layout::from_size_align(size, 1).expect("valid 1-byte-aligned layout");
    unsafe { alloc(layout) as i32 }
}

fn call_fleet(op_id: &str, params_json: &str) -> (i32, String) {
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(
            op_id.as_ptr(),
            op_id.len() as i32,
            params_json.as_ptr(),
            params_json.len() as i32,
            &mut out_ptr as *mut i32,
            &mut out_len as *mut i32,
        )
    };
    let payload = unsafe {
        let slice = std::slice::from_raw_parts(out_ptr as *const u8, out_len as usize);
        String::from_utf8_lossy(slice).into_owned()
    };
    (status, payload)
}

fn main() {
    // Two sequential calls from one guest run -- demonstrates the bridge
    // is reusable within a single execution, not just a one-shot.
    let (status1, payload1) = call_fleet("wasmcore_run.demo", r#"{"hello":"from the demo guest"}"#);
    println!("[1] wasmcore_run.demo status={status1} payload={payload1}");

    let (status2, payload2) = call_fleet("wasmcore_run.second_call", r#"{"n":2}"#);
    println!("[2] wasmcore_run.second_call status={status2} payload={payload2}");

    // Falls off main normally (no explicit exit) -- exercises
    // `GuestExit::Returned`, the counterpart to the explicit
    // `exit(7)`/`GuestExit::Exited` path already covered by
    // `tests/fleet_call_roundtrip.rs`.
}
"###;

fn usage() -> &'static str {
    "usage: cargo run --example wasmcore_run -- [GUEST.WASM]\n\n\
     GUEST.WASM   optional; path to an already-compiled wasm32-wasip1 module.\n\
                  Without it, compiles and runs a small built-in demo guest\n\
                  (via a real `rustc --target wasm32-wasip1` invocation) so\n\
                  this command is usable with zero setup.\n\n\
     Either way, runs the guest against a real WasmCoreHost with a built-in\n\
     demo fleet_call bridge that echoes back `{\"received\": <params_json>}`\n\
     for any operation_id -- this crate has no dependency on the agenterm\n\
     product crate, so this is an honest demo bridge, not a stand-in for a\n\
     real product operation."
}

fn parse_args() -> Result<Option<PathBuf>, String> {
    let mut it = std::env::args().skip(1);
    match it.next() {
        None => Ok(None),
        Some(arg) if arg == "-h" || arg == "--help" => Err(String::new()),
        Some(path) => match it.next() {
            Some(extra) => Err(format!("unrecognized extra argument '{extra}'")),
            None => Ok(Some(PathBuf::from(path))),
        },
    }
}

/// Demo fleet bridge: recognizes every operation_id and echoes
/// `params_json` back inside `{"received": <params_json>}`. Honest and
/// simple, not a stand-in for any real agenterm product operation -- see
/// this file's module docs and `README.md`'s ABI spec.
fn demo_bridge() -> WasmFleetBridgeFn {
    Arc::new(|_op_id: &str, params_json: &str| -> Result<String, String> {
        Ok(format!("{{\"received\":{params_json}}}"))
    })
}

/// Compiles [`DEMO_GUEST_SOURCE`] to a real wasm32-wasip1 module via a real
/// `rustc --target wasm32-wasip1` invocation, mirroring
/// `tests/fleet_call_roundtrip.rs`'s `compiled_guest_wasm`. Output lands
/// under this crate's own (gitignored) `target/` directory, same as the
/// test's guest build.
fn build_demo_guest() -> Result<PathBuf, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = manifest_dir.join("target").join("wasmcore-example-guest");
    std::fs::create_dir_all(&out_dir)
        .map_err(|error| format!("creating {}: {error}", out_dir.display()))?;
    let guest_src = out_dir.join("demo_guest.rs");
    std::fs::write(&guest_src, DEMO_GUEST_SOURCE)
        .map_err(|error| format!("writing {}: {error}", guest_src.display()))?;
    let out_wasm = out_dir.join("demo_guest.wasm");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let status = Command::new(&rustc)
        .args(["--target", "wasm32-wasip1", "--edition", "2021", "-O"])
        .arg(&guest_src)
        .arg("-o")
        .arg(&out_wasm)
        .status()
        .map_err(|error| {
            format!(
                "failed to spawn `{rustc} --target wasm32-wasip1 {}`: {error}\n\
                 (requires the wasm32-wasip1 target: `rustup target add wasm32-wasip1`)",
                guest_src.display()
            )
        })?;
    if !status.success() {
        return Err(format!(
            "compiling {} to wasm32-wasip1 failed with {status}",
            guest_src.display()
        ));
    }
    Ok(out_wasm)
}

fn main() -> ExitCode {
    let explicit_path = match parse_args() {
        Ok(path) => path,
        Err(message) => {
            if message.is_empty() {
                eprintln!("{}", usage());
                return ExitCode::SUCCESS;
            }
            eprintln!("error: {message}\n");
            eprintln!("{}", usage());
            return ExitCode::FAILURE;
        }
    };

    let (wasm_path, is_demo) = match explicit_path {
        Some(path) => (path, false),
        None => match build_demo_guest() {
            Ok(path) => (path, true),
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::FAILURE;
            }
        },
    };

    if is_demo {
        println!(
            "(no guest path given -- built and running the built-in demo guest: {})",
            wasm_path.display()
        );
    } else {
        println!("running guest: {}", wasm_path.display());
    }

    let host = WasmCoreHost::new();
    let result = match host.run_module(&wasm_path, Some(demo_bridge())) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("error: guest run failed: {error:#}");
            return ExitCode::FAILURE;
        }
    };

    if !result.stdout.is_empty() {
        println!("--- guest stdout ---");
        print!("{}", result.stdout);
        println!("--------------------");
    }

    match result.exit {
        GuestExit::Returned => {
            println!("guest exit: returned normally (no explicit exit code)");
            ExitCode::SUCCESS
        }
        GuestExit::Exited(code) => {
            println!("guest exit: called exit({code})");
            std::process::exit(code);
        }
    }
}
