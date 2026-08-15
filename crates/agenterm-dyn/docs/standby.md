# Standby — authorized scope closed; later work awaits direction

Updated 2026-08-15. The current authorized first-cut, harden, probes, and
examples scope is complete on `main`; do not infer an active implementation
task from this note.

Product remaining-work lives in [`prd/PRD_02_34_agenterm_dyn.md`](../../../prd/PRD_02_34_agenterm_dyn.md).
This note is the crate-side pointer so the next knife does not start from chat.

## Completed baseline

1. **first cut** — S-expr/intern/eval and fixed-width integer/pointer
   `dlcall` are shipped without C, libffi, or a fourth engine.
2. **harden** — void/arity/empty/blank/overlong/NUL names, C spelling aliases,
   and unsupported types (`f32` / `struct` / `f64` / `u128` / `usize` /
   `isize` / `bool`) reject before load/eval. Names accept 255 bytes and
   reject 256 bytes. The parser accepts 256 nested lists and rejects 257 with
   `DynError::Parse` before evaluation.
3. **probes** — Linux and macOS integer/void/ptr libc rows are live; Windows
   extra probes remain explicit placeholders. Unix `ioctl` (Linux and macOS) remains variadic
   script data, not a claimed fixed-trampoline success. `umask` restores its
   side effect. Linux caller-owned-pointer coverage includes `getcwd`, `uname`,
   `times`, `clock_gettime`, `getrusage`, and `getrlimit`.
4. **examples** — each shipped live probe has its paired S-expr document and
   README link.

Current Linux evidence, not a portable estimate: Rust 1.97
`cargo test --locked -p agenterm-dyn` passes **141** tests (21 unit, 39 errors,
11 hosts, 22 language, 48 Linux smoke; 0 doctests).

## Later — requires explicit product authorization

Fold the intern tree to the host ISA; use wasmbin only as `.wat` / `.wasm`
export, not a VM; and consider libagenterm merge only when the crate is mature.
None is implemented, scheduled, or implicitly authorized.

## Still locked

No C/libffi. No JIT/sljit. No lambda/cons/strings. No cu/platform wiring.
No thickening libagenterm. Low-risk dyn commits go to `main` with `[skip ci]`.
