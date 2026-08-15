# agenterm-dyn

Tiny in-process **live native door**: interned symbols, evaluation of a small
S-expression list language (`do` / `set` / `if` / comparisons / `not` / `and` /
`or` / fixnum `+` `-` / bounded `repeat` / `dlcall`), and one native primitive.
`dlcall` resolves symbols with `libloading` and uses bounded Rust `extern "C"`
dispatch for integer and pointer signatures.

## What this is

- A geek/hack module for calling arbitrary native symbols from a minimal
  embedded list language.
- **ISA×2 / OS×3** from day one: the same Rust API on Linux, macOS, and
  Windows for x86_64 and aarch64. OS-specific library paths and symbol names
  (`libc.so.6`, `libSystem.B.dylib`, `GetCurrentProcessId`, …) are **script
  data** passed into `dlcall`, not hard-coded crate API.

## What this is not

- **Not** a fourth script engine — `rh`, `qjs`, and `lua` stay as they are.
- **Not** libagenterm (`agenterm-abi`). `agenterm-dyn` walks **in parallel**
  with the C ABI shell; it does not export `agt_*` symbols and is not wired
  into the root `agenterm` binary yet.
- **Not** integrated with `agenterm-cu` / `agenterm-con` in this crate's
  initial landing.
- **Not** integrated with `agenterm-platform` — that wiring is explicitly
  deferred to a later milestone.

## Drift toward cu / platform surfaces (script data only)

`agenterm-dyn` is gradually naming the same libs, symbols, and bus facts that
`agenterm-cu` and `agenterm-platform` hands use — as **PLATFORM-CANDIDATE**
script data in `hosts.rs` (`CU_ADJACENT_PROBE_CATALOG`, six `{linux,macos,windows}
× {x86_64,aarch64}` rows). Dyn still does intern/bind/eval/dlcall only; **cu
remains the live hand** and there is no import of `agenterm-platform` or
`agenterm-cu`.

## Integration (deferred)

Cross-arch logic aggregation, libagenterm wiring, and `agenterm-platform`
facades are **out of scope** until `agenterm-dyn` matures on its own.

Items tagged **`PLATFORM-CANDIDATE`** in `src/hosts.rs` (and listed in
`hosts::PLATFORM_CANDIDATES`) are OS/host-facts tables — library paths, PID
symbols, ioctl request codes, console probes, and CU-adjacent probe rows — that
may move to `agenterm-platform` when that crate grows an equivalent contract.
They are **not** imported from platform today. What stays in dyn: `intern` /
`bind` / `eval`, bounded native `dlcall`, value/error/parse, and the rule that
OS names remain opaque script data at the eval boundary.

`dlcall` is an ABI-limited native door, not a general FFI. It targets only
`x86_64` and `aarch64`, and accepts fixed, non-variadic C signatures with at
most six integer or pointer arguments. The trampoline passes every input in a
`u64` slot; its supported types are `void` (return only), `i8`, `u8`, `i16`,
`u16`, `i32`, `u32`, `i64`, `u64`, and `ptr`. Narrow signed inputs are
sign-extended and narrow unsigned inputs are zero-extended before that call.

Floating-point values and aggregates have no supported ABI class. Variadic
symbols are outside the contract because the fixed trampoline cannot reliably
detect them, so callers must not use `printf` or another arbitrary variadic
symbol. The only exception is Darwin `ioctl` with the validated
`(i32, u64|i32, ptr) -> i32` signature: it transmutes the loaded
`libloading` pointer to `unsafe extern "C" fn(i32, u64, ...) -> i32`.
This remains an ABI-compatibility boundary, not an authorization
or safety policy; library names and symbols remain caller-supplied script data.

The language stores integer results as signed `i64`. A `u64` result therefore
returns an error when it exceeds `i64::MAX`; use `ptr` for address- or
handle-valued native returns instead of declaring them as `u64`.

## Parser resource bound

The list-language parser accepts at most 256 nested lists. Source that exceeds
that depth returns a `DynError::Parse` before evaluation, rather than consuming
unbounded parser stack. This is a robustness limit only: it does not grant,
deny, or otherwise change any native-door or caller authority semantics.

## Public surface

| API | Role |
|-----|------|
| `Dyn::intern` | Intern a string into a stable `Symbol` |
| `Dyn::bind` | Hand an existing pointer/handle into the environment |
| `Dyn::eval` | Evaluate S-expr source (`do`, `set`, `if`, comparisons, `not`, `and`/`or`, `+`/`-`, `repeat`, `dlcall`) |
| `dlcall` | Only native primitive — invoked from lists, not a verb table |
| `hosts::*` | Six-cell host table + CU-adjacent catalog (`PLATFORM-CANDIDATE`) |

## Six-cell host table

`src/hosts.rs` records explicit rows for every `{linux, macos, windows} ×
{x86_64, aarch64}` cell:

| Cell | PID library | PID symbol | Size probe | Secondary probe | Additional headless probes |
|------|-------------|------------|------------|-----------------|----------------------------|
| linux × x86_64/aarch64 | `libc.so.6` | `getpid` | `ioctl(TIOCGWINSZ)` | `getppid` | fixed-ABI live rows include `time`, caller-owned-pointer `times`, `getrusage(RUSAGE_SELF, …)`, `getrlimit(RLIMIT_NOFILE, …)`, `clock_gettime`, `uname`, uid/gid/pid group, `sysconf`, `getcwd`, `isatty`, `access`/`dup`/`lseek`, `getpriority`/`nice`, `sched_yield`, `alarm`, `umask`, `getdtablesize`, `gethostid`, `getpagesize`; variadic `open`/`fcntl` and Darwin-only rows are placeholders |
| macos × x86_64/aarch64 | `libSystem.B.dylib` | `getpid` | signature-gated loaded-symbol `ioctl(TIOCGWINSZ)` through Darwin's variadic ABI | `time` | shared fixed-ABI live rows plus `sysctlbyname`, `mach_absolute_time`, `getprogname`, `issetugid`, `_NSGetExecutablePath`, `proc_pidpath`, `arc4random`, `clock_gettime_nsec_np`, `sysctl`, `mach_timebase_info`, `pthread_main_np`, `getlogin_r`, `pthread_threadid_np`, `proc_pidinfo`, `_NSGetArgc`, `_NSGetArgv`, `_NSGetEnviron`, `proc_pid_rusage`, and `_dyld_image_count`; variadic `open`/`fcntl` and `mach_host_self` are placeholders |
| windows × x86_64/aarch64 | `kernel32.dll` | `GetCurrentProcessId` | `GetConsoleScreenBufferInfo` | `GetCurrentThreadId` | placeholders only |

All six rows compile as data on every host. `live_cell()` selects the row
matching `cfg(target_os)` × `cfg(target_arch)`; the other five are
placeholders for matrix completeness and future native smokes.

## Example

```lisp
(do
  (set pid (dlcall "libc.so.6" "getpid" "i32"))
  pid)
```

Bind a buffer from Rust, then pass it to `ioctl`:

```rust
dyn_env.bind("ws", ws_ptr)?;
dyn_env.eval(r#"(dlcall "libc.so.6" "ioctl" "i32" "i32" 0 "u64" 21523 "ptr" ws)"#)?;
```

## CU-adjacent script examples

These commented examples stay at the script-data boundary: they exercise the
same native facts that cu's windows, focus, and get-text hands may consume,
without wiring dyn into cu, platform, or the ABI:

- [current PID](examples/getpid.md)
- [parent process ID via `getppid`](examples/getppid.md)
- [current process group via `getpgrp`](examples/getpgrp.md)
- [current session via `getsid(0)`](examples/getsid-current.md)
- [current process group via `getpgid(0)`](examples/getpgid-current.md)
- [current user ID via `getuid`](examples/getuid.md)
- [effective user ID via `geteuid`](examples/geteuid.md)
- [current group ID via `getgid`](examples/getgid.md)
- [effective group ID via `getegid`](examples/getegid.md)
- [current priority via `getpriority(0, 0)`](examples/getpriority-current.md)
- [current nice value via `nice(0)`](examples/nice-zero.md)
- [yield the current thread via `sched_yield`](examples/sched-yield.md)
- [cancel and observe the process alarm via `alarm(0)`](examples/alarm-zero.md)
- [read and immediately restore the process `umask`](examples/umask-read-restore.md)
- [descriptor-table limit via `getdtablesize`](examples/getdtablesize.md)
- [current host ID via `gethostid`](examples/gethostid.md)
- [current working directory via `getcwd`](examples/getcwd.md)
- [process CPU ticks via `times`](examples/times.md)
- [this process's resource usage via `getrusage(RUSAGE_SELF, …)`](examples/getrusage.md)
- [descriptor resource limits via `getrlimit(RLIMIT_NOFILE, …)`](examples/getrlimit.md)
- [host page size via `sysconf`](examples/sysconf-pagesize.md)
- [host page size via `getpagesize`](examples/getpagesize.md)
- [hardware CPU count via `sysctlbyname`](examples/sysctlbyname.md) (macOS)
- [monotonic kernel ticks via `mach_absolute_time`](examples/mach-absolute-time.md) (macOS)
- [program name pointer via `getprogname`](examples/getprogname.md) (macOS)
- [set-id execution state via `issetugid`](examples/issetugid.md) (macOS)
- [executable path via `_NSGetExecutablePath`](examples/nsget-executable-path.md) (macOS)
- [process path via `proc_pidpath`](examples/proc-pidpath.md) (macOS)
- [random word via `arc4random`](examples/arc4random.md) (macOS)
- [uptime nanoseconds via `clock_gettime_nsec_np`](examples/clock-gettime-nsec-np.md) (macOS)
- [hardware CPU count via `sysctl`](examples/sysctl.md) (macOS)
- [Mach tick-to-nanosecond ratio via `mach_timebase_info`](examples/mach-timebase-info.md) (macOS)
- [main-thread predicate via `pthread_main_np`](examples/pthread-main-np.md) (macOS)
- [login name via `getlogin_r`](examples/getlogin-r.md) (macOS)
- [current thread id via `pthread_threadid_np`](examples/pthread-threadid-np.md) (macOS)
- [BSD process facts via `proc_pidinfo`](examples/proc-pidinfo.md) (macOS)
- [process argc pointer via `_NSGetArgc`](examples/nsget-argc.md) (macOS)
- [process argv pointer via `_NSGetArgv`](examples/nsget-argv.md) (macOS)
- [process environment pointer via `_NSGetEnviron`](examples/nsget-environ.md) (macOS)
- [process rusage via `proc_pid_rusage`](examples/proc-pid-rusage.md) (macOS)
- [loaded-image count via `_dyld_image_count`](examples/dyld-image-count.md) (macOS)
- [`mach_host_self` resource-safety boundary](examples/mach-host-self.md) (macOS; intentionally not live)
- [clock ticks per second via `sysconf`](examples/sysconf-clk-tck.md)
- [online processor count via `sysconf`](examples/sysconf-nprocessors-onln.md)
- [whether standard input is a terminal](examples/isatty-stdin.md)
- [whether standard output is a terminal](examples/isatty-stdout.md)
- [whether standard error is a terminal](examples/isatty-stderr.md)
- [`lseek(0, 0, SEEK_CUR)` current offset](examples/lseek-stdin-current.md)
- [`dup(0)` then `close`](examples/dup-stdin-close.md)
- [`access("/", F_OK)` success](examples/access-root-f-ok.md)
- [`access` missing-path failure](examples/access-missing-path.md)
- [terminal window size via `ioctl`](examples/ioctl-window-size.md)
- [`DISPLAY` via `getenv`](examples/getenv-display.md)
- [explicit missing-symbol failure](examples/failure-missing-symbol.md)
- [empty library-name failure](examples/failure-empty-library.md)
- [interior-NUL library-name failure](examples/failure-library-interior-nul.md)

These examples rely only on the currently shipped list-language parser. Where
C requires a pointer or writable structure, the example names the value that
the embedding Rust host must bind before calling `Dyn::eval`. The interior-NUL
example uses visible `␀` notation for an actual NUL source byte because the
language deliberately has no string escapes.

## Test suite

Independent integration tests live under `crates/agenterm-dyn/tests/`:

| File | Coverage |
|------|----------|
| `language.rs` | comparisons, `not`, `and`/`or`, `+`/`-`, `repeat`, nested logic |
| `errors.rs` | Bad S-exprs, unknown vars/forms, arity, overflow, repeat bounds |
| `hosts.rs` | Six-cell matrix completeness, `live_cell()` selection, row well-formedness |
| `macos_ioctl.rs` | Darwin variadic `ioctl(TIOCGWINSZ)` through the loaded libSystem symbol |
| `macos_probes.rs` | Darwin-only live `dlcall` facts compared with later native calls |
| `macos_resource.rs` | `mach_host_self` stays Placeholder and is never live-called |
| `smoke.rs` | Real `dlcall` into host libraries per OS (`#[cfg]`-gated) |

```bash
cargo test -p agenterm-dyn
```

**Linux** (matching-host CI smoke): `getpid` + `getppid` + `getpgrp` + `getsid(0)` + `getpgid(0)` +
`getuid` + `getgid` + `geteuid` + `getegid` cross-checked with libc; real headless
`time(NULL)`, `clock_gettime(CLOCK_MONOTONIC)`, `uname`, `sysconf(_SC_PAGESIZE)`,
`sysconf(_SC_CLK_TCK)`, `sysconf(_SC_NPROCESSORS_ONLN)`, and `getcwd` dlcalls;
real `isatty(0)`; `ioctl(TIOCGWINSZ)`
on a 24×80 pty when `openpty` succeeds; real `access("/", F_OK)` success and
missing-path failure; real `dup(0)` / `close`;
real `getpriority(PRIO_PROCESS, 0)` and `nice(0)`; `getenv("DISPLAY")`; honest `libX11`
`XOpenDisplay`; real `lseek(0, 0, SEEK_CUR)`; and AT-SPI
library existence probes (no session a11y bus). `isatty(0/1/2)` records the real
stdin/stdout/stderr state rather than requiring an interactive terminal. `sched_yield`
exercises the void-return path; `alarm(0)` returns an integer and leaves no alarm pending.
The `umask` probe reads with `umask(0)` and immediately restores the returned mask.
`times` writes a caller-owned `tms` and is compared with a later direct-libc
baseline. `getrusage(RUSAGE_SELF, …)` writes a caller-owned `rusage` and its
CPU times are compared with a later direct-libc baseline. `getrlimit(RLIMIT_NOFILE,
…)` writes a caller-owned `rlimit` whose soft and hard limits must equal the
direct-libc baseline. `getdtablesize` returns the host descriptor-table limit as a positive integer.
`gethostid` returns the host identifier as the native signed-long integer.
`getpagesize` returns the positive page size and agrees with `sysconf(_SC_PAGESIZE)`.

**macOS** (matching-host native-test smoke): the `#[cfg(target_os = "macos")]`
source defines the same integer/void/ptr libc rows as Linux against
`libSystem.B.dylib`, including caller-owned-pointer `times`,
`getrusage(RUSAGE_SELF, …)`, and `getrlimit(RLIMIT_NOFILE, …)` checks against
direct-libc baselines. Darwin-specific smokes cover `sysctlbyname`,
`mach_absolute_time`, `getprogname`, `issetugid`, `_NSGetExecutablePath`,
`proc_pidpath`, `arc4random`, `clock_gettime_nsec_np`, `sysctl`,
`mach_timebase_info`, `pthread_main_np`, `getlogin_r`, `pthread_threadid_np`,
`proc_pidinfo`, `_NSGetArgc`, `_NSGetArgv`, `_NSGetEnviron`,
`proc_pid_rusage`, and `_dyld_image_count`;
the caller-owned timebase, login, thread-id, `proc_bsdinfo`, and
`rusage_info_v4` buffers are compared with direct C baselines. The
dynamic-loader image count is an instantaneous positive fact checked against a
later native call. `mach_host_self`
remains intentionally uncalled because its returned send right has no dyn release
owner. `ioctl(TIOCGWINSZ)` uses the resolved
`libSystem.B.dylib` symbol through its signature-gated variadic ABI and an
owned pty must return the seeded 24×80 size. This records the CI-native smoke
contract; it does not claim a local macOS-machine result. `access` missing-path uses
`/tmp/…`, not `/proc`.

**Windows** (local / CI when available): `GetCurrentProcessId`,
`GetCurrentThreadId`, optional CRT `getenv("DISPLAY")`.

Non-live cells are never faked on the wrong OS — only data rows and
compile-only gates.

## Layer 3 (deferred)

Survey notes: [SLJIT vs DynASM](docs/layer3-sljit-dynasm.md). Not linked. `dlcall` is Rust dispatch, not libffi.
