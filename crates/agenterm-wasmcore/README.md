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

## AOT precompilation (measured)

`wasmtime` offers a second compile-timing strategy for the *same* portable
`.wasm` bytecode this crate already runs: ahead-of-time (AOT) precompile to
a native `.cwasm` artifact, loaded back with a light mmap+validate step
instead of a full Cranelift compile. This section reports what was really
measured on this box, not an estimate.

### The API surface added

- `WasmCoreHost::precompile_module(wasm_path) -> Result<Vec<u8>>` -- wraps
  `Engine::precompile_module`, the one-time compile-to-bytes step.
- `WasmCoreHost::run_precompiled_module(cwasm_path, fleet_bridge) -> Result<GuestRunResult>`
  (`unsafe`, matching `Module::deserialize_file`'s own contract) -- loads a
  `.cwasm` and runs it through **the exact same** instantiate/run machinery
  (`run_loaded_module`) that the existing JIT `run_module` uses. This is
  not a second, untested code path bolted on beside the real one -- both
  loading strategies funnel into one shared function; only how the
  `wasmtime::Module` gets built differs.
- Neither is wired into any product path -- same standalone,
  mechanism-proving posture as the rest of this crate this round.

[`tests/aot_precompile.rs`](tests/aot_precompile.rs) proves this end to
end: compiles the crate's real `guests/fleet_guest.rs`, runs it once
through `run_module` (JIT) and once through `precompile_module` +
`run_precompiled_module` (AOT), and asserts **byte-identical** `stdout`
and `GuestExit` on both paths -- same real unicode `fleet_call` round
trip, same real bridge echo, same real `exit(7)`.

### Real measured timings

Guest used: `guests/fleet_guest.rs` -- the same guest
`tests/fleet_call_roundtrip.rs` already runs (a real, non-trivial
`wasm32-wasip1` WASI program: `println!`/`format!`/the std allocator, one
successful and one rejected `fleet_call`, an explicit `exit(7)`), chosen
over the tiny `wasmcore_run` CLI demo guest because it is closer to what
this crate's own test suite already treats as representative, and over
the several-MB-payload hardening guests because their *code* size is not
meaningfully larger (the extra cost there is runtime payload marshalling,
not compiled code size) -- see `tests/hardening_payloads.rs`.

Measured with [`examples/aot_timing.rs`](examples/aot_timing.rs),
**release build** (`cargo build --release --example aot_timing`, then run
directly), on this real Windows x86_64 box, one shared `Engine`/
`WasmCoreHost` reused across all iterations (matching how this crate is
actually meant to be used -- construct once, run many times), 20
repetitions per phase (5 for the one-time precompile step). Every
number below is real output from one real run, not rounded/estimated:

```text
.wasm size: 2,105,917 bytes
.cwasm size: 219,896 bytes

AOT precompile (Engine::precompile_module) -- 5 runs
  first run: 95.17ms
  subsequent runs mean: 75.72ms  (n=4)
  min=53.09ms  median=86.64ms  mean=79.61ms  max=95.17ms

End-to-end JIT (Module::from_file + instantiate + run + fleet_call) -- 20 runs
  first run: 85.21ms
  subsequent runs mean: 69.79ms  (n=19)
  min=50.26ms  median=70.98ms  mean=70.56ms  max=87.97ms

End-to-end AOT (Module::deserialize_file + instantiate + run + fleet_call) -- 20 runs
  first run: 3.57ms
  subsequent runs mean: 3.32ms  (n=19)
  min=1.29ms  median=3.02ms  mean=3.33ms  max=9.59ms

Isolated: Module::from_file (JIT compile only, no run) -- 20 runs
  first run: 48.90ms
  subsequent runs mean: 66.13ms  (n=19)
  min=42.70ms  median=59.84ms  mean=65.27ms  max=129.48ms

Isolated: Module::deserialize_file (AOT load only, no run) -- 20 runs
  first run: 0.48ms
  subsequent runs mean: 0.25ms  (n=19)
  min=0.21ms  median=0.24ms  mean=0.26ms  max=0.48ms
```

Honest variance note: these are "hot" repeated-in-one-process numbers
(OS/filesystem caches already warm from earlier iterations and earlier
test runs), not an isolated fresh-process cold boot -- the `first run`
row in each block is the closest proxy this program produces, not a true
cold-boot number. The relative JIT-vs-AOT gap is large enough (roughly
20-25x end to end, ~250x for the isolated load step) that this
methodology gap is very unlikely to change the conclusion, but it is a
real limitation of what was actually measured, stated honestly rather
than glossed over.

**Interpretation:** the ~3ms/~65ms split between the end-to-end and
isolated-load-only AOT numbers shows where the cost really lives -- both
loading strategies pay roughly the same ~3ms of fixed overhead (worker
thread spawn with its 16 MiB stack, WASI context setup, linking,
instantiation, running `_start`, one `fleet_call` round trip). JIT's
extra ~65ms over that fixed floor is Cranelift actually compiling this
guest's code from scratch on every single load; AOT's isolated load step
(~0.25ms) is a light mmap + compatibility-metadata check, not a compile.
The one-time `precompile_module` cost (~53-95ms) is, unsurprisingly, in
the same range as one JIT compile -- it runs the identical Cranelift
compilation, it just serializes the result instead of also
instantiating/running it.

Also notable, and guest-composition-dependent rather than a general law:
for this guest, the `.cwasm` (220 KB) is **smaller** than the source
`.wasm` (2.1 MB) -- the source module carries a fair amount of
unstripped std/formatting machinery `wasmtime`'s compiled-code
representation for this narrow guest does not need to keep around
byte-for-byte. Do not generalize this to "AOT artifacts are always
smaller"; it was not tested for other guest shapes this round.

### Portability: what was actually tested vs cited

The claim under test: a `.cwasm` is native code for whatever target
(architecture, OS, and `wasmtime::Config`-derived compiler/tunables
settings) it was precompiled for -- it is **not** portable across those
the way the source `.wasm` is.

**Tested for real, on this box** (see
[`tests/aot_precompile.rs`](tests/aot_precompile.rs)):

- `wasmtime::Engine::detect_precompiled` recognizes `precompile_module`'s
  output as a real, distinct precompiled-module artifact (not plain wasm
  bytes) -- confirms this is a genuinely different kind of file, not
  wasm-with-extra-steps.
- The `.cwasm` bytes literally contain the readable ASCII substrings
  `"x86_64"` and `"windows"` -- wasmtime's own serialization format
  (`wasmtime-47.0.3/src/engine/serialization.rs`, `Metadata::new`) embeds
  `compiler.triple().to_string()` and `postcard`-encodes it as a plain
  length-prefixed UTF-8 string, so the host's target triple survives
  verbatim, inspectable by literal byte search -- verified by doing
  exactly that, not assumed from the format description.
- **The compatibility gate itself is live and really rejects a real
  mismatch.** This box is single-ISA (x86_64) and single-OS (Windows), so
  it cannot exercise an actual architecture/OS mismatch -- but it CAN
  construct two `wasmtime::Engine`s from different `Config`s (one with
  `epoch_interruption(false)`, one with `epoch_interruption(true)`),
  precompile with one, and attempt `Module::deserialize` with the other.
  That attempt is **really rejected**, with a real, specific error
  (`"...compiled without epoch interruption but it is enabled for the
  host"`/its mirror). This matters beyond the one flag it exercises:
  wasmtime's own `Metadata::check_compatible` (same source file) runs
  architecture, then OS, then Cranelift shared/ISA flags, then tunables
  (which is what this test's `epoch_interruption` flag actually is), then
  WASM features, all through **the same function, the same gate, on the
  same input**. Proving that gate is live and enforced via one of its
  real branches is real, verified evidence that the mechanism functions
  as documented -- not proof of the architecture/OS branches specifically.

**Cited from wasmtime's own source, not independently tested on this
box** (`wasmtime-47.0.3/src/engine/serialization.rs`,
`Metadata::check_triple`): a `.cwasm` whose embedded target
architecture or operating system does not match the loading `Engine`'s
is rejected with `"Module was compiled for architecture '<x>'"` /
`"...operating system '<x>'"` -- this crate's own committed test suite
has that source file's own unit tests
(`engine::serialization::test::test_architecture_mismatch`/
`test_os_mismatch`) doing exactly this by constructing a synthetic
`Metadata` with a different target string, which is honest evidence the
check exists and is exercised by wasmtime's own test suite -- but this
round's work did not, and on this single-ISA/single-OS box could not,
independently reproduce a real cross-architecture or cross-OS load
attempt. State this precisely rather than implying it was verified here:
**not tested by this round's work, cited from upstream source/tests.**

### Verdict

**Yes, AOT meaningfully reduces load latency for this crate's use
case, for a guest of this representative size and composition** -- real,
measured, roughly 20-25x faster end to end (median 71ms → 3ms) and
~250x faster for the isolated load step alone (59.8ms → 0.24ms median).
This is not a marginal, borderline-not-worth-it gap; JIT's compile-time
cost genuinely dominates a guest of this real (2 MB, real std program)
size, and AOT genuinely removes nearly all of it.

Whether that win is **worth the added complexity** depends on usage
pattern, and the honest answer is "it depends, and this round did not
change this crate's own default":

- **Worth it** when the same guest bytes get loaded and run repeatedly
  within a bounded, known set of targets (architecture + OS +
  `wasmtime`/`Config` version) -- e.g. a guest invoked many times across
  many short-lived host-process launches, or a plugin reloaded
  frequently in a long-lived session. `WasmCoreHost` already amortizes
  `Engine` construction across runs by design, but **not** per-guest
  `Module` compilation -- every `run_module` call recompiles from
  scratch even on a reused `Engine`, so any repeated-load pattern is
  exactly where this measured ~20x win is real, not theoretical.
- **Not worth it** when a guest is loaded once per long-lived host
  process (the one-time ~50-95ms precompile cost, plus needing to build,
  store, and distribute a separate `.cwasm` per target instead of one
  portable `.wasm`, plus needing real fallback-to-JIT handling for a
  target `precompile_module` was never run for) is real added complexity
  for a latency difference no one will observe. The source `.wasm` must
  still be shipped regardless (for any target without a matching
  `.cwasm`), so AOT is additive distribution weight, not a replacement.
- This crate is still standalone and not wired into any product path
  this round (see "Non-goals" below) -- this section reports the
  measured trade honestly; it does not argue for or against wiring AOT
  into a future product path, which depends on the not-yet-known actual
  guest-reload frequency of whatever consumes this crate.

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

## Relationship to the archived `agenterm-*core` crates

Three earlier exploration crates — `agenterm-nativecore`, `agenterm-guestcore`,
and `agenterm-dynacore` — were independent, non-overlapping approaches to "let
a script/guest reach a capability the host controls." They were removed from
the workspace (`fef91b2d`, 2026-08-09) and archived (2026-08-10, see
`plan/archive/crates-archived/`). This crate (`agenterm-wasmcore`) is the
approach that shipped: a mature, reused JIT runtime (`wasmtime`) executing real
`wasm32-wasip1` bytecode, genuinely ISA-neutral, with Cranelift JIT.

## Why standalone (not a root-workspace member)

`Cargo.toml` carries its own empty `[workspace]` table rather than being
added to the root `Cargo.toml`'s `members` list. Two reasons:

1. **Precedent.** Earlier `agenterm-*core` crates were root-workspace members
   briefly, then removed (`fef91b2d`, 2026-08-09) and later archived
   (`plan/archive/crates-archived/`). Following that same direction, this
   crate joins the root workspace only when it has a product-facing integration
   point (which it now does, via `script-wasmcore` feature).
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
