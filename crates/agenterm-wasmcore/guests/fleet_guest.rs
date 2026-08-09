// Real wasm32-wasip1 WASI guest program, used by
// `agenterm-wasmcore`'s own integration test
// (`tests/fleet_call_roundtrip.rs`) to prove the `fleet_call` marshalling
// convention end to end -- no mocked bridge, no fake wasm bytes. Compiled
// at test time via:
//
//   rustc --target wasm32-wasip1 --edition 2021 -O guests/fleet_guest.rs -o <tmp>/fleet_guest.wasm
//
// Deliberately built by invoking `rustc` directly rather than being a
// second member of this crate's own Cargo compilation unit -- this is a
// *guest* binary for a *different* target triple, matching this session's
// own precedent of building verification programs out-of-band
// (`agenterm-guestcore`'s `examples/*.rs` are `cargo run --example`, a
// distinct mechanism again, but the same underlying principle: a
// verification artifact is not part of the crate's own build graph).
// `--edition 2021` is a deliberate, simple choice for this single-file
// program -- it predates edition 2024's `unsafe extern` block requirement,
// so this file can use a plain `extern "C" { .. }` block below without
// that ceremony.
//
// ABI this file implements (see ../README.md "fleet_call calling
// convention" for the full spec a non-Rust guest author would need):
//
//   import "agenterm" "fleet_call"
//     (op_ptr: i32, op_len: i32, params_ptr: i32, params_len: i32,
//      out_ptr_ptr: i32, out_len_ptr: i32) -> i32   (0=Ok, 1=Err, 2=NoBridge)
//   export "wasmcore_alloc" (len: i32) -> i32
//   export "memory" (the wasm32-wasip1 default -- nothing extra to do)

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

/// Host-callable allocator: give the host `len` bytes of this guest's own
/// linear memory to write a `fleet_call` result into. Never freed here --
/// this is a short-lived, one-shot verification program that exits right
/// after using the result, so leaking is the honest simplest choice, not
/// an oversight (a longer-lived guest would pair this with a real
/// `wasmcore_free`).
#[no_mangle]
pub extern "C" fn wasmcore_alloc(len: i32) -> i32 {
    let size = if len < 0 { 1usize } else { (len as usize).max(1) };
    let layout = Layout::from_size_align(size, 1).expect("valid 1-byte-aligned layout");
    let ptr = unsafe { alloc(layout) };
    ptr as i32
}

/// Guest-side half of the calling convention: pass both strings as
/// `(ptr, len)` into this guest's own memory, pass the addresses of two
/// local `i32`s as the out-parameters the host fills in, then read the
/// `(ptr, len)` pair back out of them once the import call returns.
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
    // Real, non-ASCII content -- not just an empty-string smoke test.
    // Exercises: guest -> host string marshalling (the host's bridge
    // closure must receive this exact JSON back), and host -> guest
    // marshalling of the *reply* (this guest must print it back
    // correctly after crossing the boundary a second time).
    let (status, payload) = call_fleet(
        "wasmcore.echo",
        r#"{"greeting":"héllo wörld 🎉","n":42}"#,
    );
    println!("ECHO status={status} payload={payload}");

    // A second, deliberately-unrecognized operation id -- the bridge
    // returns `Err` for this in the host test, proving the error path
    // (not just the happy path) round-trips correctly too.
    let (status2, payload2) = call_fleet("wasmcore.unknown_op", "{}");
    println!("UNKNOWN status={status2} payload={payload2}");

    // Explicit, deliberate nonzero exit -- exercises the real
    // `wasmtime_wasi::I32Exit` lifecycle path (see lib.rs
    // `GuestExit::Exited`), distinct from "returned normally".
    std::process::exit(7);
}
