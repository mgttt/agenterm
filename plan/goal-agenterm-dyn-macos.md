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
  `mach_timebase_info`, `pthread_main_np`, `getlogin_r`,
  `pthread_threadid_np`, `proc_pidinfo`, `_NSGetArgc`, `proc_pid_rusage`,
  `_dyld_image_count`.
- Darwin evidence (this host): `cargo test --locked -p agenterm-dyn` **121
  passed** twice (13 unit + 38 errors + 9 hosts + 16 language + 1
  macos_ioctl + 17 macos_probes + 4 macos_resource + 23 smoke).

## Wave 4 — shipped

Catalog is 49 rows. The three Wave 4 names are Darwin Live / Linux+Windows
Placeholder. `mach_host_self` stays last Placeholder. Honesty example has
no live-call lisp.

## Wave 5 — shipped

Catalog is 52 rows. `pthread_threadid_np`, `proc_pidinfo`, and
`_NSGetArgc` are Darwin Live / Linux+Windows Placeholder.
`mach_host_self` stays last Placeholder.

## Wave 6 — shipped

Catalog is 54 rows. `proc_pid_rusage` and `_dyld_image_count` are Darwin
Live / Linux+Windows Placeholder. `mach_host_self` stays last Placeholder.

## Next leak-free Darwin candidates

`_NSGetArgv` / `_NSGetEnviron` (CRT pointers, same family as `_NSGetArgc`).
Do not live-call Mach send rights.

## Non-goals

Windows live. JIT. General variadic FFI. `getloadavg`. Re-living
`mach_host_self`.
