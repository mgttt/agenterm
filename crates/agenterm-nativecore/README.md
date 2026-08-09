# agenterm-nativecore

`agenterm-nativecore` is an interpreter that dynamically executes native machine calls
(currently x86_64/Windows) without a compiler in the loop and without ever requesting
executable memory. It reaches real platform API surface (`VirtualAlloc`, `CreateFileA`,
`CreateProcessA`, ...) directly from a neutral IR walked by a tree-walking interpreter —
it never generates machine code, never transitions a page RW→RX, and never `dlopen`s
anything. That is the whole reason this crate exists as something other than a restatement
of `agenterm-rh`'s AOT pack (which *does* need a compiler and executable memory, and is a
better tool whenever both are available — see
[`plan/design-dynacore-native-core.md`](../../plan/design-dynacore-native-core.md) §0–§1
for the measured, not-guessed, constraints that motivate the split).

**Status: v1, feature-frozen.** All of the design doc's §5/§7 acceptance criteria are met
and the code is in production use (opt-in, zero cost when unconfigured), but as of §8 the
design doc records a deliberate decision to stop adding scope to this crate and put research
effort elsewhere (see "What this crate does NOT do" below). This README documents what
exists today; it is not a roadmap.

## What it does

The crate ports/hardens Q22's research-track assembly of a neutral IR: literals, arithmetic,
raw-memory loads/stores into rodata or scratch, control flow (`Br`/`BrCond`/`Ret`/`Exit`), and
calls out to the host through a fixed set of **seven native intents**, each of which binds to a
real Win32 API sequence in `seam.rs`:

| Intent | Args | Native call(s) |
|---|---|---|
| `Alloc` | `[n]` | `VirtualAlloc` |
| `FileOpen` | `[path_ptr]` | `CreateFileA` (read) |
| `FileRead` | `[handle, buf, cap]` | `ReadFile` |
| `FileClose` | `[handle]` | `CloseHandle` |
| `WriteStdout` | `[buf, len]` | `GetStdHandle` → `WriteFile` |
| `SpawnWait` | `[]` | `CreateProcessA` → `WaitForSingleObject` → `GetExitCodeProcess` → `CloseHandle` |
| `FileWrite` | `[path_ptr, buf, len]` | `CreateFileA` (write) → `WriteFile` → `CloseHandle` |

Each intent's real argument count is fixed by its Win32-call shape, not by anything an IR
self-declares — that contract lives in `Intent::contract_arity()` (`src/ir.rs`), independent
of `ExternDecl.nargs`. This is deliberate: see the "verification pipeline" section below.

## Verification pipeline

```
pack (build time)        store (content-addressed)      verify (produce time)         run (interpreter)
──────────────────       ─────────────────────────      ──────────────────────        ─────────────────
Module (in memory)  ──►  bytes, keyed by FNV-1a/64  ──►  structural gate (7 fault  ──► step-limited walk
serialize_module()       hash, on disk (store.rs)        classes, verify.rs) PLUS      of blocks/insts,
(pack.rs)                                                per-intent native-call-      each Call routed
                                                          contract check (F1 fix)      through seam.rs's
                                                                                       do_intent() to a
                                                                                       real Win32 API
```

- **pack** (`pack.rs`) serializes a `Module` to a fixed-width wire format and derives its
  content hash.
- **store** (`store.rs`) is a content-addressed blob store keyed by that hash — `put`/`get`
  only, no notion of a pack "name". Shared in spirit (not in code) with `agenterm-dynacore`'s
  own `store.rs`.
- **verify** (`verify.rs`) is a produce-time, execution-free gate. It runs two passes: one
  over `Module::externs` cross-checking each declared `nargs` against
  `Intent::contract_arity()` (the **F1 fix** — see `verify.rs`'s header for the exact panic
  this closes), then Q19's original structural walk (`NoBlocks`, `EntryOutOfRange`,
  `ValOutOfRange`, `BlockTargetOutOfRange`, `ExternIdOutOfRange`, `ArityMismatch`,
  `RodataOffsetOutOfRange`). Passing returns a `VerifiedModule` — the *only* way to obtain
  one, and the only thing `run` accepts.
- **run** (`eval_core.rs`) interprets a `VerifiedModule`, counting one step per block
  dispatched and aborting with `Termination::StepLimitExceeded` past `DEFAULT_MAX_STEPS`
  (2,000,000) rather than hanging the host thread on a real infinite loop. Every `Call`
  dispatches through `seam.rs::do_intent`, which reaches the real OS API — no mock, no
  simulation.

`declare.rs` covers the one piece of this that verification can't reach: raw struct layout
(`STARTUPINFOA`/`PROCESS_INFORMATION`) baked into the `SpawnWait`/`FileWrite` seam bindings.
Rather than baking those offsets and hoping, `declare.rs` self-checks them against a real
round trip through the OS (redirect a real child's stdout through a real pipe; hand a real
handle back to `WaitForSingleObject`/`GetExitCodeProcess`) — Q13's "bake-and-detect" pattern,
not a bare assertion.

## Quickstart

The easiest way to try this crate is the CLI example that ships in the *root* `agenterm`
product crate — **not** in this crate's own `examples/`. Run it from the workspace root:

```sh
cargo run --example nativecore_run -- --payload spawn_echo
```

(`examples/nativecore_run.rs` lives at the repo root, alongside `agenterm`'s own
`Cargo.toml` — `cargo run --example` must be invoked from there, not from inside
`crates/agenterm-nativecore/`.) It also accepts `pure_compute`, `read_hash_print`, and
`filewrite_demo` — the same four `payloads()` functions this crate ships for its own tests.

To drive the crate directly instead, here is the minimal real pipeline — build, pack, verify,
run — using one of the built-in demo payloads. (`pack::pack`/`pack::load`/`verify::verify`
each return their own distinct error type — `io::Result`, `Result<_, String>`, and
`Result<_, IrFault>` respectively — so a real caller handles each explicitly rather than
chaining `?` through a shared error type; the `.expect(...)`s below are illustrative,
matching the style `tests/native_intents.rs` itself uses):

```rust
use agenterm_nativecore::{eval_core, pack, payloads, store::Store, verify};

let dir = tempfile::tempdir().expect("tempdir");
let store = Store::open(dir.path()).expect("open store");

// build + pack + store round trip (never reuse the in-memory Module you built)
let manifest = pack::pack(&store, &payloads::spawn_echo()).expect("pack");
let module = pack::load(&store, &manifest).expect("load");

// produce-time gate: structural + F1 native-call-contract check
let verified = verify::verify(&module).expect("well-formed IR with a valid contract");

// interpret, step-limited; every SpawnWait call here is a real CreateProcessA
let outcome = eval_core::run(&verified);
println!("{:?}", outcome.termination); // Exited(7) — a real child process spawned and awaited
```

`crates/agenterm-nativecore/tests/native_intents.rs` is this crate's own black-box test
suite and a good source of further real examples — every test in it either drives this
exact pipeline against a real Win32 API or constructs a deliberately malformed `Module` and
asserts `verify()` rejects it before any native call happens.

## What this crate does NOT do (v1 scope)

Per [`plan/design-dynacore-native-core.md`](../../plan/design-dynacore-native-core.md) §6,
this is deliberate scope, not a backlog:

- No intents beyond the seven listed above — adding an eighth is explicitly out of scope for
  v1 (see the design doc's own open question, §8's Q23, on whether/how that could ever be
  done without recompiling `agenterm.exe`).
- No cross-ISA support (x86_64/Windows only).
- No struct-by-value calls beyond register width.
- No runtime intent discovery — packs are content-addressed and hash-pinned at build time,
  never resolved by name at load time.

## Relationship to `agenterm-dynacore`

`agenterm-nativecore` and `crates/agenterm-dynacore` are **two different, unrelated crates**
despite the confusingly similar directory names — this has been a real, live source of
confusion in this project more than once, so: `agenterm-dynacore` implements a fleet-call-routed
"logic pack" (its only host-call primitive is `fleet_call(operation_id, params_json)`, routed
through the product's `OPERATION_CATALOG`). `agenterm-nativecore` (this crate) does native
Win32 execution with raw memory ops. They do not share `Inst`/`Op` IR definitions and neither
depends on the other; `agenterm-nativecore` is this design track's *actual* `dynacore`
("真身") — `agenterm-dynacore`'s name is a historical accident pending a future rename
(design doc §7.4), not evidence that the two crates are related.
