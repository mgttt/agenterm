# Goal: first-class `agenterm-dyn` on macOS

Status: active  
Owner: this session (primary) + exclusive-file subagents  
CWD: repository root. Paths: repo-relative or `~/...` only.

## Outcome

`agenterm-dyn` stays a tiny intern/eval/`dlcall` door. On Darwin it is no
longer a Linux copy with a shrug: matching-host smoke is green here, Darwin
facts that are actually useful are live, and the `ioctl` ABI hole is either
fixed in Rust or named as a typed limit — not “maybe −1”.

Evidence: `cargo test -p agenterm-dyn` on this aarch64-apple-darwin host.

## Invariants

- No C source, no libffi, no JIT, no lambda/cons/strings, no cu/platform import.
- OS names stay **script data**. Host tables stay six-cell explicit.
- Restore process-global side effects before a test ends (`umask` pattern).
- Do not fake a live probe on the wrong OS.
- Commit-pull-rebase-push on each coherent increment. Exact pathspec. No `git add -A`.
- Subagents do **not** commit or push. Primary owns I/P.

## Current facts (do not rediscover)

- `system_probes` is `[SystemProbe; 36]`. Linux + macOS rows are live; Windows
  placeholders. `size_t` / `ssize_t` / `int` / `long` already reject in
  `tests/errors.rs`.
- Darwin `ioctl(int, unsigned long, ...)` is variadic. Fixed-arity trampoline
  on arm64 often returns `-1`/`EFAULT`. Smoke only asserts the symbol resolves.
- Cheatsheet: `docs/agenterm-rust-cheatsheet.md` § Darwin ioctl. PRD:
  `prd/PRD_02_34_agenterm_dyn.md`.

## DAG (independent leaves first)

```text
G  write this goal
├── A  harden extra C-header spellings     files: tests/errors.rs only
├── B  Darwin-only live probes + examples  files: src/hosts.rs,
│                                          tests/hosts.rs,
│                                          tests/macos_probes.rs (new),
│                                          examples/{sysctlbyname,mach-absolute-time,
│                                          getprogname,issetugid}.md (new),
│                                          README.md (table + example links only)
├── C  Darwin ioctl variadic path          files: src/native.rs,
│                                          tests/macos_ioctl.rs (new)
└── I  integrate: rustfmt + cargo test -p agenterm-dyn
    └── P  commit / pull --rebase / push
```

A, B, C share **no** writable files. Each uses a private `CARGO_TARGET_DIR`
(`target/dyn-harden`, `target/dyn-probes`, `target/dyn-ioctl`). Primary owns
I/P and later docs (`prd/PRD_02_34_agenterm_dyn.md`, cheatsheet ioctl
paragraph) after C reports success or stop.

Do **not** edit `tests/smoke.rs`. Existing Linux/macOS/Windows smoke stays.

## Leaves

### A — harden (small door)

Reject more Darwin/Linux header spellings before load (do not accept as aliases):

`off_t`, `mode_t`, `pid_t`, `uid_t`, `gid_t`, `time_t`, `socklen_t`, `nfds_t`

Same shape as existing `dlcall_rejects_c_abi_aliases_before_arguments_or_library_load`:
type error, no `touched` mutation, no library load. Extend that test's list.

### B — Darwin facts that are not Linux clones

Grow `system_probes` from 36 → 40 on **all** six cells. New names (same order
on every OS):

| name | Darwin | Linux / Windows |
|------|--------|-----------------|
| `sysctlbyname` | live `libSystem.B.dylib` / `sysctlbyname` | Placeholder |
| `mach_absolute_time` | live `mach_absolute_time` | Placeholder |
| `getprogname` | live `getprogname` | Placeholder |
| `issetugid` | live `issetugid` | Placeholder |

Keep the first 36 Linux rows live. First 36 macOS rows stay live. First 36
Windows rows stay placeholders. Update `tests/hosts.rs` name/status assertions
so Linux is no longer “every slot live”.

Live smoke goes in **new** `tests/macos_probes.rs` (`#[cfg(target_os = "macos")]`):

- `sysctlbyname("hw.ncpu")` into a caller-owned buffer; `ncpu >= 1`; agree
  with `libc::sysctlbyname` or `std::thread::available_parallelism`.
- `mach_absolute_time` returns `u64` (via `i64` if it fits, else treat as
  monotonic and compare two calls); later libc/dlcall tick is not before the
  first.
- `getprogname` returns a non-null `ptr`; C string matches `libc::getprogname`.
- `issetugid` returns 0 or 1 and matches `libc::issetugid`.

One `examples/*.md` + README link per new live name. Do not rewrite Linux
smoke or existing examples.

### C — ioctl on Darwin

Fixed-arity trampoline ≠ Darwin `ioctl(int, unsigned long, ...)`.

Allowed fix, still no C file / no libffi: in `eval_dlcall`, if
`cfg(target_os = "macos")` and `symbol == "ioctl"` and the signature is
`(i32, u64|i32, ptr) -> i32`, invoke through
`unsafe extern "C" fn(i32, u64, ...) -> i32` (Apple arm64 puts unnamed
variadic args on the stack). Keep the general trampoline for every other
symbol. Do not change Linux invoke.

Evidence: `tests/macos_ioctl.rs` — `openpty` 24×80 on the **slave**,
`TIOCGWINSZ` **succeeds**, rows/cols match. If the variadic transmute still
EFAULT/returns −1, **stop**, leave this test as “symbol resolves, no success
claim”, and do not add a C shim.

### I / P

Primary integrates, `cargo fmt` + `cargo test -p agenterm-dyn`, exact-path
commit, rebase `origin/main`, push. Repeat after each landed leaf if isolated.

## Non-goals

Windows live rows. JIT. cu/platform wiring. libagenterm merge. Growing
`dlcall` into a general variadic FFI. Editing `tests/smoke.rs`.
