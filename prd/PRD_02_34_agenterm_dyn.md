# PRD 02.34 — agenterm-dyn（极小 / 动态 / 底层）

Status: active 2026-08-15. Low-risk resumption is green; remaining work stays deliberately small.
Owner: 政委定方向；主会话按独占文件域推进。

Parallel crate `crates/agenterm-dyn`, not a fourth engine, not libagenterm, not cu.

## What already landed on `main`

First cut is the body: S-expr + intern + `if` / `set` / `do` + comparisons +
fixnum `+` `-` + bounded `repeat` + one hand (`dlcall`).

- `dlcall` is a Rust integer/pointer trampoline + `libloading`. No C, no libffi.
- ioctl `TIOCGWINSZ` is the gate and is already in the crate (examples + smoke).
- Signature hardening on main: void, arity, empty/blank/overlong names,
  unknown types (`f32` / `struct` / `f64` / `u128` / `usize` / `isize` / `bool`).
- Linux live libc probes + paired S-expr examples (pid/uid/gid/pgid/sid/pgrp,
  sched/alarm, umask, descriptors, tty, access, sysconf pagesize, gethostid,
  getdtablesize, getpagesize, `times`, `getrusage`, …).
- 255-byte library/symbol names reach native processing; 256-byte names reject
  before loading or argument evaluation.
- Embedded NUL library and symbol names reject before loading or argument
  evaluation.
- Win/macOS six-cell rows stay `PLATFORM-CANDIDATE` placeholders.
- Tests grew ~62 → ~113. Low-risk dyn commits go straight to `main` with `[skip ci]`.

## Branches to resume (do these, in this order)

### 1. harden — keep the door small

More precise rejects before load/eval. Do not grow a type system.

Still useful: unknown return/arg types that show up in real libc headers, and
any hole where a bad name or pair still loads a library. The inclusive 255-byte
accept / 256-byte reject boundary is pinned.

### 2. probes — Linux live only, pointer buffers next

Integer/void libc rows are mostly filled. `getcwd`, `uname`, `times`, and
  `clock_gettime`, and `getrusage` now prove caller-owned buffers through `ptr`.
  Keep Win/macOS as placeholders. No C shim. Restore any process-global side
  effect before the test ends (`umask` is the pattern).

### 3. examples — pair every new live probe

One S-expr doc + README link per new probe. Do not duplicate existing
`examples/*.md`. No cu/platform wiring in the prose.

### 4. later (not ordered)

- Fold the intern tree to the host ISA in-process. That is the endgame.
- Absorb wasmbin only as export/pack (intern tree → `.wat` / `.wasm`, dlcall
  as import). Not a VM layer.
- Talk libagenterm merge only after this crate is mature.

## Non-goals until 政委 orders otherwise

- No JIT / sljit / DynASM / copy-and-patch.
- No lambda / cons / strings / quote.
- No cu or `agenterm-platform` import.
- No libffi, no C dependency, no fourth engine, no thickening libagenterm.

## How to run it when quota returns

Use managed local agent sessions from the repository root, with `harden`,
`probes`, and `examples` as exclusive file domains. Do not use worktrees.
Push `[skip ci]` only when `origin/main...HEAD` is `0 1`.
