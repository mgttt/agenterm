# AgenTerm Script Runtime Specification

Status: Draft specification for v0.1.9

Specification ID: `agenterm-script-runtime`

Script API currently shipped: v1

Catalog schema currently shipped: v2

Initial design date: 2026-07-28

Last reviewed: 2026-07-28
Normative language: English

Product authority:
[Rust host and Rhai scripting PRD](../prd/PRD_02_10_rhai_scripting.md)

Delivery plan:
[AgenTerm v0.1.9 public plan](../plan/plan-v0.1.9.md)

This document defines the stable AgenTerm Rhai object and interface model for
script authors, runtime implementers, tests, documentation generators, and
future tool consumers. The Rhai surface is the product contract. Rust, Node.js,
and Bun are research references only and do not own this API.

`MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT`, and `MAY` are normative as defined
by RFC 2119-style usage.

## 1. Complete public object and interface tree

Every node below includes its purpose, lifecycle status, stability class, and
design date. The date records when the node was accepted into this
specification; it is not a claim that the node shipped on that date.

Status values:

- `shipped`: implemented and exercised through a public executable;
- `planned`: accepted for the named target version but not yet shipped;
- `deferred`: intentionally outside the current target;
- `legacy-v1`: shipped only as a migration source, not the canonical future
  surface.

Stability values:

- `stable`: the Rhai path and its documented meaning are protected within the
  current Script API major;
- `reserved`: namespace ownership and purpose are protected, but leaf
  signatures remain subject to explicit design review;
- `legacy`: available for migration and eligible for removal at the declared
  next major;
- `research`: no compatibility promise.

```text
agenterm-script
│  AgenTerm's general-purpose Rhai automation runtime.
│  [planned product completion; reserved; designed 2026-07-28]
│
├─ Rhai language
│  Upstream language syntax and values; not an AgenTerm compatibility layer.
│  [shipped; upstream-defined; designed 2026-07-28]
│  ├─ control flow and functions
│  │  let/fn/if/for/while/loop/try-catch and closures.
│  │  [shipped; upstream-defined; designed 2026-07-28]
│  ├─ values
│  │  bool, integer, float, string, array, map, range, and Dynamic values.
│  │  [shipped; upstream-defined; designed 2026-07-28]
│  └─ import
│     Local project module composition; resolver policy is AgenTerm-owned.
│     [planned; reserved; designed 2026-07-28]
│
├─ globals
│  Minimal invocation prelude; general capabilities do not become globals.
│  [shipped; stable; designed 2026-07-28]
│  ├─ args
│  │  Script arguments supplied after the CLI `--` delimiter.
│  │  [shipped; stable; designed 2026-07-28]
│  └─ print(value)
│     Writes one bounded line to captured script standard output.
│     [shipped; stable; designed 2026-07-28]
│
├─ std::
│  Selected, Rust-shaped low-level local capabilities.
│  [partially shipped; reserved; designed 2026-07-28]
│  ├─ fs::
│  │  Blocking filesystem operations owned by the local profile.
│  │  [partially shipped; stable namespace; designed 2026-07-28]
│  │  ├─ read_to_string(path)
│  │  │  Reads one UTF-8 file and returns a string.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ read(path)
│  │  │  Reads one file and returns `Bytes`.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ write(path, text)
│  │  │  Replaces one explicit file with UTF-8 text.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ write_bytes(path, bytes)
│  │  │  Replaces one explicit file with typed bytes.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ exists(path)
│  │  │  Reports whether an explicit path currently exists.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ metadata(path)
│  │  │  Returns typed file-kind, length, and modified-time facts.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ read_dir(path)
│  │  │  Returns typed entries for one directory without recursive traversal.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ DirEntry
│  │  │  Exposes path, file name, file kind, symlink kind, and metadata.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ Metadata
│  │  │  Exposes is_file, is_dir, len, and modified wall-clock time.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ create_dir_all(path) / copy(from, to) / rename(from, to)
│  │  │  Explicit-target filesystem mutation.
│  │  │  [planned; reserved; designed 2026-07-28]
│  │  └─ remove_file(path) / remove_dir(path)
│  │     Explicit-target deletion with broad-target guards.
│  │     [planned; reserved; designed 2026-07-28]
│  │
│  ├─ path::
│  │  Typed, Windows-aware path construction and inspection.
│  │  [partially shipped; stable namespace; designed 2026-07-28]
│  │  ├─ PathBuf::from(value)
│  │  │  Creates an owned typed path from a string.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ join(parent, child)
│  │  │  Creates a typed path by joining two strings.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ absolute(path)
│  │     Resolves a path against the worker's current directory.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  ├─ env::
│  │  Worker environment and current-directory facts.
│  │  [partially shipped; stable delivered leaves; designed 2026-07-28]
│  │  ├─ var(name) / has(name) / names()
│  │  │  Reads environment facts without logging values.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ current_dir()
│  │  │  Returns the worker's typed current directory.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ set/remove and child environment construction
│  │     Process-global mutation is deferred; Command child overlays ship.
│  │     [partially shipped; stable child overlay; designed 2026-07-28]
│  │
│  ├─ process::
│  │  Shell-free executable-plus-argv process construction.
│  │  [shipped; stable namespace; designed 2026-07-28]
│  │  └─ command(program) -> Command
│  │     Creates a typed process builder; no implicit shell is inserted.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  └─ time::
│     Typed duration, monotonic, and wall-clock values.
│     [partially shipped; stable namespace; designed 2026-07-28]
│     ├─ Duration
│     │  A non-negative span used by deadlines and waits.
│     │  [shipped; stable; designed 2026-07-28]
│     ├─ Instant
│     │  A monotonic runtime timestamp.
│     │  [planned; reserved; designed 2026-07-28]
│     └─ SystemTime
│        Wall-clock time with now(), unix_millis, and UTC RFC 3339 rendering;
│        it is never used as a monotonic deadline.
│        [partially shipped; stable delivered leaves; designed 2026-07-28]
│
├─ rhai::
│  High-level extensions owned by AgenTerm Script Runtime.
│  [partially shipped; stable namespace; designed 2026-07-28]
│  ├─ json::
│  │  Bounded JSON conversion between text and Rhai-compatible values.
│  │  [shipped; stable namespace; designed 2026-07-28]
│  │  ├─ parse(text)
│  │  │  Parses JSON text into a Rhai value.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ stringify(value)
│  │  │  Serializes one Rhai-compatible value to compact JSON.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ stringify_pretty(value)
│  │     Serializes one Rhai-compatible value to indented JSON.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  ├─ bytes::
│  │  Construction helpers for the typed `Bytes` value.
│  │  [partially shipped; stable namespace; designed 2026-07-28]
│  │  └─ from_text(text)
│  │     Encodes UTF-8 text into `Bytes`.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  ├─ task::
│  │  Executor-neutral task composition, waiting, racing, and cancellation.
│  │  [planned; stable namespace reservation; designed 2026-07-28]
│  │  ├─ wait_all(tasks)
│  │  │  Waits for all tasks with deterministic result ordering.
│  │  │  [planned; reserved; designed 2026-07-28]
│  │  ├─ race(tasks)
│  │  │  Returns the first stable terminal outcome.
│  │  │  [planned; reserved; designed 2026-07-28]
│  │  └─ cancel_all(tasks)
│  │     Requests cancellation and reports final cancellation outcomes.
│  │     [planned; reserved; designed 2026-07-28]
│  │
│  ├─ http::
│  │  Bounded, cancellable HTTP(S) client operations.
│  │  [planned; stable namespace reservation; designed 2026-07-28]
│  │  ├─ request(method, url, options) -> HttpResponse
│  │  │  Performs one sequential request.
│  │  │  [planned; reserved; designed 2026-07-28]
│  │  └─ start(method, url, options) -> Task
│  │     Starts one explicit concurrent request.
│  │     [planned; reserved; designed 2026-07-28]
│  │
│  ├─ runtime::
│  │  Read-only invocation, profile, version, and limit facts.
│  │  [planned; stable namespace reservation; designed 2026-07-28]
│  │
│  └─ package::
│     Future package-facing capability namespace; no v0.1.9 API promise.
│     [deferred; research; designed 2026-07-28]
│
├─ typed objects
│  Values with identity, state, resource ownership, or lifecycle use `.`.
│  [partially shipped; stable rule; designed 2026-07-28]
│  ├─ PathBuf
│  │  An owned Windows path value.
│  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .join(child)
│  │  │  Mutates the path by appending one component.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .display
│  │  │  Returns a display-safe path string.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .file_name / .extension
│  │  │  Returns the final name or extension, or an empty string.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ .is_absolute
│  │     Reports whether the path is absolute.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  ├─ Bytes
│  │  An owned, bounded byte sequence.
│  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .len
│  │  │  Returns the byte length.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ .to_text()
│  │     Decodes strict UTF-8 or throws `bytes_invalid_utf8`.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  ├─ Command
│  │  A mutable executable-plus-argv process builder.
│  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .arg(value) / .args(values)
│  │  │  Appends argv entries without shell parsing.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .current_dir(path) / .env(name, value)
│  │  │  Configures child launch context.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .output() -> Output
│  │  │  Runs synchronously with bounded captured output.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ .start() -> Child
│  │     Starts an invocation-owned child; `spawn` is Rhai-reserved.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  ├─ Child / Output
│  │  Child lifecycle and bounded final process output.
│  │  [shipped; stable; designed 2026-07-28]
│  ├─ Task / Stream
│  │  Invocation-owned asynchronous completion and bounded stream state.
│  │  [planned; reserved; designed 2026-07-28]
│  ├─ HttpResponse
│  │  Status, headers, body, truncation, and transport facts.
│  │  [planned; reserved; designed 2026-07-28]
│  └─ Receipt / Event / PostState
│     Fleet mutation evidence and verified resulting state.
│     [planned; reserved; designed 2026-07-28]
│
├─ agent
│  Script API v1 read-only broker facade.
│  [legacy-v1; legacy; designed 2026-07-28]
│  ├─ .workspace()
│  │  Reads workspace metadata and event position.
│  │  [shipped v1; legacy; designed 2026-07-28]
│  ├─ .tabs() / .active_tab()
│  │  Reads the tab list or active tab.
│  │  [shipped v1; legacy; designed 2026-07-28]
│  ├─ .ui_snapshot()
│  │  Reads the bounded semantic UI snapshot.
│  │  [shipped v1; legacy; designed 2026-07-28]
│  ├─ .capture(tab, max_bytes)
│  │  Reads bounded terminal capture.
│  │  [shipped v1; legacy; designed 2026-07-28]
│  └─ .events_read(...) / .events_wait(...)
│     Reads or waits for bounded Fleet events.
│     [shipped v1; legacy; designed 2026-07-28]
│
├─ fleet
│  Canonical Script API v2 object bound to one AgenTerm server and broker.
│  [planned; stable name reservation; designed 2026-07-28]
│  ├─ .workspace
│  │  Typed workspace identity and state.
│  │  [planned; reserved; designed 2026-07-28]
│  ├─ .tabs
│  │  Typed tab discovery and mutation.
│  │  [planned; reserved; designed 2026-07-28]
│  │  ├─ .list() / .active()
│  │  │  Reads stable tab objects.
│  │  │  [planned; reserved; designed 2026-07-28]
│  │  └─ mutation methods -> Receipt
│  │     Performs typed tab/tree/composer operations.
│  │     [planned; reserved; designed 2026-07-28]
│  ├─ .terminal(tab_id)
│  │  Binds terminal observation, input, viewport, and lifecycle operations.
│  │  [planned; reserved; designed 2026-07-28]
│  └─ .events
│     Reads, waits for, or starts waits over the typed event journal.
│     [planned; reserved; designed 2026-07-28]
│
├─ project composition
│  Local modules and named task discovery.
│  [planned; stable mechanism reservation; designed 2026-07-28]
│  ├─ import "relative/module" as module
│  │  Resolves only inside the declared project root.
│  │  [planned; reserved; designed 2026-07-28]
│  └─ agenterm.tasks.json
│     Versioned local task manifest; not a package manifest.
│     [planned; stable filename reservation; designed 2026-07-28]
│
└─ discovery
   Offline and runtime-aligned interface inspection.
   [partially shipped; stable mechanism; designed 2026-07-28]
   ├─ script api --json
   │  Emits the machine-readable catalog and runtime limits.
   │  [shipped; stable; designed 2026-07-28]
   ├─ script api [PATH]
   │  Renders this tree and optionally expands one node.
   │  [planned; reserved; designed 2026-07-28]
   ├─ script check FILE
   │  Validates syntax, profile availability, and known interfaces offline.
   │  [shipped baseline; stable; designed 2026-07-28]
   └─ script task list / show / run
      Discovers and invokes named local tasks.
      [planned; reserved; designed 2026-07-28]
```

The tree is normative for namespace ownership, public path spelling, and the
meaning of nodes marked `stable`. The machine catalog is normative for exact
shipped signatures and availability in a particular build.

## 2. Core model

```text
AgenTerm Rhai Environment
  = Rhai language
  + AgenTerm stable Rhai surface
  + selected low-level local capabilities
  + Rhai-native high-level extensions
  + AgenTerm Fleet domain
```

The recommended description is:

> AgenTerm Script is a general-purpose automation runtime using Rhai as its
> language, an AgenTerm-owned stable object model, selected Rust-shaped local
> capabilities, and native Fleet integration.

The phrase "Rust-shaped" describes familiarity, not ownership. A Rust API
change, deprecation, rename, or semantic revision MUST NOT automatically
change an AgenTerm Rhai path.

## 3. Goals and non-goals

### 3.1 Goals

The runtime MUST:

- provide a small, coherent, predictable Rhai interface tree;
- keep shipped stable paths compatible within one Script API major;
- expose typed, bounded, cancellable, and observable contracts for local and
  Fleet automation;
- keep resource identity and lifecycle visible through typed objects;
- generate discovery, validation, manuals, and future tool schemas from one
  catalog;
- produce verifiable terminal outcomes after success, failure, cancellation,
  timeout, or worker failure;
- clean every invocation-owned child, task, stream, pipe, and temporary
  resource.

### 3.2 Non-goals

The runtime does not promise:

- Rust language, Rust `std`, Cargo crate, ABI, trait, generic, borrow, or
  `Result<T, E>` compatibility;
- JavaScript, TypeScript, Node.js, Bun, npm, or module-resolution
  compatibility;
- browser APIs, a general socket server, arbitrary remote imports, or a
  persistent script daemon;
- an Agent approval model, package trust root, or operating-system sandbox;
- multiple historical aliases or duplicate sync/async families for the same
  operation.

## 4. Stability authority

The following precedence is normative:

```text
stable Rhai surface_path and object semantics
  > Script API major compatibility rules
  > typed machine catalog for the current build
  > this specification's prose
  > Rust/Node/Bun comparison metadata
  > implementation module or Rust function names
```

Consequences:

1. `surface_path` is the user contract.
2. `stable_id` is the machine identity of that contract.
3. `catalog_path` is documentation taxonomy and MAY be reorganized without
   renaming the Rhai surface.
4. `rust_path` and `rust_mapping` are explanatory metadata and MAY be corrected
   without a Script API major.
5. Internal Rust module, crate, executor, or function names are not public.
6. A convenient Rust analogue MUST NOT be added if it makes the Rhai model
   ambiguous or forces Rust-only concepts into scripts.

### 4.1 Stable changes

Within one Script API major, a stable node MAY receive:

- new optional parameters with explicit defaults only when old calls remain
  unambiguous;
- new optional result fields;
- new typed error codes that do not reinterpret existing codes;
- new methods or sibling nodes;
- stricter protection against unsafe or invalid input when valid historical
  input remains valid.

Within one Script API major, a stable node MUST NOT:

- be silently renamed or moved;
- change from a property to a function, or vice versa;
- change a unit, encoding, blocking model, mutation target, or return meaning;
- broaden authority into another profile without an explicit catalog change;
- turn complete output into silently truncated output;
- change a stable error code to a message-only failure.

### 4.2 Reserved and planned nodes

A reserved namespace name and purpose are protected. Planned leaf signatures
MAY change before they become stable, but every change MUST update this tree,
the PRD, the plan, and catalog proposal in one reviewable change.

## 5. Namespace ownership

### 5.1 `std::`

`std::` contains selected low-level capabilities only when an honest Rust
standard-library analogue exists. AgenTerm owns the final Rhai signature and
semantics.

`std::fs`, `std::path`, `std::env`, `std::process`, and `std::time` are valid.
`std::http` and high-level `std::task` MUST NOT exist because Rust `std` does
not own those high-level facilities.

### 5.2 `rhai::`

`rhai::` contains high-level extensions owned by AgenTerm Script Runtime:
tasks, HTTP, JSON, bytes helpers, and runtime facts. It does not claim that
these APIs exist in upstream Rhai or in another Rhai host.

### 5.3 `fleet`

`fleet` is an invocation-bound object, not a static namespace. It represents a
selected AgenTerm server, broker, profile, and event epoch. Stateful Fleet
resources use typed objects and dot methods.

### 5.4 Globals

The global prelude is intentionally minimal. General filesystem, process,
network, and Fleet operations MUST NOT be injected as globals.

### 5.5 Static capabilities and stateful values

- Static capability groups use `::`.
- Values with identity, mutable state, or lifecycle use `.`.
- One operation SHOULD have one canonical public path.
- Convenience APIs MUST NOT conceal cancellation, truncation, ownership, or
  destructive scope.

## 6. Typed catalog contract

Every public capability MUST be represented in one catalog entry containing at
least:

```text
stable_id
catalog_path
surface_path
status
stability
designed_on
since
deprecated_since / removed_in / replacement
profiles
signatures
input / output / error schemas
authority and side-effect facts
sync / task / stream behavior
timeout and cancellation behavior
soft limits and hard ceilings
secret-bearing fields
availability or degraded reason
rust_path (nullable research metadata)
rust_mapping (research metadata)
semantic_differences (research metadata)
```

`rust_mapping` is one of:

- `direct`: the name and primary purpose closely correspond;
- `adapted`: an analogue exists, but Rhai, Windows, error, or budget semantics
  differ;
- `inspired`: only the object model is borrowed;
- `none`: the capability is AgenTerm/Rhai-specific.

The first three values MUST NOT imply compatibility.

Example:

```json
{
  "stable_id": "std.fs.read-to-string",
  "catalog_path": "system/filesystem/read-text",
  "surface_path": "std::fs::read_to_string",
  "status": "shipped",
  "stability": "stable",
  "designed_on": "2026-07-28",
  "profiles": ["local"],
  "rust_path": "std::fs::read_to_string",
  "rust_mapping": "adapted",
  "semantic_differences": [
    "returns a string directly and throws a typed script error on failure",
    "accepts UTF-8 only",
    "is governed by invocation byte and time limits"
  ]
}
```

## 7. Shipped local capability semantics

### 7.1 Files

`std::fs::read_to_string` MUST decode strict UTF-8.
`std::fs::read` MUST return `Bytes`.
`std::fs::write` and `write_bytes` MUST target one explicit path and replace
that file's contents. They do not currently promise atomic replacement.
`std::fs::read_dir` enumerates exactly one directory and returns typed
`DirEntry` values; recursion remains explicit script policy. Symlink entries
are reported as symlinks and are not silently treated as directories.
`Metadata.len` is a bounded Rhai integer and `.modified` is a `SystemTime`.

Filesystem failures MUST carry a stable error code. Safe diagnostics MAY
include the final file name but MUST NOT automatically retain a full
secret-bearing path.

### 7.2 Paths

`PathBuf` is an owned script value. It does not model Rust borrowing.

The shipped `.join(child)` method mutates its receiver. This behavior is part
of the Rhai contract even though the nearest Rust method comparison may use a
different ownership pattern. A future immutable operation MUST use a distinct,
unambiguous name rather than silently changing `.join`.

Windows drive, UNC, long-path, separator, canonicalization, and reparse-point
behavior remain subject to explicit tests before stronger guarantees are
added.

`std::path::absolute` resolves relative input against the worker current
directory. It is not a filesystem canonicalization operation and MUST NOT be
used to claim that a path exists or that symlinks were resolved.

### 7.3 JSON

JSON conversion MUST accept only Rhai values representable in the documented
JSON schema. Invalid JSON or unsupported values MUST fail with a stable code.
Input size, output size, nesting depth, and collection limits remain governed
by invocation budgets.

### 7.4 Bytes

`Bytes` is an owned byte sequence. `.len` counts bytes. `.to_text()` performs
strict UTF-8 decoding and throws `bytes_invalid_utf8` on failure.

## 8. Process and time semantics

The canonical process constructor is:

```rhai
let command = std::process::command("git");
command.arg("status");
command.arg("--short");
let output = command.output();
```

`Command::new` MUST NOT be introduced: `new` is a Rhai reserved word.
`Command.spawn` MUST NOT be introduced either: `spawn` is also Rhai-reserved.
The canonical asynchronous child constructor is `Command.start()`, while
catalog comparison metadata records Rust `Command::spawn`.
Custom syntax MUST NOT be added merely to imitate a Rust spelling.

Process launch MUST use executable plus argv. It MUST NOT pass an implicit
command string to a shell. A user who needs shell behavior explicitly launches
that shell executable.

`Command`, `Child`, and `Output` are typed objects. Parent cancellation or
termination MUST clean the invocation-owned process tree.

`std::env::var`, `has`, `names`, and `current_dir` observe the worker
environment. Environment values are available to the running script but MUST
NOT be copied into retained audit or diagnostics. `Command.env`, `env_remove`,
and `env_clear` configure only the child; they do not mutate the AgenTerm host.

The shipped process defaults are a 2,000 ms child deadline and 64 KiB retained
for each captured stream. A script MAY lower or raise them through
`Command.timeout(Duration)` and `capture_limit(bytes)`, up to hard ceilings of
10,000 ms and 256 KiB. Text stdin is limited to 256 KiB.

`Output.stdout` and `.stderr` are `Bytes`. `stdout_text()` and `stderr_text()`
perform strict UTF-8 decoding. `.truncated` MUST become true if either stream
exceeds its retained capture; readers continue draining discarded bytes so a
full pipe cannot deadlock the child. `.complete` is true only after the child
and both captured streams reach terminal state.

`Command.start()` returns an invocation-owned `Child`. `id`, `state`, `kill`,
and `wait_with_output([Duration])` are observable in the same invocation.
No child handle survives an invocation. The outer supervisor Job Object owns
the worker and its descendants, so timeout, cancellation, crash, parent exit,
or normal completion cannot intentionally detach a child process tree.

`Duration`, `Instant`, and `SystemTime` keep monotonic deadlines separate from
wall-clock time. A cancellable timer belongs to `rhai::task`, not to a fake
cancellable `std::thread::sleep`.

The shipped `SystemTime::now()` returns wall-clock time.
`.unix_millis` is milliseconds since the Unix epoch and `.rfc3339` is UTC with
millisecond precision. These values support reporting and serialization; they
MUST NOT be used as monotonic deadlines.

## 9. Typed errors

Stable public APIs MUST expose typed errors rather than requiring message
parsing. The target error object contains:

```text
class
code
operation
safe_message
retryable
target_kind
truncated
cause_class (optional)
```

Rust-style APIs do not emulate Rust `Result` or `?`. A successful call returns
its value. A failed call throws an error catchable by Rhai `try/catch`.

Messages MAY improve without a major version. Stable automation MUST depend on
typed fields, not message text.

Source text, secret environment values, credentials, HTTP bodies, full argv,
terminal content, and unbounded output MUST NOT be copied into errors, audit
records, or retained diagnostics.

## 10. Task, Stream, and asynchronous work

Rhai evaluation remains synchronous. AgenTerm does not add JavaScript-style
`async/await`. The host performs concurrent work and exposes explicit typed
handles:

```text
Rhai evaluation thread
  start() ───────────────> invocation-owned host task runtime
  Task.wait() <────────── typed completion, error, or stream state
```

Sequential calls remain shortest:

```rhai
let output = command.output();
let response = rhai::http::request("GET", url, #{});
```

Concurrent work is explicit:

```rhai
let command = std::process::command("git");
command.args(["status", "--short"]);

let child = command.start();
let web = rhai::http::start("GET", url, #{});

let output = child.wait_with_output();
let response = web.wait(std::time::Duration::from_secs(15));
```

`Task` MUST have an invocation-local stable ID, state, wait, cancel, and stable
terminal outcome. Late completion MUST NOT overwrite `cancelled`.

`Stream` MUST report readable, closed, and failed states; truncation; encoding
failure; backpressure; and cumulative limits. Truncated data MUST NOT be
reported as complete.

The public Task/Stream contract is executor-neutral. Tokio or any other
executor type MUST NOT enter the Rhai API.

## 11. Thread and host boundary

The Rhai `Engine`, `Scope`, and `Dynamic` values remain on the evaluation
thread. Background work stores Rust-native payloads, bytes, task state, and
cancellation tokens. Conversion into Rhai values occurs only at a wait/read
boundary on the evaluation thread.

Cancellation covers:

1. Ctrl+C, deadline, parent exit, or explicit cancellation;
2. HTTP, Fleet waits, timers, and child processes;
3. stable `Task.wait()` cancellation results;
4. CPU-bound Rhai interruption through the engine progress hook;
5. supervisor and Job Object cleanup after the grace period.

A script wait, panic, or worker crash MUST NOT block or terminate the AgenTerm
GUI, PTYs, or server.

## 12. Execution profiles

Profiles are runtime execution modes, not Agent approval roles.

### 12.1 `local`

`local` is the ordinary default. It has the authority of a normal local program
started by the user. It remains subject to typed errors, budgets, cancellation,
resource ownership, audit privacy, and Fleet public-operation invariants.

### 12.2 `pure`

`pure` has no ambient filesystem, environment, process, network, clock, or
Fleet authority. It is intended for deterministic computation over bounded
JSON-compatible input and output.

### 12.3 `observe`

`observe` allows read-only workspace, tab, snapshot, capture, and event
operations. It does not allow local filesystem/process/network access or Fleet
mutation.

Every catalog entry MUST state profile availability. `check` SHOULD reject a
known unavailable call before execution.

## 13. Fleet domain

Fleet APIs MUST derive from AgenTerm's typed operation catalog and MUST NOT read
private GUI or PTY fields.

Every public operation is either mapped to the Rhai surface or reported with a
stable unavailable/degraded reason. Mutations use stable targets, request
identity, receipts, event positions, and verified post-state. Retries MUST NOT
repeat committed side effects.

Script API v1 currently exposes the legacy `agent` object. Script API v2
reserves `fleet` as its only canonical facade. The v2 migration MUST provide
machine-readable replacements and MUST NOT retain two permanent equivalent
facades.

Operation classification is a tool fact, not Agent authorization. A future
Agent layer MAY filter these capabilities without redefining Script Runtime.

## 14. Modules, projects, and named tasks

Initial modules are local and project-root-relative:

```rhai
import "lib/report" as report;
report::run(args)
```

Resolution MUST reject root escape and distinguish cycles, missing modules,
duplicate identity, and parse failure. It MUST NOT scan the user's home, PATH,
or network to guess a module.

The named task manifest is `agenterm.tasks.json`. It describes local task
execution and is not a package manifest. `task list` and `task show` MUST NOT
execute user code. Invalid tasks remain discoverable with a degraded reason.

## 15. Discovery and generated manuals

The following consumers share one catalog:

```text
runtime registration
script check
api tree and api --json
reference manual
implementation coverage
Rust/Node/Bun research comparison
future MCP adapter
future Agent tool policy
```

Human-facing `api` output SHOULD show the object tree first, then expand the
selected node. Generated pages include signature, status, stability, design
date, profile, typed errors, limits, and semantic notes.

`check` MUST NOT execute user code, access the network, or require a GUI. It
validates syntax, known qualified paths, profile availability, and statically
provable limits.

## 16. Versioning and migration

Runtime version, Script API version, catalog schema version, task manifest
version, and per-entry `stable_id` are independent and discoverable.

Compatibility rules:

- stable paths and meanings do not silently change within one API major;
- optional fields MAY be added compatibly;
- rename or removal requires deprecation, a machine-readable replacement, and
  a declared removal major;
- `check` reports exact migration diagnostics;
- aliases are not retained forever solely to avoid migration;
- planned leaves may change only through synchronized specification, PRD,
  plan, and catalog updates.

Rust, Node.js, Bun, Rhai-host, crate, or dependency version changes do not
justify a Rhai breaking change.

## 17. Authority, safety, and privacy

The `local` profile is a normal local program capability, not an
Agent-approved sandbox. Its authority derives from:

- the current OS user;
- the selected execution profile;
- Fleet public typed operations;
- invocation-owned resources;
- explicit budgets and cancellation;
- any future higher Agent or package policy as a separate layer.

The runtime MUST NOT bypass Fleet operations to mutate private GUI, PTY, or
workspace state. It MUST NOT become a package-signing trust root.

Audit and diagnostics record only required operation IDs, counts,
classifications, duration, limits, and safe target facts. Secret-bearing fields
are machine-marked in the catalog.

## 18. Budgets

Every invocation has hard ceilings covering at least:

- wall time and CPU progress;
- source, input, output, and cumulative bytes;
- Rhai operations and expression/call depth;
- collection size;
- tasks, children, streams, and queues;
- HTTP body, redirects, and deadlines;
- modules, imports, and source bytes;
- Fleet waits, event batches, and captures.

Defaults and hard ceilings are published by `api --json`. Reaching a limit
returns a typed limit error, cleans owned resources, and leaves the next
invocation healthy.

## 19. Examples

Examples can contain planned APIs. Shipped status is determined by the catalog,
not by appearance in an example.

### 19.1 Shipped file, path, bytes, and JSON slice

```rhai
let path = std::path::PathBuf::from("agenterm.local.json");
let config = rhai::json::parse(std::fs::read_to_string(path.display));

let output = std::path::join("out", "summary.json");
std::fs::write(
    output.display,
    rhai::json::stringify_pretty(#{
        ok: true,
        source: "agenterm-script",
        input_extension: path.extension
    })
);
```

### 19.2 Shipped argv-safe process

```rhai
let command = std::process::command("git");
command.args(["status", "--short"]);
command.current_dir(std::env::current_dir());

let output = command.output();
if !output.success {
    throw output.error("git-status-failed");
}

print(output.stdout_text());
```

### 19.3 Planned concurrent HTTP and process work

```rhai
let command = std::process::command("git");
command.args(["rev-parse", "HEAD"]);

let git = command.start();
let release = rhai::http::start("GET", release_url, #{
    timeout: std::time::Duration::from_secs(10)
});

let commit = git.wait_with_output();
let response = release.wait(std::time::Duration::from_secs(15));
```

### 19.4 Planned Fleet mutation evidence

```rhai
let active = fleet.tabs.active();
let capture = fleet.terminal(active.id).capture(8192);

let receipt = fleet.tabs.set_note(active.id, "captured");
receipt.wait(std::time::Duration::from_secs(5));

print(#{
    tab: active.id,
    truncated: capture.truncated,
    confirmed: receipt.post_state.confirmed
});
```

## 20. Conformance

A capability becomes `shipped` only when:

1. its catalog entry includes stable identity, surface, status, stability,
   design date, profiles, schemas, and semantic differences;
2. pure logic and error behavior have unit coverage;
3. the public `agenterm-cli script` path has black-box coverage;
4. success, typed failure, timeout, cancellation, and limits have evidence;
5. no child, worker, task, stream, pipe, or temporary resource is orphaned;
6. secret sentinels do not enter output, audit, or diagnostics;
7. catalog, `check`, runtime registration, and generated manual agree;
8. profile availability agrees with runtime behavior;
9. GUI startup, PTYs, and server health do not regress;
10. a subsequent invocation succeeds after injected failure.

The suite ultimately covers Unicode, long paths, UNC, access denial,
environment inheritance, executable/argv/cwd/stdin/stdout/stderr, process
exit, concurrency, backpressure, loopback HTTP, module cycles, root escape,
Fleet receipts/events/post-state, malformed frames, worker crash, and parent
exit.

## 21. Explicitly deferred

The following are outside the v0.1.9 stable contract:

- remote package registry, dependency resolution, signing, and installation;
- npm, Cargo crate, Node.js, Bun, or complete Rust `std` compatibility;
- persistent script daemon, durable scheduler, watch mode, and REPL;
- raw sockets, listeners, WebSockets, and public network servers;
- arbitrary remote imports;
- Agent approval and natural-language authorization;
- the software marketplace and `agenterm-softmgr.exe`;
- replacing qualification, packaging, or release-critical scripts with Rhai;
- exposing executor, Tokio, or Rhai `Dynamic` internals.

Deferred nodes MAY remain visible in the catalog so users can distinguish
"not shipped yet" from "intentionally not part of this runtime."

## 22. Open design decisions

The following require spikes and public journeys before their leaves become
stable:

- the exact relationship among `Command.output`, `Child.wait_with_output`, and
  `Task`;
- whether sequential HTTP returns `HttpResponse` directly or through an
  explicitly named wait;
- whether both `Path` and `PathBuf` are useful without importing Rust borrowing
  concepts;
- foreground/background task lifetime at natural script exit;
- HTTP/TLS backend, executor, and binary-size budgets;
- local default soft budgets;
- the explicit form of destructive Fleet operations;
- whether the prelude remains exactly `args` plus `print`.

Decision order:

```text
shortest clear user path
  -> one unambiguous Rhai meaning
  -> stable cancellation and failure truth
  -> catalog generation
  -> black-box evidence
  -> implementation and binary cost
  -> optional Rust/Node/Bun comparison
```
