//! Adversarial / hardening coverage for Part 2, scenarios 3 and 4: very
//! large `params_json`/`result_json` payloads (several MB, round-tripped
//! in both directions with independent checksums so truncation would be
//! caught, not just "didn't crash"), and many sequential `fleet_call`
//! invocations from a single guest run (state-leak / use-after-free-shaped
//! bug hunting). Same real-guest, real-host discipline as
//! `tests/hardening_link_and_bounds.rs` -- see that file's module docs for
//! the shared rationale.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use agenterm_wasmcore::{WasmCoreHost, WasmFleetBridgeFn};

fn compile_guest(name: &str, source: &str) -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = manifest_dir
        .join("target")
        .join("wasmcore-hardening-guests");
    std::fs::create_dir_all(&out_dir)
        .unwrap_or_else(|e| panic!("create {}: {e}", out_dir.display()));
    let src_path = out_dir.join(format!("{name}.rs"));
    std::fs::write(&src_path, source)
        .unwrap_or_else(|e| panic!("write {}: {e}", src_path.display()));
    let out_wasm = out_dir.join(format!("{name}.wasm"));

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let status = Command::new(&rustc)
        .args(["--target", "wasm32-wasip1", "--edition", "2021", "-O"])
        .arg(&src_path)
        .arg("-o")
        .arg(&out_wasm)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn `{rustc} --target wasm32-wasip1 {}`: {e}\n\
                 (requires the wasm32-wasip1 target: `rustup target add wasm32-wasip1`)",
                src_path.display()
            )
        });
    assert!(
        status.success(),
        "compiling {} to wasm32-wasip1 failed with {status}",
        src_path.display()
    );
    out_wasm
}

/// Same simple additive checksum on both the host (this file) and guest
/// (embedded source) sides -- deliberately not a "real" hash, just enough
/// to make silent truncation/corruption detectable without needing a
/// crypto dependency this crate does not otherwise have.
fn checksum(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0u64, |acc, b| acc.wrapping_add(u64::from(*b)))
}

/// Deterministic filler content of an exact byte length -- same generator
/// shape as the guest-side `make_pattern` below, so the host can compute
/// an independent expected checksum for a payload it did not itself
/// receive the bytes of (the *result* direction: the guest is the one
/// that reads those bytes back, not this test process).
fn make_pattern(byte_len: usize, alphabet: &[u8]) -> Vec<u8> {
    (0..byte_len)
        .map(|i| alphabet[i % alphabet.len()])
        .collect()
}

// ---------------------------------------------------------------------
// Scenario 3: several-MB params_json (guest -> host) and result_json
// (host -> guest), independently checksummed on both ends so truncation
// in either direction, or a length-arithmetic edge (usize/i32 mismatch
// given WASM's i32-only ABI vs a 64-bit host), would show up as a
// checksum/length mismatch rather than silently passing.
// ---------------------------------------------------------------------

const PARAMS_MB: usize = 5;
const RESULT_MB: usize = 6; // deliberately a different size than the params direction

const GUEST_LARGE_ROUNDTRIP: &str = r###"
#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(op_ptr: *const u8, op_len: i32, params_ptr: *const u8, params_len: i32, out_ptr_ptr: *mut i32, out_len_ptr: *mut i32) -> i32;
}
#[no_mangle]
pub extern "C" fn wasmcore_alloc(len: i32) -> i32 {
    let size = if len < 0 { 1usize } else { (len as usize).max(1) };
    let layout = std::alloc::Layout::from_size_align(size, 1).unwrap();
    unsafe { std::alloc::alloc(layout) as i32 }
}
fn make_pattern(byte_len: usize, alphabet: &[u8]) -> Vec<u8> {
    (0..byte_len).map(|i| alphabet[i % alphabet.len()]).collect()
}
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |acc, b| acc.wrapping_add(*b as u64))
}
fn main() {
    // PARAMS_MB / RESULT_MB substituted at build time below.
    let big = make_pattern(__PARAMS_BYTES__, b"0123456789");
    let params = format!("{{\"payload\":\"{}\"}}", unsafe { std::str::from_utf8_unchecked(&big) });

    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(params.as_ptr(), params.len() as i32, params.as_ptr(), params.len() as i32, &mut out_ptr, &mut out_len)
    };
    let result_bytes = unsafe { std::slice::from_raw_parts(out_ptr as *const u8, out_len as usize) };
    println!(
        "status={status} params_len={} out_len={out_len} result_checksum={}",
        params.len(),
        checksum(result_bytes)
    );
}
"###;

#[test]
fn several_mb_payloads_round_trip_exactly_in_both_directions() {
    let params_bytes = PARAMS_MB * 1024 * 1024;
    let guest_src = GUEST_LARGE_ROUNDTRIP.replace("__PARAMS_BYTES__", &params_bytes.to_string());
    let wasm = compile_guest("large_roundtrip", &guest_src);
    let host = WasmCoreHost::new();

    // Independently expected params length/checksum: what the JSON
    // envelope `{"payload":"<pattern>"}` actually is, computed here (not
    // trusted from the guest's own self-report).
    let pattern = make_pattern(params_bytes, b"0123456789");
    let expected_params_json = format!(
        "{{\"payload\":\"{}\"}}",
        std::str::from_utf8(&pattern).unwrap()
    );
    let expected_params_len = expected_params_json.len();

    // What the bridge is about to return: an independently-computed
    // checksum for the exact large result payload it will produce.
    let expected_result_bytes = make_pattern(RESULT_MB * 1024 * 1024, b"abcdefghij");
    let expected_result_checksum = checksum(&expected_result_bytes);
    let expected_result_len = expected_result_bytes.len();

    let received_len = Arc::new(Mutex::new(None::<usize>));
    let received_checksum = Arc::new(Mutex::new(None::<u64>));
    let (received_len2, received_checksum2) = (received_len.clone(), received_checksum.clone());
    let result_bytes_for_bridge = expected_result_bytes.clone();

    let bridge: WasmFleetBridgeFn = Arc::new(move |_op_id: &str, params_json: &str| {
        // Prove the HOST received the guest's full, untruncated large
        // params (not just what the guest itself claims to have sent).
        *received_len2.lock().unwrap() = Some(params_json.len());
        *received_checksum2.lock().unwrap() = Some(checksum(params_json.as_bytes()));
        Ok(String::from_utf8(result_bytes_for_bridge.clone()).unwrap())
    });

    let run = host
        .run_module(&wasm, Some(bridge))
        .expect("a several-MB round trip must complete, not trap/panic/overflow");

    assert_eq!(
        received_len.lock().unwrap().expect("bridge must have run"),
        expected_params_len,
        "host must receive the guest's full params_json length, no truncation"
    );
    assert_eq!(
        received_checksum
            .lock()
            .unwrap()
            .expect("bridge must have run"),
        checksum(expected_params_json.as_bytes()),
        "host must receive the guest's exact params_json bytes, no corruption"
    );

    let stdout = run.stdout;
    assert!(
        stdout.contains("status=0"),
        "expected a successful large round trip:\n{stdout}"
    );
    assert!(
        stdout.contains(&format!("params_len={expected_params_len}")),
        "guest's own params length must match what was actually sent:\n{stdout}"
    );
    assert!(
        stdout.contains(&format!("out_len={expected_result_len}")),
        "guest must read back the FULL result length the host wrote, no truncation:\n{stdout}"
    );
    assert!(
        stdout.contains(&format!("result_checksum={expected_result_checksum}")),
        "guest's checksum of the bytes it read back must match the bridge's exact bytes -- \
         a mismatch here would mean truncation or corruption on the host->guest leg:\n{stdout}"
    );
}

// ---------------------------------------------------------------------
// Scenario 4: many sequential fleet_call invocations from ONE guest run
// -- not just the existing tests' one-call-then-exit pattern. Hunts for
// state leaks / use-after-free-shaped bugs across calls within a single
// execution (e.g. a stale `Memory`/pointer reused across calls, or a
// bridge invocation seeing residual state from a previous call).
// ---------------------------------------------------------------------

const SEQUENTIAL_CALL_COUNT: usize = 200;

const GUEST_SEQUENTIAL_CALLS: &str = r###"
#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(op_ptr: *const u8, op_len: i32, params_ptr: *const u8, params_len: i32, out_ptr_ptr: *mut i32, out_len_ptr: *mut i32) -> i32;
}
#[no_mangle]
pub extern "C" fn wasmcore_alloc(len: i32) -> i32 {
    let size = if len < 0 { 1usize } else { (len as usize).max(1) };
    let layout = std::alloc::Layout::from_size_align(size, 1).unwrap();
    unsafe { std::alloc::alloc(layout) as i32 }
}
fn call_fleet(op_id: &str, params_json: &str) -> (i32, String) {
    // Deliberately reuses the SAME two out-parameter locals across every
    // call in the loop below, like a realistic long-lived guest would --
    // stresses whether the host leaves any stale state behind between
    // calls rather than freshly overwriting them every time.
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(op_id.as_ptr(), op_id.len() as i32, params_json.as_ptr(), params_json.len() as i32, &mut out_ptr as *mut i32, &mut out_len as *mut i32)
    };
    let payload = unsafe {
        let slice = std::slice::from_raw_parts(out_ptr as *const u8, out_len as usize);
        String::from_utf8_lossy(slice).into_owned()
    };
    (status, payload)
}
fn main() {
    for i in 0..__CALL_COUNT__ {
        let op = format!("wasmcore.seq.{i}");
        let params = format!("{{\"i\":{i}}}");
        let (status, payload) = call_fleet(&op, &params);
        println!("[{i}] status={status} payload={payload}");
    }
}
"###;

#[test]
fn two_hundred_sequential_calls_from_one_guest_run_show_no_state_leak_or_reordering() {
    let guest_src =
        GUEST_SEQUENTIAL_CALLS.replace("__CALL_COUNT__", &SEQUENTIAL_CALL_COUNT.to_string());
    let wasm = compile_guest("sequential_calls", &guest_src);
    let host = WasmCoreHost::new();

    let seen = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let seen2 = seen.clone();
    let bridge: WasmFleetBridgeFn = Arc::new(move |op_id: &str, params_json: &str| {
        seen2
            .lock()
            .unwrap()
            .push((op_id.to_owned(), params_json.to_owned()));
        Ok(format!(
            "{{\"echo_op\":\"{op_id}\",\"echo_params\":{params_json}}}"
        ))
    });

    let run = host
        .run_module(&wasm, Some(bridge))
        .expect("200 sequential calls in one guest run must all complete");

    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(
        lines.len(),
        SEQUENTIAL_CALL_COUNT,
        "expected exactly one stdout line per call, no dropped/duplicated calls"
    );

    let captured = seen.lock().unwrap();
    assert_eq!(
        captured.len(),
        SEQUENTIAL_CALL_COUNT,
        "the bridge must see exactly one invocation per guest call, in order"
    );

    for i in 0..SEQUENTIAL_CALL_COUNT {
        // Bridge-side: exact op_id/params_json for call i, in the right
        // order -- proves no cross-call leakage/reordering on the way in.
        assert_eq!(captured[i].0, format!("wasmcore.seq.{i}"));
        assert_eq!(captured[i].1, format!("{{\"i\":{i}}}"));

        // Guest-stdout-side: exact echoed payload for call i -- proves no
        // cross-call leakage on the way back (e.g. a stale/reused output
        // pointer would show call i's stdout containing call i-1's data).
        let expected_line = format!(
            "[{i}] status=0 payload={{\"echo_op\":\"wasmcore.seq.{i}\",\"echo_params\":{{\"i\":{i}}}}}"
        );
        assert_eq!(
            lines[i], expected_line,
            "call {i}'s echoed result must be exactly its own, not a neighbor's"
        );
    }
}
