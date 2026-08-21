# QJWasm / WA2X source review for tinyvm

Reviewed 2026-08-21. This is a clean source-level design review, not a runtime
dependency proposal. Nostalgia Arcade cartridges remain standard `.wasm`
interpreted by tinyvm on iOS; QuickJS, H5, JIT and downloaded native AOT
artifacts remain outside the product.

## Pinned sources and reuse boundary

- [QJWasm `fccfcde`](https://github.com/LazyBoy-KK/QJWasm/tree/fccfcde22cb8936709039586ea70a9fd4d8a772e)
- [QJWasm rquickjs fork `85ef18f`](https://github.com/LazyBoy-KK/rquickjs/tree/85ef18f5a610a5afdea27f08542122be9ced339f)
- [QJWasm QuickJS fork `d473bab`](https://github.com/LazyBoy-KK/quickjs/tree/d473bab1ed05668370f76ab368077a62a57b96d8)
- [WA2X for QJWasm `20d698a`](https://github.com/LazyBoy-KK/wa2x_for_qjwasm/tree/20d698a15a127833137cd38f9679d65a5c9e3cad)
- [QJWasm paper](https://doi.org/10.1016/j.sysarc.2026.103926)

WA2X has an Apache-2.0 root license. The pinned rquickjs and QuickJS forks each
carry their upstream license. QJWasm declares `license = "MIT"` in
`Cargo.toml`, but the reviewed root has no LICENSE file and GitHub reports no
detected license. Do not copy QJWasm implementation code until the authors add
an unambiguous repository license; architecture learned from the paper and
source should be reimplemented behind tinyvm's own tests.

The QJWasm release is not presently a reproducible dependency pin. Its README
still says WA2X is temporarily closed, while `Cargo.toml` expects
`../iwasm-rs/runtime` rather than the now-published repository and does not pin
that runtime revision. Both QJWasm and the WA2X publication are single squashed
commits, so their public history cannot establish how the integration evolved.

## What the implementation actually does

### Cross-runtime ownership

JS wrapper objects retain persistent QuickJS values and WA2X instance state.
Linear memory is exposed as an `ArrayBuffer` over the WA2X allocation instead
of copied into a second buffer. An `Arc`-backed memory wrapper keeps that
allocation alive, while growth detaches or refreshes the cached JS buffer.
Functions, tables, globals, externrefs and imported JS functions maintain
additional reference maps so values reachable from either runtime remain
marked during QuickJS collection.

This validates tinyvm's direction: public `Store`, `Function`, `Memory`,
`Table`, `Global` and funcref handles need explicit owner identity and one
acyclic lifetime graph. The useful lesson is the ownership invariant, not
QJWasm's implementation with shared `UnsafeCell` state and broad manual
`Send`/`Sync` assertions.

### Three logical message paths

The paper's three-channel description maps to these source-level paths:

1. JS-to-Wasm requests enter a per-instance unbounded crossbeam channel. A
   dedicated Rust/WA2X thread receives closures and runs them serially.
2. Wasm completion work enters a mutex-protected JS queue. The first item added
   to an empty queue writes one byte to a nonblocking OS pipe watched by the
   QuickJS loop; the JS thread converts results and resolves or rejects the
   saved Promise.
3. A Wasm-to-JS imported-function callback enters a separate queue in the same
   wakeup mechanism. The WA2X thread parks until the JS thread executes the
   callback, stores its result and unparks the caller.

The empty-to-nonempty wakeup coalescing and separation of request, callback and
completion semantics are reusable. The concrete queues are not: they are
unbounded, send failures panic, callbacks have no deadline or cancellation,
and one worker thread is created per async instance.

QJWasm also shares a safeguard across an instance and its exported resources.
An outstanding async call sets it, rejecting overlapping sync/async access
until result conversion completes. This is evidence that re-entrancy and
concurrent mutation must be a first-class runtime state, but a plain shared
boolean is too weak for tinyvm. A future tinyvm async owner should use an
explicit state machine with request identity, generation and terminal error.

## Benchmark interpretation

The reported `6.78x` and `35.97x` measure different things. The first is the
application benefit of migrating selected compute-heavy JS work to AOT Wasm;
it is not a claim that WA2X executes Wasm 6.78 times faster than another Wasm
VM. The latter is asynchronous boundary throughput against QuickJS Worker
messaging, reaching its largest difference for large typed arrays.

The checked-in Worker baseline posts a typed array to another JS runtime and
then copies it again into Wasm memory with `TypedArray.set`. The QJWasm path
allocates in the instance's already shared linear memory and sends only scalar
pointer/length values. Its benchmark loop does not populate the newly
allocated region before the measured Wasm call, so it demonstrates the cost
avoided by changing ownership and transfer semantics more than an
apples-to-apples payload-processing speedup.

Tinyvm already uses the corresponding product shape: native modules and game
lifecycle functions exchange bounded pointer/length regions in guest memory,
and Rust memory views borrow the live allocation without serializing it. We
should preserve that shape and measure it directly instead of quoting the
QJWasm headline multiplier.

## Adopt, adapt, reject

Adopt as invariants and tests:

- one explicit owner graph for instances and exported resource handles;
- borrowed/shared linear-memory views rather than boundary copies;
- wake once on an empty-to-nonempty transition;
- distinct request, guest-to-host callback and completion/result semantics;
- a runtime-visible busy/re-entrancy state;
- benchmarks split into execution, scheduling, conversion and byte-copy cost.

Adapt before any future async tinyvm API:

- bounded queues with admission failure and per-cartridge quotas;
- monotonically identified requests and deterministic FIFO ordering;
- cancellation, timeout, shutdown and receiver-drop outcomes;
- one host-owned executor policy rather than one hidden thread per instance;
- explicit `Idle / Running / InCallback / Completing / Failed` transitions;
- fallible errors instead of panic/unwrap across the embedding boundary;
- generation-checked memory leases across `memory.grow`.

Reject for the iOS cartridge runtime:

- QuickJS or a JS event loop as a product dependency;
- WA2X JIT, dynamic libraries or downloaded native AOT artifacts;
- nonstandard `instance.exportsAsync` in the cartridge ABI;
- unbounded channels and blocking `park`/`unpark` callbacks;
- raw cross-thread `UnsafeCell` sharing and blanket manual `Send`/`Sync`;
- pointer-address identity without store and generation validation.

## Executable follow-ups

1. [x] `smoke-boundary-benchmark.sh` separately records empty-call cost,
   scalar argument conversion, borrowed guest-memory access, an intentional
   host copy and a constant-work guest call at 0, 64, 1 KiB, 64 KiB and one
   76,800-byte frame-sized payload.
2. [x] Tinyvm and development-only JavaScriptCore execute the identical WABT
   fixture and emit the same CSV dimensions, so execution is not confused with
   data movement. Timings are observations, never pass/fail thresholds.
3. [x] Memory/global/table/function handle tests retain live resources after
   public instance handles are dropped. Function-reference tests also reject
   wrong-store values and a stale token whose original Store is already gone.
4. [x] Standard externref function/global values use opaque monotonic host
   identities rather than pointer addresses. One independently WABT-compiled
   module preserves the same object identity through tinyvm and JavaScriptCore;
   tinyvm never dereferences the token or claims ownership of the host object.
5. [ ] If background execution becomes a product requirement, first specify and
   test bounded mailbox saturation, cancellation, callback re-entrancy,
   shutdown and Promise-equivalent completion semantics without adding JS.
