# AgenTerm Script Runtime Specification

Status: Draft specification for v0.1.9

Specification ID: `agenterm-script-runtime`

Script API currently shipped: v2

Catalog schema currently shipped: v3

Initial design date: 2026-07-28

Last reviewed: 2026-07-29
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
│  │  ├─ create_dir(path) / create_dir_all(path)
│  │  │  Creates one explicit directory or directory tree.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ copy(from, to) / rename(from, to)
│  │  │  Explicit-target filesystem mutation.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ remove_file(path) / remove_dir(path) / remove_dir_all(path)
│  │     Explicit-target deletion with broad-target guards.
│  │     [shipped; stable; designed 2026-07-28]
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
│  │  [partially shipped; stable delivered leaves; designed 2026-07-28]
│  │  ├─ after(duration) / sleep(duration)
│  │  │  Starts an invocation-owned timer or waits sequentially.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ wait_all(tasks)
│  │  │  Waits for all tasks with deterministic result ordering.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ race(tasks)
│  │  │  Returns the stable index of the first completed task.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ cancel_all(tasks)
│  │     Requests cancellation and returns the changed-state count.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  ├─ http::
│  │  Bounded, cancellable HTTP(S) client operations.
│  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ request(method, url, options) -> HttpResponse
│  │  │  Performs one sequential request.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ start(method, url, options) -> Task
│  │     Starts one explicit concurrent request.
│  │     [shipped; stable; designed 2026-07-28]
│  │
│  ├─ runtime::
│  │  Invocation-owned resources and safe runtime facts.
│  │  [partially shipped; stable delivered leaves; designed 2026-07-28]
│  │  ├─ temp_dir() -> PathBuf
│  │  │  Returns the current invocation's host-cleaned temporary directory.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ atomic_write(path, text)
│  │  │  Publishes UTF-8 text through same-volume atomic replacement.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ atomic_write_bytes(path, bytes)
│  │     Publishes typed bytes through same-volume atomic replacement.
│  │     [shipped; stable; designed 2026-07-28]
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
│  │  Child lifecycle, live stdout/stderr Streams, and bounded final output.
│  │  [shipped; stable; designed 2026-07-28]
│  ├─ Task
│  │  Invocation-owned timer or HTTP state with id/kind/wait/cancel facts.
│  │  [shipped timer and HTTP payloads; stable; designed 2026-07-28]
│  ├─ Stream
│  │  Invocation-owned bytes stream with bounded queue and backpressure.
│  │  [shipped for child stdout/stderr and HTTP bodies; stable; designed 2026-07-28]
│  │  ├─ .id / .kind / .state / .buffered_bytes
│  │  │  Reports stable identity, bytes kind, lifecycle, and queued bytes.
│  │  ├─ .read(max_bytes[, timeout]) / .collect(max_bytes[, timeout])
│  │  │  Reads one bounded chunk or collects the remaining bounded stream.
│  │  └─ .close() / .truncated / .complete
│  │     Cancels consumption and distinguishes incomplete capture from EOF.
│  ├─ HttpResponse
│  │  Status, HTTP version, bytes-first headers, and a bounded body Stream.
│  │  [shipped; stable; designed 2026-07-28]
│  └─ Receipt / Event / PostState
│     Fleet mutation evidence and verified resulting state.
│     [shipped; stable; designed 2026-07-28]
│
├─ fleet
│  Canonical Script API v2 object bound to one AgenTerm server and broker.
│  [shipped; stable; designed 2026-07-28]
│  ├─ .protocol.info()
│  │  Reads protocol, build, command, operation, and event discovery facts.
│  │  [shipped; stable; designed 2026-07-28]
│  ├─ .workspace
│  │  Typed workspace identity and state.
│  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .info()
│  │  │  Reads workspace metadata with an event position.
│  │  │  [shipped; stable; designed 2026-07-28]
│  │  └─ .shutdown() -> Receipt
│  │     Executes the native destructive workspace operation.
│  │     [shipped local only; stable; designed 2026-07-28]
│  ├─ .tabs
│  │  Typed tab discovery and mutation from the public operation catalog.
│  │  [shipped; stable; designed 2026-07-28]
│  │  ├─ .list() / .active()
│  │     Reads stable tab objects.
│  │     [shipped; stable; designed 2026-07-28]
│  │  └─ .set_note(tab_id, note) -> Receipt
│  │     Mutates one stable tab and verifies its event and post-state.
│  │     [shipped local only; stable; designed 2026-07-28]
│  ├─ .ui.snapshot()
│  │  Reads the bounded semantic UI snapshot.
│  │  [shipped; stable; designed 2026-07-28]
│  ├─ .ui.tabs
│  │  Controls the sidebar through typed native operations.
│  │  [shipped local only; stable; designed 2026-07-28]
│  │  ├─ .show() / .hide() / .toggle() -> Receipt
│  │  └─ .set_width(width) -> Receipt
│  ├─ .terminal(tab_id)
│  │  Binds a stable tab ID and exposes .capture(max_bytes).
│  │  [shipped observation slice; stable; designed 2026-07-28]
│  ├─ .events
│     Reads, waits for, or starts waits over the typed event journal.
│     [shipped read/wait slice; stable; designed 2026-07-28]
│  ├─ .server.kill([target]) -> Receipt
│  │  Executes the native destructive server operation.
│  │  [shipped local only; stable; designed 2026-07-28]
│  └─ .operations()
│     Lists all catalog-derived operations and profile availability.
│     [shipped; stable; designed 2026-07-28]
│
├─ project composition
│  Local modules and named task discovery.
│  [shipped; stable mechanism; designed 2026-07-28]
│  ├─ import "relative/module" as module
│  │  Resolves only inside the declared project root.
│  │  [shipped; stable; designed 2026-07-28]
│  └─ agenterm.tasks.json
│     Versioned local task manifest; not a package manifest.
│     [shipped schema v1; stable; designed 2026-07-28]
│
└─ discovery
   Offline and runtime-aligned interface inspection.
   [partially shipped; stable mechanism; designed 2026-07-28]
   ├─ script api --json
   │  Emits the machine-readable catalog and runtime limits.
   │  [shipped; stable; designed 2026-07-28]
   ├─ script api [MODULE] [--status shipped|planned|all]
   │  Renders a deterministic filtered object tree from stable catalog IDs.
   │  [shipped; stable; designed 2026-07-28]
   ├─ script check FILE
   │  Validates syntax, profile availability, and known interfaces offline.
   │  [shipped baseline; stable; designed 2026-07-28]
   └─ script task list / show / run
      Discovers and invokes named local tasks.
      [shipped; stable; designed 2026-07-28]
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

### 2.1 Research comparison contract

Catalog schema v3 adds one reviewed Node.js and Bun classification to every
entry. These fields exist for horizontal discovery, gap analysis, generated
trees, and future reference-manual generation. They MUST NOT be interpreted as
source, module, binary, behavioral, or package compatibility.

Each `comparisons.nodejs` and `comparisons.bun` object contains:

- `relationship`: `similar`, `agenterm_specific`, `deferred`, or
  `not_applicable`;
- `path`: the closest public analogue when the relationship is `similar`;
- `documentation` and `reviewed_version`: the reviewed external reference;
- `reviewed_on`: the date of the comparison review;
- `semantic_note`: the important reason AgenTerm behavior differs.

The initial comparison set was reviewed on 2026-07-29 against
[Node.js 26.5.0 API documentation](https://nodejs.org/docs/latest/api/) and
[Bun 1.3.14 runtime documentation](https://bun.com/docs/runtime/bun-apis).
Those versions identify research inputs only. Updating a comparison never
renames or changes a stable AgenTerm Rhai interface.

`agenterm-script api --tree` renders a compact Rust/Node.js/Bun index from the
same entries returned by `agenterm-script api --json`. Long-form prose may add
examples and rationale, but exact callable identity, status, signatures,
availability, limits, and comparisons come from the machine catalog.

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
`rhai::runtime::temp_dir()` returns a typed `PathBuf` for a directory owned by
the current local invocation. The host removes it after success, script
failure, worker crash, or timeout; the next invocation also prunes roots whose
owning client process died before normal cleanup.
`rhai::runtime::atomic_write(path, text)` and `atomic_write_bytes(path, bytes)`
write and sync a unique sibling staging file before a same-volume atomic
replacement. Failed publication removes the staging file and never reports a
partial target as success.
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

`std::env::get`, `has`, `names`, and `current_dir` observe the worker
environment. `get` is the Rhai spelling of Rust `std::env::var` because `var`
is reserved by the language. Environment values are available to the running
script but MUST NOT be copied into retained audit or diagnostics.
`Command.env`, `env_remove`, and `env_clear` configure only the child; they do
not mutate the AgenTerm host.

The shipped process defaults are a 2,000 ms child deadline and 64 KiB retained
for each captured stream. A script MAY lower or raise them through
`Command.timeout(Duration)` and `capture_limit(bytes)`, up to hard ceilings of
10,000 ms and 256 KiB. Text stdin is limited to 256 KiB.

`Output.stdout` and `.stderr` are `Bytes`. `stdout_text()` and `stderr_text()`
perform strict UTF-8 decoding. `.truncated` MUST become true if either stream
exceeds its retained capture; readers continue draining discarded bytes so a
full pipe cannot deadlock the child. `.complete` is true only after the child
and both captured streams reach terminal state.
`Output.require_success(code)` returns normally for exit code zero and throws
the stable `child_nonzero` failure otherwise. `code` is a caller-selected,
privacy-safe identifier limited to 1–64 lowercase ASCII letters, digits,
periods, underscores, or hyphens. Scripts that accept nonzero status inspect
`Output.success` and `exit_code` directly instead.

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

The public result envelope and process exit status use these stable classes:

| Exit class | Process code | Meaning |
|---|---:|---|
| `success` | 0 | The script and required foreground work completed. |
| `script` | 1 | Rhai parse, runtime, result conversion, or user failure. |
| `protocol` | 1 | Worker framing or identity failure. |
| `host` | 1 | Worker launch, crash, or host invariant failure. |
| `configuration` | 2 | Invalid arguments, profile, manifest, or unavailable API. |
| `limit` | 3 | A time, operation, value, output, task, or stream ceiling. |
| `child` | 4 | Child execution failed, or required child status was nonzero. |
| `cancelled` | 5 | Explicit cooperative invocation cancellation. |
| `fleet` | 6 | Fleet transport, restart, event, receipt, or post-state failure. |

Unhandled child runtime failures and `Output.require_success(code)` use
`child`; Fleet broker failures use `fleet`. A Rhai `try/catch` may deliberately
handle either failure and return a successful result.

`Output.require_success(code)` is the first shipped catchable typed-error
slice. Its caught value contains every field listed above, and an unhandled
instance drives the CLI `child` result from that same object. Other runtime
APIs still migrate incrementally from stable coded strings to this object;
their documented codes and outer result classification remain stable during
that migration.

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
let response = web.wait(std::time::Duration::from_secs(10));
```

`Task` MUST have an invocation-local stable ID, state, wait, cancel, and stable
terminal outcome. Late completion MUST NOT overwrite `cancelled`.

The shipped task payloads are timers and HTTP responses. `after(Duration)`
starts a timer, while `sleep(Duration)` provides the sequential form.
`wait_all` preserves input ordering, `race` returns the winning input index,
and `cancel_all` returns the number of tasks whose state changed. `Task.kind`
distinguishes `timer` from `http`; `Task.state` also exposes a stable `failed`
terminal state. Composition and active host work each accept at most 64 tasks.
Wait timeouts do not silently cancel a still-pending task.

The shipped first `Stream` payload is child stdout/stderr. `Child.stdout` and
`Child.stderr` return invocation-local bytes streams. Each producer has a
64 KiB queue; a full queue blocks that producer until `read`, `collect`, or
`wait_with_output` drains space. Each read is limited to 64 KiB. Collect and
the cumulative final capture are additionally bounded by the invocation's
capture ceiling (256 KiB hard maximum).

`Stream.state` is `pending`, `readable`, `closed`, `failed`, or `cancelled`.
`read(max_bytes[, timeout])` returns an empty `Bytes` only after clean EOF.
`collect(max_bytes[, timeout])` consumes the remaining stream. `close()` wakes
a backpressured producer, marks the stream cancelled and incomplete, and is
idempotent. `truncated` and `complete` remain independent facts; truncated or
consumer-closed data MUST NOT be reported as complete.

`Child.wait_with_output` drains both live queues while retaining one separately
bounded final capture. Reading a live stream therefore does not remove bytes
from the final `Output`, and a child producing more than one queue can still
make progress when the caller chooses the final-output path. Stream-delivery
truncation and final-capture truncation are distinct: a Stream may deliver the
whole body and report `complete=true` while the separately bounded `Output`
reports `truncated=true` and `complete=false`.

The public Task/Stream contract is executor-neutral. Tokio or any other
executor type MUST NOT enter the Rhai API.

### 10.1 HTTP client

`rhai::http` is a client-only AgenTerm extension. It is deliberately not
`std::http`, because Rust `std` has no high-level HTTP client. The shipped
methods are `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, and `OPTIONS`;
only absolute `http` and `https` URLs are accepted.

The optional map supports:

```text
headers          map<string, string | array<string>>
body             string | Bytes
timeout          std::time::Duration
max_body_bytes   integer
max_redirects    integer
proxy            false | proxy URL
```

An absent `proxy` uses the process `HTTP_PROXY`, `HTTPS_PROXY`, and
`NO_PROXY` environment contract. `false` disables proxy discovery for that
request. The default timeout is 2 seconds and the hard maximum is 10 seconds.
The default response-body ceiling is 64 KiB and the hard ceiling is 256 KiB;
request bodies are also limited to 256 KiB. URLs are limited to 8 KiB,
request headers to 64 fields and 32 KiB, and redirects to 10.

`HttpResponse.status` is an integer, `version` is a stable HTTP version string,
and `headers` maps lower-case names to arrays of `Bytes`, preserving duplicate
values. `header(name)` returns the corresponding array. `body` is a
bytes-first `Stream`; data beyond `max_body_bytes` is discarded and reported
with `truncated=true` and `complete=false`.

HTTPS uses the platform TLS implementation and platform root verifier. Public
errors expose stable categories such as `http_timeout`, `http_proxy`,
`http_tls`, and `http_transport`; they never retain or echo the URL,
credentials, header values, or body.

`http::start` returns a `Task<HttpResponse>`. Explicit cancellation changes
the Task to `cancelled` immediately, wakes waiters, and prevents a late
transport completion from overwriting that terminal state. The underlying
blocking transport remains bounded by the request's maximum 10-second
deadline and by supervisor process cleanup; prompt in-process socket abort is
not part of this first transport adapter.

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

Script API v2 exposes `fleet` as its only canonical facade. The former v1
`agent` object is not registered as an alias; `script check` reports
`script_api_migrated` with the matching v2 path. Every operation in the public
typed operation catalog has exactly one Script API entry. Read-only operations
are available to `observe` and `local`; control and destructive operations are
available only to `local` and are independently revalidated by the host broker.

Mutation methods return a typed `Receipt`. It carries the native control
receipt, bounded correlated events, and a `PostState` containing `verified`,
`reason`, and the resulting public state. A missing native receipt fails
closed. When a destructive operation makes subsequent observation impossible,
`verified` remains false with `destructive_post_state_unavailable` rather than
inventing success evidence.

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
The public executable is `agenterm-script.exe`; `agenterm-cli.exe script ...`
is a compatibility route to the same parser, catalog, supervisor, and runtime.
The reserved `--worker` and `--framed-worker` modes are internal host protocol
entry points, not alternate user APIs.

Schema v1 is:

```json
{
  "schema_version": 1,
  "project": {"id": "daily-tools", "version": "1.0.0"},
  "tasks": [
    {
      "id": "daily-check",
      "description": "Run the local daily check",
      "entry": "tasks/daily-check.rhai",
      "profile": "local",
      "cwd": ".",
      "args": [],
      "env": ["REQUIRED_ENV_NAME"]
    }
  ]
}
```

Project/task IDs, project version, entry, profile, working directory, default
arguments, and required environment names are inspectable without execution.
`env` contains names only; values are inherited at invocation and are never
copied into the manifest, task catalog, audit, or diagnostics. Entries and
working directories MUST resolve inside the manifest directory. Discovery
walks from the current directory to its ancestors unless `--manifest` is
explicit. Task execution appends caller arguments after manifest defaults.

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

Human-facing `api` output shows the stable-ID object tree and accepts one
module selector plus `--status shipped|planned|all`. Selectors recognize stable
IDs, Rhai surface paths, and catalog taxonomy paths; `::`, `/`, and `.` are
normalized only for selection and do not rename the returned identities.
Unknown modules and statuses fail with stable configuration codes.

`api --json` retains the ordinary result envelope and exact catalog schema. It
filters `entries` identically and adds a `view` object containing `module`,
`status`, and `entry_count`. Ordering is deterministic. Comparison metadata and
generated comparison/manual pages remain separate work and MUST NOT be inferred
from the Rust mapping fields alone.

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
output.require_success("git-status-failed");

print(output.stdout_text());
```

### 19.3 Shipped concurrent HTTP and process work

```rhai
let command = std::process::command("git");
command.args(["rev-parse", "HEAD"]);

let git = command.start();
let release = rhai::http::start("GET", release_url, #{
    timeout: std::time::Duration::from_secs(10)
});

let commit = git.wait_with_output();
let response = release.wait(std::time::Duration::from_secs(10));
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
- whether both `Path` and `PathBuf` are useful without importing Rust borrowing
  concepts;
- foreground/background task lifetime at natural script exit;
- whether a future transport adapter should add prompt in-process socket abort
  beyond Task cancellation, bounded deadlines, and supervisor cleanup;
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
