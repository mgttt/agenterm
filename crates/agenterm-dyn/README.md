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

`dlcall` accepts fixed signatures with at most six arguments. Supported ABI
types are `void` (return only), `i8`, `u8`, `i16`, `u16`, `i32`, `u32`, `i64`,
`u64`, and `ptr`. Floating-point values, aggregates, and variadic calls are not
supported and fail explicitly.

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
| linux × x86_64/aarch64 | `libc.so.6` | `getpid` | `ioctl(TIOCGWINSZ)` | `getppid` | live `time`, `clock_gettime`, `uname`, `getuid`, `getgid`, `getppid`, `getpgrp`, `getsid(0)`, `getpgid(0)`, `geteuid`, `getegid`, `getpriority(PRIO_PROCESS, 0)`, `nice(0)`, `sysconf(_SC_PAGESIZE)`, `sysconf(_SC_CLK_TCK)`, `sysconf(_SC_NPROCESSORS_ONLN)`, `getcwd`, `isatty(0/1/2)`, `open("/dev/null")` + `isatty` + `close`, `access` success/failure, `fcntl(0, F_GETFD)`, `fcntl(0, F_GETFL)`, `lseek(0, 0, SEEK_CUR)`, and `dup(0)` + `close` dlcalls |
| macos × x86_64/aarch64 | `libSystem.B.dylib` | `getpid` | `ioctl(TIOCGWINSZ)` | `time` | placeholders only |
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
- [effective group ID via `getegid`](examples/getegid.md)
- [current priority via `getpriority(0, 0)`](examples/getpriority-current.md)
- [current nice value via `nice(0)`](examples/nice-zero.md)
- [current working directory via `getcwd`](examples/getcwd.md)
- [clock ticks per second via `sysconf`](examples/sysconf-clk-tck.md)
- [online processor count via `sysconf`](examples/sysconf-nprocessors-onln.md)
- [whether standard input is a terminal](examples/isatty-stdin.md)
- [`open("/dev/null")` then `isatty` then `close`](examples/open-dev-null-isatty-close.md)
- [`fcntl(0, F_GETFD)` descriptor flags](examples/fcntl-stdin-getfd.md)
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
| `smoke.rs` | Real `dlcall` into host libraries per OS (`#[cfg]`-gated) |

```bash
cargo test -p agenterm-dyn
```

**Linux** (CI): `getpid` + `getppid` + `getpgrp` + `getsid(0)` + `getpgid(0)` +
`getuid` + `getgid` + `geteuid` + `getegid` cross-checked with libc; real headless
`time(NULL)`, `clock_gettime(CLOCK_MONOTONIC)`, `uname`, `sysconf(_SC_PAGESIZE)`,
`sysconf(_SC_CLK_TCK)`, `sysconf(_SC_NPROCESSORS_ONLN)`, and `getcwd` dlcalls;
real `isatty(0)` plus `open("/dev/null")` / `isatty` / `close`; `ioctl(TIOCGWINSZ)`
on a 24×80 pty when `openpty` succeeds; real `access("/", F_OK)` success and
missing-path failure; real `fcntl(0, F_GETFD)` and `dup(0)` / `close`;
real `getpriority(PRIO_PROCESS, 0)` and `nice(0)`; `getenv("DISPLAY")`; honest `libX11`
`XOpenDisplay`; real `lseek(0, 0, SEEK_CUR)` and `fcntl(0, F_GETFL)`; and AT-SPI
library existence probes (no session a11y bus). `isatty(0/1/2)` records the real
stdin/stdout/stderr state rather than requiring an interactive terminal.

**macOS** (local / CI when available): `getpid`, `time(NULL)`, optional
`ioctl` on `/dev/tty`, `getenv("DISPLAY")`.

**Windows** (local / CI when available): `GetCurrentProcessId`,
`GetCurrentThreadId`, optional CRT `getenv("DISPLAY")`.

Non-live cells are never faked on the wrong OS — only data rows and
compile-only gates.

## Layer 3 (deferred)

Survey notes: [SLJIT vs DynASM](docs/layer3-sljit-dynasm.md). Not linked. `dlcall` is Rust dispatch, not libffi.
