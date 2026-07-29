# Rust host + Rhai scripting

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Runtime contract: [AgenTerm Script Runtime specification](../docs/agenterm-script-runtime.md)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Shipped baseline

- [x] `agenterm-script.exe` is the public `run`, `eval`, `check`, `api`, and
  named-task CLI while retaining private `--worker`/`--framed-worker` modes;
  `agenterm-cli.exe script ...` is a thin compatibility route to the same
  catalog, parser, supervisor, and runtime.
- [x] every ordinary invocation receives the same unrestricted local runtime
  surface. No invocation mode, task label, profile, catalog capability, caller,
  or entry point gates API registration or execution; the future Agent harness
  owns Agent permissions outside this executable.
- [x] Script API v2 maps every current typed operation exactly once to `fleet` and verifies mutation receipts, correlated events, and post-state.
- [x] task-manifest schema v2 publishes an inclusive required Script API range
  and stable capability IDs; list/show preserve incompatible projects for
  inspection while check/run fail closed before source execution.
- [x] the public `examples/script-daily-check` north-star task combines Unicode
  configuration, invocation-owned temp, two concurrent argv-safe children,
  loopback HTTP, JSON aggregation, typed Fleet note mutation, atomic result
  publication, restoration, and orphan-free cleanup in one invocation.
- [x] deterministic JSON-compatible values, arguments, bounded computation,
  and captured stdout are ordinary facilities in the same complete runtime
  surface.
- [x] a Rhai-independent supervisor owns a kill-on-close Windows Job Object,
  parent deadline, cooperative cancellation followed by forced termination,
  concurrency ceilings, and worker cleanup.
- [x] a versioned inherited-pipe frame protocol separates invocation, broker
  request/response, cancellation, and result frames; script stdout cannot
  corrupt protocol frames.
- [x] source, value, operation, call/expression depth, collection/string,
  output, wall-time, broker, capture, event, wait, and concurrency limits have
  typed failure classes and immutable hard ceilings.
- [x] public result envelopes expose stable success, configuration, limit,
  script, child, cancelled, Fleet, protocol, and host classes; the CLI maps
  them to documented process codes, and `Output.require_success(code)`
  explicitly propagates a required nonzero child exit.
- [x] `Output.require_success(code)` is the first catchable typed-error slice:
  Rhai receives class, code, operation, safe message, retryability, target
  kind, truncation, and optional cause class from the same object used by the
  unhandled CLI result; other public runtime errors migrate incrementally.
- [x] privacy-bounded audit records contain identity, source fingerprint and
  label, API/runtime/budget facts, broker operation IDs, duration, result
  class, failure, cancellation, timeout, and crash, but never source, argv,
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

- the AgenTerm Rhai object/interface tree is the primary stable contract.
  Rust std is a naming and object-model research reference where an honest
  analogue exists, but upstream Rust stability or change never drives a Rhai
  rename or semantic change;
- Node.js and Bun are coverage and use-case references, not API-shape
  specifications. AgenTerm does not inherit callback/Promise duality,
  sync/async duplication, legacy aliases, module-resolution compatibility, or
  platform history merely because an analogue exists;
- each domain selects one AgenTerm-native, Rhai-native, typed, Windows-first,
  bounded, cancellable, and observable contract. Compatibility aliases require
  a concrete AgenTerm migration need rather than resemblance to another
  runtime;

- an explicit human invocation of ordinary `script run` or `script eval`
  receives the same unrestricted local-program capabilities expected by that
  user; Script Runtime does not make Agent authorization decisions;
- deterministic computation and read-only Fleet access are libraries and use
  patterns, never profiles that remove filesystem, process, network, terminal,
  or mutation APIs;
- technical budgets, typed errors, cancellation, process isolation, audit
  privacy, and product data-integrity checks are runtime robustness controls,
  never permissions;
- [x] explicit local repository tasks may raise their invocation wall-time
  budget to the stable 120-second hard ceiling; child-process deadlines retain
  an independent 60-second ceiling while HTTP retains its stricter 10-second
  ceiling;
- agent-specific tool visibility, approval, path/domain/target policy,
  credentials, quotas, and natural-language intent belong exclusively to the
  future Agent harness, which may constrain an Agent before invoking this
  unrestricted runtime;
- local scripts do not bypass native product invariants: live close
  confirmation, remain-on-exit, stable IDs, tree-cycle rejection, replay
  protection, and truthful typed outcomes continue to apply.

## v0.1.9 runtime architecture

- [x] one invocation still owns one fresh `agenterm-script.exe` sidecar; it is
  not a persistent system daemon and keeps no mutable state across invocations.
- [x] an invocation may own a bounded task scheduler. Asynchronous APIs return
  typed task handles consumed through `wait` and bounded `stream` operations.
- [x] the Rhai engine and its `Scope` remain on one evaluation thread.
  Background I/O stores Rust-native typed payloads and bytes in an
  invocation-owned registry; only the evaluation thread converts completion
  values into Rhai `Dynamic`, so host concurrency does not require sharing
  script values or the engine across threads.
- [x] the public Task/Stream contract is executor-neutral. A bounded worker/
  channel implementation and a small Rust async executor are compared by
  cancellation correctness, streaming simplicity, dependency/binary cost and
  throughput before selecting an implementation; Tokio is not an inherited
  requirement.
- [x] the sidecar remains alive while reachable tasks, timers, child-process
  I/O, HTTP bodies, or Fleet waits are active, and exits naturally when no
  foreground task remains.
- [~] Ctrl+C, parent exit, timeout, server restart, task cancellation, and
  worker failure propagate to every owned task and stream without orphaning a
  child, blocking the GUI, or damaging PTYs or workspace state.
- [x] task and stream queues have explicit item/byte/concurrency limits and
  backpressure; truncation, cancellation, and incomplete output cannot be
  reported as success.
- [ ] a bounded compiled-AST cache may be keyed by source fingerprint, API
  version, and runtime version, but is not required for the first
  usable local-runtime slice.

## Rust-shaped subset and Rhai-native extensions

- [x] namespace ownership is explicit: `std::` contains only selected
  capabilities with an honest Rust standard-library analogue; `rhai::`
  contains runtime-native higher-level extensions; `fleet` remains a bound
  AgenTerm object; Rhai language primitives, project modules, manifests and
  CLI discovery are not wrapped in artificial namespaces.
- [~] `std::fs` covers bounded `read`, `read_to_string`, `write`, directory
  listing/creation, metadata, copy, rename and explicit-target deletion.
  - [x] blocking `read`, `read_to_string`, text/bytes `write`, `exists`,
    directory creation, copy, rename and explicit-target file/directory/tree
    removal ship through the public CLI. Destructive helpers reject empty,
    root, current-workspace and ancestor targets; metadata, directory listing
    and cumulative byte budgets remain open.
- [~] `std::path` provides a selected `Path`/`PathBuf` object model for Windows
  normalization, composition, relative paths, working directories, Unicode,
  long paths and canonical/reparse-point facts without copying Rust borrowing.
  - [x] first local slice ships typed `PathBuf::from`, join, display, file name,
    extension and absolute-path facts; canonical, UNC/reparse and long-path
    policy remains open.
- [x] `std::env` reads/enumerates worker environment and current-directory
  facts; worker-local mutation and child environment inheritance/overlay/
  replace/remove semantics are explicit and never leak values to diagnostics.
  Process-global mutation remains deferred; child overlay/clear/remove ships.
- [x] `std::process` uses the Rust-shaped `Command -> Child/Output` model with
  executable plus argv, cwd, env, stdin, bounded stdout/stderr, timeout,
  explicit kill and typed exit state; it never substitutes an implicit shell
  command string or exposes Rust ownership/trait/OS-handle internals. Children
  are invocation-owned and inherit supervisor process-tree cleanup;
  `std::process::id()` exposes the current supervised worker PID for
  owned-resource naming and live-owner protocols without treating it as stable
  invocation identity.
  `std::process::list()` exposes an unrestricted PID-sorted typed
  operating-system process inventory (`ProcessInfo.id` and
  `.executable_name`) on Windows, Linux, and macOS; it is a point-in-time
  observation rather than an Agent allowlist or authorization surface.
  `Command.start()` is the script spelling because Rhai reserves `spawn`;
  catalog metadata retains the Rust `Command::spawn` comparison.
- [~] `std::time` provides selected `Duration`, `Instant`, and `SystemTime`
  values while keeping monotonic deadlines and wall time distinct; high-level
  sleep/timer/task composition is not misrepresented as Rust `std`.
  - [x] bounded `Duration` constructors and wall-clock `SystemTime` reporting
    ship; monotonic `Instant` remains open.
- [~] `rhai::task` owns executor-neutral Task/Stream composition, cancellable
  sleep/timer, wait-all, race, cancel and bounded backpressure.
  - [x] the executor-neutral timer slice ships Task identity/state,
    `after`/`sleep`, wait with optional timeout, idempotent cancellation,
    deterministic `wait_all`, indexed `race`, and `cancel_all`, without moving
    Rhai `Dynamic` or `Engine` across threads.
  - [x] child stdout/stderr expose a bytes-first `Stream` with invocation-local
    identity, pending/readable/closed/failed/cancelled state, a 64 KiB queue,
    blocking read and bounded collect with optional timeout, producer
    backpressure, close, cumulative capture limits and truthful truncation.
    `Child.wait_with_output` drains live queues while preserving the bounded
    final capture, so large output cannot deadlock the child and truncation
    never reports `complete=true`.
  - [x] HTTP `start` ships a typed `Task<HttpResponse>` payload, `kind=http`,
    stable failed/cancelled outcomes, and late-completion rejection.
  - [ ] Fleet Task payloads and prompt in-process transport cancellation remain
    open; the first HTTP adapter bounds blocking transport work to 10 seconds
    and relies on supervisor process cleanup after invocation exit.
- [~] `rhai::json` plus Rhai-native strings and a typed `Bytes` object provide
  bounded parsing, serialization, Unicode/encoding and explicit conversions
  without duplicating language primitives as fake Rust collections.
  - [x] first local slice ships JSON parse/compact/pretty serialization and
    typed UTF-8 `Bytes` conversion/length.
- [x] `rhai::http` provides HTTP(S) method, URL, headers, body, timeout,
  status, bounded response streaming, cancellation, proxy/TLS diagnostics and
  credential-safe errors. Rust std has no high-level HTTP client, so
  the AgenTerm-native high-level client lives under `rhai::http`; typed TCP,
  UDP, listeners, and WebSockets remain runtime expansion work rather than
  forbidden authority.
  - [x] `request` and `start` use Windows native TLS and the system root store;
    Unix targets retain Rustls/WebPKI. They also provide
    environment/disabled/explicit proxy selection, bytes-first duplicate
    headers, 64 KiB default and 256 KiB maximum bodies, a 2-second default and
    10-second hard deadline, stable privacy-safe error codes, and the shared
    bounded `Stream`/`Task` contracts.
  - [x] the 2026-07-29 standard Windows release measurement for the complete
    v0.1.9 `agenterm-script.exe` is 2,740,224 bytes with the reviewed
    native-TLS feature set, 405,504 bytes below the existing 3 MiB artifact
    gate; the gate was not raised.
- [x] `std::net::TcpStream` provides unrestricted DNS/IPv4/IPv6 TCP client
  connections, typed 1..60,000 ms connect/read/write deadlines, typed text or
  bytes writes, bounded bytes/UTF-8-line reads, address facts, nodelay, flush,
  and shutdown. Per-call 1 MiB I/O and 32-resolution-result ceilings are
  robustness bounds, never endpoint permissions; its first repository journey
  owns raw deadline-expired IPC evidence for the wake-delivery regression.
- [x] `std::net::TcpListener` provides unrestricted IPv4/IPv6 bind, local
  address facts, explicit native nonblocking mode, blocking accept, and typed
  accept deadlines. Accepted streams are restored to blocking I/O on every
  platform; the first repository journey uses it to replace the loopback HTTP
  PowerShell fixture with Rhai.
- [x] typed `Bytes` supports unsigned byte lookup, owned slicing, and append,
  allowing raw protocol fixtures to remain bytes-first without shell or
  PowerShell escape hatches.
- [x] invocation-owned `Child` values provide typed native top-level-window
  key and pointer delivery on Windows. The API re-resolves the child's window
  from its PID and never turns the opaque observation token into a persisted
  native handle; theme, selection, workbench, and UX journeys share it.
- [x] `rhai::image::inspect_png` decodes one explicit PNG into typed
  dimensions, sampled RGB, and luminance facts under a 64 MiB decoded-memory
  robustness bound, removing `System.Drawing` from visual qualification.
- [~] `rhai::runtime` exposes stable invocation/API/version/limits facts;
  unstable implementation handles are not part of the current public object
  tree, but this is API design rather than a permission boundary.
  - [x] `temp_dir` exposes only the current invocation-owned directory;
    `atomic_write` and `atomic_write_bytes` publish a complete same-volume
    replacement without exposing supervisor or OS handles.
- [x] filesystem and temporary-resource helpers have explicit ownership and
  cleanup behavior. Canonicalization, reparse points, atomic replacement, and
  failure paths cannot silently target a different path than the result
  reports. Normal completion removes the invocation root immediately; a later
  invocation prunes roots abandoned by a dead parent, and atomic staging files
  are removed on both promotion and ordinary failure.
- [x] the catalog taxonomy is not copied into the script surface.
  Resource-bearing values use custom-type methods (`Child.wait`,
  `Task.cancel`, `Stream.read`, response/output access), while modules,
  named-task manifests, catalog and diagnostics remain language/CLI mechanisms
  rather than artificial runtime namespaces.
- [x] globals remain minimal (`args` and `print` baseline). Native Rhai string
  and collection operations are reused instead of wrapping every value under
  `data`; `system`, `network`, `code-and-automation`, and `observability` are
  catalog/manual groupings, never mandatory call prefixes.

## Local modules, tasks, and named commands

- [x] local modules resolve from an explicit script/project root using
  deterministic relative paths; missing modules, root escape, cycles, duplicate
  identities, incompatible versions, and parse failures are typed. The shipped
  resolver embeds local imports into a self-contained AST and never searches
  home, PATH, or the network. This defines deterministic `import` identity only:
  it does not restrict `std::fs`, `std::process`, `std::net`, `rhai::http`,
  Fleet mutation, or any other runtime API from accessing user-selected paths,
  processes, endpoints, or targets.
- [x] project tasks use one versioned declarative manifest that maps stable task
  IDs to a script/module entry point, arguments, working directory, environment
  construction, and inert legacy compatibility data. Schema v2 uses a project
  identity/version, an inclusive Script API range, required stable capability
  IDs, and an ordered task array with `id`, `description`, `entry`, legacy
  `profile`, `cwd`, default `args`, and required environment-name `env` fields;
  it stores no environment values. The legacy `profile` value is ignored for
  API registration, visibility, arguments, targets, and execution behavior; a
  later manifest schema may remove the field entirely.
- [x] v0.1.9 selects versioned JSON at `agenterm.tasks.json`; it is explicitly
  a local task manifest rather than a package/download/signature manifest.
- [x] project tasks and user-level named commands are discoverable through one
  typed catalog. Invalid entries remain visible with a stable degraded reason
  instead of disappearing.
- [x] CLI listing, inspection, no-execution compatibility checking, and
  invocation of named tasks is P0 for v0.1.9.
  A GUI command palette is a P1 consumer of the same catalog and does not own a
  second registry.

## AgenTerm Fleet API

- [x] the canonical bound user facade is `fleet`, because it carries selected
  server and broker identity. It exposes typed workspace, tabs,
  terminal and events service objects; ordinary calls do not require users to
  type raw operation IDs even though results and the catalog retain operation,
  request, receipt, event and post-state identities.
- [x] v0.1.9 ships Script API v2 and removes the ambiguous v1 `agent`
  facade rather than retaining a permanent alias that conflicts with the
  future `agenterm-agent.exe`; `check` emits a targeted migration diagnostic
  for old `agent.*` source.
- [x] generate the script-facing Fleet API systematically from the public typed
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
  [Agent control plane](PRD_02_07_agent_control_plane.md); scripting reuses
  those typed contracts, while additional low-level runtime adapters may ship
  independently without turning this catalog into an Agent permission layer.

## Discovery and tool schema

- [ ] `script api --json` is the exact versioned runtime catalog and matches
  the engine, standard library, modules/tasks, Fleet operations,
  defaults, hard ceilings, and availability.
  - [x] catalog schema v3 separates its schema version from stable Script API
    v2 and provides one typed source for every public typed Fleet operation,
    explicitly planned nodes, and reviewed Node.js/Bun analogue metadata; full
    engine/module/task conformance remains open.
  - [x] the local runtime runs without requiring a server; the first useful
    fs/path/bytes/JSON slice has shipped.
  - [x] the second local slice ships typed one-directory enumeration,
    `DirEntry`, metadata, absolute-path resolution, and wall-clock
    `SystemTime`; the repository Cargo target inventory is its first migrated
    production consumer.
- [ ] each callable entry describes stable ID, signature, result/error schema,
  filesystem/process/network/Fleet access, mutation and destructive facts,
  expected duration, cancellation and streaming support, and any dry-run or
  inspect support.
  - [x] every current entry publishes `stability`, `designed_on`, and `since`
    facts; the English runtime specification opens with the complete
    human-readable object/interface tree carrying node descriptions, status,
    stability, and design dates.
- [~] the catalog is hierarchical, using stable
  `domain -> capability group -> callable/type` paths and ordering. Human
  `script api [MODULE]` output renders that same tree; unavailable, degraded,
  planned, deferred, and intentionally out-of-scope nodes do not disappear.
  - [x] `script api [MODULE] [--status shipped|planned|all] [--tree|--json]` renders one deterministic hierarchical object tree with reviewed Node.js/Bun analogues and returns the same filtered versioned catalog with explicit view and comparison metadata.
  - [ ] deferred and intentionally out-of-scope nodes require catalog status
    expansion beyond the current shipped/planned schema.
- [ ] every entry separates `catalog_path` from its shallow `surface_path`, so
  product taxonomy can evolve without forcing nested namespaces into user
  source or silently renaming callable contracts.
  - [x] schema v2 entries carry both paths and a stable ID; Script API v2 maps
    each typed operation exactly once to `fleet`, and `check` reports a targeted
    migration diagnostic for removed `agent.*` calls.
- [ ] every entry also carries nullable `rust_path`, a mapping level
  (`direct`, `adapted`, `inspired`, or `none`) and machine-readable semantic
  differences for error, type, blocking, cancellation, platform and limits.
  An API without an honest Rust std analogue cannot enter `std::`.
  - [x] schema v2 establishes these fields and publishes semantic differences
    for shipped and planned entries.
  - [x] `surface_path` and Rhai object semantics outrank `rust_path`;
    Rust/Node/Bun comparison metadata may be corrected without changing the
    Script API major.
- [x] optional comparison metadata maps every capability to a reviewed Node.js or
  Bun analogue as `similar`, `agenterm-specific`, `deferred`, or
  `not-applicable`, with source/version and review date. It supports gap
  analysis and generated manuals but never claims JavaScript, Node, Bun, npm,
  module, or binary compatibility.
- [~] the human API tree and compact Node.js/Bun comparison index are generated
  from the same catalog entries and `api --json` is the machine matrix;
  generated long-form reference pages and a no-second-callable-list alignment
  gate remain open.
- [ ] these are availability and interface facts for people and future tool
  consumers, not authorization decisions. A future Agent harness may filter
  what it offers an Agent without changing or reimplementing Script Runtime.
- [ ] `script check` validates imports, task entries, API names,
  signatures, versions, static limits, and unavailable/degraded calls without
  executing user code or requiring a GUI.
- [x] runtime, module and task identities expose version, origin/provenance
  hooks, required AgenTerm API/capabilities and stable entry-point metadata so
  future local package tooling can inspect them without executing source.
  This is a package-ready contract, not a registry, downloader, installer,
  signature policy or second package manifest in v0.1.9.
  - [x] the module/task slice exposes manifest path, canonical project root,
    project ID/version, stable task ID, entry, cwd, default argv,
    required environment names, readiness and degraded reason without running
    task source.
  - [x] schema v2 exposes the inclusive required Script API range and stable
    capability IDs, reports compatibility through list/show, and makes
    check/run reject unknown, unavailable, or version-incompatible
    requirements before source execution.
  - [x] optional bounded `local|repository` origin ID and producer/revision
    provenance hooks complete the package-ready identity contract without
    accepting URLs, credentials, hashes, signatures, dependency resolution,
    installation metadata, or trust claims.

## Repository dogfood and gradual replacement

- [x] start a parallel Rhai script set in v0.1.9 instead of rewriting or
  deleting the existing PowerShell automation.
  - [x] `scripts/rhai/verify-script-contract.rhai` uses the shipped local
    fs/JSON surface to validate the English runtime specification and the
    versioned API catalog through the public CLI black-box suite.
  - [x] `scripts/rhai/internal-version-policy.rhai` is the second migrated
    production responsibility and exercises argv-safe process execution,
    bounded capture, typed exit status, cwd, and repository file reads.
- [~] migrate one independently testable responsibility at a time through
  `parallel -> parity-proven -> default-rhai -> PowerShell deleted`; obsolete
  unreachable responsibilities may instead cross a caller-audited functional
  deletion boundary. The first twenty-three completed baseline scripts have left
  the v0.1.10 working tree.
- [ ] parity evidence compares the same inputs, structured outputs, exit
  classification, diagnostics, cancellation, cleanup, encoding, path behavior,
  and clean-machine recovery; a Rhai failure cannot hide the PowerShell
  last-known-good result.
- [~] once one Rhai responsibility reaches parity and all normal callers switch
  to it, or an obsolete responsibility is proven unreachable and superseded,
  delete that PowerShell implementation immediately instead of accumulating a
  release-wide migration backlog; twenty-six of 43 baseline scripts are deleted.
- [~] every migrated item records its old path, replacement path, switching
  commit, parity evidence, and deletion state in this PRD. Git history is the
  only archive after the explicit rollback window closes.
- [ ] build, check, qualification, package, release, credential, and GitHub
  workflow entry points may gain parallel Rhai candidates but do not switch
  their default implementation in v0.1.9.

Migration ledger:

| Responsibility | Replacement | Removed source | Switching commit | Evidence | v0.1.10 state |
|---|---|---|---|---|---|
| Cargo target inventory | `scripts/rhai/target-report.rhai` | `scripts/archive/powershell/target-report.ps1` | `b9d1906` | public CLI fixture plus live PowerShell/Rhai field parity, reconfirmed on 2026-07-29 against an absent target | deleted; Git history is the rollback source |
| Internal-only version policy | `scripts/rhai/internal-version-policy.rhai` | `scripts/archive/powershell/internal-version-policy.ps1` | `b0010f5` | public CLI `check` plus identical live PowerShell/Rhai PASS result, reconfirmed on 2026-07-29 | deleted; Git history is the rollback source |
| README artifact and command alignment | `scripts/rhai/readme-examples.rhai` | `tests/readme_examples.ps1` | `667f6d6` | exact live stdout parity against the six-artifact manifest and offline CLI/Mux catalogs on 2026-07-29; Rhai candidate `a20655a` | deleted; Git history is the rollback source |
| Locked and obsolete staged-artifact cleanup | `scripts/rhai/clean-locked-artifacts.rhai` | `scripts/clean-locked-artifacts.ps1` | `be9a538` | public `agenterm-script task run` black-box tests prove owned-name cleanup, unrelated-file retention, obsolete-name cleanup, and path-escape rejection | deleted; both normal stage-build callers use the named Rhai task |
| Cargo target cleanup preparation | `scripts/rhai/prepare-target-clean.rhai` | `scripts/prepare-target-clean.ps1` | `c20acc7` | public CLI black-box tests prove Git-native exact-root binding (including Windows short/long path aliases), allowed target set, idempotent cache-tag creation, and invalid-path/tag rejection | deleted; release build calls the named Rhai task before `cargo clean` |
| Single executable staging | `scripts/rhai/stage-artifact.rhai` | `scripts/stage-artifact.ps1` | `e087842` | public CLI black-box tests prove normal replacement, invalid-name rejection, and Windows running-image parking before replacement | deleted; shared Rhai staging orchestration reuses the same module for each manifest artifact |
| Local executable manifest validation | `scripts/rhai/validate-artifact-manifest.rhai` | `scripts/artifact-manifest.ps1` | `e2276cc` | public CLI black-box tests prove the canonical schema and reject duplicate/invalid names, invalid subsystem/probe contracts, empty roles, and missing size budgets | deleted; build staging, metadata writing, and artifact verification invoke the named Rhai task |
| Source build identity freeze | `scripts/rhai/build-identity.rhai` | `scripts/build-identity.ps1` | `b082c3b` | public CLI black-box tests prove exact Git-root binding, clean/dirty truth, profile validation, batch-safe fields, and exact SHA-256 build-input identities | deleted; `build.bat` and both check lanes bootstrap the current Script Runtime before invoking the named Rhai task |
| Staged build provenance | `scripts/rhai/write-build-metadata.rhai` plus shared artifact/metadata modules | `scripts/write-build-metadata.ps1` | current migration change | public CLI black-box tests prove frozen and live identities, executable size/hash capture, clean-source and frozen-input drift rejection; direct old/new field parity excludes only the generation timestamp | deleted; the standalone task and `stage-build.rhai` reuse the same project modules |
| Built artifact orchestration | `scripts/rhai/stage-build.rhai` plus shared artifact/metadata modules | `scripts/stage-build.ps1` | current migration change | public CLI composition fixture proves cleanup, obsolete removal, staging, metadata and pre-mutation Git-root rejection; actual `build.bat` and direct old/new directory parity cover the six-artifact path | deleted; `build.bat` invokes one bounded Rhai task and no longer launches PowerShell |
| Read-only release preflight | `scripts/rhai/preflight.rhai` | `scripts/preflight.ps1` and `scripts/preflight-selftest.ps1` | current migration change | public CLI real-Git fixtures prove clean/CRLF success, dirty/wrong-branch/bad-lock/bad-manifest fail-closed reports, nested output creation, and remote credential redaction | deleted; `check.ps1` runs the Rust black-box fixture and invokes the named Rhai task for release preflight |
| Preflight latency benchmark | `scripts/rhai/preflight-benchmark.rhai` | `scripts/preflight-benchmark.ps1` | current migration change | public worker black-box benchmark against a clean Git clone proves five successful preflight subprocesses, p95 target enforcement, durable JSON evidence, and scratch cleanup | deleted; release check invokes the named Rhai task directly |
| Locked dependency and SPDX inventory | `scripts/rhai/supply-chain.rhai` | `scripts/supply-chain.ps1` | current migration change | public task covers every resolved Cargo.lock package, reviewed licenses, direct-notice alignment, deterministic ordinal ordering, SPDX structure and scratch cleanup; old/new semantic parity differs only in producer identity and ordering | deleted; ordinary and release checks invoke the named Rhai task |
| Obsolete v0.1.8 public-candidate decision and self-test | Current qualification receipt, byte-qualified package, release preflight, and explicit approval boundaries | `scripts/public-candidate-policy.ps1` and `scripts/public-candidate-policy-selftest.ps1` | current migration change | the legacy self-test passed before removal; `git grep` proves no operational caller, while current qualification/package self-tests own the retained fail-closed invariants | deleted as unreachable version-specific duplication; Git history is the rollback source |
| Unintegrated PowerShell journey-manifest prototype and self-test | Planned shared Rhai smoke harness plus the existing qualification receipt boundary | `tests/JourneyManifest.ps1` and `tests/journey_manifest_selftest.ps1` | current migration change | the prototype self-test passed before removal, but `git grep` proves no smoke, check, CI, qualification, or release caller; delivery PRD already marks the machine-readable step manifest incomplete | deleted rather than preserving a second unused evidence model; Git history is the rollback source |
| PRD, evidence, CLI/protocol, and Mux catalog alignment | `scripts/rhai/prd-alignment.rhai` | `tests/prd_alignment.ps1` | current migration change | public task reproduces the exact legacy live-catalog PASS result and isolated black-box coverage rejects an unsupported alignment-contract schema | deleted; both quick and full checks invoke the named Rhai task |
| Owned-child cleanup self-test and first shared Rhai harness foundation | `scripts/rhai/harness-cleanup-selftest.rhai` plus `scripts/rhai/lib/test_harness.rhai` | `tests/harness_cleanup_selftest.ps1` | current migration change | public task and Windows integration prove exact registered-child forced cleanup, survival of an unregistered sibling until explicit cleanup, orphan-free persisted proof, and original-failure retention | deleted; the quality gate invokes the named Rhai task and later smoke migrations extend the same module |
| Working-context proxy privacy and restart journey | `scripts/rhai/working-context-smoke.rhai` plus the shared Rhai test harness | `tests/working_context_smoke.ps1` | current migration change | named task and Windows public integration prove isolated GUI/server launch, safe proxy facts, archived-control non-mutation, no secret in snapshot/pane/workspace/events/stderr, non-persistence across restart, stable completed-child identity, and orphan-free cleanup | deleted; qualification declaration discovery and execution now use the Rhai journey directly |
| Headless server authority journey | `scripts/rhai/server-smoke.rhai` plus typed owned-child platform facts and the shared Rhai harness | `tests/server_smoke.ps1` | current migration change | named task and Windows public integration preserve no-top-level-window evidence, exact PID/ownership, lease lifecycle and gated interaction, real PTY, replay/conflict/asynchronous receipts, causal events, workspace persistence, graceful shutdown, and orphan-free cleanup | deleted; qualification discovery and execution use the Rhai journey directly |
| Same-server real-byte GUI upgrade and rollback journey | `scripts/rhai/remote-ui-upgrade-smoke.rhai` plus typed owned-child opaque window identity and the shared Rhai harness | `tests/remote_ui_upgrade_smoke.ps1` | current migration change | named task preserves distinct GUI hashes/build identities, competing-lease rejection, one stable server/epoch/PTY/draft/CWD/proxy state, output streamed across replacement, incompatible-protocol rejection, rollback scrollback continuity, public close/detach behavior, and orphan-free cleanup | deleted; qualification discovery and execution use the Rhai journey directly |
| Coalesced wake, concurrent IPC, PTY output, and expired raw mutation journey | `scripts/rhai/wake-smoke.rhai` plus unrestricted `std::net::TcpStream`, `rhai::hash::fnv1a64`, and the shared Rhai harness | `tests/wake_smoke.ps1` | current migration change | named task and Windows public integration preserve an isolated headless server, 32 concurrent versioned snapshot clients, 80-line PTY progress, raw newline-delimited IPC, typed expired no-op receipt, unchanged tab note, and orphan-free owned-child cleanup | deleted; qualification discovery and execution use the Rhai journey directly |
| Native first-window and asynchronous terminal startup journey | `scripts/rhai/startup-smoke.rhai` plus unrestricted `std::process::list()` and the shared Rhai harness | `tests/startup_smoke.ps1` | current migration change | named task preserves the one-second native-window budget, public asynchronous terminal-ready wait, exact window title and inherited-stderr guidance, second-launch handoff, nonblocking CLI-style/invalid GUI arguments, absence of a nested Script worker, graceful shutdown, and orphan-free cleanup | deleted; ordinary qualification and Windows CI invoke the named Rhai journey |
| Public CLI, typed control, UI bridge, and PTY lifecycle journey | `scripts/rhai/cli-smoke.rhai` plus the shared Rhai harness | `tests/cli_smoke.ps1` | current migration change | named task preserves all nine public evidence IDs across receipt replay/conflict, offline validation, operation/UI-bridge discovery, renderer-neutral bootstrap/delta causality, typed Tabs actions/events, Composer/PTY/Backspace/scroll/screenshots, stable creation IDs, remain-on-exit, and explicit close; command evidence is now bounded per record | deleted; ordinary qualification and Windows CI invoke the named Rhai journey |
| Loopback HTTP test fixture | `scripts/rhai/script-http-fixture.rhai` plus unrestricted `std::net::TcpListener` and raw `Bytes` operations | `tests/script_http_fixture.ps1` | current migration change | full public Script smoke preserves status/echo/large/async/slow/cancel/malformed/disconnect/TLS paths, privacy-bounded audit, typed host failures, cleanup, and a delayed-first-byte Windows accepted-socket regression | deleted; the Script smoke launches only the Rhai fixture |
| Theme preview, persistence, PTY continuity, and rendered differentiation | `scripts/rhai/theme-smoke.rhai` plus typed child-window input, `rhai::image::inspect_png`, and the shared Rhai harness | `tests/theme_smoke.ps1` | current migration change | live old/new parity preserves Dark/Light preview, Cancel and physical Escape rollback, settings persistence, stable PTY PID/output, native PNG luminance separation, restart state, bounded retained failure diagnostics, and orphan-free cleanup | deleted; qualification and diagnostic-bundle probes invoke the named Rhai task |
| Workbench inline editing, archived Proxy chrome, and compact tree geometry | `scripts/rhai/workbench-smoke.rhai` plus typed child-window click dispatch and the shared Rhai harness | `tests/workbench_smoke.ps1` | current migration change | live old/new parity preserves physical Edit/Save/Cancel, independent name/note drafts, Composer isolation, archived Proxy geometry/actions, a four-level CJK hierarchy, 180/250/480 px density, bounded row geometry, and orphan-free cleanup | deleted; qualification discovery and execution invoke the named Rhai task |
| Unintegrated professional terminal-selection prototype | Future public Rhai professional-selection journey after the product slice ships | `tests/terminal_selection_smoke.ps1` | current migration change | caller audit finds no check, CI, qualification, or registered evidence consumer; direct execution fails before interaction at zero terminal columns, while the Windows double-click handler does not implement the claimed word/third-click behavior and the product PRD keeps that slice planned | deleted as misleading dead test code; Git history retains the prototype |

### v0.1.10 completion commitment

- [ ] v0.1.10 completes the replacement of repository-owned PowerShell
  automation; this is a release completion gate rather than a best-effort
  migration track.
- [x] the dated 2026-07-29 frozen baseline is 43 tracked `.ps1` files: 3 at
  the repository root, 17 under `scripts/`, 21 under `tests/`, and 2 retained in
  `scripts/archive/powershell/`.
- [~] migration progress is 30/43 deleted and 13/43 remaining; progress is
  counted only after parity evidence plus caller cutover, or caller-audited
  functional deletion of obsolete behavior, and source deletion.
- [x] `scripts/powershell-migration.json` freezes all 43 baseline paths under
  stable migration IDs with responsibility groups, replacement task IDs, and
  explicit `inventory`/`deleted` state.
- [x] the public `migration-audit` Rhai task compares the ledger with
  `git ls-files '*.ps1'`, rejects an unplanned script, a returned deleted
  script, an operational reference to a deleted script, an unrecorded removal,
  duplicate paths, invalid states, and count drift; ordinary and release
  qualification invoke it as a required gate.
- [~] the repository-root `agenterm.tasks.json` is now the offline task
  catalog and ships twenty-five ready tasks (`bootstrap-info`, `build-identity`,
  `harness-cleanup-selftest`, `migration-audit`, `target-report`, `internal-version-policy`,
  `verify-docs-site`, `readme-examples`, `clean-locked-artifacts`,
  `prepare-target-clean`, `preflight`, `preflight-benchmark`, `prd-alignment`,
  `stage-artifact`, `stage-build`, `supply-chain`, `server-smoke`, `startup-smoke`, `cli-smoke`,
  `working-context-smoke`, `workbench-smoke`, `wake-smoke`, `remote-ui-upgrade-smoke`,
  `validate-artifact-manifest`, and `write-build-metadata`). The existing
  two-input Script contract verifier is
  intentionally not advertised as ready until catalog fixture production is
  part of its task. Build, lint, test, qualification, package, rehearsal,
  release, dependency, platform, side-effect, and evidence metadata remain
  incomplete.
- [ ] completion requires `git ls-files '*.ps1'` to return no files. Tests,
  helpers, and archived implementations are not exceptions; Git history is the
  permanent archive after each parity and rollback boundary closes.
- [ ] `agenterm.tasks.json` and shared Rhai modules own build, lint, test,
  qualification, packaging, release rehearsal, and approved release semantics.
- [ ] batch files, Unix shell entry points, and CI YAML may bootstrap a pinned
  Rust toolchain and forward arguments/exit status to the same Rhai task, but
  must not duplicate task selection, budgets, evidence, packaging, or release
  policy.
- [ ] the migration proceeds from low-side-effect rules and reports through
  build/static quality, public black-box tests, and finally qualification and
  delivery. Each responsibility must prove normalized parity or stronger public
  evidence before callers switch and its `.ps1` leaves the tree.
- [ ] Script Runtime gaps are filled with stable typed APIs or shared Rhai
  modules. Rhai scripts must never invoke PowerShell as an escape hatch.
- [ ] a no-PowerShell clean-checkout qualification and a zero-`.ps1` drift gate
  prevent the old automation layer from returning.
- [ ] “PowerShell replacement” applies to repository-owned automation and its
  delivery process, not to users launching PowerShell as a terminal shell or
  to terminal-compatibility coverage. Such compatibility tests must be driven
  by the Rhai harness and cannot carry repository business rules in
  PowerShell.
- [ ] completion is measured only after parity evidence, every caller cutover,
  source `.ps1` deletion, and drift-gate coverage. Static zero-file evidence is
  paired with clean-checkout process-tree evidence proving that bootstrap,
  build, check, qualification, packaging, and release rehearsal do not spawn
  `powershell.exe` or `pwsh.exe`.

## Public black-box acceptance

- [x] tests invoke only released `agenterm-cli script` commands and compare the
  offline catalog with actual runtime behavior.
- [ ] isolated temporary roots cover Unicode and long paths, metadata,
  directory operations, atomic replacement, interruption, access failure, and
  cleanup without changing files outside the fixture.
- [ ] process fixtures cover argv boundaries, spaces, Unicode, cwd, env, stdin,
  separate stdout/stderr, nonzero exit, timeout, cancellation, parent exit,
  backpressure, and orphan-free cleanup.
  - [x] the first public CLI process fixture covers executable-plus-argv, cwd,
    child env overlay, text stdin, separate stdout/stderr, nonzero exit,
    bounded Duration timeout, explicit Child kill/wait, and recovery on the
    immediately following invocation.
  - [x] the public CLI stream fixture covers live child stdout, bounded
    chunked read and collect, final capture preservation, clean EOF, queue
    facts, typed read timeout, explicit close/cancellation, and capture
    truncation that reports `Output.complete=false` without falsely truncating
    the fully delivered Stream; unit coverage fills the queue, rejects
    oversized collect, and proves close wakes a backpressured producer.
- [x] an independent loopback HTTP fixture covers request/response, headers,
  body, status, bounded streaming, timeout, cancellation, malformed data,
  connection failure, proxy/TLS-safe diagnostics, and no public-network
  dependency.
- [ ] timer and task fixtures prove concurrent progress, deterministic wait and
  stream results, natural worker exit, cancellation propagation, bounded
  queues, and recovery on the next invocation.
  - [x] timer composition, child-process Stream state/backpressure, typed HTTP
    Task payload, timeout, cancellation with rejected late completion, bounded
    body Stream and immediate next-invocation recovery have unit and public-CLI
    evidence; Fleet payload propagation and prompt transport abort remain
    open.
- [ ] module/task fixtures cover roots, relative imports, cycles, duplicate and
  missing modules, manifest version/error handling, named-task discovery,
  degraded entries, arguments, and working directory.
  - [x] the first public fixture covers valid relative import, root escape,
    missing module, cycle, bad manifest version, duplicate identity, unknown
    field, ready/degraded discovery, list/show/check without code execution,
    default-plus-caller argv, cwd, required environment-name validation and
    successful named invocation.
- [ ] Fleet conformance compares every operation-catalog entry with its script
  exposure or explicit degraded reason; mutations verify typed receipts,
  correlated public post-state/events, no duplicate side effect, and honest
  close/send/restart failures.
  - [x] Script API v2 maps all 18 current typed operations exactly once; the
    public isolated-server journey proves observation plus a reversible UI
    mutation and typed tab-note mutation with native request/operation
    identity, receipt, correlated event, verified snapshot, restoration, and
    audit attribution.
  - [x] `fleet.tabs.set_note` returns a receipt, causal `tab.note` event, and
    verified tab snapshot for one stable tab ID.
    Destructive failure/restart and future operation families remain open.
- [ ] the unrestricted local runtime proves deterministic computation,
  observation, mutation, filesystem, process, and network paths through one
  consistent API surface.
- [ ] script error, worker crash, timeout, cancellation, parent exit, server
  restart, and unfinished task cleanup leave GUI, PTY, workspace, and the next
  script invocation healthy.
- [ ] retained diagnostics and audit fixtures prove that file content,
  arguments, environment secrets, HTTP credentials/bodies, terminal content,
  and script stdout do not leak.
  - [x] the HTTP loopback journey checks URL, path and proxy credential
    sentinels against returned diagnostics, the reusable audit JSONL and the
    redacted command record.
- [ ] every slice records worker/CLI/GUI size, first-window/no-script startup,
  duration, limits, and orphan cleanup before its status changes to shipped.

## Explicitly deferred

- npm compatibility, arbitrary remote imports, third-party package lifecycle,
  and Node/Bun binary or API compatibility; an AgenTerm-owned signed
  package/application catalog remains a future optional-component track rather
  than npm emulation inside the script runtime;
- a persistent or automatically started script daemon and cross-invocation
  mutable runtime state;
- UDP, WebSockets, and higher-level network modules beyond the shipped
  unrestricted TCP stream/listener primitives remain planned Script Runtime
  expansion and are not permission-gated;
- event handlers, watch mode, REPL, and durable background scheduling unless a
  later owned slice supplies separate acceptance;
- Agent permission, approval, credential, quota, and natural-language policy
  belong to the separate Agent harness;
- GUI command palette delivery beyond its P1 consumption of the shared named
  task catalog.
