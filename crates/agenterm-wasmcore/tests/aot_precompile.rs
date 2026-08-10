//! Real, end-to-end verification of the AOT precompilation path this
//! decisive experiment adds: `Engine::precompile_module` (via
//! `WasmCoreHost::precompile_module`) producing a `.cwasm`, then
//! `Module::deserialize_file` (via `WasmCoreHost::run_precompiled_module`)
//! loading it back and running it through the exact same instantiate/run
//! machinery `run_module` (JIT) uses. See `README.md`'s "AOT
//! precompilation" section for the measured numbers and the honest
//! verdict this file's passing tests back up.
//!
//! Real `rustc --target wasm32-wasip1` compile, real `wasmtime` engine
//! calls, real bytes on disk -- no mocked wasm, no mocked compatibility
//! check.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use agenterm_wasmcore::{GuestExit, WasmCoreHost};

/// Same guest program `tests/fleet_call_roundtrip.rs` uses -- this crate's
/// own canonical ABI-reference guest (real `println!`/`format!`/std
/// allocator code, a real `fleet_call` round trip and an explicit
/// `exit(7)`), reused here rather than duplicated so the AOT-vs-JIT
/// comparison is against the exact same bytes on both paths.
fn compiled_guest_wasm() -> &'static Path {
    static WASM_PATH: OnceLock<PathBuf> = OnceLock::new();
    WASM_PATH.get_or_init(|| {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let guest_src = manifest_dir.join("guests").join("fleet_guest.rs");
        let out_dir = manifest_dir.join("target").join("wasmcore-aot-test-guests");
        std::fs::create_dir_all(&out_dir)
            .unwrap_or_else(|e| panic!("create {}: {e}", out_dir.display()));
        let out_wasm = out_dir.join("fleet_guest.wasm");

        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
        let status = Command::new(&rustc)
            .args(["--target", "wasm32-wasip1", "--edition", "2021", "-O"])
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
        out_wasm
    })
}

fn echo_and_reject_bridge() -> agenterm_wasmcore::WasmFleetBridgeFn {
    std::sync::Arc::new(|op_id: &str, params_json: &str| -> Result<String, String> {
        match op_id {
            "wasmcore.echo" => Ok(format!(
                "{{\"echoed_op\":\"{op_id}\",\"echoed_params\":{params_json}}}"
            )),
            other => Err(format!("unknown_op: {other}")),
        }
    })
}

/// Core proof: precompile the real guest to a real `.cwasm` on disk via
/// `WasmCoreHost::precompile_module`, load it back via the real
/// (`unsafe`) `WasmCoreHost::run_precompiled_module`, and confirm the
/// guest-observable output is byte-for-byte identical to the JIT
/// (`run_module`) path -- same stdout, same unicode payload, same bridge
/// echo, same explicit `exit(7)`. This is what proves AOT-then-load is a
/// real, working alternate path through the exact same host/bridge code,
/// not an untested, separate mechanism.
#[test]
fn aot_precompiled_module_produces_identical_output_to_the_jit_path() {
    let wasm_path = compiled_guest_wasm();
    let host = WasmCoreHost::new();

    // --- JIT path: the existing, already-hardened `run_module`. ---
    let jit_result = host
        .run_module(wasm_path, Some(echo_and_reject_bridge()))
        .expect("JIT guest run should complete");

    // --- AOT path: precompile to a real .cwasm file, then load it back. ---
    let cwasm_bytes = host
        .precompile_module(wasm_path)
        .expect("precompiling the guest module should succeed on this host");
    assert!(
        !cwasm_bytes.is_empty(),
        "a real precompiled artifact must not be empty"
    );
    assert_eq!(
        wasmtime::Engine::detect_precompiled(&cwasm_bytes),
        Some(wasmtime::Precompiled::Module),
        "precompile_module's output must be recognized by wasmtime's own \
         detector as a real precompiled module artifact, not plain wasm bytes"
    );

    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("wasmcore-aot-test-guests");
    let cwasm_path = out_dir.join("fleet_guest.cwasm");
    std::fs::write(&cwasm_path, &cwasm_bytes)
        .unwrap_or_else(|e| panic!("write {}: {e}", cwasm_path.display()));

    // SAFETY: `cwasm_path` was just written, unmodified, from this same
    // process's own `precompile_module` call above -- exactly the
    // trusted-input contract `run_precompiled_module` documents.
    let aot_result = unsafe { host.run_precompiled_module(&cwasm_path, Some(echo_and_reject_bridge())) }
        .expect("AOT guest run (deserialize_file + run) should complete");

    assert_eq!(
        jit_result.exit, aot_result.exit,
        "JIT and AOT loading of the same guest bytes must observe the same exit"
    );
    assert_eq!(
        jit_result.exit,
        GuestExit::Exited(7),
        "sanity: this guest always calls std::process::exit(7)"
    );
    assert_eq!(
        jit_result.stdout, aot_result.stdout,
        "JIT and AOT loading of the same guest bytes must produce byte-identical \
         stdout -- same fleet_call round trip, same real result"
    );
    assert!(
        aot_result.stdout.contains("héllo wörld 🎉"),
        "AOT path must round-trip real unicode through fleet_call exactly like JIT:\n{}",
        aot_result.stdout
    );
    assert!(
        aot_result.stdout.contains(r#""echoed_op":"wasmcore.echo""#),
        "AOT path must reach the real fleet bridge, not skip it:\n{}",
        aot_result.stdout
    );
}

/// Empirical (not merely cited) verification of the non-portability claim:
/// a `.cwasm` precompiled under one `wasmtime::Config` is rejected when
/// loaded into an `Engine` built from an incompatibly different `Config`.
/// This box is single-ISA (x86_64) and single-OS (Windows), so this test
/// cannot and does not exercise the architecture/OS mismatch branch
/// directly -- see `README.md` for exactly what that means and what is
/// cited from wasmtime's own source instead of tested here. What this
/// DOES really test: the exact same `Module::deserialize`/`check_compatible`
/// gate wasmtime uses for the architecture/OS check (per
/// `wasmtime-47.0.3/src/engine/serialization.rs`) also runs a
/// `Config`-derived-settings check first-class, and really rejects a real
/// mismatch with a real, specific error -- proving the compatibility gate
/// is live and enforced by the public API, not merely documented.
#[test]
fn aot_precompiled_module_is_rejected_by_an_engine_with_incompatible_config() {
    let wasm_path = compiled_guest_wasm();
    let wasm_bytes = std::fs::read(wasm_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", wasm_path.display()));

    let mut config_a = wasmtime::Config::new();
    config_a.epoch_interruption(false);
    let engine_a =
        wasmtime::Engine::new(&config_a).expect("building an engine with epoch_interruption(false)");
    let cwasm_bytes = engine_a
        .precompile_module(&wasm_bytes)
        .expect("precompiling with engine_a should succeed");

    let mut config_b = wasmtime::Config::new();
    config_b.epoch_interruption(true);
    let engine_b =
        wasmtime::Engine::new(&config_b).expect("building an engine with epoch_interruption(true)");

    // SAFETY: `cwasm_bytes` is the real, unmodified, in-memory output of
    // `engine_a.precompile_module` above -- a trusted, same-process input.
    // This is expected to be REJECTED, not accepted -- that rejection is
    // exactly the behavior under test.
    let result = unsafe { wasmtime::Module::deserialize(&engine_b, &cwasm_bytes) };
    let err = result.expect_err(
        "a .cwasm precompiled for one engine Config must be rejected when loaded into an \
         Engine built from an incompatible Config -- this is the real, live gate that also \
         guards architecture/OS mismatches (same check_compatible code path)",
    );
    let message = err.to_string();
    assert!(
        message.contains("epoch interruption"),
        "expected a real epoch-interruption compatibility rejection, got: {message}"
    );
}

/// Literal-byte inspection of what `precompile_module`'s output actually
/// encodes: confirm the host's own target triple (architecture and
/// operating system) is really embedded, readable, as raw UTF-8 bytes in
/// the serialized artifact -- not merely asserted from wasmtime's docs.
/// (`wasmtime-47.0.3`'s `engine/serialization.rs` `Metadata::new` stores
/// `compiler.triple().to_string()` and `postcard`-encodes it as a plain
/// length-prefixed UTF-8 string, so the ASCII text survives verbatim in
/// the file -- verified here by literal substring search, not assumed.)
#[test]
fn aot_cwasm_bytes_literally_embed_the_host_target_triple() {
    let wasm_path = compiled_guest_wasm();
    let host = WasmCoreHost::new();
    let cwasm_bytes = host
        .precompile_module(wasm_path)
        .expect("precompiling the guest module should succeed on this host");

    // This crate only runs on Windows per its own hardening-test/AGENTS.md
    // discipline (`WORKER_STACK_BYTES`'s doc comment cites a Windows-only
    // crash this crate exists to work around) -- assert the two triple
    // components this test can state unconditionally for *any* real x86_64
    // Windows build of this crate, rather than hard-coding the exact
    // `target_lexicon` spelling (e.g. "pc"/"msvc" vendor/environment
    // details), which is real but incidental to the portability claim.
    assert_eq!(
        std::env::consts::ARCH,
        "x86_64",
        "sanity: this test's byte-search assumptions are for this box's real arch"
    );
    assert_eq!(
        std::env::consts::OS,
        "windows",
        "sanity: this test's byte-search assumptions are for this box's real OS"
    );
    assert!(
        contains_subslice(&cwasm_bytes, b"x86_64"),
        ".cwasm bytes must literally contain the host architecture as embedded metadata"
    );
    assert!(
        contains_subslice(&cwasm_bytes, b"windows"),
        ".cwasm bytes must literally contain the host operating system as embedded metadata"
    );
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
