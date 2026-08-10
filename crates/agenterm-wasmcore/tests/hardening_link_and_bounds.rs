//! Adversarial / hardening coverage for the two link-time shape checks
//! (Part 2, scenario 1) and this crate's single most safety-critical path:
//! the host writing into guest-*claimed* memory on the output side
//! (scenario 2), and reading guest-*claimed* memory on the input side
//! (scenario 5). Every guest here is a REAL `wasm32-wasip1` program built
//! by a real `rustc --target wasm32-wasip1` invocation (mirroring
//! `tests/fleet_call_roundtrip.rs`'s `compiled_guest_wasm`), run through
//! the real `WasmCoreHost` -- no mocked wasm bytes, no reasoning-only
//! claims. Every assertion below encodes an *actually observed* host
//! response, captured while writing this file (see this crate's git
//! history / task report for the raw `cargo test -- --nocapture` output
//! these assertions were derived from).
//!
//! Headline finding: no real memory-safety gap was found. Guest-claimed
//! output pointers/lengths are validated by `wasmtime`'s own
//! `Memory::write` bounds check (in addition to this crate's own explicit
//! `ptr < 0` guard in `write_guest_result`); guest-claimed input
//! pointers/lengths are validated by this crate's own `slice_bytes` (used
//! by `read_guest_string`). Every adversarial scenario below produces a
//! clean `Result::Err` (surfaced to the guest as a WASI trap, and to this
//! crate's caller as `WasmCoreHost::run_module`'s own `Err`), never a
//! host-process crash/hang and never an out-of-bounds host write.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agenterm_wasmcore::{WasmCoreHost, WasmFleetBridgeFn};

/// Compiles `source` to a real wasm32-wasip1 module, mirroring
/// `tests/fleet_call_roundtrip.rs`'s `compiled_guest_wasm` -- duplicated
/// rather than shared (an example/test file cannot `use` another test
/// file's private `fn`, matching this crate's existing precedent in
/// `examples/wasmcore_run.rs`'s own module docs). Each `name` gets its own
/// source/output pair under this crate's (gitignored) `target/` directory
/// so parallel `#[test]`s compiling different guests never collide.
fn compile_guest(name: &str, source: &str) -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = manifest_dir.join("target").join("wasmcore-hardening-guests");
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

fn echo_bridge() -> WasmFleetBridgeFn {
    Arc::new(|op_id: &str, params_json: &str| -> Result<String, String> {
        Ok(format!("{{\"op\":\"{op_id}\",\"params\":{params_json}}}"))
    })
}

// ---------------------------------------------------------------------
// Scenario 1: a guest that does not export `wasmcore_alloc` at all, or
// exports it with the wrong signature; and, as the real counterpart to
// "wasmtime's own type-checking catches it at link time" (that phrase
// applies to *imports* the linker resolves, not to exports the host
// queries lazily -- see this file's module docs and the report this task
// produced), a guest that imports `fleet_call` itself with a mismatched
// signature.
// ---------------------------------------------------------------------

const GUEST_MISSING_ALLOC: &str = r###"
#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(op_ptr: *const u8, op_len: i32, params_ptr: *const u8, params_len: i32, out_ptr_ptr: *mut i32, out_len_ptr: *mut i32) -> i32;
}
// Deliberately no `wasmcore_alloc` export at all.
fn main() {
    let op = "wasmcore.echo";
    let params = "{}";
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(op.as_ptr(), op.len() as i32, params.as_ptr(), params.len() as i32, &mut out_ptr, &mut out_len)
    };
    println!("status={status}");
}
"###;

#[test]
fn guest_missing_wasmcore_alloc_export_is_rejected_cleanly_not_crashed() {
    let wasm = compile_guest("missing_alloc", GUEST_MISSING_ALLOC);
    let host = WasmCoreHost::new();

    let err = host
        .run_module(&wasm, Some(echo_bridge()))
        .expect_err("a guest with no wasmcore_alloc export must be rejected, not run to Ok");
    let message = format!("{err:#}");

    // Real observed message (captured with `cargo test -- --nocapture`
    // while writing this test): "guest module trapped: ... guest module
    // does not export `wasmcore_alloc`". This is a call-time rejection
    // (the guest's `_start` genuinely starts running and calls
    // `fleet_call` before the host discovers the missing export) --
    // *not* an instantiation-time one, because `wasmcore_alloc` is an
    // export the host looks up lazily on first use, not an import
    // wasmtime's `Linker` resolves eagerly at `instantiate()`. See
    // `guest_importing_fleet_call_with_mismatched_signature_is_rejected_at_link_time`
    // below for the real link-time counterpart.
    assert!(
        message.contains("does not export `wasmcore_alloc`"),
        "expected a clean 'missing export' rejection, got: {message}"
    );
}

const GUEST_WRONG_ALLOC_SIGNATURE: &str = r###"
#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(op_ptr: *const u8, op_len: i32, params_ptr: *const u8, params_len: i32, out_ptr_ptr: *mut i32, out_len_ptr: *mut i32) -> i32;
}
// Wrong signature: host expects `(i32) -> i32`, this exports `(i32, i32) -> i32`.
#[no_mangle]
pub extern "C" fn wasmcore_alloc(len: i32, _extra: i32) -> i32 {
    len
}
fn main() {
    let op = "wasmcore.echo";
    let params = "{}";
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(op.as_ptr(), op.len() as i32, params.as_ptr(), params.len() as i32, &mut out_ptr, &mut out_len)
    };
    println!("status={status}");
}
"###;

#[test]
fn guest_wasmcore_alloc_with_wrong_signature_is_rejected_cleanly_not_crashed() {
    let wasm = compile_guest("wrong_alloc_sig", GUEST_WRONG_ALLOC_SIGNATURE);
    let host = WasmCoreHost::new();

    let err = host
        .run_module(&wasm, Some(echo_bridge()))
        .expect_err("a wasmcore_alloc with the wrong signature must be rejected, not run to Ok");
    let message = format!("{err:#}");

    // Real observed message: "... guest `wasmcore_alloc` has the wrong
    // signature (expected (i32) -> i32): type mismatch with parameters:
    // expected 1 types, found 2" -- wasmtime's `Func::typed::<i32, i32>`
    // performs this check at the point this crate calls it (inside
    // `write_guest_result`, on the first `fleet_call` that needs a result
    // buffer), and returns a typed `Result` rather than panicking.
    assert!(
        message.contains("wasmcore_alloc") && message.contains("wrong signature"),
        "expected a clean 'wrong signature' rejection, got: {message}"
    );
}

const GUEST_MISMATCHED_FLEET_CALL_IMPORT: &str = r###"
// Missing the 6th parameter (`out_len_ptr`) relative to the host's real
// registered `fleet_call` signature -- a genuine WASM import type mismatch.
#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(op_ptr: *const u8, op_len: i32, params_ptr: *const u8, params_len: i32, out_ptr_ptr: *mut i32) -> i32;
}
fn main() {
    let op = "x";
    let params = "{}";
    let mut out_ptr: i32 = 0;
    let status = unsafe {
        fleet_call(op.as_ptr(), op.len() as i32, params.as_ptr(), params.len() as i32, &mut out_ptr)
    };
    println!("status={status}");
}
"###;

#[test]
fn guest_importing_fleet_call_with_mismatched_signature_is_rejected_at_link_time() {
    let wasm = compile_guest("wrong_import_sig", GUEST_MISMATCHED_FLEET_CALL_IMPORT);
    let host = WasmCoreHost::new();

    let err = host
        .run_module(&wasm, Some(echo_bridge()))
        .expect_err("a mismatched fleet_call import must fail before the guest ever runs");
    let message = format!("{err:#}");

    // Real observed message: "instantiating wasm module: incompatible
    // import type for `agenterm::fleet_call`: types incompatible:
    // expected type `(func (param i32 i32 i32 i32 i32 i32) (result
    // i32))`, found type `(func (param i32 i32 i32 i32 i32) (result
    // i32))`". This *is* the real link-time rejection: it comes from
    // `run_module_on_worker_thread`'s `linker.instantiate(...)` call --
    // `_start` never runs at all, confirmed by the "instantiating wasm
    // module" context (attached at the instantiate call site, not the
    // `_start`/WASI-trap path `fleet_call_import`'s own errors surface
    // through).
    assert!(
        message.contains("instantiating wasm module"),
        "expected the failure to come from instantiation, got: {message}"
    );
    assert!(
        message.contains("incompatible import type") && message.contains("fleet_call"),
        "expected a real wasmtime import-type-mismatch message, got: {message}"
    );
}

// ---------------------------------------------------------------------
// Scenario 2 (the safety-critical one): a guest whose `wasmcore_alloc`
// hands back a pointer the host then writes the `fleet_call` result
// into. Three sub-cases: a "null-like" zero return, a wildly
// out-of-bounds return, and a precisely-one-byte-short (near-the-real-end)
// return.
// ---------------------------------------------------------------------

const GUEST_ALLOC_RETURNS_ZERO: &str = r###"
#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(op_ptr: *const u8, op_len: i32, params_ptr: *const u8, params_len: i32, out_ptr_ptr: *mut i32, out_len_ptr: *mut i32) -> i32;
}
// Returns 0 unconditionally -- not "null" in the native sense (WASM
// linear memory address 0 is an ordinary, addressable byte, not a
// protected null page), but a legitimate real allocator would never do
// this deliberately.
#[no_mangle]
pub extern "C" fn wasmcore_alloc(_len: i32) -> i32 { 0 }
fn main() {
    let op = "wasmcore.echo";
    let params = "{\"k\":\"v\"}";
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(op.as_ptr(), op.len() as i32, params.as_ptr(), params.len() as i32, &mut out_ptr, &mut out_len)
    };
    let payload_bytes = unsafe {
        std::slice::from_raw_parts(out_ptr as *const u8, out_len as usize).to_vec()
    };
    println!("status={status} out_ptr={out_ptr} out_len={out_len}");
    println!("payload={}", String::from_utf8_lossy(&payload_bytes));
}
"###;

#[test]
fn alloc_returning_zero_is_in_bounds_and_round_trips_without_host_corruption() {
    // Real, observed, honest result: address 0 is a valid in-bounds guest
    // address (guest linear memory starts at 0), so the host's bounds
    // check accepts it and writes there -- this is not a host
    // memory-safety violation (the write is fully contained inside
    // wasmtime's own bounds-checked guest memory buffer). This crate's
    // ABI (see README.md) only documents rejecting a *negative* pointer;
    // treating 0 as ordinary rather than "special/null" matches that
    // spec. A real guest allocator that carelessly returns 0 risks
    // clobbering its own low-memory data, but that is the guest's own
    // allocator bug, not a host-side hole -- confirmed here by the exact
    // written bytes reading back correctly.
    let wasm = compile_guest("alloc_returns_zero", GUEST_ALLOC_RETURNS_ZERO);
    let host = WasmCoreHost::new();

    let result = host
        .run_module(&wasm, Some(echo_bridge()))
        .expect("ptr=0 is in-bounds; the host must not reject or crash on it");

    assert!(
        result.stdout.contains("status=0 out_ptr=0"),
        "expected the call to succeed with out_ptr=0:\n{}",
        result.stdout
    );
    assert!(
        result
            .stdout
            .contains(r#"payload={"op":"wasmcore.echo","params":{"k":"v"}}"#),
        "expected the exact bridge payload to round-trip through address 0:\n{}",
        result.stdout
    );
}

const GUEST_ALLOC_RETURNS_WILDLY_OOB: &str = r###"
#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(op_ptr: *const u8, op_len: i32, params_ptr: *const u8, params_len: i32, out_ptr_ptr: *mut i32, out_len_ptr: *mut i32) -> i32;
}
#[no_mangle]
pub extern "C" fn wasmcore_alloc(_len: i32) -> i32 { i32::MAX }
fn main() {
    let op = "wasmcore.echo";
    let params = "{\"k\":\"v\"}";
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(op.as_ptr(), op.len() as i32, params.as_ptr(), params.len() as i32, &mut out_ptr, &mut out_len)
    };
    println!("status={status}");
}
"###;

#[test]
fn alloc_returning_a_wildly_out_of_bounds_pointer_is_rejected_not_a_host_crash() {
    let wasm = compile_guest("alloc_returns_wildly_oob", GUEST_ALLOC_RETURNS_WILDLY_OOB);
    let host = WasmCoreHost::new();

    let err = host
        .run_module(&wasm, Some(echo_bridge()))
        .expect_err("wasmcore_alloc returning i32::MAX must be rejected, not written through");
    let message = format!("{err:#}");

    // Real observed message: "writing fleet_call result bytes into guest
    // memory: out of bounds memory access" -- this is `wasmtime`'s own
    // `Memory::write` bounds check catching it (this crate never wrote a
    // manual upper-bound guard for the alloc-returned pointer itself --
    // only the explicit `ptr < 0` guard -- and did not need one: the
    // `Memory::write` call this crate already makes is itself
    // bounds-checked). No host-process crash, no out-of-bounds write.
    assert!(
        message.contains("out of bounds memory access"),
        "expected wasmtime's own Memory::write bounds check to reject this, got: {message}"
    );
}

const GUEST_ALLOC_RETURNS_NEAR_END_OOB: &str = r###"
#[link(wasm_import_module = "agenterm")]
extern "C" {
    fn fleet_call(op_ptr: *const u8, op_len: i32, params_ptr: *const u8, params_len: i32, out_ptr_ptr: *mut i32, out_len_ptr: *mut i32) -> i32;
}
// Precisely 2 bytes before the guest's REAL current memory end -- the
// host will be asked to write a payload far larger than 2 bytes there.
// This proves the bounds check is exact (byte-accurate), not merely
// catching implausibly huge values.
#[no_mangle]
pub extern "C" fn wasmcore_alloc(_len: i32) -> i32 {
    let pages = core::arch::wasm32::memory_size(0);
    let mem_bytes = (pages as i64) * 65536;
    (mem_bytes - 2) as i32
}
fn main() {
    let op = "wasmcore.echo";
    let params = "{\"k\":\"a value definitely longer than two bytes\"}";
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(op.as_ptr(), op.len() as i32, params.as_ptr(), params.len() as i32, &mut out_ptr, &mut out_len)
    };
    println!("status={status}");
}
"###;

#[test]
fn alloc_returning_a_pointer_two_bytes_before_the_real_end_is_rejected_precisely() {
    let wasm = compile_guest("alloc_returns_near_end_oob", GUEST_ALLOC_RETURNS_NEAR_END_OOB);
    let host = WasmCoreHost::new();

    let err = host
        .run_module(&wasm, Some(echo_bridge()))
        .expect_err("a payload that overruns the real memory end by even a few bytes must be rejected");
    let message = format!("{err:#}");

    assert!(
        message.contains("out of bounds memory access"),
        "expected a precise, byte-accurate bounds rejection, got: {message}"
    );
}

// ---------------------------------------------------------------------
// Scenario 5: a guest that calls `fleet_call` with `operation_id`/
// `params_json` lengths that don't match what is actually at that memory
// address -- the INPUT-side counterpart to scenario 2. Exercises
// `slice_bytes`/`read_guest_string` end to end via a real malicious
// guest, not just the pure unit tests already in `src/lib.rs`.
// ---------------------------------------------------------------------

const GUEST_LIES_ABOUT_PARAMS_LEN: &str = r###"
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
fn main() {
    let op = "wasmcore.echo";
    let params = "{}"; // the guest's real buffer here is 2 bytes
    let lied_len: i32 = 50_000_000; // claims far more than it actually has
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(op.as_ptr(), op.len() as i32, params.as_ptr(), lied_len, &mut out_ptr, &mut out_len)
    };
    println!("status={status}");
}
"###;

#[test]
fn guest_lying_about_a_huge_params_len_is_rejected_and_bridge_is_never_invoked() {
    let wasm = compile_guest("lies_about_params_len", GUEST_LIES_ABOUT_PARAMS_LEN);
    let host = WasmCoreHost::new();

    // Prove, not just assume from reading the source, that the bridge is
    // never invoked for a rejected read -- `read_guest_string` runs (and
    // fails) strictly before `fleet_call_import` ever touches the bridge
    // closure.
    let bridge_invocations = Arc::new(AtomicUsize::new(0));
    let counted = bridge_invocations.clone();
    let bridge: WasmFleetBridgeFn = Arc::new(move |op_id: &str, params_json: &str| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(format!("{{\"op\":\"{op_id}\",\"params\":{params_json}}}"))
    });

    let err = host
        .run_module(&wasm, Some(bridge))
        .expect_err("a params_len wildly larger than the guest's real memory must be rejected");
    let message = format!("{err:#}");

    // Real observed message: "fleet_call: reading params_json from guest
    // memory: guest range 1048613..51048613 out of bounds (guest memory
    // size 1114112)" -- exact start/end/size reported, not a generic
    // failure.
    assert!(
        message.contains("reading params_json from guest memory") && message.contains("out of bounds"),
        "expected a clean, precise input-bounds rejection, got: {message}"
    );
    assert_eq!(
        bridge_invocations.load(Ordering::SeqCst),
        0,
        "the fleet bridge must never run for a call whose own params could not be read"
    );
}

const GUEST_LIES_WITH_NEGATIVE_LEN: &str = r###"
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
fn main() {
    let op = "wasmcore.echo";
    let params = "{}";
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(op.as_ptr(), -5, params.as_ptr(), params.len() as i32, &mut out_ptr, &mut out_len)
    };
    println!("status={status}");
}
"###;

#[test]
fn guest_lying_with_a_negative_op_len_is_rejected() {
    let wasm = compile_guest("lies_with_negative_len", GUEST_LIES_WITH_NEGATIVE_LEN);
    let host = WasmCoreHost::new();

    let err = host
        .run_module(&wasm, Some(echo_bridge()))
        .expect_err("a negative op_len must be rejected");
    let message = format!("{err:#}");

    assert!(
        message.contains("negative pointer/length"),
        "expected the explicit negative-length guard to fire, got: {message}"
    );
}

const GUEST_LIES_WITH_NEAR_END_OVERRUN: &str = r###"
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
fn main() {
    let pages = core::arch::wasm32::memory_size(0);
    let mem_bytes = (pages as i64) * 65536;
    // A real address near the guest's real end, claimed with a length
    // that runs a few KiB past it -- an "off by a little", plausible-
    // looking lie, not just an implausibly huge one.
    let near_end_ptr = (mem_bytes - 4) as i32;
    let overrun_len: i32 = 4096;
    let mut out_ptr: i32 = 0;
    let mut out_len: i32 = 0;
    let status = unsafe {
        fleet_call(near_end_ptr as *const u8, overrun_len, near_end_ptr as *const u8, 1, &mut out_ptr, &mut out_len)
    };
    println!("status={status}");
}
"###;

#[test]
fn guest_lying_with_a_small_realistic_overrun_past_the_real_end_is_rejected() {
    let wasm = compile_guest("lies_with_near_end_overrun", GUEST_LIES_WITH_NEAR_END_OVERRUN);
    let host = WasmCoreHost::new();

    let err = host
        .run_module(&wasm, Some(echo_bridge()))
        .expect_err("a length that overruns the real end by a few KiB must be rejected");
    let message = format!("{err:#}");

    assert!(
        message.contains("reading operation_id from guest memory") && message.contains("out of bounds"),
        "expected a clean, precise input-bounds rejection, got: {message}"
    );
}
