# Standby — remaining branches

Parked 2026-08-15. Grok Bot Cursor quota is exhausted; resume after reset.

Product remaining-work lives in [`prd/PRD_02_34_agenterm_dyn.md`](../../../prd/PRD_02_34_agenterm_dyn.md).
This note is the crate-side pointer so the next knife does not start from chat.

## Resume in this order

1. **harden** — more signature/name rejects before load/eval. Door stays small.
   Void, arity, empty/blank/overlong names, and unknown types
   (`f32` / `struct` / `f64` / `u128` / `usize` / `isize` / `bool`) are already on `main`.
2. **probes** — Linux live only. Integer/void libc rows are mostly filled.
   Next useful ones need a caller-owned `ptr` buffer: `getcwd`, `uname`,
   `times`, `clock_gettime`. Restore process-global side effects (`umask` pattern).
3. **examples** — one S-expr doc + README link per new live probe.

## Later (not ordered)

Fold the intern tree to the host ISA. wasmbin only as `.wat` / `.wasm` export,
not a VM. Talk libagenterm merge only after this crate is mature.

## Still locked

No C/libffi. No JIT/sljit. No lambda/cons/strings. No cu/platform wiring.
No thickening libagenterm. Low-risk dyn commits go to `main` with `[skip ci]`.
