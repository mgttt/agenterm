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

## Integration (deferred)

Cross-arch logic aggregation and libagenterm wiring are explicitly **out of
scope** until `agenterm-dyn` matures on its own. Future work may pile higher
layers on top of this crate; that integration is a separate milestone.

## Layer 3 codegen

Research for a future layer that lowers the same intern list (`do` / `set` /
`if` / `dlcall`) to native code — **not implemented**; this crate has no JIT
today. Survey notes (SLJIT vs DynASM, sizes, W^X, lowering sketch):
[`docs/layer3-sljit-dynasm.md`](docs/layer3-sljit-dynasm.md).

## Public surface

| API | Role |
|-----|------|
| `Dyn::intern` | Intern a string into a stable `Symbol` |
| `Dyn::bind` | Hand an existing pointer/handle into the environment |
| `Dyn::eval` | Evaluate S-expr source (`do`, `set`, `if`, `dlcall`) |
| `dlcall` | Only native primitive — invoked from lists, not a verb table |

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

## Tests

```bash
cargo test -p agenterm-dyn
```

Smoke tests prove `getpid`/`GetCurrentProcessId` via `dlcall` and, on Linux,
that `ioctl(TIOCGWINSZ)` is actually invoked.
