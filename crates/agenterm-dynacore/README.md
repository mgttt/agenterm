# agenterm-dynacore

`agenterm-dynacore` is a **logic-pack** mechanism: a neutral, interpret-only intermediate
representation (IR), a produce-time well-formedness verification gate, a content-addressed
pack store, and a step-limited interpreter — whose only host-call primitive is
`fleet_call(operation_id, params_json)`. It lets a running `agenterm` process load, verify,
and execute a small piece of typed, agent- or tool-produced logic **without recompiling or
restarting the binary**, as long as that logic's only way to reach the host is through the
product's existing `fleet.*` operation catalog (`src/operations.rs::OPERATION_CATALOG` —
the same `fleet_call(operation_id, params_json)` binding shape the `rh`/`lua`/`qjs` script
engines already use). See
[`plan/design-dynacore-logic-pack.md`](../../plan/design-dynacore-logic-pack.md) for the
full product design, acceptance criteria, and scope decisions this README summarizes.

**Status: v1, in active hardening.** The core pipeline (pack → store → verify → run) is
implemented and covered by a black-box test suite exercising the real pipeline end to end —
no mocks of internal state. This crate's name is permanent: per the design doc, `dynacore` is
the confirmed long-term name for this crate specifically, not a placeholder pending a rename.

## What it does

The crate ports the useful half of `research/dynamic-core/` (Q1/Q3/Q9/Q18/Q19/Q21/Q22's
assembly) — a typed three-address IR, a structural verification gate, and an ISA-independent
interpreter — while deliberately cutting everything that research track needed only to reach
arbitrary native OS calls (raw-memory ops, a table of Win32-bound `Intent`s, cross-ISA
codegen). What's left is scoped to exactly one host-call primitive:

| Component | File | Role |
|---|---|---|
| Neutral IR | `ir.rs` | Typed, virtual-register three-address IR: literals, wrapping-u64 arithmetic (`Add`/`Sub`/`Mul`/`Xor`/`And`/`Or`/`Shl`/`Shr`/`Ult`), control flow (`Br`/`BrCond`/`Ret`/`Exit`), and exactly one way to call the host — `FleetCall(dest, extern_id)`, resolving to a pinned `(operation_id, params_json)` pair recorded in `Module::externs`. |
| Verification gate | `verify.rs` | Produce-time, execution-free structural walk (`IrFault`: `NoBlocks`, `EntryOutOfRange`, `ValOutOfRange`, `BlockTargetOutOfRange`, `ExternIdOutOfRange`) PLUS a per-`FleetCall` check against a caller-supplied operation catalog (`UnknownOperation`, `ParamsMismatch`) — the fix `research/dynamic-core/assembled/RESULTS.md`'s F1 finding named as missing: a `FleetCall` naming an operation the host doesn't recognize, or whose `params_json` doesn't match that operation's declared parameter schema, is rejected here, before `eval_core::run` ever sees it. Passing returns a `VerifiedModule` — the *only* way to obtain one, and the only thing `run` accepts. |
| Interpreter | `eval_core.rs` | Tree-walking interpreter over a `VerifiedModule`, counting one step per block dispatched and aborting with `Termination::StepLimitExceeded` past `DEFAULT_MAX_STEPS` (1,000,000) instead of hanging the host thread on a real infinite loop — a well-formed pack can still contain a back-edge that never falls out of a loop, and that is not something `verify()` can or should catch. Every `FleetCall` is routed through a caller-supplied bridge closure (`Fn(&str, &str) -> Result<String, String>`) and logged (operation_id, params_json, result) in `RunOutcome::calls`. |
| Content-addressed store | `store.rs` | `put`/`get` keyed by FNV-1a/64 hash of the content, no notion of a pack "name" at all (name→hash binding is pinned at build time by the caller, never resolved at load time — see the design doc §2's "no runtime discovery service"). `get` recomputes the hash and refuses to return content that doesn't match — corrupted or tampered blobs are rejected, never served silently. |
| Pack wire format + manifest | `pack.rs` | `serialize_module`/`deserialize_module`: a deliberately naive fixed-width wire format (no varint/compression — the property under test is "loaded from a store", not "a good wire format"). `pack()`/`load()` tie this to `store.rs`: build-time content-addressing, run-time fetch-by-hash. `PackManifest` is what a loader holds before ever touching pack bytes: the hash to fetch, plus the `operation_id`s the pack declares, so dependencies can be audited without deserializing or verifying first. |

## The pipeline

```
build (Module, in memory)  ──serialize──►  store (content-addressed, on disk)
        │                                          │
        │                                    load (by hash)
        │                                          ▼
        │                                  Module (deserialized)
        │                                          │
        │                                   verify(module, catalog)
        │                                          │
        │                              VerifiedModule  or  IrFault
        │                                          │
        └───────────────── only path to ──►  eval_core::run(verified, bridge)
                                                     │
                                              RunOutcome { termination, calls }
```

`verify()` is the only constructor for `VerifiedModule`, and `run`/`run_with_step_limit` are
the only functions that accept one — there is no code path from raw `pack::load` output to
`eval_core::run` that skips verification.

## Quickstart

```rust
use agenterm_dynacore::ir::{Builder, Term};
use agenterm_dynacore::pack;
use agenterm_dynacore::store::Store;
use agenterm_dynacore::verify::{self, OperationParamSchema, OperationSchema};

// A caller-supplied mirror of the host's real operation catalog (in
// production, `src/script_dynacore_host.rs::operation_catalog_schema()`
// builds this once from `OPERATION_CATALOG`; this crate has no dependency on
// that catalog and cannot reference it directly).
let catalog = vec![OperationSchema {
    id: "demo.echo".to_string(),
    available: true,
    parameters: vec![OperationParamSchema {
        name: "n".to_string(),
        value_type: "uint32".to_string(),
        required: true,
        minimum: Some(0),
        maximum: Some(100),
    }],
}];

// Build a tiny pack: call demo.echo with n=3, exit with its dest word
// (1 on Ok, 0 on Err).
let mut b = Builder::new();
let v = b.fleet_call("demo.echo", "{\"n\":3}");
b.term(Term::Exit(v));
let module = b.finish("demo_echo_pack", 0);

// Build time: content-address it into a store.
let dir = tempfile::tempdir().expect("tempdir");
let store = Store::open(dir.path()).expect("open store");
let manifest = pack::pack(&store, &module).expect("pack");

// Run time: fetch by hash, verify against the catalog, interpret.
let loaded = pack::load(&store, &manifest).expect("load");
let verified = verify::verify(&loaded, &catalog).expect("well-formed, catalog-valid pack");

let bridge = |operation_id: &str, params_json: &str| -> Result<String, String> {
    println!("fleet_call({operation_id}, {params_json})");
    Ok("{\"ok\":true}".to_string())
};
let outcome = agenterm_dynacore::eval_core::run(&verified, &bridge);
assert_eq!(outcome.result(), Some(1)); // FleetCall dest is 1 on Ok
```

`crates/agenterm-dynacore/tests/` is this crate's own black-box test suite and a good source
of further real examples:

- `pack_lifecycle.rs` — the core acceptance path (pack → store → load → verify → run against
  a real fleet call), plus tampered/absent-hash rejection and the step-limit safety net.
- `verify_faults.rs` — one deliberately malformed `Module` per `IrFault` variant, asserting
  rejection before any interpretation happens.
- `determinism_and_concurrency.rs` — repeated verify/run agreement, and concurrent/repeated
  loads against the same content-addressed store.

The real product-side host binding (`src/script_dynacore_host.rs`, in the root `agenterm`
crate, not in this crate) wires this pipeline to the real `OPERATION_CATALOG` and exposes a
runnable demo pack; see that file's `demo_pack_module` for a slightly richer example
(a real `fleet.tabs.list` call, branching on Ok/Err).

## What this crate does NOT do (v1 scope)

Per [`plan/design-dynacore-logic-pack.md`](../../plan/design-dynacore-logic-pack.md) §2, this
is deliberate scope, not a backlog:

- **No codegen/JIT backend.** `eval_core.rs`'s tree-walking interpreter is the only execution
  path. There is no compiler in this crate and it never requests executable memory.
- **No native OS call surface.** A pack's only way to reach the host is `fleet_call`, routed
  through the caller-supplied operation catalog. It cannot open a file, spawn a process, or
  touch raw memory directly — those capabilities exist only insofar as some `fleet.*`
  operation already exposes them, exactly like `rh`/`lua`/`qjs` scripts today. This is not a
  reduced capability set relative to the rest of the product; it is the same surface every
  script engine already has.
- **No cross-ISA support.** A pack is IR, interpreted the same way regardless of host ISA —
  there is no ISA-specific lowering in this crate to begin with, so "cross-ISA" is not a
  distinction this crate makes.
- **No runtime pack-discovery service.** `store::Store` only ever takes a hash the caller
  already has (`put`/`get`); there is no name→hash lookup, registry, or "find me a pack that
  does X" API. The design doc's verdict (from `research/dynamic-core/`'s Q18): discovery is a
  build-time problem, fully hoistable out of the runtime.
- **No signature/provenance authentication.** Content addressing gives *integrity* (`store.get`
  recomputes the hash and refuses to serve tampered content) — it does not give *authenticity*
  (who produced this content, and should it be trusted). The v1 trust boundary is "who can put
  a pack into the store directory the loader reads from," identical to the existing trust
  boundary for `.rh`/`.lua`/`.js` script files on disk. This is a known, deferred design
  question (design doc §5), not something this crate's hardening pass silently answered.
- **No expression language for consuming `fleet_call` results.** A `FleetCall`'s dest word is
  always exactly `1` (Ok) or `0` (Err) — nothing in `Op`/`Inst` parses the JSON result string
  into further `Val`s. A pack can branch on whether a call succeeded; it cannot yet act on
  *what* the call returned.

## Relationship to `agenterm-nativecore`

`agenterm-nativecore` and this crate (`agenterm-dynacore`) are **two independent, unrelated
crates**, despite both having been ported from the same `research/dynamic-core/` research
track and having superficially similar names. As of the current design doc
(`plan/design-dynacore-logic-pack.md`, 2026-08-09): **`agenterm-dynacore` keeps the name
"dynacore" permanently** — this is a settled decision, not pending a future rename — **and
`agenterm-nativecore` is archived** (its own design doc,
[`plan/archive/design-dynacore-native-core.md`](../../plan/archive/design-dynacore-native-core.md), records it
as feature-complete, 38 tests green, but no longer receiving investment or considered a
candidate for the "dynacore" name). If you find older material asserting the reverse
direction (that `agenterm-nativecore` is the "real" dynacore pending a rename of this crate),
treat that material as superseded by the design doc's explicit correction — this has been a
real, repeated source of confusion in this project and the design doc is the single source of
truth for which direction is current.

The two crates do not share `Op`/`Inst`/`Module` IR definitions, neither depends on the other,
and they solve different problems:

- **`agenterm-dynacore`** (this crate) implements a *fleet-call-routed logic pack* — its only
  host-call primitive is `fleet_call(operation_id, params_json)`, routed through the product's
  `OPERATION_CATALOG`. It has no notion of raw memory, native calling conventions, or OS
  handles at all.
- **`agenterm-nativecore`** implements *native Win32 execution* — literals, arithmetic, raw
  rodata/scratch memory, and calls that reach real Win32 APIs (`VirtualAlloc`, `CreateFileA`,
  `CreateProcessA`, …) directly, without a compiler in the loop and without executable memory.

Both crates independently ported the *shape* of a produce-time verification gate and a
content-addressed store from the same research track (a structural resemblance, not a code or
type dependency) — see each crate's own `verify.rs`/`store.rs` for the details specific to its
own IR and call surface.
