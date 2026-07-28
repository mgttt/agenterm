# Rust host + Rhai scripting

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Runtime contract: [AgenTerm Script Runtime specification](../docs/agenterm-script-runtime.md)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Shipped baseline

- [x] `agenterm-script.exe` executes pure Rhai `run`, `eval`, `check`, and API
  discovery in a fresh sidecar process.
- [x] the explicit `observe` profile exposes typed, bounded workspace, tab,
  snapshot, capture, and event broker methods without direct Win32, PTY, or
  mutable GUI-state access.
- [x] the explicit `pure` profile provides deterministic JSON-compatible
  values, arguments, bounded computation, and captured stdout without ambient
  filesystem, environment, process, network, clock, terminal, or Fleet access.
- [x] a Rhai-independent supervisor owns a kill-on-close Windows Job Object,
  parent deadline, cooperative cancellation followed by forced termination,
  concurrency ceilings, and worker cleanup.
- [x] a versioned inherited-pipe frame protocol separates invocation, broker
  request/response, cancellation, and result frames; script stdout cannot
  corrupt protocol frames.
- [x] source, value, operation, call/expression depth, collection/string,
  output, wall-time, broker, capture, event, wait, and concurrency limits have
  typed failure classes and immutable hard ceilings.
- [x] privacy-bounded audit records contain identity, source fingerprint and
  label, API/profile/budget facts, broker operation IDs, duration, result
  class, denial, cancellation, timeout, and crash, but never source, argv,
  output, pane content, environment values, clipboard data, or credentials.
- [x] normal GUI startup constructs no Rhai engine, scans no script directory,
  and remains independent of Rhai engine types.
- [x] Design choice: Rust (`.rs`) implements the host and Rhai (`.rhai`)
  implements user-authored runtime programs.

## v0.1.9 product position

`agenterm-script.exe` is AgenTerm's general-purpose local scripting runtime:
Rhai language plus a Rust-shaped selected `std::` subset, Rhai-native
extensions, and the AgenTerm-bound Fleet domain. This is a capability overlay,
not Rust, Node.js, Bun, npm, Cargo, or another Rhai host compatibility layer,
and it is not positioned as a restricted security plugin.

- Rust std is the primary naming and object-model reference where a stable
  analogue exists, but every mapping declares how Rhai errors, types, blocking,
  cancellation, limits and Windows behavior differ;
- Node.js and Bun are coverage and use-case references, not API-shape
  specifications. AgenTerm does not inherit callback/Promise duality,
  sync/async duplication, legacy aliases, module-resolution compatibility, or
  platform history merely because an analogue exists;
- each domain selects one AgenTerm-native, Rhai-native, typed, Windows-first,
  bounded, cancellable, and observable contract. Compatibility aliases require
  a concrete AgenTerm migration need rather than resemblance to another
  runtime;

- an explicit human invocation of ordinary `script run` or `script eval`
  defaults to `local`, with the authority expected of an ordinary local
  program launched by that user;
- `pure` and `observe` remain explicit specialized profiles for deterministic
  and read-only Fleet use, not the platform capability ceiling;
- technical budgets, typed errors, cancellation, process isolation, audit
  privacy, and product data-integrity checks remain mandatory in every mode;
- agent-specific tool visibility, approval, path/domain/target policy,
  credentials, quotas, and natural-language intent belong to a future agent
  layer, not to this runtime;
- local scripts do not bypass native product invariants: live close
  confirmation, remain-on-exit, stable IDs, tree-cycle rejection, replay
  protection, and truthful typed outcomes continue to apply.

## v0.1.9 runtime architecture

- [ ] one invocation still owns one fresh `agenterm-script.exe` sidecar; it is
  not a persistent system daemon and keeps no mutable state across invocations.
- [ ] an invocation may own a bounded task scheduler. Asynchronous APIs return
  typed task handles consumed through `wait` and bounded `stream` operations.
- [ ] the Rhai engine and its `Scope` remain on one evaluation thread.
  Background I/O stores Rust-native typed payloads and bytes in an
  invocation-owned registry; only the evaluation thread converts completion
  values into Rhai `Dynamic`, so host concurrency does not require sharing
  script values or the engine across threads.
- [ ] the public Task/Stream contract is executor-neutral. A bounded worker/
  channel implementation and a small Rust async executor are compared by
  cancellation correctness, streaming simplicity, dependency/binary cost and
  throughput before selecting an implementation; Tokio is not an inherited
  requirement.
- [ ] the sidecar remains alive while reachable tasks, timers, child-process
  I/O, HTTP bodies, or Fleet waits are active, and exits naturally when no
  foreground task remains.
- [ ] Ctrl+C, parent exit, timeout, server restart, task cancellation, and
  worker failure propagate to every owned task and stream without orphaning a
  child, blocking the GUI, or damaging PTYs or workspace state.
- [ ] task and stream queues have explicit item/byte/concurrency limits and
  backpressure; truncation, cancellation, and incomplete output cannot be
  reported as success.
- [ ] a bounded compiled-AST cache may be keyed by source fingerprint, API
  version, runtime version, and profile, but is not required for the first
  usable local-runtime slice.

## Rust-shaped subset and Rhai-native extensions

- [ ] namespace ownership is explicit: `std::` contains only selected
  capabilities with an honest Rust standard-library analogue; `rhai::`
  contains runtime-native higher-level extensions; `fleet` remains a bound
  AgenTerm object; Rhai language primitives, project modules, manifests and
  CLI discovery are not wrapped in artificial namespaces.
- [ ] `std::fs` covers bounded `read`, `read_to_string`, `write`, directory
  listing/creation, metadata, copy, rename and explicit-target deletion.
- [ ] `std::path` provides a selected `Path`/`PathBuf` object model for Windows
  normalization, composition, relative paths, working directories, Unicode,
  long paths and canonical/reparse-point facts without copying Rust borrowing.
- [ ] `std::env` reads/enumerates worker environment and current-directory
  facts; worker-local mutation and child environment inheritance/overlay/
  replace/remove semantics are explicit and never leak values to diagnostics.
- [ ] `std::process` uses the Rust-shaped `Command -> Child/Output` model with
  executable plus argv, cwd, env, stdin, bounded stdout/stderr, timeout,
  cancellation and typed exit state; it never substitutes an implicit shell
  command string or exposes Rust ownership/trait/OS-handle internals.
- [ ] `std::time` provides selected `Duration`, `Instant`, and `SystemTime`
  values while keeping monotonic deadlines and wall time distinct; high-level
  sleep/timer/task composition is not misrepresented as Rust `std`.
- [ ] `rhai::task` owns executor-neutral Task/Stream composition, cancellable
  sleep/timer, wait-all, race, cancel and bounded backpressure.
- [ ] `rhai::json` plus Rhai-native strings and a typed `Bytes` object provide
  bounded parsing, serialization, Unicode/encoding and explicit conversions
  without duplicating language primitives as fake Rust collections.
- [ ] `rhai::http` provides HTTP(S) method, URL, headers, body, timeout,
  status, bounded response streaming, cancellation, proxy/TLS diagnostics and
  credential-safe errors. Rust std has no high-level HTTP client, so
  `std::http` is forbidden; low-level sockets are outside v0.1.9.
- [ ] `rhai::runtime` may expose only safe invocation/API/profile/version/
  limits facts, never private supervisor, HWND, renderer, PTY or broker
  handles.
- [ ] filesystem and temporary-resource helpers have explicit ownership and
  cleanup behavior. Canonicalization, reparse points, atomic replacement, and
  failure paths cannot silently target a different path than the result
  reports.
- [ ] the catalog taxonomy is not copied into the script surface.
  Resource-bearing values use custom-type methods (`Child.wait`,
  `Task.cancel`, `Stream.read`, response/output access), while modules,
  named-task manifests, catalog and diagnostics remain language/CLI mechanisms
  rather than artificial runtime namespaces.
- [ ] globals remain minimal (`args` and `print` baseline). Native Rhai string
  and collection operations are reused instead of wrapping every value under
  `data`; `system`, `network`, `code-and-automation`, and `observability` are
  catalog/manual groupings, never mandatory call prefixes.

## Local modules, tasks, and named commands

- [ ] local modules resolve from an explicit script/project root using
  deterministic relative paths; missing modules, root escape, cycles, duplicate
  identities, incompatible versions, and parse failures are typed.
- [ ] project tasks use one versioned declarative manifest that maps stable task
  IDs to a script/module entry point, arguments, working directory, environment
  construction, and execution profile.
- [ ] TOML versus reuse of an existing AgenTerm manifest encoding is the one
  remaining implementation-before-parser decision. v0.1.9 selects a versioned
  `agenterm.tasks.json` format and records the exact schema before
  task-manifest parsing begins, but does not block the selected `std::` or
  `rhai::task` contracts.
- [ ] project tasks and user-level named commands are discoverable through one
  typed catalog. Invalid entries remain visible with a stable degraded reason
  instead of disappearing.
- [ ] CLI listing, inspection, and invocation of named tasks is P0 for v0.1.9.
  A GUI command palette is a P1 consumer of the same catalog and does not own a
  second registry.

## AgenTerm Fleet API

- [ ] the canonical bound user facade is `fleet`, because it carries selected
  server, profile and broker identity. It exposes typed workspace, tabs,
  terminal and events service objects; ordinary calls do not require users to
  type raw operation IDs even though results and the catalog retain operation,
  request, receipt, event and post-state identities.
- [ ] v0.1.9 proposes Script API v2 and removes the ambiguous v1 `agent`
  facade rather than retaining a permanent alias that conflicts with the
  future `agenterm-agent.exe`; `check` emits a targeted migration diagnostic
  for old `agent.*` source.
- [ ] generate the script-facing Fleet API systematically from the public typed
  operation catalog rather than maintaining a hand-selected parallel list.
- [ ] every entry exposes stable operation ID, classification, typed
  parameters/result/errors, resolved target rules, receipt/wait behavior,
  side-effect facts, version, and availability.
- [ ] an operation that cannot be represented safely remains discoverable as
  unsupported/degraded with a typed reason; it is never silently omitted or
  reported as successful.
- [ ] observation covers workspace, server, tabs/tree, focus, UI, terminal
  capture/viewport/lifecycle, and Observable Fleet reads and waits.
- [ ] explicit mutations cover tab/tree metadata, Composer, terminal input and
  viewport, workspace, and lifecycle operations as their underlying catalog
  contracts become complete.
- [ ] destructive calls use explicit names and arguments, require the native
  confirmation or documented noninteractive operation contract, carry request
  identity/deadline/receipt, and preserve remain-on-exit, close, tree, replay,
  and server-lifecycle invariants.
- [ ] the control-plane catalog, dispatch, receipt, error, event-correlation,
  replay, and deterministic-wait prerequisites are owned by
  [Agent control plane](PRD_02_07_agent_control_plane.md); scripting consumes
  that authority and never reads private GUI state.

## Discovery and tool schema

- [ ] `script api --json` is the exact versioned runtime catalog and matches
  the engine, standard library, modules/tasks, profiles, Fleet operations,
  defaults, hard ceilings, and availability.
  - [x] catalog schema v2 separates its schema version from Script API v1 and
    provides one typed source for shipped v1 observations plus explicitly
    planned v0.1.9 nodes; full engine/module/task conformance remains open.
  - [x] an explicit `local` profile foundation runs base Rhai without requiring
    a server or inheriting observe authority. It remains opt-in until the first
    useful local standard-library slice ships.
- [ ] each callable entry describes stable ID, signature, result/error schema,
  filesystem/process/network/Fleet access, mutation and destructive facts,
  expected duration, cancellation and streaming support, and any dry-run or
  inspect support.
- [ ] the catalog is hierarchical, using stable
  `domain -> capability group -> callable/type` paths and ordering. Human
  `script api [MODULE]` output renders that same tree; unavailable, degraded,
  planned, deferred, and intentionally out-of-scope nodes do not disappear.
- [ ] every entry separates `catalog_path` from its shallow `surface_path`, so
  product taxonomy can evolve without forcing nested namespaces into user
  source or silently renaming callable contracts.
  - [x] schema v2 entries carry both paths and a stable ID; the current
    `agent.*` v1 surface is reported honestly while `fleet` migration remains
    planned.
- [ ] every entry also carries nullable `rust_path`, a mapping level
  (`direct`, `adapted`, `inspired`, or `none`) and machine-readable semantic
  differences for error, type, blocking, cancellation, platform and limits.
  An API without an honest Rust std analogue cannot enter `std::`.
  - [x] schema v2 establishes these fields and publishes semantic differences
    for shipped and planned entries.
- [ ] optional comparison metadata maps a capability to a reviewed Node.js or
  Bun analogue as `similar`, `agenterm-specific`, `deferred`, or
  `not-applicable`, with source/version and review date. It supports gap
  analysis and generated manuals but never claims JavaScript, Node, Bun, npm,
  module, or binary compatibility.
- [ ] the API tree, comparison matrix, reference-manual index, and coverage
  report are generated from the catalog; a second handwritten callable list is
  rejected by alignment tests.
- [ ] these are capability facts for people and future tool consumers, not an
  authorization decision. A future agent layer may filter or constrain the
  schema without reimplementing the runtime.
- [ ] `script check` validates imports, task entries, API names, profiles,
  signatures, versions, static limits, and unavailable/degraded calls without
  executing user code or requiring a GUI.
- [ ] runtime, module and task identities expose version, origin/provenance
  hooks, required AgenTerm API/capabilities and stable entry-point metadata so
  future local package tooling can inspect them without executing source.
  This is a package-ready contract, not a registry, downloader, installer,
  signature policy or second package manifest in v0.1.9.

## Public black-box acceptance

- [ ] tests invoke only released `agenterm-cli script` commands and compare the
  offline catalog with actual runtime behavior.
- [ ] isolated temporary roots cover Unicode and long paths, metadata,
  directory operations, atomic replacement, interruption, access failure, and
  cleanup without changing files outside the fixture.
- [ ] process fixtures cover argv boundaries, spaces, Unicode, cwd, env, stdin,
  separate stdout/stderr, nonzero exit, timeout, cancellation, parent exit,
  backpressure, and orphan-free cleanup.
- [ ] an independent loopback HTTP fixture covers request/response, headers,
  body, status, bounded streaming, timeout, cancellation, malformed data,
  connection failure, proxy/TLS-safe diagnostics, and no public-network
  dependency.
- [ ] timer and task fixtures prove concurrent progress, deterministic wait and
  stream results, natural worker exit, cancellation propagation, bounded
  queues, and recovery on the next invocation.
- [ ] module/task fixtures cover roots, relative imports, cycles, duplicate and
  missing modules, manifest version/error handling, named-task discovery,
  degraded entries, arguments, and working directory.
- [ ] Fleet conformance compares every operation-catalog entry with its script
  exposure or explicit degraded reason; mutations verify typed receipts,
  correlated public post-state/events, no duplicate side effect, and honest
  close/send/restart failures.
- [ ] local mode proves the general runtime loop while regression fixtures keep
  pure deterministic and observe read-only.
- [ ] script error, worker crash, timeout, cancellation, parent exit, server
  restart, and unfinished task cleanup leave GUI, PTY, workspace, and the next
  script invocation healthy.
- [ ] retained diagnostics and audit fixtures prove that file content,
  arguments, environment secrets, HTTP credentials/bodies, terminal content,
  and script stdout do not leak.
- [ ] every slice records worker/CLI/GUI size, first-window/no-script startup,
  duration, limits, and orphan cleanup before its status changes to shipped.

## Explicitly deferred

- npm compatibility, arbitrary remote imports, third-party package lifecycle,
  and Node/Bun binary or API compatibility; an AgenTerm-owned signed
  package/application catalog remains a future optional-component track rather
  than npm emulation inside the script runtime;
- a persistent or automatically started script daemon and cross-invocation
  mutable runtime state;
- low-level sockets, unsolicited listeners, and a general network sidecar;
- event handlers, watch mode, REPL, and durable background scheduling unless a
  later owned slice supplies separate acceptance;
- agent permission, approval, credential, quota, and natural-language policy;
- GUI command palette delivery beyond its P1 consumption of the shared named
  task catalog.
