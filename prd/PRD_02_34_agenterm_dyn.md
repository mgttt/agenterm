# PRD 02.34 — agenterm-dyn（极小 / 动态 / 底层）

Status: current authorized scope complete 2026-08-15. Further product work requires 政委 authorization.
Owner: 政委定方向；主会话按独占文件域推进。

Parallel crate `crates/agenterm-dyn`, not a fourth engine, not libagenterm, not cu.

## Completed authorized scope on `main`

First cut is the body: S-expr + intern + `if` / `set` / `do` + comparisons +
fixnum `+` `-` + bounded `repeat` + one hand (`dlcall`).

- `dlcall` is a Rust integer/pointer trampoline + `libloading`. No C, no libffi.
- ioctl `TIOCGWINSZ` is the gate and is already in the crate (examples + smoke).
- Signature hardening on main: void, arity, empty/blank/overlong names,
  unknown types (`f32` / `struct` / `f64` / `u128` / `usize` / `isize` / `bool`).
- Linux live libc probes + paired S-expr examples (pid/uid/gid/pgid/sid/pgrp,
  sched/alarm, umask, descriptors, tty, access, sysconf pagesize, gethostid,
  getdtablesize, getpagesize, `times`, `getrusage`, `getrlimit`, …).
- 255-byte library/symbol names reach native processing; 256-byte names reject
  before loading or argument evaluation.
- Embedded NUL library and symbol names reject before loading or argument
  evaluation.
- C spelling aliases outside the fixed-width ABI whitelist reject before
  loading or argument evaluation.
- The parser accepts exactly 256 nested lists; 257 nested lists return
  `DynError::Parse` before evaluation. This is a stack-resource bound, not a
  caller-policy boundary.
- Win six-cell extra probes stay placeholders. **macOS** has the shared
  fixed-ABI live libc rows plus `sysctlbyname`, `mach_absolute_time`, `getprogname`,
  `issetugid`, `_NSGetExecutablePath`, `proc_pidpath`, `arc4random`,
  `clock_gettime_nsec_np`, `sysctl`, `mach_timebase_info`, `pthread_main_np`,
  and `getlogin_r` against `libSystem.B.dylib`.
  `mach_host_self` stays a placeholder because dyn has no ownership-aware
  release path for its send right. Darwin `ioctl` calls its resolved symbol through a
  signature-gated Rust variadic path for `(i32, u64|i32, ptr) -> i32`, not
  general variadic FFI. CU-adjacent macOS notes name AX as a cu live hand.
- Current Linux evidence is `cargo test --locked -p agenterm-dyn` with Rust
  1.97: **121 passed** (12 unit + 38 errors + 9 hosts + 16 language + 46
  cfg-gated Linux smoke; 0 doctests). Current Darwin evidence on this
  aarch64-apple-darwin host: **116 passed** (13 unit + 38 errors + 9 hosts +
  16 language + 1 macos_ioctl + 12 macos_probes + 4 macos_resource + 23
  cfg-gated macOS smoke; 0 doctests). The Wave 4 `mach_timebase_info`,
  `pthread_main_np`, and `getlogin_r` rows were live-`dlcall`ed here and
  compared to later native calls. Host-specific counts, not a cross-platform
  estimate.

## Completed branch accounting

### first cut

Implemented: intern/eval/list language, bounded repeat, and fixed-width
integer/pointer `dlcall` with no C or libffi dependency.

### harden

Implemented: signature/name rejection before load or argument evaluation,
including the pinned 255-byte accept / 256-byte reject name boundary and the
256-list accept / 257-list parse-reject boundary. The shipped ABI remains
deliberately small; this does not authorize a broader type system.

### probes

Integer/void/ptr libc rows are live on Linux (`libc.so.6`) and macOS
(`libSystem.B.dylib`); macOS additionally covers `sysctlbyname`,
`mach_absolute_time`, `getprogname`, `issetugid`, `_NSGetExecutablePath`,
`proc_pidpath`, `arc4random`, `clock_gettime_nsec_np`, `sysctl`,
`mach_timebase_info`, `pthread_main_np`, and `getlogin_r`.
`mach_host_self` remains a placeholder because dyn cannot release its returned
Mach send right. Windows extra probes stay placeholders. No
C shim.
Restore process-global side effects before the test ends (`umask` pattern).
Darwin `ioctl` transmutes its already-resolved symbol only for the validated
`(i32, u64|i32, ptr) -> i32` signature; the fixed trampoline remains for every
other call and this does not authorize general variadic FFI.
Linux caller-owned `ptr` coverage includes `getcwd`, `uname`, `times`,
`clock_gettime`, `getrusage`, and `getrlimit`.

### examples

Each shipped live probe has its paired S-expr documentation and README link.
The prose adds no cu or platform wiring.

## Later — not authorized or implemented

- Fold the intern tree to the host ISA in-process (endgame).
- Absorb wasmbin only as export/pack (intern tree → `.wat` / `.wasm`, `dlcall`
  as import), not as a VM layer.
- Consider libagenterm merge only after this crate is mature.

These are open product decisions, not scheduled branches and not evidence of
implemented functionality. Do not begin them without explicit 政委 direction.

## Non-goals until 政委 orders otherwise

- No JIT / sljit / DynASM / copy-and-patch.
- No lambda / cons / strings / quote.
- No cu or `agenterm-platform` import.
- No libffi, no C dependency, no fourth engine, no thickening libagenterm.

## If a new authorized increment is opened

Use managed local agent sessions from the repository root, with `harden`,
`probes`, and `examples` as exclusive file domains. Do not use worktrees.
Push `[skip ci]` only when `origin/main...HEAD` is `0 1`.
