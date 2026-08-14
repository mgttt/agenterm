# agenterm-dyn

Tiny in-process **live native door**: interned symbols, evaluation of a small
S-expression list language (`do` / `set` / `if`), and one native primitive
`dlcall` implemented with **libffi** (no writable executable pages, no DIY JIT).

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

## Integration (deferred)

Cross-arch logic aggregation, libagenterm wiring, and `agenterm-platform`
facades are **out of scope** until `agenterm-dyn` matures on its own.

Items tagged **`PLATFORM-CANDIDATE`** in `src/hosts.rs` (and listed in
`hosts::PLATFORM_CANDIDATES`) are OS/host-facts tables — library paths, PID
symbols, ioctl request codes, console probes — that may move to
`agenterm-platform` when that crate grows an equivalent contract. They are
**not** imported from platform today. What stays in dyn: `intern` / `bind` /
`eval`, `dlcall` + libffi, value/error/parse, and the rule that OS names
remain opaque script data at the eval boundary.

## Public surface

| API | Role |
|-----|------|
| `Dyn::intern` | Intern a string into a stable `Symbol` |
| `Dyn::bind` | Hand an existing pointer/handle into the environment |
| `Dyn::eval` | Evaluate S-expr source (`do`, `set`, `if`, `dlcall`) |
| `dlcall` | Only native primitive — invoked from lists, not a verb table |
| `hosts::*` | Six-cell host table (`PLATFORM-CANDIDATE` — may move to platform later) |

## Six-cell host table

`src/hosts.rs` records explicit rows for every `{linux, macos, windows} ×
{x86_64, aarch64}` cell:

| Cell | PID library | PID symbol | Size probe | Secondary probe |
|------|-------------|------------|------------|-----------------|
| linux × x86_64/aarch64 | `libc.so.6` | `getpid` | `ioctl(TIOCGWINSZ)` | `getppid` |
| macos × x86_64/aarch64 | `libSystem.B.dylib` | `getpid` | `ioctl(TIOCGWINSZ)` | `time` |
| windows × x86_64/aarch64 | `kernel32.dll` | `GetCurrentProcessId` | `GetConsoleScreenBufferInfo` | `GetCurrentThreadId` |

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

## Test suite

Independent integration tests live under `crates/agenterm-dyn/tests/`:

| File | Coverage |
|------|----------|
| `language.rs` | `intern`, `bind`, `do` / `set` / `if`, nested lists, truthiness |
| `errors.rs` | Bad S-exprs, unknown vars/forms, arity, bad FFI types, missing lib/symbol |
| `hosts.rs` | Six-cell matrix completeness, `live_cell()` selection, row well-formedness |
| `smoke.rs` | Real `dlcall` into host libraries per OS (`#[cfg]`-gated) |

```bash
cargo test -p agenterm-dyn
```

**Linux** (CI): `getpid` + `getppid` cross-checked with libc and a second
`dlcall`; `ioctl(TIOCGWINSZ)` on a 24×80 pty when `openpty` succeeds.

**macOS** (local / CI when available): `getpid`, `time(NULL)`, optional
`ioctl` on `/dev/tty`.

**Windows** (local / CI when available): `GetCurrentProcessId`,
`GetCurrentThreadId`.

Non-live cells are never faked on the wrong OS — only data rows and
compile-only gates.
