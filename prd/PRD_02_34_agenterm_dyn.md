# PRD 02.34 — agenterm-dyn（极小 / 动态 / 底层）

Status: parked 2026-08-15. Grok Bot Cursor quota exhausted; resume after reset (~20+ days).
Owner: 政委定方向；本云机 grok-bot 待命，不向军团要主刀。

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
  getdtablesize, getpagesize, …).
- Win/macOS six-cell rows stay `PLATFORM-CANDIDATE` placeholders.
- Tests grew ~62 → ~113. Low-risk dyn commits go straight to `main` with `[skip ci]`.

## Branches to resume (do these, in this order)

### 1. harden — keep the door small

More precise rejects before load/eval. Do not grow a type system.

Still useful: inclusive 255-byte accept vs 256 reject (if not already pinned),
unknown return/arg types that show up in real libc headers, and any hole where
a bad name or pair still loads a library.

### 2. probes — Linux live only, pointer buffers next

Integer/void libc rows are mostly filled. The missing useful ones need a
caller-owned buffer (`ptr`): `getcwd`, `uname`, `times`, `clock_gettime`.
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

Do not use Cursor cloud. Orchestrate local tmux Codex TUI on session `sljit`,
windows `harden` / `probes` / `examples`, worktrees
`/workspace/agenterm-dyn-{harden,probes,examples}`. Concurrent, not serial.
Push `[skip ci]` only when `origin/main...HEAD` is `0 1`.
