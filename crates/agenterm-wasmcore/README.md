# agenterm-wasmcore

A real [`wasmtime`](https://docs.rs/wasmtime) + [`wasmtime-wasi`](https://docs.rs/wasmtime-wasi)
(p1 feature) host: loads and runs `wasm32-wasip1` guest modules, and
exposes to those guests the **same** `fleet_call(operation_id, params_json)
-> Result<result_json, error>` bridge shape every other engine backend in
this repo already shares
(`ScriptFleetBridgeFn = Arc<dyn Fn(&str, &str) -> Result<String, String> +
Send + Sync>`, defined in `src/script_engine.rs`). This crate's whole
value is exposing that exact capability to WASM guests, not inventing a
new one.

Standalone crate, own `[workspace]` table (see "Why standalone" below) --
not a member of the root workspace, and **not wired into
`execute_inner`/the product script path this round**. This is a
mechanism-proving crate: it proves the ABI and the round trip work for
real, nothing more.

## JIT, deliberately

`wasmtime`'s default [`Engine`] uses the Cranelift JIT: it generates and
executes real native machine code, which needs real RW->RX executable
memory. That is an **explicit, accepted design decision for this crate**,
and a deliberate contrast with `agenterm-nativecore`/`agenterm-guestcore`'s
"never touch executable memory" discipline. This crate does not try to
force `wasmtime` into a pure-interpretation mode, and does not add any
RWX-avoidance machinery -- that would contradict the whole point of using
a mature, JIT-based runtime. See "Relationship to the other
`agenterm-*core` crates" below for what this crate is trading that
discipline away *for*.

## Quickstart

```rust
use std::sync::Arc;
use agenterm_wasmcore::{WasmCoreHost, WasmFleetBridgeFn, GuestExit};

let bridge: WasmFleetBridgeFn = Arc::new(|op_id: &str, params_json: &str| {
    match op_id {
        "protocol.info" => Ok(r#"{"version":1}"#.to_owned()),
        other => Err(format!("unknown_op: {other}")),
    }
});

let host = WasmCoreHost::new();
let result = host.run_module("guest.wasm", Some(bridge))?;

match result.exit {
    GuestExit::Returned => println!("guest returned normally"),
    GuestExit::Exited(code) => println!("guest called exit({code})"),
}
println!("guest stdout:\n{}", result.stdout);
# Ok::<(), wasmtime::Error>(())
```

`WasmCoreHost::run_module` runs the guest's `_start` entry point on a
dedicated worker thread with a 16 MiB stack (see "Real exit/lifecycle
handling" below for why), and returns once the guest finishes -- whether
by returning normally or by calling `exit`/`proc_exit`.

## The `fleet_call` calling convention (the real ABI)

WASM imports only carry `i32`/`i64`/`f32`/`f64` values -- there is no
native way to pass a string across the guest/host boundary. This is the
concrete, tested marshalling convention this crate implements. A guest
author in **any** language that can target `wasm32-wasip1` (not just Rust)
can follow this spec by hand without reading this crate's Rust source.

### Import the host provides

Module `"agenterm"`, function `"fleet_call"`:

```text
fleet_call(
    op_ptr: i32, op_len: i32,           // guest memory (ptr, len) of operation_id, UTF-8
    params_ptr: i32, params_len: i32,   // guest memory (ptr, len) of params_json, UTF-8
    out_ptr_ptr: i32, out_len_ptr: i32, // addresses of two guest-owned i32 out-params
) -> i32                                // status: 0 = Ok, 1 = Err, 2 = NoBridge
```

- `op_ptr`/`op_len` and `params_ptr`/`params_len` are `(pointer, length)`
  pairs into the **guest's own** linear memory (exported as `"memory"`,
  the `wasm32-wasip1` default), pointing at valid UTF-8 bytes. The guest
  retains ownership of these bytes for the duration of the call -- the
  host only reads them, never writes or frees them.
- `out_ptr_ptr`/`out_len_ptr` are addresses, **inside the guest's own
  memory**, of two `i32` slots the guest allocated (e.g. two local
  variables it took the address of) for the host to fill in. The host
  never reads their prior contents, only writes to them.
- Return value (`i32` status code):
  - **`0` (Ok)** -- the bridge call succeeded. The out-buffer (described
    by the `(ptr, len)` the host wrote to `out_ptr_ptr`/`out_len_ptr`)
    holds `result_json`, UTF-8.
  - **`1` (Err)** -- the bridge call returned an application-level error
    (e.g. "unknown operation", "invalid params"). The out-buffer holds the
    error message, UTF-8. This is a normal, expected outcome, not a
    crash -- guests should check the status and branch, not assume `0`.
  - **`2` (NoBridge)** -- this particular host run was not configured with
    a fleet bridge at all (`fleet_bridge: None`). The out-buffer holds a
    fixed host-authored diagnostic string. Distinct from `1` so a guest
    can tell "this host wasn't wired up for fleet calls this run" apart
    from "the operation itself was rejected" without string-matching the
    payload.

### Export the guest must provide

```text
wasmcore_alloc(len: i32) -> i32
```

Before writing the result string, the host calls **back into the guest's
own exported allocator** to obtain a buffer the guest itself owns, sized
to fit the result. This is a deliberate design choice over two simpler
but worse alternatives:

- **A fixed-capacity guest-provided buffer** would force the guest to
  guess a "large enough" size up front, or add a second retry round trip
  when it guesses wrong.
- **The host guessing at the guest's allocator/heap layout directly**
  would be fragile and toolchain-specific (Rust's `wasm32-wasip1` `std`
  allocator, a hand-rolled C bump allocator, and a `wee_alloc`-style guest
  each lay out memory differently).

Calling back into a guest-exported `wasmcore_alloc` sidesteps both: the
guest's own toolchain/allocator decides how to satisfy the request, and
the host only ever writes into memory the guest explicitly handed it.
`len` is always `>= 0`; guests are free to treat `len == 0` as "at least 1
byte" if their allocator doesn't like zero-sized allocations (this crate's
own guest test program does exactly that). The guest is not required to
free this memory for the host's sake -- `wasmcore_alloc` is an allocation
handshake, not a paired allocate/free protocol; a guest that wants to free
it later is free to track the returned pointer/length itself (a real
implementation would export a matching `wasmcore_free(ptr, len)`, but nothing
in this ABI calls one).

### Guest-side call sequence

1. Have the operation id and params JSON as UTF-8 bytes somewhere in your
   own linear memory (e.g. `&str::as_ptr()`/`.len()` in Rust).
2. Reserve two `i32` slots in your own memory for the out-parameters
   (e.g. two local variables), and take their addresses.
3. Call the imported `fleet_call` with all six arguments.
4. Read the status code from the return value.
5. Read the `(ptr, len)` pair back out of your two out-parameter slots
   (the host has now written into them), and interpret the bytes at that
   `(ptr, len)` in your own memory as UTF-8 -- `result_json` if status was
   `0`, an error/diagnostic message otherwise.

See [`guests/fleet_guest.rs`](guests/fleet_guest.rs) for a complete, real,
compiled reference implementation of both sides of this sequence
(`call_fleet` and `wasmcore_alloc`), and
[`tests/fleet_call_roundtrip.rs`](tests/fleet_call_roundtrip.rs) for the
host side driving it end to end.

## Real exit/lifecycle handling

A guest command program's `_start` either returns normally, or calls
`exit`/`proc_exit(code)`, which `wasmtime-wasi` p1 surfaces as a **trap**
carrying a `wasmtime_wasi::I32Exit(code)` value -- a real, expected
lifecycle signal, not a crash. `WasmCoreHost::run_module` downcasts the
trap and reports it as `GuestExit::Exited(code)`; any other trap is
propagated as a real error. [`tests/fleet_call_roundtrip.rs`](tests/fleet_call_roundtrip.rs)'s
guest program calls `std::process::exit(7)` explicitly and the test
asserts `GuestExit::Exited(7)` -- this is the exact verified-working
pattern from this round's scratch proof, not new/untested code.

That scratch proof also found a real crash on this box: Windows' default
main-thread stack is only 1 MiB (vs Linux's typical 8 MiB), which is too
small for Cranelift-JIT-compiled guest execution. `run_module` always runs
the guest on a dedicated worker thread with a 16 MiB stack for this
reason -- every caller gets the fix automatically instead of needing to
know about it.

## CLI example: `wasmcore_run`

[`examples/wasmcore_run.rs`](examples/wasmcore_run.rs) is the
CLI-triggerable entry point for this crate:

```sh
cargo run --example wasmcore_run -- path/to/guest.wasm   # run an existing guest
cargo run --example wasmcore_run                         # zero-setup demo
```

With no path, it compiles a small self-contained demo guest at run time
(a real `rustc --target wasm32-wasip1` invocation, same mechanism as
`tests/fleet_call_roundtrip.rs`), so the command works out of the box with
no `.wasm` file to hand it first. Either way, the guest runs against a
built-in demo `fleet_call` bridge that recognizes every `operation_id` and
echoes its params back inside `{"received": <params_json>}` -- this crate
has no dependency on the `agenterm` product crate, so this is an honest
demo of the ABI round trip, not a stand-in for a real product operation.
The example prints the guest's real stdout and its real `GuestExit`
status, and propagates a guest's explicit `exit(code)` as its own process
exit code.

## Hardening / adversarial testing

Beyond the happy-path round trip in `tests/fleet_call_roundtrip.rs`, this
crate carries two adversarial test files --
[`tests/hardening_link_and_bounds.rs`](tests/hardening_link_and_bounds.rs)
and
[`tests/hardening_payloads.rs`](tests/hardening_payloads.rs) -- that each
build a REAL malicious/malformed `wasm32-wasip1` guest via a real `rustc`
invocation and run it through the real `WasmCoreHost`, then assert on the
*actually observed* host behavior (not just reasoned-about). Headline
result: **no memory-safety gap was found.** Every adversarial scenario
below produces a clean `Result::Err` (surfaced to the guest as a WASI
trap), never a host-process crash/hang and never an out-of-bounds host
write:

- **Missing/wrong-signature `wasmcore_alloc` export.** A guest that does
  not export `wasmcore_alloc` at all is rejected the first time the host
  needs it (`... does not export \`wasmcore_alloc\``) -- this is a
  call-time rejection (the export is looked up lazily), not an
  instantiation-time one. A guest that exports it with the wrong
  signature (`(i32, i32) -> i32` instead of `(i32) -> i32`) is rejected
  just as cleanly via `wasmtime`'s own `Func::typed` check (`... wrong
  signature ... type mismatch with parameters: expected 1 types, found
  2`).
- **Wrong-signature `fleet_call` *import*.** The real link-time
  counterpart: a guest that imports `fleet_call` itself with a mismatched
  signature (one argument short) fails at `Linker::instantiate` --
  `_start` never runs at all (`instantiating wasm module: incompatible
  import type for \`agenterm::fleet_call\`: types incompatible: ...`).
  This is where "wasmtime's own type-checking" genuinely applies; the
  guest-export case above is a related but distinct call-time check this
  crate performs itself.
- **`wasmcore_alloc` returning an out-of-bounds pointer (the single most
  safety-critical path in this crate: the host writing into
  guest-claimed memory).** A guest whose allocator returns `i32::MAX`, a
  moderately-OOB value, or a pointer just **2 bytes** before the guest's
  real current memory end (so the host's write payload overruns the real
  end by only a few dozen bytes) is rejected in every case by
  `wasmtime`'s own bounds-checked `Memory::write` (`writing fleet_call
  result bytes into guest memory: out of bounds memory access`) -- exact,
  byte-accurate, not merely catching implausibly huge values. This crate's
  own explicit `ptr < 0` guard in `write_guest_result` catches negative
  pointers; `Memory::write`'s own bounds check independently catches
  every positive-but-out-of-bounds one. A guest whose allocator returns
  **0** is a distinct, non-crash case: address 0 is an ordinary in-bounds
  guest address (not a protected native null page), so the host accepts
  it and the write round-trips correctly -- verified, not assumed. This
  is correct per the ABI (only negative pointers are documented as
  invalid); a real guest allocator that carelessly returns 0 risks
  clobbering its own low memory, but that is the guest's own allocator
  bug, not a host-side hole.
- **A guest lying about its own `operation_id`/`params_json` lengths (the
  input-side counterpart).** A guest claiming a 50,000,000-byte
  `params_json` when its real buffer is 2 bytes, a negative `op_len`, and
  a small, realistic overrun (a real near-the-end pointer claimed a few
  KiB too long) are all rejected by `slice_bytes`/`read_guest_string`
  with exact, reported bounds (`guest range 1048613..51048613 out of
  bounds (guest memory size 1114112)`) -- and a call-counting bridge
  confirms the fleet bridge is **never invoked** for a call whose own
  params could not be read.
- **Several-MB payloads, both directions.** A 5 MiB `params_json` and a
  (deliberately differently-sized) 6 MiB `result_json` round-trip with
  independently-computed checksums matching exactly on both the
  host-received and guest-read-back sides -- proving no truncation and no
  `usize`/`i32` length-arithmetic edge under WASM's i32-only ABI on this
  64-bit host.
- **200 sequential `fleet_call` invocations from one guest run,** reusing
  the same two guest-owned out-parameter locals across every call (the
  realistic pattern a long-lived guest would use) -- every call's bridge
  invocation and every call's echoed stdout line match their own index
  exactly, with no dropped, duplicated, reordered, or cross-call-leaked
  data.

## Non-goals (explicit, this round)

- **No `wasm64`.** Not attempted.
- **No component model / WASI p2.** This crate targets `wasm32-wasip1`
  (core wasm modules + the `wasi_snapshot_preview1` ABI) only --
  `wasmtime-wasi`'s p1 support does not work with `wasmtime::component`
  anyway (see `wasmtime_wasi::p1`'s own module docs).
- **No `wasm32-unknown-unknown`.** This crate is specifically about WASI
  guest programs, not a generic sandboxed-compute target.
- **No product wiring.** Not called from `execute_inner`, `script_engine.rs`,
  or any other product script-execution path this round. Same phased
  "prove the mechanism standalone first" discipline every other crate in
  this session followed.

## Relationship to the other `agenterm-*core` crates

Four deliberately non-overlapping approaches to "let a script/guest reach
a capability the host controls," not competing implementations of the
same idea:

| Crate | Mechanism | Portability | Executable memory | Guest source requirement |
|---|---|---|---|---|
| `agenterm-nativecore` | Direct native Win32 calls behind a typed `Intent` seam | Windows-native only | Never | N/A (host-side library) |
| `agenterm-guestcore` | Hand-rolled x86_64 machine-code *interpreter*, translating a Linux syscall subset to `agenterm-nativecore` | Same-ISA only (x86_64 guest on x86_64 host); runs **existing** Linux x86_64 binaries unmodified | Never -- guest bytes are read as data, never executed by the host CPU | None -- runs real, already-compiled ELF binaries as-is |
| `agenterm-dynacore` | A custom bytecode VM + `Intent`-shaped IR, its own pack format | ISA-neutral by construction (it's not machine code at all) | Never | Guest logic must be authored against dynacore's own IR/pack format |
| **`agenterm-wasmcore`** (this crate) | A mature, reused JIT runtime (`wasmtime`) executing real `wasm32-wasip1` bytecode | **Genuinely ISA-neutral** -- any `wasm32-wasip1` toolchain output runs unmodified on any host `wasmtime` supports | **Yes, deliberately** -- Cranelift JIT | Guest source must specifically target WASM (`wasm32-wasip1`) -- unlike guestcore, this does *not* run an arbitrary pre-existing Linux binary |

What is unique about this crate: it is the only one of the four that
reuses an existing, independently-maintained, mature runtime rather than
implementing its own interpreter/VM -- which is a large engineering-cost
reduction, at the explicit, accepted cost of depending on that runtime's
JIT and its executable-memory requirement. It is also the only one whose
"bytecode" is a real, independently-specified, portable ISA (WebAssembly)
rather than this project's own invention (dynacore's IR) or an existing
*hardware* ISA re-interpreted in software (guestcore's x86_64 decoder) --
in exchange, a guest has to be built for WASM specifically; you cannot
point this crate at an arbitrary already-compiled Linux binary the way
`agenterm-guestcore` can.

## Why standalone (not a root-workspace member)

`Cargo.toml` carries its own empty `[workspace]` table rather than being
added to the root `Cargo.toml`'s `members` list. Two reasons:

1. **Precedent already reversed once.** `agenterm-nativecore`,
   `agenterm-dynacore`, and `agenterm-guestcore` were briefly root-workspace
   members and were removed the same day this crate was built (`git log --
   Cargo.toml`, commit `fef91b2d` "cleanup: remove dynacore/nativecore
   integration") by a concurrent session's large refactor. Following that
   same, freshly-reaffirmed direction rather than re-adding a fourth
   specialized crate back into the root workspace is the safer, more
   consistent choice.
2. **Build isolation.** `wasmtime` is a heavy dependency tree (Cranelift,
   `wasmtime-wasi`, and their transitive graph). Keeping it out of the
   root `Cargo.lock` avoids bloating every other crate's build in this
   workspace with a dependency only this crate needs.

Build and test it directly from its own directory:

```sh
cd crates/agenterm-wasmcore
cargo test
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps
```

## Requirements to build/test this crate

- The `wasm32-wasip1` Rust target (`rustup target add wasm32-wasip1`) --
  `tests/fleet_call_roundtrip.rs` compiles a real guest program with a
  real `rustc --target wasm32-wasip1` invocation at test time (no
  committed `.wasm` binary in this repo).
