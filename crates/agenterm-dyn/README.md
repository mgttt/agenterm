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
| linux × x86_64/aarch64 | `libc.so.6` | `getpid` | `ioctl(TIOCGWINSZ)` | `getppid` | live `time`, `clock_gettime`, `uname`, `getuid`, `getgid`, `geteuid`, `getegid`, `sysconf(_SC_PAGESIZE)`, `sysconf(_SC_CLK_TCK)`, `sysconf(_SC_NPROCESSORS_ONLN)`, `getcwd` dlcalls |
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
- [current user ID via `getuid`](examples/getuid.md)
- [effective user ID via `geteuid`](examples/geteuid.md)
- [effective group ID via `getegid`](examples/getegid.md)
- [current working directory via `getcwd`](examples/getcwd.md)
- [clock ticks per second via `sysconf`](examples/sysconf-clk-tck.md)
- [online processor count via `sysconf`](examples/sysconf-nprocessors-onln.md)
- [terminal window size via `ioctl`](examples/ioctl-window-size.md)
- [`DISPLAY` via `getenv`](examples/getenv-display.md)
- [explicit missing-symbol failure](examples/failure-missing-symbol.md)

The fenced forms contain only the currently shipped list language. Where C
requires a pointer or writable structure, the example names the value that the
embedding Rust host must bind before calling `Dyn::eval`.

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

**Linux** (CI): `getpid` + `getppid` + `getuid` + `getgid` + `geteuid` +
`getegid` cross-checked with libc; real headless
`time(NULL)`, `clock_gettime(CLOCK_MONOTONIC)`, `uname`, `sysconf(_SC_PAGESIZE)`,
`sysconf(_SC_CLK_TCK)`, `sysconf(_SC_NPROCESSORS_ONLN)`, and `getcwd` dlcalls;
`ioctl(TIOCGWINSZ)`
on a 24×80 pty when `openpty` succeeds; `getenv("DISPLAY")`; honest `libX11`
`XOpenDisplay` and AT-SPI library existence probes (no session a11y bus).

**macOS** (local / CI when available): `getpid`, `time(NULL)`, optional
`ioctl` on `/dev/tty`, `getenv("DISPLAY")`.

**Windows** (local / CI when available): `GetCurrentProcessId`,
`GetCurrentThreadId`, optional CRT `getenv("DISPLAY")`.

Non-live cells are never faked on the wrong OS — only data rows and
compile-only gates.

## Layer 3 (deferred)

Survey notes: [SLJIT vs DynASM](docs/layer3-sljit-dynasm.md). Not linked. `dlcall` is Rust dispatch, not libffi.
