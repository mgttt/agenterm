# Goal: first-class `agenterm-dyn` on macOS (standing)

Status: **active — do not stop between waves**  
Owner: this session (`agenterm-osx`) + exclusive-file subagents **in background**  
CWD: repository root. Paths: repo-relative or `~/...` only.  
Git identity: `agenterm-osx <agenterm@mgttt.com>`.

The human does **not** need to re-send `/goal`. After I/P, immediately
plan the next wave and spawn again until 政委 says stop.

## Outcome

Tiny intern/eval/`dlcall` door. Darwin is first-class and honest: live
integer/ptr facts, no leaked Mach rights / fds, ioctl only via the
loaded-symbol variadic gate. Windows stays placeholder.

Evidence every wave: `cargo test -p agenterm-dyn` on this aarch64-apple-darwin
host.

## Invariants

- No C, no libffi, no JIT, no lambda, no cu/platform import.
- Do not fake live probes on Linux/Windows.
- Do **not** live-call `mach_host_self` (send right, no release path).
  Keep it Placeholder.
- Do not allocate fds/ports without restore/close in the same test.
- Subagents do **not** commit. Primary owns I/P as `agenterm-osx`.
- Commit-pull-rebase-push on each coherent increment. Exact pathspec.
- Already shipped — do not redo: ioctl gate, `sysctlbyname`,
  `mach_absolute_time`, `getprogname`, `issetugid`, `_NSGetExecutablePath`,
  `proc_pidpath`, `arc4random`, `clock_gettime_nsec_np`, `sysctl`,
  `mach_timebase_info`, `pthread_main_np`, `getlogin_r`.
- Darwin evidence (this host): `cargo test --locked -p agenterm-dyn` **116
  passed** twice (13 unit + 38 errors + 9 hosts + 16 language + 1
  macos_ioctl + 12 macos_probes + 4 macos_resource + 23 smoke).

## Wave 4 — shipped

Catalog is 49 rows. The three Wave 4 names are Darwin Live / Linux+Windows
Placeholder. `mach_host_self` stays last Placeholder. Honesty example has
no live-call lisp.

## Wave 5 DAG

```text
G  this standing goal
├── A  (none) Wave 4 already closed the C-alias list
├── B  leak-free Darwin probes    files: src/hosts.rs (system_probes only),
│                                 tests/hosts.rs (system_probes asserts),
│                                 tests/macos_probes.rs (append),
│                                 examples/{pthread-threadid-np,
│                                 proc-pidinfo,nsget-argc}.md (new)
├── C  resource honesty           files: tests/macos_resource.rs
│                                 (keep mach_host_self last + no-dlcall scan)
└── I/P  primary README/PRD + test + commit + rebase + push
         then immediately open wave 6
```

Private dirs: `target/dyn-probes5`, `target/dyn-res5`.

## Leaves

### B — probes (49 → 52)

Keep `mach_host_self` last and Placeholder. Insert three **leak-free**
live rows immediately before it:

| name | Darwin | Linux / Windows |
|------|--------|-----------------|
| `pthread_threadid_np` | live `pthread_threadid_np` | Placeholder |
| `proc_pidinfo` | live `proc_pidinfo` | Placeholder |
| `nsget_argc` | live `_NSGetArgc` | Placeholder |

Tests (`#[cfg(macos)]` append):

- `pthread_threadid_np(NULL, &tid)` rc 0, `tid != 0`, matches later libc
  with a null thread pointer (do not spell `pthread_t` in the S-expr).
- `proc_pidinfo(getpid(), PROC_PIDTBSDINFO, …)` writes caller-owned
  `proc_bsdinfo`; returned size matches the struct; `pbi_pid`/`pbi_ppid`
  match `getpid`/`getppid` and a later libc call.
- `_NSGetArgc` returns a non-null `int*`; `*argc >= 1` and matches
  `libc::_NSGetArgc()`.

If a symbol is missing, omit that row. One example md per new live name.

### C — do not leak Mach rights

`tests/macos_resource.rs` must keep finding `mach_host_self` by name as
Placeholder on every cell and must keep the no-`dlcall` source scan.
Do not live-call Mach send-right symbols.

## Non-goals

Windows live. JIT. General variadic FFI. `getloadavg`. Re-living
`mach_host_self`.
