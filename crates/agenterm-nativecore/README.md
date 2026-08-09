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

**Status: ARCHIVED (2026-08-09).** This is not "dynacore" — see the disambiguation section
below; `crates/agenterm-dynacore` keeps that name permanently. All of the design doc's §5/§7/§9
acceptance criteria were met before archiving and the code stays in production use (opt-in,
zero cost when unconfigured), but it receives no further investment. The rest of this README
describes what exists, as a historical/reference record, not a roadmap. (Pre-archive framing,
kept accurate below: §8 froze new *intents* on the seven-intent path; §9 opened one narrow
exception, the **signature-registry-backed call path** described below, before the crate was
archived outright.)

<details>
<summary>Pre-archive status line (superseded by "ARCHIVED" above, kept for history)</summary>

All of the design doc's §5/§7 acceptance criteria are met and the code is in production use
(opt-in, zero cost when unconfigured); as of §8 the design doc records a deliberate decision to
stop adding *intents* to this crate and put research effort elsewhere (see "What this crate does
NOT do" below — that freeze still holds for the seven-intent path). §9 opened exactly one,
narrow exception on top of that freeze: a **signature-registry-backed call path** (see
"v2: registry-backed calls" below) that lets a pack reach a hand-reviewed *symbol* without
adding a new `Intent`. This is not a re-opening of v1's scope — no new `Intent`, no new
CLI surface, no product wiring — see §9.3 for the exact boundary.

</details>

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

## v2: registry-backed calls (design doc §9)

`research/dynamic-core/runtime-intent/RESULTS.md` (Q23) measured a real gap in the seven-intent
design: adding an eighth native call meant editing `ir.rs`/`verify.rs`/`seam.rs` and
recompiling `agenterm.exe`, yet what that recompile actually *bought* was never a machine
check — `Intent::contract_arity()` is a human-read-the-docs constant, same as any other. Q23's
verdict was a decisive split: the *internal-arity* half of that machine check moves cleanly to
pack-load time (**if and only if the contract is derived from the pack's own call recipe, never
declared as a second, disagreeable field**); the *real-ABI* half cannot, because Windows x64
exports carry no queryable signature metadata — closing it needs an independent, human-reviewed
assertion, the same thing recompilation was quietly buying all along.

This crate now ships that assertion as data instead of as a match arm: `src/registry.rs`'s
`SIGNATURE_REGISTRY`, a small, compiled-in, human-authored table of
`(symbol, module, real arity)` rows — currently three genuine kernel32 exports
(`MulDiv`, `lstrlenA`, `GetTickCount`), none of which is among the seven intents above. A pack
can reference one of these BY NAME (`Builder::call_reg`/`Inst::CallReg`, and the parallel
`Module::registry_externs` table), without adding a new compile-time `Intent` variant, without
touching `ir.rs`'s `Intent` enum, and without disturbing the seven-intent path in any way — the
two paths are fully separate: separate IR variant, separate `IrFault` variants, separate
dispatcher in `seam.rs` (`do_registry_call`, a generic `LoadLibraryA`/`GetProcAddress` +
transmute-dispatch trampoline over the integer/pointer word subset, 0..=4 args).

The contract-arity discipline is the same "derive, not declare" rule Q23 found necessary:
`verify()` never trusts a registry extern's own declared `nargs` as the real contract — it
looks the `(module, symbol)` pair up in `SIGNATURE_REGISTRY` and derives the contract from
THAT. Not found → rejected outright (`IrFault::SymbolNotInRegistry`, with an error message that
says plainly the symbol is not in the signature registry and cannot be called this way — a pack
cannot get an unreviewed symbol admitted no matter what arity it asserts for it). Found, but the
extern's declared `nargs` disagrees with the registry's arity → also rejected
(`IrFault::RegistryArityMismatch`), even if the declaration is perfectly self-consistent with
the rest of the IR — self-consistency was never the property that mattered (this is exactly the
F1-class hole Q23 named S4). `tests/registry_intents.rs` reproduces Q23's S1–S5 findings for
real, inside this crate, against real kernel32 exports.

**This is a mechanism proof, not a distribution/signing pipeline** — `registry.rs`'s table is
compiled in, human-reviewed via ordinary code review, and not runtime-updatable; extending it to
a new symbol is a human decision (design doc §9.3 explicitly defers hot-reload, remote
distribution, and signing to a future round, if ever). It also does not touch product wiring:
`execute_inner` and the rest of `agenterm`'s command surface are unaware this path exists, same
as the seven-intent path.

## What this crate does NOT do (v1 scope)

Per [`plan/design-dynacore-native-core.md`](../../plan/design-dynacore-native-core.md) §6,
this is deliberate scope, not a backlog:

- No EIGHTH compile-time `Intent` — still true; the seven listed above are the entire
  `Intent` enum and stay that way. The v2 registry-backed path above (§9) answers the design
  doc's own open question (§8's Q23) about extending native-call reach WITHOUT a ninth
  `Intent`/recompile, but it is a genuinely separate, additive path — it does not add an
  eighth `Intent` variant, and the seven-intent path's own scope (below) is unaffected by it.
- No cross-ISA support (x86_64/Windows only) — applies to both paths.
- No struct-by-value calls beyond register width — applies to both paths; the registry path
  is further bounded to 0..=4 integer/pointer-width args (design doc §9.3).
- No runtime intent discovery for the seven-intent path — packs are content-addressed and
  hash-pinned at build time, never resolved by name at load time. (The registry path's whole
  point is resolving a NAME at load/dispatch time — that is deliberately scoped to a small,
  human-reviewed table, §9's "human review" step, not general runtime discovery of arbitrary
  symbols.)
- No registry hot-reload, remote distribution, or signing pipeline (design doc §9.3) — the
  registry is compiled into this crate and reviewed the same way any other source change is.

## Relationship to `agenterm-dynacore`

`agenterm-nativecore` and `crates/agenterm-dynacore` are **two different, unrelated crates**
despite the confusingly similar directory names — this has been a real, live source of
confusion in this project more than once, so, settled plainly (2026-08-09, no longer pending):
`agenterm-dynacore` implements a fleet-call-routed "logic pack" (its only host-call primitive is
`fleet_call(operation_id, params_json)`, routed through the product's `OPERATION_CATALOG`).
`agenterm-nativecore` (this crate) does native Win32 execution with raw memory ops. They do not
share `Inst`/`Op` IR definitions and neither depends on the other.

**`agenterm-dynacore` keeps the "dynacore" name permanently.** An earlier version of this
document called this crate "the actual dynacore" pending a rename — that plan was reversed:
the repeated confusion between the two names cost more than a rename would have saved, so
`agenterm-dynacore` was confirmed as the one true "dynacore" and **this crate
(`agenterm-nativecore`) is archived** (code stays, tested, opt-in, zero cost when unconfigured,
no further feature investment). If you are looking for "dynacore", you want
`crates/agenterm-dynacore`, not this crate.
