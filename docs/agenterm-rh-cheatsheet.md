# rh condensed manual

Practical reference for writing `scripts/rh/**.rh`. The full interface tree is
[`docs/agenterm-rh-runtime.md`](agenterm-rh-runtime.md) — that document is a
*specification*; this one exists to stop the syntax and semantics mistakes that
actually cause rework. Companion: [`docs/agenterm-qjs-cheatsheet.md`](agenterm-qjs-cheatsheet.md).

rh is **not** general Rust and **not** general Rhai. It is a small subset that a
transpiler must be able to lower to Rust and compile into a cdylib "native
pack". Anything the transpiler cannot lower silently degrades to a slower
host-evaluated path — or changes meaning. Every rule below is derived from
`crates/agenterm-rh/src/transpile.rs`, `crates/agenterm-rh/src/shipped_surfaces.rs`,
and the scripts under `scripts/rh/` that are proven to compile native.

---

## 0. The one command you must run after editing a `.rh`

```bash
# From repository root
cargo run -p agenterm-rh --example mode_probe -- --root . scripts/rh/your-script.rh
```

Expect `mode=native host_eval_int=0`. Anything else means part of your script
fell back to host evaluation, which is both slower and **semantically
different** (see §7). Then:

```bash
agenterm rh check scripts/rh/your-script.rh                      # subset validation
agenterm rh eval scripts/rh/your-script.rh                       # check + pack + call entry
agenterm rh task run <task-id> --manifest agenterm.tasks.json    # as the gates run it
```

When a task fails without useful output, the worker's stderr is `Stdio::null()`
by default. Set `AGENTERM_SCRIPT_WORKER_STDERR=inherit` or you lose the whole
`STEP` trail.

---

## 1. Skeleton

```rust
// One-line purpose.
//
// Args: REPO [OPTIONAL_BINARY]

import "scripts/rh/lib/my_helpers" as helpers;

fn entry() {
    if args.len != 1 && args.len != 2 {
        return rh::fail("expected: REPO [OPTIONAL_BINARY]");
    }
    let repo = std::path::absolute(args[0]).display;
    require(std::fs::exists(repo), "missing_repo");

    print("PASS my-script");
    0
}

fn cc_lines() {
    ["what this script proves", "second claim"]
}
```

- `fn entry()` is mandatory. Its value is an **i64** — return `0` for success.
- `fn cc_lines()` returns a string array of capability claims.
- `args` is a global. **`args.len` is a property, not a call** — `args.len()` is
  wrong. Index with `args[0]`.
- `print(...)` writes a line to the task's stdout.

## 2. Control flow — what exists

| Exists | Rejected by the subset |
|--------|-----------------------|
| `if` / `else if` / `else` | `switch`, `do` — `RH_SUBSET_NO_LOOP: do/switch are not in rh-3` |
| `while`, `break`, `continue` | `loop { }` — `RH_SUBSET_WHILE_COND: while condition must be a pure int expression in rh-3` |
| `for x in array`, `for i in 0..200` | ternary `cond ? a : b` (Rhai has none either) |
| `return`, `throw`, `try { } catch { }` | closures / lambdas — `unsupported expression in rh-2: Fn*(...)` |
| `let`, `const`, `+=` | — |

`agenterm rh check` is a permissive subset validator; **`agenterm rh transpile` is
the real gate**, because it is the lowering step. All four rejections above pass
`check` and fail `transpile`. When you are unsure whether a construct is allowed,
run `transpile` on a two-line probe — the `RH_SUBSET_*` code tells you the rule
by name.

There is no ternary and no `let` in expression position, so the idiom for a
conditional value is **declare-then-reassign**:

```rust
let binary = "";
if args.len == 2 {
    binary = std::path::absolute(args[1]).display;
} else {
    binary = std::path::join(repo, "target/debug/agenterm").display;
}
```

The generated Rust will warn `value assigned is never read` for the initial
`""`. That warning is expected for this idiom and is not a defect.

## 3. Strings

**Properties (no parentheses):** `.display` (PathBuf), `.len`, `.id`,
`.file_name`, `.extension`, `.is_absolute`, `.acquired` (FileLock).

**Methods (parentheses):** `.contains()`, `.starts_with()`, `.ends_with()`,
`.split()`, `.sub_string()`, `.parse_int()`, `.to_string()`.

**Mutate-in-place-as-a-statement:** `trim()`, `to_lower()`, `replace(a, b)`.
Used as a bare statement they rewrite the binding; used as an rvalue they return
a new value. Both forms work, but the statement form **requires a plain local
binding as the receiver** — `some.json.field.trim();` will not lower.

```rust
let head = output.stdout_text();
head.trim();                    // rewrites `head` in place
let lowered = head.to_lower();  // also fine, returns a new value
```

**Concatenation:** both `"text" + string_binding` and `"text" + int_binding`
lower and stay `mode=native` (verified with `mode_probe`; a comment in
`scripts/rh/rh-aot-smoke.rh` claiming otherwise is stale). What does *not* work
is concatenating a **JSON object** — see §7 trap 2.

## 4. Collections and maps

```rust
let names = [];
names.push("first");            // in-place, statement form
for name in names { print(name); }

let options = #{ current_dir: repo, env: environment };
```

`#{ ... }` is a map literal, used mainly for the options argument of process
calls. Map values are typed by inference — see §7 trap 3 for how a wrong
inference corrupts a serialized field.

## 5. Errors — three different mechanisms, do not mix them up

| Form | Lowers to | Effect |
|------|-----------|--------|
| `require(cond, "tag")` | `if cond == 0 { rh_fail("tag"); return <typed default>; }` | records the failure **and returns from the current function** |
| `return rh::fail("msg")` | fail-return | same recording, explicit |
| `throw msg` | catchable | unwinds to the nearest `try { } catch { }` |

Two consequences that cost real debugging time:

1. **`rh_fail` records and continues.** Only the *first* recorded failure is
   reported. A task can print `PASS: ...` and still fail, because the recording
   happened earlier in a helper.
2. **`require` inside a helper does not stop the caller.** The helper returns a
   typed default (`""`, `0`, `Value::Null`, `Vec::new()`, ...) and the caller
   keeps running with that default. If a leg must abort the whole task, put the
   `require` in `entry()`, or run the thing you are asserting about as a **child
   process** and assert on its exit code.

## 6. Modules

```rust
import "scripts/rh/lib/prune_target_incremental" as prune;
// then
prune::exact_repo_root(repo);
```

- The specifier is **project-root relative**, without the `.rh` extension.
  (qjs is the opposite — it resolves relative to the importing file. See the qjs
  manual.)
- Absolute paths and `..` traversal are rejected by
  `agenterm_rh::project_import::checked_module_file`.
- **Never** write a host absolute path into a `.rh`. See AGENTS.md's path policy.

## 7. Native-pack traps (all observed, all cost hours)

These are places where the same `.rh` behaves differently once `mode=native`.

1. **A missing JSON field aborts; it is not `()`.** `type_of(x.field) == "()"`
   probing works in the interpreter and becomes `rh_fail: json_path: field`
   natively. Use `obj.contains("field")` for existence, and
   `rh::json::stringify(value) == "null"` for a null sentinel. Note that
   `json_is_present(tab.title)` cannot guard anything — the argument is
   evaluated before the call.
2. **Objects do not stringify by concatenation.** `"" + tab.working_context`
   fails with `json_string_path`. Use `rh::json::stringify(binding)`, and bind
   first — an inline expression perturbs type inference.
3. **int/string getters can be selected wrongly.** `.id` inlined inside a string
   array is sometimes inferred as an integer getter, while the stable tab id is
   `@1`. Bind explicitly first: `let id = "" + x.id;`.
4. **Adding a local function can break unrelated code.** Return-kind inference
   is a fixpoint over the whole file; a new helper can flip a kind elsewhere and
   turn a previously native expression into "unsupported". Prefer inlining at
   the call site over extracting a helper.
5. **Some chains cannot be emitted inside a local function.** `child.stderr.read(...)`
   (Child → Stream → method) only lowers at the top level; pass the `Stream` in
   as a parameter instead of passing the `Child`.
6. **Arithmetic over host reads is INT, but a bare host read may infer Bool.**
   `let pid = 0 + row.pid;` exists in real scripts specifically to force INT —
   without it a later `#{ pid: pid }` serialized `"pid": true`.

## 8. Shipped surface index

`crates/agenterm-rh/src/shipped_surfaces.rs` is the authoritative list. Grouped:

- **fs** — `std::fs::` `exists`, `exists_case_exact`, `read`, `read_to_string`,
  `read_dir`, `write`, `write_bytes`, `copy`, `rename`, `create_dir`,
  `create_dir_all`, `remove_file`, `remove_dir`, `remove_dir_all`, `metadata`,
  `symlink_metadata`, `try_lock_exclusive`
- **path** — `std::path::` `absolute`, `join`, `parent`, `PathBuf::from`;
  `PathBuf.display/join/file_name/extension/is_absolute`
- **env** — `std::env::` `current_dir`, `get`, `has`, `names`
- **process** — `std::process::` `command`, `command_status`,
  `command_stdout_file`, `id`, `kill`, `list`; `Command.*`, `Child.*`,
  `Output.*`, `Stream.*`
- **net** — `std::net::TcpListener::bind`, `std::net::TcpStream::connect`,
  `connect_timeout`; `TcpListener.*`, `TcpStream.*`
- **time** — `std::time::Duration::from_millis/from_secs`,
  `std::time::SystemTime::now`; `SystemTime.rfc3339/unix_millis`
- **rh::** — `fail`, `json::{parse,parse_file,stringify,stringify_pretty}`,
  `crypto::{sha256,sha256_file,tree_metadata_digest}`, `hash::fnv1a64`,
  `bytes::{from_array,from_text}`, `http::{request,start}`,
  `image::inspect_png`, `clipboard::{get_text,set_text}`,
  `runtime::{atomic_write,atomic_write_bytes,append_sync,append_sync_bytes,temp_dir}`,
  `task::{sleep,after,race,wait_all,cancel_all}`
- **fleet.** — the whole UI/tab/terminal/window/settings control surface; see the
  shipped list for exact operation names.

### Not in the catalog but usable in `.rh`

`std::fs::try_remove_file` and `std::fs::try_remove_dir_all` are lowered by the
rh transpiler but are **not** in `src/script_catalog.rs` and have no interpreter
implementation. They are rh-native-only conveniences: a script using them cannot
fall back to host evaluation for that call. Prefer them for best-effort cleanup
(they do not fail when the target is absent), and keep `mode=native` verified.

## 9. Debug checklist when a task fails

1. `AGENTERM_SCRIPT_WORKER_STDERR=inherit` — recover the `STEP` trail.
2. `mode_probe` — confirm `mode=native host_eval_int=0`.
3. Read the *first* recorded failure, not the last line of output (§5).
4. Gate runs redirect each child to
   `target/qualification/scratch/command-<n>.{stdout,stderr}`; the gate itself
   only prints a PASS/FAIL summary. Read those files.
5. Ask when this gate last actually ran green. A gate that never ran is more
   likely to contain an assertion that was **never** satisfiable than a
   regression.
