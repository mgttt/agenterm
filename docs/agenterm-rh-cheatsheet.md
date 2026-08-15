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

Task execution prepends the manifest task's `args` and then appends values
after the CLI `--`. A manifest default such as `args: ["."]` is therefore not
overridden by `-- REPO TARGET`; the script receives all three values. For tasks
whose callers supply an exact repository or target, omit manifest defaults and
make every caller explicit. Test the real manifest-owned task path, not only a
direct script invocation.

`mode=native host_eval_int=0` is necessary but **not sufficient** evidence for
the complete task. `mode_probe` proves that its probe path selected native mode
without host integer evaluation; it does not prove that the full imported
module graph and every inferred call can be lowered, compiled and executed as
one AOT native pack. The owning task gate must actually compile **and run** the
real task entry with its production manifest and arguments.

Failures that can appear only at that complete native-pack stage include:

- calls reached through imported modules that the isolated probe did not lower;
- `throw` lowering inside a `try` body;
- mutable calls on `Child` values obtained from a list.

The module bundler currently recognizes `import` without checking the token's
left boundary. An identifier ending in `import`, such as `installed_import`,
can therefore raise `script_module_import_path` when punctuation follows it.
Do not use `import` as a whole identifier segment; choose a semantic name such
as `installed_link_library` until the bundler boundary is corrected.

Do not promote a successful `mode_probe` to task evidence when the actual
native pack was never compiled and run.

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

Bind an array literal before iterating it. `for name in ["a", "b"]` passes the
surface parser but native transpilation rejects the literal iterable; use
`let names = ["a", "b"]; for name in names { ... }` and confirm with
`mode_probe`.

## 5. Errors — three different mechanisms, do not mix them up

| Form | Lowers to | Effect |
|------|-----------|--------|
| `require(cond, "tag")` | `if cond == 0 { rh_fail("tag"); return <typed default>; }` | records the failure **and returns from the current function** |
| `return rh::fail("msg")` | fail-return | same recording, explicit |
| `throw msg` | catchable | unwinds to the nearest `try { } catch { }` |

Three consequences that cost real debugging time:

1. **`rh_fail` records and continues.** Only the *first* recorded failure is
   *returned*. A task can print `PASS: ...` and still fail, because the recording
   happened earlier in a helper.

   Every failure is nonetheless **traced in order** to the same channels as
   `print`, so run with `AGENTERM_SCRIPT_WORKER_STDERR=inherit` and read them all
   at once rather than fixing one per run:

   ```
   RH_FAIL[1] rh_fail: json_path: tabs.len
   RH_FAIL[2] rh_fail: json_integer_value
   RH_FAIL[3] rh_fail: remote_ui_new_modal_invalid
   ```

   `RH_FAIL[1]` is the one to fix; interleave the list with the `STEP` lines and
   the cascade is usually obvious. A count in the thousands means a polling loop
   is asserting on a path that cannot appear — bound such loops by wall clock,
   not by attempt count.

   A parent task that invokes another task must not reduce every child failure
   to a generic tag such as `build_stage`. Use bounded `Command.output()`, replay
   the child's stdout/stderr, and include that evidence in the parent's first
   failure. Preserve every explicit environment override from the old status
   helper when converting it to `Command`; otherwise better diagnostics can
   silently change build identity or incremental behavior.
2. **`require` inside a helper does not stop the caller.** The helper returns a
   typed default (`""`, `0`, `Value::Null`, `Vec::new()`, ...) and the caller
   keeps running with that default. If a leg must abort the whole task, put the
   `require` in `entry()`, or run the thing you are asserting about as a **child
   process** and assert on its exit code.
3. **A module-qualified `require` is always case 2.** `test_harness::require(...)`
   is a helper call, so it cannot stop *your* function. This is not theoretical:
   `script-smoke.rh` asserted `holders.len > 0` that way and then indexed
   `holders[0]` regardless.

   When the guarded value is about to be indexed or unwrapped, write a real
   early return instead:

   ```rust
   if holders.len == 0 {
       return rh::fail("script_supervisor_no_holder_acquired");
   }
   ```

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

1. **Reading a missing JSON field is a failure, not `()`.** `"" + x.field` on an
   absent path is `rh_fail: json_path: field`. Use `obj.contains("field")` for
   existence and `rh::json::stringify(value) == "null"` for a null sentinel.

   `type_of(binding.path) == "()"` is the **one** probe that does work: the
   transpiler lowers it to `rh_json_type_name`, which returns `"()"` for a
   missing or null path on purpose, so optional probes stay native. But it only
   lowers when the transpiler can see the argument as a JSON path on a binding —
   hand it something it cannot resolve that way (a loop variable, an inlined
   call result) and it falls back to a path that does fail. Bind first, and
   re-check with `mode_probe`.

   Either way, wrapping it in a helper cannot guard anything:
   `json_is_present(tab.title)` evaluates `tab.title` **before** the call.
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
7. **Not every list is the same kind, and only some index safely.** A plain
   `[]` that you push strings into is JSON-backed, so `items[9]` on a short list
   is a clean `rh_fail: json_array_index: 9`. A list of Children
   (`ValueKind::ChildList`) is a real Rust `Vec`, so the same index is a **panic**.
8. **Do not nest JSON reads inside one arithmetic expression.** Write

   ```rust
   let left = 0 + rect.x;              // one read per binding
   let width = 0 + rect.width;
   let centre = left + (width / 2);
   ```

   not `(0 + rect.x) + ((0 + rect.width) / 2)`. The outer `+` sees two
   parenthesised sub-expressions containing JSON paths, picks the **string**
   lane, and emits `format!(...)` — which then fails to compile as an `INT`
   argument. Every scrollbar step in `remote-ui-smoke.rh` binds components first
   for exactly this reason.
9. **Removing a branch can erase native type specialization.** A branch that
   combines imported helpers with `Command`, `Child`, `Output`, and JSON paths
   can be the only expression topology that closes the file-wide return-kind
   fixpoint. Replacing it with a smaller helper or an unexecuted type-witness
   branch may pass `mode_probe` yet fail the real task pack. Preserve the useful
   topology with a real bounded probe that has no product side effect, such as
   running alternate executable bytes with `--version` and then asserting that
   the live lease, server PID, and PTY identity did not change.
10. **Compare JSON numbers as numbers, not serialized text.** The same integral
    geometry can arrive as integer `840` from native enumeration and `840.0`
    from a floating-point calculation. `rh::json::stringify` preserves that
    representation difference. Bind each numeric path with `0 + value.path`
    and compare fields; keep stringify only for diagnostics.
11. **Assigning a string local transfers ownership in the generated Rust.** If
    a temporary string must also be compared, inspect it before assigning it
    to another binding. `observed = value; if value == expected` can compile as
    use-after-move even though host evaluation accepts it. Reorder the compare
    before the assignment and run the complete native-pack task.

These three used to be traps and are now fixed in codegen 98 — they work, and
you should use them freely:

- `let n = doc.items.len;` in a bare `let` (it used to lower to a path get for
  the literal key `len` and silently produce null; comparisons always worked).
- Booleans in string concatenation: `"visible=" + control.visible`.
- `rh::json::stringify(doc.tabs)` on a **path**, not just on a bare binding.

## 7a. When a script panics

`rh_entry` is `extern "C"` and cannot unwind, so a panic in generated code used
to abort the whole script worker: exit `0xC0000409` / `-1073740791`, no message,
and the task runner then waited on a worker that was already dead. The generated
prelude now catches it and reports

```
rh pack panicked: src\lib.rs:2321:55: index out of bounds: the len is 0 but the index is 0
```

The `file:line:column` is a position in the **generated** crate, not in your
`.rh`. To map it back, find the operation named in the message (here an index)
and look for the corresponding construct in the step that was last printed. The
generated source lives in a `tempfile::tempdir()` that is deleted after the
build, so you cannot open it after the fact — the last `STEP` line plus the
operation name is what you have. That is also why every step should print one.

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

On Windows, a completed compiler command does not prove that compiler helpers
have released inherited directories or output handles. `VsDevCmd.bat` can
launch the `vctip` telemetry helper, which may outlive `cl.exe` and make cleanup
fail with sharing error 32. Set `VSCMD_SKIP_SENDTELEMETRY=1` on scripted
`VsDevCmd.bat` commands so that helper is not launched. Also use a stable working
directory plus absolute output paths, then perform bounded
`try_remove_dir_all` retries and require the disposable directory to become
absent. A final cleanup failure is failed evidence, not a warning to suppress.

Release-only static policy is too late when the same defect is cheap to reject
on every push. If Candidate exposes deterministic repository drift such as a
new PowerShell automation reference, keep the Candidate gate but also invoke
its owning named Rh task from ordinary CI. Do not copy the policy into workflow
shell; one task remains the authority and both lanes consume it.

## 9. Debug checklist when a task fails

1. `AGENTERM_SCRIPT_WORKER_STDERR=inherit` — recover the `STEP` trail.
2. `mode_probe` — confirm the necessary `mode=native host_eval_int=0` signal.
3. Compile and run the real task native pack; the probe does not cover the full
   import graph, `try`/`throw` lowering or mutable `Child` list calls.
4. Read the *first* recorded failure, not the last line of output (§5).
5. Gate runs redirect each child to
   `target/qualification/scratch/command-<n>.{stdout,stderr}`; the gate itself
   only prints a PASS/FAIL summary. Read those files.
6. Ask when this gate last actually ran green. A gate that never ran is more
   likely to contain an assertion that was **never** satisfiable than a
   regression.

`cargo metadata --no-deps` still applies its selected platform when reporting
target-conditioned dependency declarations. A cross-platform notice catalog
must merge metadata from every supported `--filter-platform` target; one host's
manifest view can miss macOS-only or Windows-only direct dependencies and make
the formal Candidate fail before compilation.

### Cross-build fixtures must live outside Cargo target trees

Formal release builds reclaim the complete `target/` and release scratch trees.
A fixture needed both before and after such a build must therefore be parked in
the host temporary directory, use a process-qualified filename for parallel-run
isolation, and be removed on both success and terminal failure. Windows-only
qualification can require and read `TEMP`; although `rh::runtime::temp_dir()`
is catalogued, the native pack transpiler does not currently lower that call.
Copying a file from `target/release-fast/` into `target/qualification/` does not
preserve it: both source and destination belong to the later Cargo cleanup.

Staging cleanup owns two races: stale files present before publication and old
or newly unlocked aliases present after publication. Keep the same explicit
obsolete-name list on both sides of staging, preferably as matching data loops,
and make native-entry regression evidence assert both occurrences. A generic
locked-artifact sweep only knows manifest-derived `*.locked-*` names; it cannot
infer that a historical executable alias is obsolete.

## Byte-exact raw protocol fixtures

When a temporary Windows fixture must shell out for a raw TCP request because
the corresponding Rh native emitter has not shipped, do not wrap the network
stream in PowerShell `StreamWriter`. Its encoding preamble and buffering are
not a byte-exact protocol contract. Encode the bounded request explicitly with
`[Text.Encoding]::UTF8.GetBytes(...)` and write those bytes directly to the
stream. Keep the shell-out migration marker and replace it with Rh
`TcpStream` once native emission exists.

## Wait for product readiness, not transport readiness

A successful CLI response proves only that the endpoint is accepting requests.
Asynchronous product state in that response may still be incomplete, such as an
initial tab whose PTY process ID is zero. GUI journeys must poll the public
atomic snapshot until the required stable ID and readiness fields agree, retain
an explicit deadline and early-owner-exit failure, and only then index the
matching object. Never index with a sentinel such as `-1` before the readiness
condition has been established.
