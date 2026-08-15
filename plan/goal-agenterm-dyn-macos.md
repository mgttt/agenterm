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
  `pthread_threadid_np`, `pthread_getname_np`, `proc_pidinfo`, `_NSGetArgc`,
  `_NSGetArgv`, `_NSGetEnviron`, `proc_pid_rusage`, `_dyld_image_count`,
  `getentropy`, `proc_name`, `pthread_get_stackaddr_np`,
  `pthread_get_stacksize_np`, `pthread_self`, `pthread_cpu_number_np`,
  `malloc_good_size`, `_NSGetProgname`, `proc_libversion`,
  `pthread_jit_write_protect_supported_np`, `sysctlnametomib`,
  `pthread_equal`, `gethostname`, `confstr`, `clock_getres`,
  `pthread_is_threaded_np`, `_NSGetMachExecuteHeader`,
  `_dyld_get_image_name`, `_dyld_get_image_vmaddr_slide`, `dladdr`,
  `gethostuuid`, `_dyld_get_image_header`.
- Darwin evidence (this host): `cargo test --locked -p agenterm-dyn` with
  `CARGO_TARGET_DIR=target/dyn-macos-docs8` **176 passed** (25 unit + 40
  errors + 11 hosts + 26 language + 1 macos_ioctl + 42 macos_probes + 4
  macos_resource + 27 smoke; 0 doctests).

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

## Wave 7 — shipped

Catalog is 76 rows. `gethostname`, `confstr`, `clock_getres`,
`pthread_is_threaded_np`, `_NSGetMachExecuteHeader`, `_dyld_get_image_name`,
and `_dyld_get_image_vmaddr_slide` are Darwin Live / Linux+Windows
Placeholder. `mach_host_self` stays last Placeholder.

## Wave 8 — shipped

Catalog is 79 rows. `dladdr`, `gethostuuid`, and `_dyld_get_image_header`
are Darwin Live / Linux+Windows Placeholder. `mach_host_self` stays last
Placeholder.

## Next leak-free Darwin candidates

`arc4random_uniform`, `getdomainname`, `statvfs` (leak-free; no Mach send
rights). Do not live-call Mach send rights. Do not pick
`os_proc_available_memory` — the macOS SDK marks that symbol unavailable.
Keep `mach_host_self` Placeholder.

## Non-goals

Windows live. JIT. General variadic FFI. `getloadavg`. Re-living
`mach_host_self`.
