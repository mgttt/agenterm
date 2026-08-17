# PRD 02.35 — agenterm-tinyvm（WASM 1.0 解释核）

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module is the product contract for the parallel crate
`crates/agenterm-tinyvm`. It is **not** an appendix of
[34 agenterm-dyn](PRD_02_34_agenterm_dyn.md), not a Script engine, and not a
`cu` / chassis surface.

Status: active product node — slot A (WebAssembly 1.0 interpret) is the
authorized face. Slot B is deferred.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Product outcome

- [x] one public face: `eval(bytes) →` a typed value sequence or a loud
  error. The program is a standard WebAssembly 1.0 module (magic `\0asm`,
  version 1). WAT exists only for humans to read; it is not an input.
- [x] slot A interprets the WebAssembly 1.0 MVP instruction index — all
  **172** opcodes, including `else` and `end`. Dual-green is required:
  in-crate checks plus an independent fixture suite whose expected values
  are spec-derived, not snapshots of this interpreter.
- [ ] slot B (desktop dyn / AOT acceleration of the same face) is parked.
  This module does not authorize opening it.
- [x] the host door is an **import table** (`module.field` → function), not
  a general FFI, not `dlcall`, and not a native ISA emitter.

The product succeeds when a caller can hand the crate standard `.wasm`
bytes, optionally bind named imports, and get a value or a trap/decode
error without linking a third-party WASM runtime.

## Frozen identity

These terms are frozen. Do not “clarify” them into a second language, a
second face, or a packaging project.

1. **One face.** `eval(bytes) →` value or error. Loading a module and
   invoking its start / first export / first defined function is that face,
   not a second product. The historical toy `Instr` / text-asm `Vm` remains
   a crate-local leftover; it is not slot A and must not grow a second
   language.
2. **Slot A = WebAssembly 1.0 interpret.** All 172 MVP opcodes. No WASM
   2.0, SIMD, threads, WASI, or multi-memory in this module.
3. **Slot B (dyn / AOT) is later.** This knife does not open it.
4. **Program bytes = standard `.wasm`.** WAT is documentation / review
   material only.
5. **Host door = import table.** A module names `module.field` functions;
   the host binds those names. Unbound imports trap. This is not a
   large FFI, not libffi, and not dyn `dlcall`.
6. **The kernel stays thin and size-bounded.** `staticcore` stripped
   executable `< 100 KiB` (`crates/agenterm-tinyvm/measure-core.sh`,
   selftest rc `42`). Sidecars (fixture suites, WAT notes, measurement
   scripts) may grow; the interpret core must not absorb them.
7. **Multi-arch / APE is delivery, not the engine.** Do not fold packaging,
   fat binaries, or host-ISA wrappers into the interpret kernel.

## Product boundary

### Shipped and owned here

- Decode and interpret of WebAssembly 1.0 MVP function bodies and a
  minimal module (type / import / function / table / memory-less default
  page / global / export / start / elem / code).
- Loud decode and trap errors (`WasmError`), never silent corruption or an
  unbounded spin (step and depth budgets).
- Host functions registered against the import table, with linear memory
  visible to the host callback.
- The size-measurement static core (`--features staticcore`) and its
  `tinyvm_selftest` entry.

### Explicit non-goals

- [x] do **not** merge into `agenterm-cu`.
- [x] do **not** touch `crates/agenterm-dyn`, `crates/agenterm-chassis`, or
  GitHub `#78`.
- [x] do **not** vendor or link wasmtime, wasmi, sljit, nano, nano-fg3, or
  wasmbin source. Absorb conclusions only.
- [x] no JIT / AOT / host-ISA folding in this module (that is slot B, later).
- [x] WAT is not an input language; do not grow a WAT parser in the kernel.
- [x] no WASI, no preview1 syscall shim, no general libc trampoline.
- [x] multi-arch / APE / packaging wrappers are not kernel work.

## Governing invariants

- A malformed module fails to decode. A run-time fault traps. The
  interpreter does not invent a success value on either path.
- Import indices occupy the low function-index space. An imported function
  that has not been bound traps with a distinct unbound error rather than
  executing a guest body.
- Dual-green: a change that only makes the in-crate examples pass, or only
  makes the independent fixtures pass, is not done.
- The stripped static core stays under 100 KiB. Evidence is
  `measure-core.sh`, not a full-workspace `check`.
- `no_std` + `alloc` on the library path (except tests). No `fmt` in the
  interpret core. Errors are `&'static str` codes.

## Observable success evidence

- Independent fixtures live outside `src/wasm.rs` (crate `tests/` /
  `tests/fixtures/`). They enumerate all 172 MVP opcodes plus a host-import
  bind and an unbound-import case.
- Each opcode family (control, parametric, locals/globals, memory, i32,
  i64, f32, f64, conversions, host import) has at least one golden whose
  operands, stack depth, memory layout, or control flow differ from the
  matching in-crate `#[test]` examples.
- `cargo test -p agenterm-tinyvm` is green, including the independent
  suite.
- `crates/agenterm-tinyvm/measure-core.sh` prints
  `OK: < 100 KiB and selftest==42`.

## Safe failure

- Decode failure and trap are the only unsuccessful results of the face.
- Unbound import, stack underflow, divide-by-zero / overflow traps, out of
  bounds memory, unknown function, and budget/depth overrun all trap.
- The crate never links dyn, cu, or chassis in order to “fail closed”.

## Relationship to other modules

- [34](PRD_02_34_agenterm_dyn.md) remains the intern / `dlcall` native
  door. tinyvm does not become a layer of dyn, and dyn does not grow this
  interpreter.
- [28](PRD_02_28_agenterm_cu.md) and chassis are out of scope.
- [10](PRD_02_10_rhai_scripting.md) is the Script engine family. This crate
  is not a fourth script engine and does not take Rhai/rh/qjs policy.
- [18](PRD_02_18_roadmap.md) records the parked-crate row owned by this
  file.

## Absorbed conclusions (not source)

Copied as product rules, not as code:

- **wasmbin:** the program is standard WASM bytes; WAT is for humans; the
  host door is an import table, not a kitchen-sink FFI.
- **nano:** the kernel stays thin so sidecars can grow; dual-green is what
  counts as an interpreter.
- **nano-fg3:** multi-architecture / APE is a delivery problem, not an
  engine problem. Do not build wrapping into the core.
