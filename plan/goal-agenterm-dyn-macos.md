# Goal: first-class `agenterm-dyn` on macOS

Status: **active** — user re-authorized continuing past Wave 9
Execution: Wave 10 shipped (catalog 85); next leak-free candidates below.
Paths: repository-relative or `~/...` only.

## Outcome

Tiny intern/eval/`dlcall` door. Darwin is first-class and honest: live
integer/ptr facts, no leaked Mach rights / fds, ioctl only via the
loaded-symbol variadic gate. Windows stays placeholder.

Native Darwin evidence is required for a wave to ship; compilation on another
host is not a substitute.

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
  `gethostuuid`, `_dyld_get_image_header`, `arc4random_uniform`,
  `getdomainname`, `statvfs`, `gettimeofday`, `getgroups`, `realpath`.
- Wave 9 Darwin evidence: GitHub Actions `CI / agenterm` success run
  [31873334933](https://github.com/mgttt/agenterm/actions/runs/31873334933)
  at SHA `36e80aa9`, which contains Wave 9 commit `49d8c9af` as an ancestor.
  Native `aarch64-apple-darwin` and `x86_64-apple-darwin` jobs each reported
  **182 passed, 0 failed**: 25 unit + 3 catalog/docs + 40 errors + 11 hosts +
  26 language + 1 macos_ioctl + 45 macos_probes + 4 macos_resource + 27
  cfg-gated macOS smoke; 0 doctests.

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

## Wave 9 — shipped

Catalog is 82 rows. `arc4random_uniform`, `getdomainname`, and `statvfs`
are Darwin Live / Linux+Windows Placeholder. `arc4random_uniform` is checked
by its bound rather than equality across random calls; `getdomainname` uses
Darwin's `i32` length and independent caller buffers; `statvfs` compares only
stable filesystem fields because capacity counters can change between calls.
The portable catalog/documentation gate runs on every host. In success run
[31873334933](https://github.com/mgttt/agenterm/actions/runs/31873334933), both
Darwin architectures reported
`dlcall_arc4random_uniform_respects_each_upper_bound`,
`dlcall_getdomainname_matches_independent_caller_buffer`, and
`dlcall_statvfs_matches_stable_root_filesystem_fields` as `ok`. Windows
compilation is not live evidence. Keep `mach_host_self` last and Placeholder.

Do not live-call Mach send rights. Do not pick `os_proc_available_memory` —
the macOS SDK marks that symbol unavailable.

## Wave 10 — shipped

Catalog is 85 rows. `gettimeofday`, `getgroups`, and `realpath` are Darwin
Live / Linux+Windows Placeholder. `gettimeofday` writes a caller-owned
`timeval` and uses a null timezone pointer; `getgroups` fills a caller-owned
`gid_t` array up to the bound capacity; `realpath` writes a caller-owned
`PATH_MAX` buffer. None allocates a descriptor or Mach right.
`mach_host_self` stays last and Placeholder. Measured on this
aarch64-apple-darwin host: **185 passed** (25 unit + 3 catalog/docs + 40
errors + 11 hosts + 26 language + 1 macos_ioctl + 48 macos_probes + 4
macos_resource + 27 smoke; 0 doctests).

Next leak-free candidates: `ttyname_r`, `statfs`, `arc4random_buf`. Do not
pick `os_proc_available_memory`. Do not live-call `mach_host_self`.

## Non-goals

Windows live. JIT. General variadic FFI. `getloadavg`. Re-living
`mach_host_self`.
