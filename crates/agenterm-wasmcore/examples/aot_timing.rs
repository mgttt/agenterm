//! `aot_timing` -- the measurement tool for this crate's AOT-precompilation
//! decisive experiment (see `README.md`'s "AOT precompilation" section for
//! the real numbers this program produced on the box it was run on, and
//! the honest verdict drawn from them).
//!
//! Measures, on THIS real machine, with real repeated runs (not a single
//! noisy sample):
//!
//! 1. **One-time AOT precompile cost** -- `WasmCoreHost::precompile_module`
//!    (wraps `Engine::precompile_module`), several repetitions.
//! 2. **End-to-end JIT compile-and-run** -- `WasmCoreHost::run_module`
//!    (`Module::from_file` + instantiate + run `_start` + one `fleet_call`
//!    round trip), several repetitions.
//! 3. **End-to-end AOT load-and-run** -- `WasmCoreHost::run_precompiled_module`
//!    (`Module::deserialize_file` + the same instantiate/run machinery),
//!    several repetitions.
//! 4. As a supplementary, more mechanistic breakdown: the SAME comparison
//!    with instantiate/run cost subtracted out, using `wasmtime::Module`
//!    directly (`Module::from_file` alone vs `Module::deserialize_file`
//!    alone) -- isolates the pure compile/load cost AOT actually changes,
//!    separate from the (identical either way) instantiate+run cost.
//!
//! Every phase's first iteration is reported separately from the rest --
//! the OS/filesystem/CPU-cache state that first call runs under is not
//! reproduced by any later iteration in the same process, so this program
//! does not claim its "warm" numbers are true fresh-process cold-start
//! numbers. See `README.md` for the exact honest framing.
//!
//! Uses the crate's own canonical ABI-reference guest
//! (`guests/fleet_guest.rs`, same one `tests/fleet_call_roundtrip.rs` and
//! `tests/aot_precompile.rs` use) as "a representative guest this crate's
//! own tests already run" -- see `README.md` for why that guest (not the
//! tiny CLI demo guest, not one of the several-MB-payload hardening
//! guests) was judged the representative choice for this measurement.
//!
//! Usage: `cargo run --release --example aot_timing`

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agenterm_wasmcore::{WasmCoreHost, WasmFleetBridgeFn};

const ITERATIONS: usize = 20;
const PRECOMPILE_ITERATIONS: usize = 5;

fn bridge() -> WasmFleetBridgeFn {
    Arc::new(|op_id: &str, params_json: &str| -> Result<String, String> {
        match op_id {
            "wasmcore.echo" => Ok(format!(
                "{{\"echoed_op\":\"{op_id}\",\"echoed_params\":{params_json}}}"
            )),
            other => Err(format!("unknown_op: {other}")),
        }
    })
}

fn compile_guest() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let guest_src = manifest_dir.join("guests").join("fleet_guest.rs");
    let out_dir = manifest_dir.join("target").join("wasmcore-aot-timing");
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
        .unwrap_or_else(|e| panic!("failed to spawn `{rustc}`: {e}"));
    assert!(status.success(), "compiling {} failed", guest_src.display());
    out_wasm
}

/// Runs `f` `n` times, returns the wall-clock duration of each call in
/// order (index 0 is the first / coldest-in-this-process call).
fn time_n<T>(n: usize, mut f: impl FnMut() -> T) -> Vec<Duration> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        let _ = f();
        out.push(start.elapsed());
    }
    out
}

fn report(label: &str, mut durations: Vec<Duration>) {
    assert!(!durations.is_empty());
    let first = durations[0];
    // Computed from `rest` before sorting, since sorting needs a mutable
    // borrow of the whole `Vec` and `rest` is only ever an immutable slice
    // of the pre-sort (i.e. run) order -- keep it that way, don't reuse
    // `rest` after the sort below.
    let rest_summary = (durations.len() > 1).then(|| {
        let rest = &durations[1..];
        let rest_sum: Duration = rest.iter().sum();
        (rest_sum / rest.len() as u32, rest.len())
    });

    durations.sort();
    let min = durations[0];
    let max = durations[durations.len() - 1];
    let median = durations[durations.len() / 2];
    let sum: Duration = durations.iter().sum();
    let mean = sum / durations.len() as u32;

    println!("--- {label} ({} runs) ---", durations.len());
    println!("  first run (this process): {first:?}");
    if let Some((rest_mean, rest_len)) = rest_summary {
        println!("  subsequent runs mean:     {rest_mean:?}  (n={rest_len})");
    }
    println!("  min={min:?}  median={median:?}  mean={mean:?}  max={max:?}");
    println!();
}

fn main() {
    let wasm_path = compile_guest();
    println!("guest: {}", wasm_path.display());

    let wasm_len = std::fs::metadata(&wasm_path)
        .expect("stat guest.wasm")
        .len();
    println!(".wasm size: {wasm_len} bytes");

    let host = WasmCoreHost::new();

    // --- Phase 1: one-time AOT precompile cost. ---
    let mut cwasm_bytes = Vec::new();
    let precompile_durations = time_n(PRECOMPILE_ITERATIONS, || {
        cwasm_bytes = host
            .precompile_module(&wasm_path)
            .expect("precompile_module should succeed");
    });
    let cwasm_path = wasm_path.with_extension("cwasm");
    std::fs::write(&cwasm_path, &cwasm_bytes).expect("write .cwasm");
    let cwasm_len = cwasm_bytes.len();
    println!(".cwasm size: {cwasm_len} bytes");
    println!();
    report(
        "AOT precompile (Engine::precompile_module)",
        precompile_durations,
    );

    // --- Phase 2: end-to-end JIT compile-and-run (WasmCoreHost::run_module). ---
    let jit_durations = time_n(ITERATIONS, || {
        host.run_module(&wasm_path, Some(bridge()))
            .expect("JIT run_module should succeed")
    });
    report(
        "End-to-end JIT (Module::from_file + instantiate + run + fleet_call)",
        jit_durations,
    );

    // --- Phase 3: end-to-end AOT load-and-run (WasmCoreHost::run_precompiled_module). ---
    let aot_durations = time_n(ITERATIONS, || {
        // SAFETY: `cwasm_path` was just written, unmodified, from this same
        // process's own `precompile_module` call above.
        unsafe { host.run_precompiled_module(&cwasm_path, Some(bridge())) }
            .expect("AOT run_precompiled_module should succeed")
    });
    report(
        "End-to-end AOT (Module::deserialize_file + instantiate + run + fleet_call)",
        aot_durations,
    );

    // --- Phase 4: isolated compile/load cost only, no instantiate/run,
    // via wasmtime's own API directly -- separates what AOT actually
    // changes from the (identical either way) instantiate+run cost that
    // phases 2/3 both also pay. ---
    let engine = wasmtime::Engine::default();
    let jit_load_only = time_n(ITERATIONS, || {
        wasmtime::Module::from_file(&engine, &wasm_path).expect("Module::from_file")
    });
    report(
        "Isolated: Module::from_file (JIT compile only, no run)",
        jit_load_only,
    );

    let aot_load_only = time_n(ITERATIONS, || {
        // SAFETY: same trusted, same-process, unmodified .cwasm as phase 3.
        unsafe { wasmtime::Module::deserialize_file(&engine, &cwasm_path) }
            .expect("Module::deserialize_file")
    });
    report(
        "Isolated: Module::deserialize_file (AOT load only, no run)",
        aot_load_only,
    );

    println!(
        "Done. See README.md \"AOT precompilation\" for the verdict drawn from these numbers."
    );
}
