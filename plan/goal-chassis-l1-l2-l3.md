# Goal: freeze a thin Chassis-L1 so development stops waiting on six-cell CI

Status: **active**  
Execution: [`refactor-chassis-l1-l2-l3.md`](refactor-chassis-l1-l2-l3.md)  
Surface: [`chassis-l1-surface.json`](chassis-l1-surface.json)  
Paths: repository-relative or `~/...` only.

## 0.1.16 parallel DAG

1. `sync` (primary): confirm clean `main` at the unpublished 0.1.16 baseline.
2. `identity-pack` (lane A): own release/pack entrypoints and focused tests; depends on `sync`.
3. `l1-gate` (lane B): own L1-change classification and focused tests; depends on `sync`.
4. `l2-artifact` (lane C): own `crates/agenterm-chassis/l2/**` plus new isolated tests; depends on `sync`.
5. `catalog` (lane C): classify Fleet/tab/clipboard/CC/CU as L2; CU stays a rare native plugin.
6. `workbench-loader` (later lane): own shared frontend image-load contract, then both adapters; depends on image format stability.
7. `docs-contract` (primary): own PRD/architecture/status convergence after behavior is proven.
8. `review` (primary): inspect each unstaged handoff and reject scope/file-owner overlap.
9. `verify` (primary): run redaction, `./lint.sh`, targeted chassis tests, and compose/pack evidence serially.
10. `deliver` (primary): small pathspec commits, then pull-rebase and push each green slice.

**Chassis** = the product frame you bolt packs onto. It is not a command-line
shell and not the script L1/L2/L3 boundary.

## Outcome

Chassis-L1 is thin and named. Six-cell Candidate runs because L1
changed, not because an app, a catalog row, or a cu hand changed.

Apps (Chassis-L3) call a versioned Host ABI (Chassis-L2). L2 updates do not rebuild
the Base PE. Daily work packs frozen L1 loaders with L2/L3; it does not rustc
the workspace.

## L2 execution (decision)

L2 is stronger as a **tiny custom-ISA AOT → bytecode → bounded VM**. Not:

- libtcc / an embedded C compiler
- rustc of L2 on the daily path
- cranelift / LLVM JIT
- dyn `dlcall` from L3

Why: tcc-like *economics* (small, fast turnaround, tiny size) without pulling
a C compiler into the chassis. rustc remains only for Chassis-L1 (rare,
six-cell) and rare native L2 plugins (cu).

## Done

- W0: L1 path surface named.
- W1: plan states the pack-not-compile loop; `scripts/chassis-compose-product.py`
  copies frozen six-cell L1 loaders plus L2/L3 into a deterministic archive
  without cargo; `scripts/chassis-compose-product-test.py` proves L1 SHA
  identity, determinism, and a PATH with no working cargo.
- Rename: Shell-L* → Chassis-L*.
- Independent `crates/agenterm-chassis`: compose/check/inspect, frozen L2
  host ABI, example L3 app. No workbench dependency.
- W2b: bounded L2 bytecode VM with validation and fail-closed execution.
- Versioned Host ABI plus the L2 catalog classify Fleet/tab/clipboard/Control
  Center/computer-use at L2; `active-tab` is the first real replaceable L2
  artifact.
- Both workbench adapters fail closed on an invalid product image before
  presentation. The standalone `agenterm-chassis-loader` validates a composed
  image and then hands presentation to the native host.

This is a partial substrate. The live `agenterm` workbench PE is not yet
replaced by these loaders, and PTY/IPC/L2 Host ABI dispatch has not migrated.

## Next (later waves)

- Replace the live workbench PE path and migrate PTY/IPC/L2 Host ABI dispatch.
- Connect the replaceable L2 artifacts to production dispatch, then adopt the
  planned v0.1.18 portable QJS `.agp` as L3.

## Non-goals

Electron in the official PE. App-level `dlcall`. A second platform inside dyn.
Renaming the script L1/L2/L3 documents away. libtcc / embedded C as L2.
cranelift / LLVM JIT. rustc of L2 on the daily path.
