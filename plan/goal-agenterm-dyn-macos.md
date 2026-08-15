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
  `proc_pidpath`, `arc4random`, `clock_gettime_nsec_np`, `sysctl`.

## Wave 4 DAG

```text
G  this standing goal
├── A  more C-header aliases      files: tests/errors.rs only
├── B  leak-free Darwin probes    files: src/hosts.rs (system_probes only),
│                                 tests/hosts.rs (system_probes asserts),
│                                 tests/macos_probes.rs (append),
│                                 examples/{mach-timebase-info,
│                                 pthread-main-np,getlogin-r}.md (new)
├── C  resource honesty           files: tests/macos_resource.rs (new),
│                                 examples/mach-host-self.md (new honesty)
└── I/P  primary README/PRD + test + commit + rebase + push
         then immediately open wave 5
```

Private dirs: `target/dyn-harden4`, `target/dyn-probes4`, `target/dyn-res4`.

## Leaves

### A — harden

Extend the existing C-alias reject list (same Type error, no `touched`,
no library load): `blkcnt_t`, `nlink_t`, `suseconds_t`, `useconds_t`,
`fsblkcnt_t`, `pthread_t`. Do not accept as aliases.

### B — probes (46 → 49)

Keep `mach_host_self` as Placeholder. Append three **leak-free** live rows:

| name | Darwin | Linux / Windows |
|------|--------|-----------------|
| `mach_timebase_info` | live `mach_timebase_info` | Placeholder |
| `pthread_main_np` | live `pthread_main_np` | Placeholder |
| `getlogin_r` | live `getlogin_r` | Placeholder |

Tests (`#[cfg(macos)]` append):

- `mach_timebase_info` writes caller-owned `{numer,denom}`; rc 0; both > 0;
  agree with a later libc/`mach` call.
- `pthread_main_np` returns 0 or 1 and matches `libc::pthread_main_np`.
- `getlogin_r` into a caller-owned 256-byte buffer; rc 0 or ERANGE-handled
  with a bigger buffer; C string matches `libc::getlogin_r`.

If a symbol is missing, omit that row. One example md per new live name.

### C — do not leak Mach rights

New `tests/macos_resource.rs`: assert catalog `mach_host_self` is
**Placeholder** on the live macOS cell; do **not** `dlcall` it.

Restore `examples/mach-host-self.md` as an honesty note: symbol exists,
dyn will not live-call it because the send right has no release path.
No “successful dlcall” lisp.

## Non-goals

Windows live. JIT. General variadic FFI. `getloadavg`. Re-living
`mach_host_self`.
