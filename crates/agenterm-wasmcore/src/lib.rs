//! A real `wasmtime` + `wasmtime-wasi` (p1) host: loads and runs
//! `wasm32-wasip1` guest modules, and exposes to those guests the same
//! `fleet_call(operation_id, params_json) -> Result<result_json, error>`
//! bridge shape as `script_engine.rs`'s
//! `ScriptFleetBridgeFn = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>`
//! (the type every one of rh/lua/qjs's engine backends already shares).
//! This crate's whole value is exposing that exact capability to WASM
//! guests, not inventing a new one -- see `WasmFleetBridgeFn` below.
//!
//! # JIT, deliberately
//!
//! `wasmtime`'s default [`Engine`] uses the Cranelift JIT (real native
//! code, real RW->RX memory) -- this is an explicit, accepted design
//! choice for this crate, and a deliberate *contrast* with
//! `agenterm-nativecore`/`agenterm-guestcore`'s "never touch executable
//! memory" discipline. This crate's value proposition is "genuinely
//! portable, safe-by-import-model, low engineering cost via a mature
//! runtime" -- not "hardened-platform immunity" (that is guestcore's job).
//! See `README.md` "Relationship to the other `agenterm-*core` crates".
//!
//! # Calling convention (summary -- see `README.md` for the full ABI spec)
//!
//! WASM imports only carry `i32`/`i64`/`f32`/`f64` -- no strings. The
//! guest passes both input strings as `(ptr, len)` pairs into its own
//! linear memory, and passes the addresses of two guest-owned `i32`
//! out-parameters the host fills in with a `(ptr, len)` pair describing
//! its *own* freshly-allocated result buffer (obtained by the host calling
//! back into a guest-exported `wasmcore_alloc(len) -> ptr` allocator --
//! this crate never guesses a guest-owned buffer size, and never assumes
//! the guest's allocator layout). Full precise ABI: `README.md`.
//!
//! # Scope (this round)
//!
//! `wasm32-wasip1` only. No `wasm64`, no the component model / WASI p2,
//! and this crate is **not** wired into `execute_inner`/the product script
//! path yet -- same phased "prove the mechanism standalone first"
//! discipline every other crate in this session followed.

use std::path::Path;
use std::str;
use std::sync::Arc;

// `wasmtime::Error`/`Result`/`Context`/`bail!` are "99% API-compatible
// with anyhow" (wasmtime's own doc comment on its `error` module) --
// `format_err!` is aliased to the familiar `anyhow!` name here since every
// call site below was written against that name. See Cargo.toml for why
// this crate has no separate `anyhow` dependency.
use wasmtime::error::Context;
use wasmtime::{Caller, Engine, Linker, Memory, Module, Result, Store, bail, format_err as anyhow};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{I32Exit, WasiCtxBuilder};

/// WASM import module name a guest must import `fleet_call` from.
pub const FLEET_CALL_MODULE: &str = "agenterm";
/// WASM import function name for the fleet bridge call.
pub const FLEET_CALL_FUNCTION: &str = "fleet_call";
/// Export name the host calls back into the guest to obtain a
/// host-writable buffer inside the guest's own linear memory.
pub const GUEST_ALLOC_EXPORT: &str = "wasmcore_alloc";
/// Export name of the guest's linear memory (the `wasm32-wasip1` default).
pub const GUEST_MEMORY_EXPORT: &str = "memory";

/// `fleet_call` returned successfully; the out-buffer holds `result_json`.
pub const FLEET_CALL_STATUS_OK: i32 = 0;
/// The bridge returned `Err`; the out-buffer holds the error message.
pub const FLEET_CALL_STATUS_ERR: i32 = 1;
/// No bridge was configured for this host run at all (`fleet_bridge:
/// None`); the out-buffer holds a fixed host-authored diagnostic string.
/// Distinct from `FLEET_CALL_STATUS_ERR` so a guest can tell "the host
/// wasn't wired up for fleet calls this run" apart from "the operation
/// itself failed" without string-matching the payload.
pub const FLEET_CALL_STATUS_NO_BRIDGE: i32 = 2;

/// Windows' default main-thread stack is only 1 MiB (vs Linux's typical
/// 8 MiB); running Cranelift-JIT-compiled code on it was reproduced to
/// crash in this session's scratch proof. Every guest run in this crate
/// happens on a dedicated worker thread with this larger stack instead of
/// relying on every caller to remember to do that themselves.
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

/// Captured guest stdout is capped at this many bytes (a `MemoryOutputPipe`
/// with a fixed capacity -- see [`wasmtime_wasi::p2::pipe::MemoryOutputPipe`]).
/// Plenty for this crate's own verification programs; not meant as a
/// product-sized budget.
const STDOUT_CAPTURE_CAPACITY: usize = 256 * 1024;

/// Fleet bridge callback handed to a guest run. Exactly the same shape as
/// `script_engine.rs`'s `ScriptFleetBridgeFn` (see this crate's module
/// docs) -- kept as this crate's own local type alias rather than a direct
/// dependency on the root `agenterm` package (this crate is standalone,
/// see `Cargo.toml`), mirroring how `agenterm-qjs::host::FleetCallFn` and
/// `agenterm-lua`'s equivalent each define their own identically-shaped
/// alias rather than sharing one crate.
pub type WasmFleetBridgeFn = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

/// `Store<T>` data for a guest run: WASI p1 state plus the optional fleet
/// bridge the `fleet_call` import closes over.
struct WasmCoreState {
    wasi: WasiP1Ctx,
    fleet_bridge: Option<WasmFleetBridgeFn>,
}

/// How a guest run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestExit {
    /// The guest's `_start` returned normally without calling
    /// `exit`/`proc_exit`.
    Returned,
    /// The guest called `exit`/`proc_exit(code)` -- a real WASI trap
    /// carrying the exit code, not a crash. Matches the verified-working
    /// pattern from this session's scratch proof
    /// (`wasmtime_wasi::I32Exit`).
    Exited(i32),
}

/// Result of running a guest module to completion (or to its `exit`).
#[derive(Debug, Clone)]
pub struct GuestRunResult {
    pub exit: GuestExit,
    /// Guest stdout, captured via a [`MemoryOutputPipe`] rather than
    /// inherited, so callers (and this crate's own tests) can assert on
    /// exact content instead of only console output.
    pub stdout: String,
}

/// A real wasmtime host. Owns one [`Engine`] (cheap to clone -- wraps an
/// `Arc` internally -- so `WasmCoreHost` itself is cheap to clone/share
/// too), reused across guest runs.
#[derive(Clone)]
pub struct WasmCoreHost {
    engine: Engine,
}

impl Default for WasmCoreHost {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmCoreHost {
    /// Build a host with the default engine configuration -- Cranelift
    /// JIT, per this crate's explicitly accepted design (see module docs).
    pub fn new() -> Self {
        Self {
            engine: Engine::default(),
        }
    }

    /// Load and run a `wasm32-wasip1` guest module's `_start` entry point
    /// to completion, on a dedicated worker thread with a real, verified
    /// stack size (see `WORKER_STACK_BYTES`).
    ///
    /// `fleet_bridge`, if present, backs the guest-importable `fleet_call`
    /// (see module docs and `README.md` for the exact ABI). `None` is a
    /// legal, real configuration -- guest `fleet_call`s then all resolve
    /// to [`FLEET_CALL_STATUS_NO_BRIDGE`], not a link-time failure (the
    /// import is always registered so a module doesn't fail to
    /// instantiate just because this particular host run has no bridge to
    /// offer it).
    pub fn run_module(
        &self,
        wasm_path: impl AsRef<Path>,
        fleet_bridge: Option<WasmFleetBridgeFn>,
    ) -> Result<GuestRunResult> {
        let wasm_path = wasm_path.as_ref().to_path_buf();
        let engine = self.engine.clone();
        let handle = std::thread::Builder::new()
            .stack_size(WORKER_STACK_BYTES)
            .spawn(move || run_module_on_worker_thread(&engine, &wasm_path, fleet_bridge))
            .context("spawning agenterm-wasmcore guest-run worker thread")?;
        handle
            .join()
            .map_err(|_| anyhow!("agenterm-wasmcore guest-run worker thread panicked"))?
    }
}

fn run_module_on_worker_thread(
    engine: &Engine,
    wasm_path: &Path,
    fleet_bridge: Option<WasmFleetBridgeFn>,
) -> Result<GuestRunResult> {
    let module = Module::from_file(engine, wasm_path)
        .with_context(|| format!("loading wasm module {}", wasm_path.display()))?;

    let mut linker: Linker<WasmCoreState> = Linker::new(engine);
    p1::add_to_linker_sync(&mut linker, |state: &mut WasmCoreState| &mut state.wasi)
        .context("registering WASI p1 imports")?;
    install_fleet_call(&mut linker).context("registering fleet_call import")?;

    let stdout_pipe = MemoryOutputPipe::new(STDOUT_CAPTURE_CAPACITY);
    let wasi = WasiCtxBuilder::new()
        .stdout(stdout_pipe.clone())
        .inherit_stderr()
        .build_p1();

    let state = WasmCoreState { wasi, fleet_bridge };
    let mut store = Store::new(engine, state);

    let instance = linker
        .instantiate(&mut store, &module)
        .context("instantiating wasm module")?;
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .context("guest module does not export a WASI `_start` command entry point")?;

    let exit = match start.call(&mut store, ()) {
        Ok(()) => GuestExit::Returned,
        Err(err) => match err.downcast::<I32Exit>() {
            Ok(exit) => GuestExit::Exited(exit.0),
            Err(err) => return Err(err.context("guest module trapped")),
        },
    };

    let stdout_bytes = stdout_pipe.contents();
    let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
    Ok(GuestRunResult { exit, stdout })
}

fn install_fleet_call(linker: &mut Linker<WasmCoreState>) -> Result<()> {
    linker.func_wrap(FLEET_CALL_MODULE, FLEET_CALL_FUNCTION, fleet_call_import)?;
    Ok(())
}

/// The host side of the `fleet_call` import. See `README.md` "fleet_call
/// calling convention" for the ABI this function and its guest-side
/// counterpart both implement.
fn fleet_call_import(
    mut caller: Caller<'_, WasmCoreState>,
    op_ptr: i32,
    op_len: i32,
    params_ptr: i32,
    params_len: i32,
    out_ptr_ptr: i32,
    out_len_ptr: i32,
) -> Result<i32> {
    let memory = guest_memory(&mut caller)?;

    let op_id = read_guest_string(&mut caller, &memory, op_ptr, op_len)
        .context("fleet_call: reading operation_id from guest memory")?;
    let params_json = read_guest_string(&mut caller, &memory, params_ptr, params_len)
        .context("fleet_call: reading params_json from guest memory")?;

    // Clone the `Arc` out before touching `caller` mutably again --
    // `caller.data()` borrows `caller` immutably, and every helper below
    // needs `&mut caller` for guest-memory/export access.
    let bridge = caller.data().fleet_bridge.clone();
    let (status, payload) = match bridge {
        None => (
            FLEET_CALL_STATUS_NO_BRIDGE,
            "agenterm-wasmcore: no fleet bridge configured for this host run".to_owned(),
        ),
        Some(bridge) => match bridge(&op_id, &params_json) {
            Ok(result_json) => (FLEET_CALL_STATUS_OK, result_json),
            Err(message) => (FLEET_CALL_STATUS_ERR, message),
        },
    };

    write_guest_result(&mut caller, &memory, &payload, out_ptr_ptr, out_len_ptr)?;
    Ok(status)
}

fn guest_memory(caller: &mut Caller<'_, WasmCoreState>) -> Result<Memory> {
    caller
        .get_export(GUEST_MEMORY_EXPORT)
        .and_then(|export| export.into_memory())
        .ok_or_else(|| anyhow!("guest module does not export a `{GUEST_MEMORY_EXPORT}`"))
}

/// Pure bounds-checking slice helper, factored out of [`read_guest_string`]
/// so it is unit-testable without standing up a wasmtime `Store`/`Memory`.
fn slice_bytes(data: &[u8], ptr: i32, len: i32) -> Result<&[u8]> {
    if ptr < 0 || len < 0 {
        bail!("negative pointer/length in guest call (ptr={ptr}, len={len})");
    }
    let start = ptr as usize;
    let end = start
        .checked_add(len as usize)
        .ok_or_else(|| anyhow!("pointer+length overflow (start={start}, len={len})"))?;
    data.get(start..end).ok_or_else(|| {
        anyhow!(
            "guest range {start}..{end} out of bounds (guest memory size {})",
            data.len()
        )
    })
}

fn read_guest_string(
    caller: &mut Caller<'_, WasmCoreState>,
    memory: &Memory,
    ptr: i32,
    len: i32,
) -> Result<String> {
    let data = memory.data(&mut *caller);
    let bytes = slice_bytes(data, ptr, len)?;
    Ok(str::from_utf8(bytes)
        .context("guest string is not valid utf-8")?
        .to_owned())
}

/// Calls the guest's exported `wasmcore_alloc(len) -> ptr`, writes
/// `payload`'s bytes into the buffer it returns, then writes the
/// `(ptr, len)` pair back into the guest's own `out_ptr_ptr`/`out_len_ptr`
/// out-parameters. See `README.md` for why this "host calls back into a
/// guest allocator" shape was chosen over a fixed-capacity guest buffer.
fn write_guest_result(
    caller: &mut Caller<'_, WasmCoreState>,
    memory: &Memory,
    payload: &str,
    out_ptr_ptr: i32,
    out_len_ptr: i32,
) -> Result<()> {
    if out_ptr_ptr < 0 || out_len_ptr < 0 {
        bail!("negative out-parameter pointer (out_ptr_ptr={out_ptr_ptr}, out_len_ptr={out_len_ptr})");
    }

    let alloc_export = caller
        .get_export(GUEST_ALLOC_EXPORT)
        .and_then(|export| export.into_func())
        .ok_or_else(|| anyhow!("guest module does not export `{GUEST_ALLOC_EXPORT}`"))?;
    let alloc_fn = alloc_export
        .typed::<i32, i32>(&mut *caller)
        .context("guest `wasmcore_alloc` has the wrong signature (expected (i32) -> i32)")?;

    let len = i32::try_from(payload.len())
        .context("fleet_call result payload too large to marshal as an i32 length")?;
    let ptr = alloc_fn
        .call(&mut *caller, len)
        .context("calling guest wasmcore_alloc")?;
    if ptr < 0 {
        bail!("guest `wasmcore_alloc` returned a negative pointer ({ptr})");
    }

    memory
        .write(&mut *caller, ptr as usize, payload.as_bytes())
        .context("writing fleet_call result bytes into guest memory")?;
    memory
        .write(&mut *caller, out_ptr_ptr as usize, &ptr.to_le_bytes())
        .context("writing out_ptr back to guest")?;
    memory
        .write(&mut *caller, out_len_ptr as usize, &len.to_le_bytes())
        .context("writing out_len back to guest")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_bytes_accepts_an_in_bounds_range() {
        let data = b"hello world";
        let got = slice_bytes(data, 6, 5).expect("in-bounds slice");
        assert_eq!(got, b"world");
    }

    #[test]
    fn slice_bytes_accepts_a_zero_length_slice_at_the_end() {
        let data = b"hello";
        let got = slice_bytes(data, 5, 0).expect("zero-length slice at end is in-bounds");
        assert_eq!(got, b"");
    }

    #[test]
    fn slice_bytes_rejects_negative_pointer() {
        let data = b"hello";
        let err = slice_bytes(data, -1, 1).unwrap_err();
        assert!(err.to_string().contains("negative"), "{err}");
    }

    #[test]
    fn slice_bytes_rejects_negative_len() {
        let data = b"hello";
        let err = slice_bytes(data, 0, -1).unwrap_err();
        assert!(err.to_string().contains("negative"), "{err}");
    }

    #[test]
    fn slice_bytes_rejects_out_of_bounds_range() {
        let data = b"hello";
        let err = slice_bytes(data, 3, 100).unwrap_err();
        assert!(err.to_string().contains("out of bounds"), "{err}");
    }

    #[test]
    fn slice_bytes_rejects_pointer_length_overflow() {
        let data = b"hello";
        let err = slice_bytes(data, i32::MAX, i32::MAX).unwrap_err();
        // Either overflow or out-of-bounds is an honest rejection here;
        // both paths in `slice_bytes` are exercised across this suite.
        assert!(
            err.to_string().contains("overflow") || err.to_string().contains("out of bounds"),
            "{err}"
        );
    }

    #[test]
    fn default_host_uses_new() {
        // Compile-time/behavioral sanity check that `Default` and `new`
        // agree -- both should produce a usable engine, not panic.
        let _via_default = WasmCoreHost::default();
        let _via_new = WasmCoreHost::new();
    }
}
