# Rust host + script engines (Rhai / rh / lua / qjs)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Runtime contract: [AgenTerm Script Runtime specification](../docs/agenterm-rhai-runtime.md)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Script engine family

This PRD's scope grew from one embedded language to a **family of script
engines** sharing one Rust host, one L2 Facade/catalog (`fleet.*`, `std.*`),
and one product-boundary philosophy (unrestricted local runtime; the future
Agent harness owns authorization, not the engine — see "v0.1.9 product
position" below). Four engines, in lineage order:

- **Rhai — cancelled as the forward engine direction (2026-08-07).** Was the
  original and, through v0.1.15, default embedded language (tree-walking
  interpreter, `rhai` crate). `agenterm-rhai.exe` remains shipped as a
  **compatibility shim** for existing `.rhai` callers per "Shipped baseline"
  below and receives shim-hardening fixes (`plan/plan-rh-3.md` M23d), but no
  further Rhai-native capability investment continues — every new capability
  targets rh, and now lua/qjs. This PRD's title and most of its accumulated
  detail below describe Rhai/rh history; treat "Rhai" content as the
  wound-down lineage, not the active target.
- **rh — self-developed, deeper host integration than Rhai
  (`crates/agenterm-rh`, `agenterm-rh` CLI).** Syntax and object model are
  informed by Rhai and by Rust's own `std::` shape, but rh is not an
  interpreter: a checked subset transpiles to native Rust and compiles via
  `rustc` (AOT), giving generated packs a direct low-level host ABI (native
  `i64` entry points, no embedded interpreter runtime shipped in the pack)
  that Rhai's tree-walking design never had. Default execution backend since
  M22f (`AGENTERM_SCRIPT_BACKEND=rh`); status/detail: "Shipped baseline"
  below, [`plan/plan-rh-3.md`](../plan/plan-rh-3.md),
  [`plan/design-rh-aot.md`](../plan/design-rh-aot.md).
- **lua — `agenterm-lua`, capability-aligned with rh (Windows-side, in
  progress).** New sibling engine; framed-worker integration, `LuaEngine` +
  host functions, and a growing `std.*` surface (`std.fs.*` etc.) landed
  2026-08-07 (commit `8b3764f5` and follow-ups). Not built by the same agent
  driving this PRD file — tracked here for cross-session awareness, see
  `plan/plan-v0.1.16.md` §1 "Rh. 脚本引擎矩阵".
- **qjs — `agenterm-qjs`, capability-aligned with rh, QuickJS-based (in
  progress).** New sibling engine using `rquickjs` (bundled quickjs-ng C
  source) as the binding. QJS-M0 (skeleton + minimal eval, `140a9152`) and
  QJS-M1 (`check`/`eval`/`check-many` CLI verbs field-for-field aligned
  with rh's contract, `cc7df9df`) shipped 2026-08-07, followed by root-
  workspace integration (`d5ac9a8a`, mirrors rh's `[[bin]]`-in-root-package
  shape). QJS-M2 in progress (`e50fbbf8`, 2026-08-08): `__host` function
  binding (`fleet_call`/`args_len`/`arg`/`print`) deliberately matches
  `agenterm_lua::LuaHostFunctions`'s shape/naming rather than inventing a
  qjs-specific convention, and `scripts/qjs/lib/fleet.js` is a near-line-
  for-line port of `scripts/lua/lib/fleet.lua` — proven end-to-end against
  the real file. A real memory-safety bug (GC-uncollectable reference
  cycle from capturing `Ctx` in a bound closure, crashed the process) was
  found and fixed along the way — see the "Thread/concurrency model
  mismatch" risk entry below. Still open: `agenterm::script_backend`
  wiring and `task`/`run`/`pack`/`qualify` verbs. Open risks and remaining
  milestones: see "Future" § **qjs execution backend** below.

"Capability alignment" between rh/lua/qjs means: same L2 facade/catalog
surface, same CLI verb contract (check/eval/pack/check-many/task — see rh's
shipped shape below), same typed JSON/exit-code envelope, same worker/
framed-worker integration points. It explicitly does **not** mean matching
rh's AOT/native-codegen execution strategy — that is rh-specific; lua/qjs
may use their own VM/bytecode execution as long as the L2 contract and CLI
behavior match. See
[`plan/design-scripting-boundary-comparison.md`](../plan/design-scripting-boundary-comparison.md)
§2.1/§6 for the L1/L2/L3 boundary this rests on.

## Shipped baseline

- [x] `agenterm-rh.exe` is the default task and supervised worker front door:
  one-shot and persistent supervisors resolve it before the explicit
  `agenterm-rhai.exe` compatibility fallback and default its execution backend
  to `rh`. It directly hosts named tasks plus private
  `--worker`/`--framed-worker` modes through the shared library.
  `agenterm-rhai.exe` remains the compatibility public `run`, `eval`, `repl`,
  `check`, and `api` CLI;
  its PE now owns only incremental-wrapper and command dispatch code, while
  `script_worker` in the shared `agenterm` library owns worker, framed
  protocol, REPL and execute behavior;
  on Windows, `agenterm-cli.exe script repl ...` is a thin process-forwarding
  compatibility route to the adjacent `agenterm-rhai` sidecar, inherits
  stdio and exit status, and never links a second Rhai engine into the control
  client. Linux and macOS callers invoke `agenterm-rhai` directly because the
  CLI-hosted Script route is not available there. The existing one-shot
  supervisor routes retain their single worker topology. All expose the same
  catalog, parser, supervisor, and runtime.
- [x] rh-generated native packs use an AgenTerm-owned `i64` entry ABI and their
  generated Cargo manifest no longer depends on the Rhai crate. Native subset
  packs and compatibility-delegating pack stubs therefore contain no embedded
  Rhai runtime; parsing and explicit host-eval/host-run compatibility remain
  host responsibilities until their later migration slices.
- [x] host API v4 executes literal-path `std::fs::exists` calls through a typed
  Rust filesystem callback rather than constructing a Rhai Engine. Missing or
  invalid host registration fails closed, callback errors retain the shared
  typed host-error path, and older v2/v3 packs remain loadable. Dynamic path
  expressions and other `std::` APIs remain explicit migration work.
- [x] named-task manifests and corpus checks accept `.rh` entries. The public
  `agenterm-rh task run` path owns a native task probe whose transpiled Rust is
  qualified to contain neither whole-script `rh_host_run_script` nor localized
  `rh_host_eval_int`; production task migration must satisfy the same gate
  before its `.rhai` entry is archived.
- [x] host API v5 exposes invocation argument count directly to native packs.
  Both `args.len` and `args.len()` transpile to `rh_args_len()` without a Rhai
  Engine, the manifest-owned native task proves real appended arguments through
  the public CLI, and v2-v4 pack registration remains backward compatible.
- [x] host API v6 exposes bounded UTF-8 `args[index]` reads to native packs.
  Native string bindings and `.len` use Unicode scalar counts rather than byte
  counts; missing/non-string arguments and output-limit violations retain typed
  host failures, while v2-v5 pack registration remains backward compatible.
- [x] `std::fs::exists` accepts native UTF-8 argument bindings as well as string
  literals. Dynamic task paths call the typed Rust filesystem callback without
  host evaluation; the public native task verifies a real repository path and
  remains covered by the no-interpreter-fallback gate.
- [x] host API v7 provides 1 MiB-bounded UTF-8 `std::fs::read_to_string` for native
  argument bindings. Native string values support literal substring checks;
  missing, invalid UTF-8, or oversized files retain typed host failures, and
  v2-v6 pack registration remains backward compatible.
- [x] `std::path::join(base, child).display` lowers to native Rust path
  composition when both operands are native strings. The result remains a typed
  string binding consumable by filesystem callbacks without host evaluation.
- [x] host API v8 provides typed native task failure and case-exact file-name
  checks. `verify-docs-site` is the first production named task migrated from
  active `.rhai` to a zero-fallback `.rh` entry; its interpreted implementation
  is retained only under `scripts/archive/rhai/` for migration archaeology.
- [x] Candidate and Promotion execute all active scripting steps through the
  packaged `agenterm-rh` front door. Workflow policy tests reject any restored
  `agenterm-rhai` executable reference; `.rhai` source paths remain explicit
  interpreter-migration debt rather than executable-entry ownership.
- [x] host API v9 exposes native bounded `std::process::command_status` without
  command or path allowlists; timeout, owned process-tree cleanup and typed
  failures remain robustness controls. `internal-version-policy` uses Git's
  authoritative `show-ref --verify` path in a zero-fallback `.rh` task and its
  interpreted implementation is archived.
- [x] top-level compatibility scripts without an explicit `fn entry()` can no
  longer qualify as a native zero-return stub. `.rhai` entries execute the
  complete source through the compatibility boundary; `.rh` named tasks without
  an entry point fail qualification. The cache revision invalidates affected
  packs, and public task regression coverage proves invalid arguments no longer
  succeed silently.
- [x] native `.rh` packs parse general JSON values and read integer object
  properties without interpreter evaluation, whole-script delegation,
  substring validation, or task-specific host utilities. They also read array
  lengths, iterate JSON arrays, access integer and string element properties,
  evaluate `type_of`, compare and concatenate strings, run string methods and
  character loops, emit dynamic `rh::fail`/`require` messages, track
  bool-keyed MapSet membership (`#{}` / `.contains` / `[key]=true`), resolve
  `std::path::absolute(...).display`, inspect `symlink_metadata` and
  `std::fs::metadata` file / dir / len / symlink / reparse flags, resolve
  `std::env::current_dir().display`, `PathBuf::from` plus `.is_absolute` /
  `.display`, and `rhai::json::parse_file` as `parse(read_to_string)` sugar,
  and flatten project-relative `import` graphs into one native pack entirely
  in generated Rust. Public qualification executes the generated native packs.
  `validate-artifact-manifest` now ships as
  native `scripts/rh/validate-artifact-manifest.rh` plus
  `scripts/rh/lib/artifact_manifest.rh` (import-bundled, no substring or
  task-specific host validator); remaining Rhai callers still import
  `scripts/rhai/lib/artifact_manifest.rhai` until those tasks migrate.
- [x] v0.1.12 retains `agenterm-rhai.exe` / `agenterm-rhai` as the canonical
  public executable. Although Rhai is the stable runtime contract, the version
  has no complete external-usage inventory or migration/removal evidence, and
  introducing a second name during convergence would expand packaging,
  bootstrap and Candidate scope without user value. A future rename decision
  must inventory every CI/package/documentation/external caller and keep any
  compatibility entry as a thin forwarding surface over the one unrestricted
  implementation; it must never create a second runtime or reduced API.
- [x] every ordinary invocation receives the same unrestricted local runtime
  surface. No invocation mode, task label, profile, catalog capability, caller,
  or entry point gates API registration or execution; the future Agent harness
  owns Agent permissions outside this executable.
- [x] the 2026-07-30 `AGENTS.md` and `PRD*.md` policy audit found no Rhai
  permission tier or capability denial. Raw sockets, listeners, arbitrary
  endpoints and local paths, child-process control, destructive filesystem
  targets, and Fleet mutation are valid runtime surface; an unshipped adapter
  remains a product gap and must not be replaced by a policy-reduced variant.
  Allowlist or scoped-capability language retained in MCP, model, gateway, or
  Agent requirements governs those callers only and cannot alter Script API
  registration or execution.
- [x] Script API v2 maps every current typed operation exactly once to `fleet` and verifies mutation receipts, correlated events, and post-state. The v0.1.12
  frontend additions are callable as `fleet.ui.window.activate()` and
  `fleet.terminal.paste()` while preserving the existing
  `fleet.terminal("@ID").capture(...)` overload. Activation waits for native
  focus truth; paste waits for its asynchronous typed result and correlated
  `terminal.pasted` event rather than treating request acceptance as completion.
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

## Persistent REPL slice (designed 2026-07-31)

- [x] `agenterm-rhai repl [OPTIONS] [--] [ARGS...]` starts a true
  process-local Rhai session. Variables and script-defined functions survive
  from one successful cell to the next; it does not loop over the one-shot
  `eval` command.
- [x] the reusable `ReplSession` core owns language state but no terminal UI.
  The CLI adapter, black-box tests, and future Control Center or Agent adapters
  may drive the same session contract without duplicating Engine construction,
  API registration, state-commit, value, or failure semantics.
- [x] ordinary workers and REPL sessions use one shared Engine configuration
  function, so the unrestricted local `std::`, `rhai::`, and bound `fleet`
  surface cannot drift between execution modes.
- [x] each cell evaluates against a visible Scope clone plus a functions-only
  AST. Success atomically commits the candidate Scope and function table;
  compile, runtime, timeout, operation, or output-limit failure commits neither
  language binding. Filesystem, child-process, network, Fleet, and shared
  handle effects that already reached the outside world are real effects and
  are never described as rolled back.
- [x] TTY input has a banner, primary/continuation prompts, multiline
  structural assembly, EOF handling, and session-local history. Piped input
  emits no prompt or banner, supports the same multiline cells, and `--json`
  emits one JSON object per cell or meta result.
- [x] the stable initial meta surface is `:help`, `:quit`/`:exit`, `:reset`,
  `:history`, `:vars`, `:functions`, `:limits`, `:api [MODULE]`, `:load FILE`,
  and `:json on|off`. History is memory-only and `:vars` exposes names and
  types, not values.
- [x] failures are recoverable by default and make the final process status
  nonzero; `--fail-fast` stops after the first failed cell. EOF with an
  incomplete cell is a typed failure. Unit and public cross-platform
  pipe/NDJSON tests prove persistence, multiline functions, rollback, reset,
  failure recovery, fail-fast, no prompts in pipes, and incomplete EOF.
- [x] the Windows x86_64 size-optimized release built on 2026-07-31 is
  3,092,480 bytes, within the unchanged 3,145,728-byte Script artifact budget.
  The remaining 53,248 bytes is narrow headroom and must be remeasured for
  future editor/history dependencies rather than silently raising the budget.
- [x] Windows hosted persistent REPL supervision preserves one worker and
  committed language state across cooperative Ctrl+C, hard-kills and reaps a
  non-cooperative worker tree within 150 ms plus bounded cleanup, then reports
  a fresh language/history generation with no side-effect replay before the
  same outer CLI continues on a new worker. Public direct and Windows-hosted
  tests separately prove the 32-cell generation limit and cell-33 replacement.
- [~] Long-lived REPL hardening remains open for arrow-key editing/history;
  the child-session protocol, bounded generation replacement, and Windows
  hosted Ctrl+C recovery are shipped. A stable
  length-prefixed child-session protocol now covers Open, Inspect, Evaluate,
  Query, Reset, Cancel, and Close with session/generation/sequence identities;
  constant-space request/response validators reject stale generations,
  sequences, phases, queries, and cell mismatches without feeding REPL traffic
  into the legacy HashSet tracker. The framed worker owns `ReplSession` on a
  dedicated session thread, preserves state across cells, keeps stdin available
  for pre-start or in-flight Cancel and broker responses, returns typed state,
  and joins on Close/EOF. Unit evidence proves persistence, reset/query/close,
  a cancellation race, canonical mismatch failures, and legacy isolation.
  Commit `e731ee3` connected the public REPL to its dedicated parent-side
  `PersistentReplClient`: public direct-entry and Windows hosted tests prove one
  worker PID and language state across 32 cells, replacement before cell 33,
  a new generation/PID, and an explicit fresh-session receipt that resets
  language/history state and never replays external side effects. The Windows
  `script.repl-supervision` ConPTY journey proves that Ctrl+C cooperatively
  cancels a CPU-bound cell while preserving the worker and committed state,
  then exercises the 150 ms non-cooperative hard-kill/reap path for a blocking
  cell and its owned nested process; the same outer CLI continues with a fresh
  worker, emits `reason=hard_interrupt` and `side_effects_replayed=false`, and
  leaves no worker or nested-process orphan. Linux/macOS retain the direct
  `agenterm-rhai` protocol and compile/unit coverage, but no Unix hosted-CLI
  or native interactive Ctrl+C parity is claimed without its own public native
  journey. Ordinary CI run `30716123255` exposed that a nested Script command
  can create another Unix process group outside the outer worker's `killpg`.
  Commit `72eb861` records transitive descendants before termination, verifies
  start identity before killing cross-group children, classifies Linux zombie/
  dead proc states correctly, and adds a matching-host process-group contract
  test. Its REPL fixture gives the nested worker a 10-second deadline but
  requires cleanup within two seconds, so self-timeout cannot fake orphan-free
  evidence. A new Linux/macOS CI receipt is still required. Arrow-key editing/
  history remains open; memory-only `:history` is not presented as that editor
  behavior.

## v0.1.9 product position

`agenterm-rhai.exe` is AgenTerm's general-purpose local scripting runtime:
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
- [x] explicit local tasks may raise their invocation wall-time budget to the
  stable one-hour robustness ceiling so build and qualification orchestration
  can remain Rhai-owned; the ordinary default remains two seconds,
  child-process call deadlines may use the same one-hour ceiling, and HTTP
  retains its stricter 10-second ceiling. The explicit operation ceiling is
  100,000,000 so the coordinator's bounded process observation and wait loops
  can cover a complete stress-inclusive release rehearsal;
- agent-specific tool visibility, approval, path/domain/target policy,
  credentials, quotas, and natural-language intent belong exclusively to the
  future Agent harness, which may constrain an Agent before invoking this
  unrestricted runtime;
- local scripts do not bypass native product invariants: live close
  confirmation, remain-on-exit, stable IDs, tree-cycle rejection, replay
  protection, and truthful typed outcomes continue to apply.

## v0.1.9 runtime architecture

- [x] one-shot `run`, `eval`, `check`, `api`, and task invocations still own
  one fresh `agenterm-rhai.exe` sidecar. `repl` owns one explicit,
  foreground, process-local session; neither form creates a persistent system
  daemon or mutable state that survives its public process.
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
  listing/creation, metadata, copy, rename and arbitrary explicit-target
  deletion.
  - [x] blocking `read`, `read_to_string`, text/bytes `write`, `exists`,
    directory creation, copy, rename and explicit-target file/directory/tree
    removal ship through the public CLI. Script Runtime defines no protected
    path, root/workspace/ancestor filter, or caller allowlist; destructive
    target selection is the invoking user's responsibility. Metadata,
    directory listing and cumulative byte budgets remain open.
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
  are invocation-owned and inherit supervisor process-tree cleanup. v0.1.12
  also gives each `Child` its own Windows kill-on-close Job Object or Unix
  process group. Unix cleanup also snapshots transitive descendants with start
  identities and terminates children that deliberately create nested process
  groups; `Child.kill_tree()` and deadline/Drop cleanup terminate only that
  owned tree, are idempotent after disarm, and preserve unrelated processes;
  `std::process::id()` exposes the current supervised worker PID for
  owned-resource naming and live-owner protocols without treating it as stable
  invocation identity.
  `std::process::list()` exposes an unrestricted PID-sorted typed
  operating-system process inventory (`ProcessInfo.id`, `.parent_id`, and
  `.executable_name`) on Windows, Linux, and macOS; parent identity is zero
  where unavailable, and the inventory is a point-in-time observation rather
  than an Agent allowlist or authorization surface.
  `std::process::kill(pid)` forcefully terminates any selected operating-system
  process without owner, executable, path, ancestry, or Agent-policy filtering;
  the remote-UI recovery journey uses it for real server-fault injection.
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
  the AgenTerm-native high-level client lives under `rhai::http`; raw sockets,
  typed TCP/UDP, listeners, and WebSockets are runtime expansion work rather
  than forbidden authority and must not acquire address, endpoint, port, path,
  process, or caller allowlists.
  - [x] `request` and `start` use Windows native TLS and the system root store;
    Unix targets retain Rustls/WebPKI. They also provide
    environment/disabled/explicit proxy selection, bytes-first duplicate
    headers, 64 KiB default and 256 KiB maximum bodies, a 2-second default and
    10-second hard deadline, stable privacy-safe error codes, and the shared
    bounded `Stream`/`Task` contracts.
  - [x] the 2026-07-29 standard Windows release measurement for the complete
    v0.1.9 `agenterm-rhai.exe` is 2,740,224 bytes with the reviewed
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
- [x] typed `Bytes` supports construction from arbitrary integer byte arrays,
  unsigned byte lookup, owned slicing, and append; `Command.stdin_bytes`
  transmits those bytes without text recoding, allowing raw protocol fixtures
  to remain bytes-first without shell or PowerShell escape hatches.
- [x] invocation-owned `Child` values provide typed native top-level-window
  key, pointer, raw message, geometry, nonactivating resize, and child-control
  text/click operations on Windows. The API re-resolves the child's window and
  control IDs on every call and never turns the opaque observation token into
  a persisted native handle; theme, selection, workbench, remote-UI, and UX
  journeys share it.
- [x] `rhai::clipboard` provides unrestricted operating-system Unicode text
  get/set operations on Windows. It filters neither content nor caller and is
  a general local runtime API rather than an Agent permission boundary.
- [x] `rhai::image::inspect_png` decodes one explicit PNG into typed
  dimensions, sampled RGB, and luminance facts under a 64 MiB decoded-memory
  robustness bound, removing `System.Drawing` from visual qualification.
- [~] `rhai::runtime` exposes stable invocation/API/version/limits facts;
  unstable implementation handles are not part of the current public object
  tree, but this is API design rather than a permission boundary.
  - [x] `temp_dir` exposes only the current invocation-owned directory;
    `atomic_write` and `atomic_write_bytes` publish a complete same-volume
    replacement without exposing supervisor or OS handles.
  - [x] `append_sync` and `append_sync_bytes` append one bounded record without
    truncation, `sync_all` it, and sync a newly-created parent directory. They
    provide the durable JSONL journal primitive used by the isolated system-
    WebView measurement migration; write/open/sync phases remain typed runtime
    failures rather than an Agent permission boundary.
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
- [ ] an operation that is not yet represented faithfully with truthful typed
  outcomes remains discoverable as unimplemented/degraded with a typed reason;
  it is never silently omitted, reported as successful, or replaced by a
  policy-filtered variant.
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
  - [x] `scripts/rh/internal-version-policy.rh` is the second migrated
    production responsibility and exercises argv-safe process execution,
    bounded capture, typed exit status, cwd, and repository file reads.
- [x] migrate one independently testable responsibility at a time through
  `parallel -> parity-proven -> default-rhai -> PowerShell deleted`; obsolete
  unreachable responsibilities may instead cross a caller-audited functional
  deletion boundary. All 43 frozen baseline scripts have left the v0.1.10
  working tree.
- [x] parity evidence compares the same inputs, structured outputs, exit
  classification, diagnostics, cancellation, cleanup, encoding, path behavior,
  and clean-machine recovery; a Rhai failure cannot hide the PowerShell
  last-known-good result.
- [x] once one Rhai responsibility reaches parity and all normal callers switch
  to it, or an obsolete responsibility is proven unreachable and superseded,
  delete that PowerShell implementation immediately instead of accumulating a
  release-wide migration backlog; all 43 baseline scripts are deleted.
- [x] every migrated item records its old path, replacement path, switching
  commit, parity evidence, and deletion state in this PRD. Git history is the
  only archive after the explicit rollback window closes.
- [ ] build, check, qualification, package, release, credential, and GitHub
  workflow entry points may gain parallel Rhai candidates but do not switch
  their default implementation in v0.1.9.

Migration ledger:

| Responsibility | Replacement | Removed source | Switching commit | Evidence | v0.1.10 state |
|---|---|---|---|---|---|
| Cargo target inventory | `scripts/rh/target-report.rh` | `scripts/archive/powershell/target-report.ps1` | `b9d1906` | public CLI fixture plus live PowerShell/Rhai field parity, reconfirmed on 2026-07-29 against an absent target | deleted; Git history is the rollback source |
| Internal-only version policy | `scripts/rh/internal-version-policy.rh` | `scripts/archive/powershell/internal-version-policy.ps1` | `b0010f5` | public CLI `check` plus identical live PowerShell/Rhai PASS result, reconfirmed on 2026-07-29 | deleted; Git history is the rollback source |
| README artifact and command alignment | `scripts/rh/readme-examples.rh` | `tests/readme_examples.ps1` | `667f6d6` | exact live stdout parity against the six-artifact manifest and offline CLI/Mux catalogs on 2026-07-29; native `.rh` cutover on cg22 | deleted; Git history is the rollback source |
| Locked and obsolete staged-artifact cleanup | `scripts/rh/clean-locked-artifacts.rh` (+ `scripts/rh/lib/artifact_files.rh`) | `scripts/clean-locked-artifacts.ps1` | `be9a538` then native `.rh` cutover | public task black-box tests prove owned-name cleanup, unrelated-file retention, obsolete-name cleanup, and path-escape rejection; native pack uses `read_dir`/`try_remove_file` | PowerShell deleted; task entry is native `.rh`; archived Rhai entry under `scripts/archive/rhai/` |
| Cargo target cleanup preparation | `scripts/rh/prepare-target-clean.rh` | `scripts/prepare-target-clean.ps1` | `c20acc7` then native `.rh` cutover | public CLI black-box tests prove Git-native exact-root binding (including Windows short/long path aliases), allowed target set, idempotent cache-tag creation, and invalid-path/tag rejection | PowerShell deleted; task entry is native `.rh`; archived Rhai under `scripts/archive/rhai/` |
| Single executable staging | `scripts/rh/stage-artifact.rh` (+ `scripts/rh/lib/artifact_files.rh` `stage`/`stage_as`) | `scripts/stage-artifact.ps1` | `e087842` then native `.rh` cutover | public CLI black-box tests prove normal replacement, invalid-name rejection, and Windows running-image parking before replacement | PowerShell deleted; task entry is native `.rh`; archived Rhai under `scripts/archive/rhai/`; `stage-build` now reuses the same native INT `stage`/`stage_as` |
| Local executable manifest validation | `scripts/rh/validate-artifact-manifest.rh` (+ `scripts/rh/lib/artifact_manifest.rh`) | `scripts/artifact-manifest.ps1` | `e2276cc` then native `.rh` cutover | public CLI black-box tests prove the canonical schema and reject duplicate/invalid names, invalid subsystem/probe contracts, empty roles, and missing size budgets; native pack qualifies without host_eval/run_script | PowerShell deleted; task entry is native `.rh`; archived Rhai entry retained under `scripts/archive/rhai/` while sibling Rhai tasks still load the shared `.rhai` lib |
| Source build identity freeze | `scripts/rh/build-identity.rh` (+ `scripts/rh/lib/build_identity.rh`) | `scripts/build-identity.ps1` | `b082c3b` then native `.rh` cutover | public CLI black-box tests prove exact Git-root binding, clean/dirty truth, profile validation, batch-safe fields, and exact SHA-256 build-input identities | PowerShell deleted; task entry is native `.rh`; archived Rhai under `scripts/archive/rhai/` |
| Staged build provenance | `scripts/rh/write-build-metadata.rh` (+ `scripts/rh/lib/build_metadata.rh`) | `scripts/write-build-metadata.ps1` | current migration change then native `.rh` cutover | public CLI black-box tests prove frozen and live identities, executable size/hash capture, clean-source and frozen-input drift rejection; direct old/new field parity excludes only the generation timestamp | PowerShell deleted; task entry is native `.rh`; archived Rhai under `scripts/archive/rhai/`; shared with `stage-build` |
| Built artifact orchestration | `scripts/rh/stage-build.rh` (+ native `artifact_files` / `build_metadata`) | `scripts/stage-build.ps1` | current migration change then native `.rh` cutover | public CLI composition fixture proves cleanup, obsolete removal, staging, metadata and pre-mutation Git-root rejection; actual `build.bat` and direct old/new directory parity cover the six-artifact path | PowerShell deleted; task entry is native `.rh`; archived Rhai under `scripts/archive/rhai/`; `stage`/`stage_as` results are INT 0/1 only |
| Read-only release preflight | `scripts/rh/preflight.rh` | `scripts/preflight.ps1` and `scripts/preflight-selftest.ps1` | current migration change | public CLI real-Git fixtures prove clean/CRLF success, dirty/wrong-branch/bad-lock/bad-manifest fail-closed reports, nested output creation, and remote credential redaction | deleted; `check.cmd` runs the Rust black-box fixture and invokes the named Rhai task for release preflight |
| Preflight latency benchmark | `scripts/rh/preflight-benchmark.rh` | `scripts/preflight-benchmark.ps1` | current migration change | public worker black-box benchmark against a clean Git clone proves five successful preflight subprocesses, p95 target enforcement, durable JSON evidence, and scratch cleanup | deleted; release check invokes the named Rhai task directly |
| Locked dependency and SPDX inventory | `scripts/rh/supply-chain.rh` | `scripts/supply-chain.ps1` | current migration change | public task covers every resolved Cargo.lock package, reviewed licenses, direct-notice alignment, deterministic ordinal ordering, SPDX structure and scratch cleanup; old/new semantic parity differs only in producer identity and ordering | deleted; ordinary and release checks invoke the named Rhai task |
| Obsolete v0.1.8 public-candidate decision and self-test | Current qualification receipt, byte-qualified package, release preflight, and explicit approval boundaries | `scripts/public-candidate-policy.ps1` and `scripts/public-candidate-policy-selftest.ps1` | current migration change | the legacy self-test passed before removal; `git grep` proves no operational caller, while current qualification/package self-tests own the retained fail-closed invariants | deleted as unreachable version-specific duplication; Git history is the rollback source |
| Unintegrated PowerShell journey-manifest prototype and self-test | Planned shared Rhai smoke harness plus the existing qualification receipt boundary | `tests/JourneyManifest.ps1` and `tests/journey_manifest_selftest.ps1` | current migration change | the prototype self-test passed before removal, but `git grep` proves no smoke, check, CI, qualification, or release caller; delivery PRD already marks the machine-readable step manifest incomplete | deleted rather than preserving a second unused evidence model; Git history is the rollback source |
| PRD, evidence, CLI/protocol, and Mux catalog alignment | `scripts/rh/prd-alignment.rh` | `tests/prd_alignment.ps1` | current migration change | public task reproduces the exact legacy live-catalog PASS result and isolated black-box coverage rejects an unsupported alignment-contract schema | deleted; both quick and full checks invoke the named Rhai task |
| Owned-child cleanup self-test and first shared Rhai harness foundation | `scripts/rh/harness-cleanup-selftest.rh` plus `scripts/rh/lib/test_harness.rh` (archived Rhai under `scripts/archive/rhai/`) | `tests/harness_cleanup_selftest.ps1` | native `.rh` cutover (codegen rev 40) | public task and Windows integration prove exact registered-child forced cleanup, survival of an unregistered sibling until explicit cleanup, orphan-free persisted proof, and original-failure retention; Native pack he_count=1 | deleted; task entry is native `.rh`; quality gate invokes the named task |
| Shared black-box process, diagnostics, evidence, and cleanup harness | `scripts/rhai/lib/test_harness.rhai` | `tests/TestHarness.ps1` | current migration change | every retained smoke journey plus diagnostic and qualification self-tests now use the shared Rhai module; its dedicated self-test proves exact child ownership, bounded evidence, orphan-free cleanup, and original-failure retention | deleted after the final qualification-selftest caller switched; no operational PowerShell consumer remains |
| Working-context proxy privacy and restart journey | `scripts/rh/working-context-smoke.rh` plus the shared harness (archived Rhai under `scripts/archive/rhai/`) | `tests/working_context_smoke.ps1` | native `.rh` cutover (codegen rev 50) | named task and Windows public integration prove isolated GUI/server launch, safe proxy facts, archived-control non-mutation, no secret in snapshot/pane/workspace/events/stderr, non-persistence across restart, stable completed-child identity, and orphan-free cleanup; Native pack he_count=1 | deleted; task entry is native `.rh`; qualification discovery and execution use the named journey |
| Headless server authority journey | `scripts/rhai/server-smoke.rhai` plus typed owned-child platform facts and the shared Rhai harness | `tests/server_smoke.ps1` | current migration change | named task and Windows public integration preserve no-top-level-window evidence, exact PID/ownership, lease lifecycle and gated interaction, real PTY, replay/conflict/asynchronous receipts, causal events, workspace persistence, graceful shutdown, and orphan-free cleanup | deleted; qualification discovery and execution use the Rhai journey directly |
| Same-server real-byte GUI upgrade and rollback journey | `scripts/rh/remote-ui-upgrade-smoke.rh` plus CLI lease probes and the shared harness (archived Rhai under `scripts/archive/rhai/`) | `tests/remote_ui_upgrade_smoke.ps1` | native `.rh` cutover (codegen rev 50) | named task preserves distinct GUI hashes/build identities, competing-lease rejection, one stable server/epoch/PTY/draft state, output streamed across replacement, incompatible-protocol rejection, rollback scrollback continuity, public close/detach behavior, and orphan-free cleanup; Native pack he_count=1 | deleted; task entry is native `.rh`; qualification discovery and execution use the named journey |
| Coalesced wake, concurrent IPC, PTY output, and expired raw mutation journey | `scripts/rhai/wake-smoke.rhai` plus unrestricted `std::net::TcpStream`, `rhai::hash::fnv1a64`, and the shared Rhai harness | `tests/wake_smoke.ps1` | current migration change | named task and Windows public integration preserve an isolated headless server, 32 concurrent versioned snapshot clients, 80-line PTY progress, raw newline-delimited IPC, typed expired no-op receipt, unchanged tab note, and orphan-free owned-child cleanup | deleted; qualification discovery and execution use the Rhai journey directly |
| Native first-window and asynchronous terminal startup journey | `scripts/rh/startup-smoke.rh` plus native process inventory, child platform facts/stream bytes, and the shared native harness (archived Rhai under `scripts/archive/rhai/`) | `tests/startup_smoke.ps1` | native `.rh` cutover (codegen rev 60) | bundled validation and pack build prove Native execution with he_count=1 while preserving the one-second native-window budget, public asynchronous terminal-ready wait, inherited-stderr guidance, second-launch handoff, nonblocking CLI-style/invalid GUI arguments, absence of a nested Script worker, graceful shutdown, and orphan-free cleanup | task entry is native `.rh`; ordinary qualification and Windows CI invoke the named journey; the old Rhai source is archived |
| Public CLI, typed control, UI bridge, and PTY lifecycle journey | `scripts/rhai/cli-smoke.rhai` plus the shared Rhai harness | `tests/cli_smoke.ps1` | current migration change | named task preserves all nine public evidence IDs across receipt replay/conflict, offline validation, operation/UI-bridge discovery, renderer-neutral bootstrap/delta causality, typed Tabs actions/events, Composer/PTY/Backspace/scroll/screenshots, stable creation IDs, remain-on-exit, and explicit close; command evidence is now bounded per record | deleted; ordinary qualification and Windows CI invoke the named Rhai journey |
| Loopback HTTP test fixture | `scripts/rhai/script-http-fixture.rhai` plus unrestricted `std::net::TcpListener` and raw `Bytes` operations | `tests/script_http_fixture.ps1` | current migration change | full public Script smoke preserves status/echo/large/async/slow/cancel/malformed/disconnect/TLS paths, privacy-bounded audit, typed host failures, cleanup, and a delayed-first-byte Windows accepted-socket regression | deleted; the Script smoke launches only the Rhai fixture |
| Theme preview, persistence, PTY continuity, and rendered differentiation | `scripts/rh/theme-smoke.rh` plus native typed child-window input, structured PNG inspection, and the shared native harness (archived Rhai under `scripts/archive/rhai/`) | `tests/theme_smoke.ps1` | native `.rh` cutover (codegen rev 59) | bundled validation and pack build prove Native execution with he_count=1 while preserving Dark/Light preview, Cancel and physical Escape rollback, settings persistence, stable PTY PID/output, native PNG luminance separation, restart state, bounded retained failure diagnostics, and orphan-free cleanup | task entry is native `.rh`; qualification and diagnostic-bundle probes invoke the named task; the old Rhai source is archived |
| Workbench inline editing, archived Proxy chrome, and compact tree geometry | `scripts/rhai/workbench-smoke.rhai` plus typed child-window click dispatch and the shared Rhai harness | `tests/workbench_smoke.ps1` | current migration change | live old/new parity preserves physical Edit/Save/Cancel, independent name/note drafts, Composer isolation, archived Proxy geometry/actions, a four-level CJK hierarchy, 180/250/480 px density, bounded row geometry, and orphan-free cleanup | deleted; qualification discovery and execution invoke the named Rhai task |
| Fleet discovery, events, launch context, restart, and Mux safety | `scripts/rhai/fleet-smoke.rhai` plus the shared Rhai harness | `tests/fleet_smoke.ps1` | current migration change | live old/new ordinary and 16×258 concurrent-load parity preserves all six evidence IDs, dead-record pruning, explicit/implicit/ambiguous instance selection, scoped environment and Codex launch, typed event catalog/causality/wait timeout/cancellation/restart/gap, Mux compatibility/destructive safety, and multi-address cleanup; the Rhai stress path completes in about 63 seconds versus 116 seconds for PowerShell | deleted; ordinary qualification skips the explicit load while stress/release appends `--event-load` to the same named task |
| Replaceable GUI, physical workbench, and in-place server-fault recovery | `scripts/rhai/remote-ui-smoke.rhai` plus unrestricted native child-window, operating-system clipboard, arbitrary-process termination, and the shared Rhai harness | `tests/remote_ui_smoke.ps1` | current migration change | live PowerShell last-known-good and named Rhai task both pass the complete journey: split GUI/server roles, detach and same-server PTY continuity, replacement UI, physical toolbar/modal/Settings/CWD/tree/scroll/selection/clipboard behavior, real server force-termination, hidden offline input, local disconnected close, same GUI PID/window identity reconnect to a new PID/epoch/lease, screenshots, stale-registration pruning, and zero owned orphans | deleted; qualification discovery and execution invoke the named Rhai task |
| Unrestricted Script Runtime public regression | `scripts/rhai/script-smoke.rhai` plus `scripts/rhai/lib/script_smoke_helpers.rhai`, arbitrary `Bytes` construction, and binary child stdin | `tests/script_smoke.ps1` | current migration change | the named Rhai task emits all eighteen registered Script evidence IDs and passes API/comparison discovery, process/stream/task/HTTP/filesystem lifecycles, modules and compatible/degraded tasks, raw line/framed worker protocols, timeout/crash/parent-exit/concurrency supervision, typed Fleet observation/mutation, the direct-entry north-star task, audit privacy, recovery, and retained failure diagnostics in about 24 seconds | deleted; qualification and diagnostic-bundle probes invoke the named Rhai task |
| CLI, GUI, and Script retained failure-bundle self-test | `scripts/rhai/diagnostic-bundle-selftest.rhai` plus the shared Rhai harness | `tests/diagnostic_bundle_selftest.ps1` | current migration change | the named task creates one intentional CLI bundle and concurrently drives real theme and Script failure probes; it verifies exact identity markers, bounded local diagnostics and command logs, GUI/worker success evidence, privacy metadata and source redaction, orphan-free cleanup, and removal of only newly owned bundles | deleted; the full quality gate invokes the named Rhai task directly |
| Qualification fail-closed self-test | `scripts/rhai/qualification-selftest.rhai` plus `scripts/rhai/lib/qualification.rhai` and the shared Rhai harness | `scripts/qualification-selftest.ps1` | current migration change | named task and Windows public integration reject a missing manifest, failed/skipped gates, missing stress evidence, mismatched artifact-manifest provenance, and an invalid CLI diagnostics bundle while cleaning only invocation-owned scratch | deleted; internal dry-run and ordinary qualification callers invoke the named Rhai task |
| Byte-qualified offline Windows packaging | `scripts/rh/package-qualified.rh` plus `scripts/rhai/lib/package_qualified.rhai` | `scripts/package-qualified.ps1` | current migration change | named task validates the exact receipt, HEAD, lockfile, manifests, SBOM, metadata, executable hashes, gate set, and static payload before staging a ZIP through `tar.exe`; the module contains no build, tag, push, or release action | deleted; the named task is the production package boundary |
| Qualified package boundary self-test | `scripts/rh/package-qualified-selftest.rh` plus the shared package module | `scripts/package-qualified-selftest.ps1` | current migration change | old and Rhai self-tests pass; named task and Windows public integration prove exact ZIP members and reject Cargo.lock, artifact-manifest, executable-byte, and Git-HEAD drift with exact owned-fixture cleanup | deleted; ordinary qualification invokes the named rh task |
| Unintegrated professional terminal-selection prototype | Future public Rhai professional-selection journey after the product slice ships | `tests/terminal_selection_smoke.ps1` | current migration change | caller audit finds no check, CI, qualification, or registered evidence consumer; direct execution fails before interaction at zero terminal columns, while the Windows double-click handler does not implement the claimed word/third-click behavior and the product PRD keeps that slice planned | deleted as misleading dead test code; Git history retains the prototype |
| Semantic UX geometry, interaction, restart, and no-activate journey | `scripts/rhai/remote-ui-smoke.rhai`, `scripts/rhai/startup-smoke.rhai`, and `scripts/rhai/working-context-smoke.rhai` plus typed foreground-window observation | `tests/ux_smoke.ps1` | current migration change | the three named Rhai journeys emit all nineteen declared UX/parity evidence IDs and prove semantic minimize/maximize/resize, locale, modal wait, hierarchy cycle rejection and child promotion, adaptive Tabs, physical focus/scroll/selection/clipboard, Settings isolation, safe CWD quoting and OSC 7, close/detach, exact no-activate observation, and persistent tab metadata across restart | deleted; qualification owns the behavior through the three existing journeys without launching a duplicate GUI fleet |
| Cross-platform UX parity orchestration and evidence matrix | `scripts/rh/platform-ux-parity-smoke.rh` plus the shared native harness (archived Rhai under `scripts/archive/rhai/`) | `scripts/rhai/platform-ux-parity-smoke.rhai` | native `.rh` cutover (codegen rev 58) | bundled validation and pack build prove Native execution with he_count=1 while preserving startup, wake, platform branch execution, partial evidence attribution, failure-root classification, and JSON/CSV matrix production | all three Windows/Linux/macOS task entries are native `.rh` and invoke `agenterm-rh`; the old Rhai source is archived |
| Fast repository lint | `scripts/rh/lint.rh` plus thin `lint.cmd` bootstrap | `lint.ps1` | current migration change | public task and wrapper pass JSON parsing, strict UTF-8/NUL/conflict-marker hygiene, production Rhai checks, malformed JSON/UTF-8/conflict/Rhai self-tests, and Rust rustfmt/Clippy mode; the bounded `check-many` manifest path runs each source in a fresh Engine with typed per-file diagnostics while avoiding one Script process per file (67 production files: 5,398 ms to 2,097 ms on the 2026-07-31 Windows baseline); process inspection confirms no PowerShell child | deleted; check calls the same Rhai task and the batch file owns only Script worker bootstrap and argument forwarding |
| Integrated quality-gate orchestration | `scripts/rhai/check.rhai`, `scripts/rh/artifact-verification.rh`, and thin `check.cmd` bootstrap | `check.ps1` | current migration change | `check.cmd --quick` and `--skip-smoke` pass through the public named task; the latter completes 255 library tests plus integration groups, staged six-artifact verification, declaration discovery, diagnostics, timing, and fail-closed qualification bookkeeping without PowerShell | deleted; CI and release workflow call the same batch bootstrap and Rhai task |
| Qualification result and receipt production | `scripts/rhai/check.rhai` plus `scripts/rhai/lib/qualification.rhai` | `scripts/qualification.ps1` | current migration change | the named check task validates exact required gates and executable evidence declarations, rejects failed/skipped or missing-stress results, binds artifact metadata to source state, and writes the receipt only for a complete stress-inclusive run | deleted; quality orchestration imports the shared Rhai module directly |
| Approved release coordination and rehearsal | `scripts/rhai/release.rhai`, shared qualified-package module, and thin `release.cmd` bootstrap | `release.ps1` | current migration change | release requires clean `main`, rejects v0.1.7 and an existing tag, runs stress-inclusive qualification, creates a byte-qualified package and durable hash-bound rehearsal report, and exposes a no-mutation rehearsal mode; publish creates an annotated tag and atomically pushes `main` plus tag with local-tag rollback on failure | deleted; `release.cmd` is the sole explicit publication boundary and CI validates the same check task |

### v0.1.10 completion commitment

- [x] v0.1.10 completes the replacement of repository-owned PowerShell
  automation; this is a release completion gate rather than a best-effort
  migration track.
- [x] the dated 2026-07-29 frozen baseline is 43 tracked `.ps1` files: 3 at
  the repository root, 17 under `scripts/`, 21 under `tests/`, and 2 retained in
  `scripts/archive/powershell/`.
- [x] migration progress is 43/43 deleted and 0/43 remaining; progress is
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
- [x] the repository-root `agenterm.tasks.json` is now the offline task
  catalog and ships forty-six ready tasks. The public task catalog is the
  authoritative live inventory rather than a duplicated PRD command list.
  Each task now
  exposes validated same-manifest dependencies, a closed Windows/Linux/macOS
  platform set, and a closed side-effect classification in both human and JSON
  offline listings. Unknown, self, duplicate, or cyclic dependencies degrade
  before execution. The existing
  two-input Script contract verifier is
  intentionally not advertised as ready until catalog fixture production is
  part of its task. Build, lint, test, qualification, package, rehearsal,
  release, and evidence metadata are declared by schema-v3 contracts.
- [x] `git ls-files '*.ps1'` returns no files. Tests,
  helpers, and archived implementations are not exceptions; Git history is the
  permanent archive after each parity and rollback boundary closes.
- [x] `agenterm.tasks.json` and shared Rhai modules own build, lint, test,
  qualification, packaging, release rehearsal, and approved release semantics.
- [x] batch files, Unix shell entry points, and CI YAML may bootstrap a pinned
  Rust toolchain and forward arguments/exit status to the same Rhai task, but
  must not duplicate task selection, budgets, evidence, packaging, or release
  policy. The four root Windows aliases are exact one-line task selectors;
  `scripts/bootstrap.cmd` is the sole generic stage-0 implementation and only
  discovers the repository, builds/copies the Script worker, forwards one task
  plus argv/exit status, and cleans its owned copy. `scripts/rhai/build.rhai`
  owns build profile selection, frozen identity, Cargo invocation, staging,
  target reporting and release-target cleanup; check, lint, release mode and
  nested qualification remain in their named Rhai tasks. The migration audit
  freezes all five batch files, exact aliases, the generic bootstrap boundary,
  and zero known business-rule tokens.
  Four matching root `.sh` aliases and `scripts/bootstrap.sh` provide the same
  stage-0 contract on Linux/macOS. Native Unix build compiles the five client
  roles; default Unix check is the portable Quick lane and default Unix
  release is validation-only. Full GUI qualification, Windows packaging and
  publication remain explicitly unavailable rather than silently degraded.
  Every repository-owned `.cmd` or `.bat` must therefore either be replaced by
  a named Rhai task or have an equivalent checked-in `.sh` entry for
  Linux/macOS. Matching names alone are insufficient: the cross-platform audit
  and CI must execute the Unix entry through the native `agenterm-rhai` task
  host and verify argument, exit-status, side-effect, and evidence parity.
- [x] the migration proceeds from low-side-effect rules and reports through
  build/static quality, public black-box tests, and finally qualification and
  delivery. Each responsibility must prove normalized parity or stronger public
  evidence before callers switch and its `.ps1` leaves the tree.
- [x] Script Runtime gaps are filled with stable typed APIs or shared Rhai
  modules. Rhai scripts must never invoke PowerShell as an escape hatch.
- [x] the zero-`.ps1` drift gate prevents the old source layer from returning;
  clean-checkout qualification and rehearsal process-tree evidence is
  complete.
  The same gate also freezes four Windows aliases, four Unix aliases, both
  generic bootstraps, and zero batch/shell business-rule drift.
- [x] “PowerShell replacement” applies to repository-owned automation and its
  delivery process, not to users launching PowerShell as a terminal shell or
  to terminal-compatibility coverage. Such compatibility tests must be driven
  by the Rhai harness and cannot carry repository business rules in
  PowerShell.
- [x] completion is measured only after parity evidence, every caller cutover,
  source `.ps1` deletion, and drift-gate coverage. Static zero-file evidence is
  paired with clean-checkout process-tree evidence proving that repository
  automation under build, check, qualification, packaging, and release
  rehearsal does not spawn `powershell.exe` or `pwsh.exe`. An explicitly
  declared PowerShell terminal payload inside the remote-UI compatibility
  journey is recorded separately and carries no repository automation logic.
  A 2026-07-30 artifact-free clone of commit `7c88ff0` completed all 34
  stress-inclusive gates, produced a qualified rehearsal package, observed no
  PowerShell automation or undeclared terminal payload across 728 process-tree
  scans, left remote refs and local tags unchanged, and removed its owned
  process tree and clone.

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
  - [x] Script API v2 maps all 24 current typed operations exactly once; the
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

## Future (pre-v0.2.0; schedule after v0.1.15)

Parallel **rh** AOT track is shipped on `main` for trial; full Rhai cutover and
layered deployment productization are **not** in v0.1.15 scope. Design SSOT:
[`plan/design-rh-aot.md`](../plan/design-rh-aot.md).

- [~] **rh execution backend** (`crates/agenterm-rh`, `agenterm-rh` CLI):
  rh-0→rh-2 — subset check, transpile→rustc AOT, pack qualify, fleet native
  shim, host eval (stdlib parity via `configure_engine`), source-hash AOT cache,
  default `AGENTERM_SCRIPT_BACKEND=rh`, project/API-aware check-many, native
  range and break/continue control flow, and dedicated `./rh-check.sh` suite.
- [~] **Rhai → rh migration** (incremental, compatibility boundary explicit):
  `agenterm-rh` is the bootstrap task command front door and the sole
  stage-0 cached worker. It directly hosts the task engine, framed worker and
  incremental Rust compiler wrapper through the shared library; CI task
  callers and authoritative worker/check-many black boxes use this entry.
  Its dedicated gate now includes an isolated single-PE CLI suite, an external
  public-library contract suite, complete fixture checking, typed budget
  failures, and native AOT qualification for supported control flow.
  The task manifest and automation corpus remain `.rhai`.
  Worker/REPL implementation has moved from the
  `agenterm-rhai` binary into the shared `script_worker` library, but its
  interpreter, host-eval fallback, and AST parsing remain on Rhai while their
  typed boundaries migrate. Release verification requires both manifest roles
  and validates each role's declared offline version probe.
- [ ] **Layered deployment** (JVM / JAR analogue):
  - **Base runtime** — stable PE family (`agenterm`, `agenterm-rhai`, …):
    host Facade, broker, supervision, qualification; rebases rarely
  - **Application layer** — signed **rh pack** (native artifact + manifest;
    future Logic Pack / gateway pack): load in-process via `script_rh_pack`,
    hot-swappable without rebuilding the base PE
  - Control Center, gateway sidecar, and task runners consume the same host
    C ABI (`rh_register_host_v2`, `rh_host_eval`, fleet shim)
- [~] **qjs execution backend** (`crates/agenterm-qjs`, in progress,
  capability parity with rh): QJS-M0 (skeleton + `rquickjs` binding +
  minimal eval, `140a9152`), QJS-M1 (`check`/`eval`/`check-many` CLI verb
  parity, `cc7df9df`), root-workspace integration (`d5ac9a8a`, same
  `[[bin]]`-in-root-package shape as rh), and the QJS-M2 host-binding layer
  (`__host.fleet_call`/`args_len`/`arg`/`print` matching
  `agenterm_lua::LuaHostFunctions`'s shape + a near-line-for-line
  `scripts/qjs/lib/fleet.js` port of `scripts/lua/lib/fleet.lua`,
  `e50fbbf8`) are shipped; see "Script engine family" above and
  `plan/plan-v0.1.16.md` §1 QJS-M0/M1/M2. `agenterm::script_backend`
  wiring and `task`/`run`/`pack`/`qualify` remain open (QJS-M2 continues).
  Open risk
  questions recorded here so they aren't silently assumed away by "it built
  and evaluated `1+2`" — none are blocking QJS-M0, all should be resolved
  (or explicitly accepted) before qjs leaves "planned" for "partial":
  - **Parallel-spec drift vs lua.** lua is currently the only independent
    implementation stress-testing "capability parity with rh" in practice;
    qjs is being built in parallel rather than after it (user-accepted risk,
    2026-08-07, see `plan/plan-v0.1.16.md` §7). The two engines may resolve
    the same L2/CLI ambiguity differently; a reconciliation pass against
    both is likely needed once both have real usage, not assumed away by
    either engine shipping first.
  - **Root-workspace C-dependency interaction — resolved (2026-08-07).**
    rquickjs bundles quickjs-ng C source built via the `cc` crate; qjs is now
    wired into the root workspace (commit `d5ac9a8a`) alongside lua's own
    C-bundled binding. `cargo check --workspace` /
    `cargo build -p agenterm --bin agenterm-qjs` /
    `cargo test -p agenterm-qjs` are all clean — no MSVC CRT linkage
    mismatch, no symbol collisions, no `cc`/`bindgen` interaction surprises.
    Verified, not assumed.
  - **Thread/concurrency model mismatch — partially validated the hard way
    (2026-08-08).** Rhai/rh keep "the engine and its Scope … on one
    evaluation thread" (see "v0.1.9 runtime architecture" above). QuickJS's
    `Runtime`/`Context` are similarly not freely Send+Sync (single-
    threaded-VM design, same family as V8 isolates) — this stopped being a
    theoretical concern when the first `__host` binding attempt
    (`crates/agenterm-qjs/src/host.rs`) captured a cloned `Ctx<'js>` inside
    a bound closure's environment. That closure lives inside the JS heap as
    a Function value *and* holds a Rust-level handle back into the same
    context — a reference cycle QuickJS-ng's GC can't collect, which
    crashed the whole process on `Runtime` teardown
    (`Assertion failed: list_empty(&rt->gc_obj_list)`,
    `STATUS_STACK_BUFFER_OVERRUN`), not just a failing test. Fixed by
    taking `Ctx<'_>` as an ordinary *per-call* closure parameter
    (`rquickjs::FromParam` supplies a fresh one each invocation) instead of
    capturing it — reproduced with a 15-line minimal case before landing
    the fix, now locked in by regression tests
    (`eval::tests::calls_host_args`, `fleet_call_error_surfaces_as_js_exception`).
    **Still unverified:** whether the existing one-thread-per-engine
    worker pattern transfers directly to qjs's worker/framed-worker
    integration (QJS-M2 continuation) — this incident narrows the risk
    to "know the failure mode and its fix," not "resolved."
  - **No AOT — different performance character than rh.** qjs has no
    analogue to rh's T0–T3 layered AOT (`plan/plan-rh-3.md` §1 point 3);
    QuickJS interprets its own bytecode with no JIT. Capability parity does
    **not** imply performance parity; any future latency-sensitive
    consumer must evaluate qjs on its own merits rather than assume rh-like
    throughput.
  - **Unrestricted-runtime philosophy must carry over, not just the API
    shape — partially verified (2026-08-08).** Rhai/rh's product position
    (see "v0.1.9 product position" above) is a deliberately
    **unrestricted** local runtime — no sandbox, no capability tiers;
    authorization is the future Agent harness's job, not the engine's.
    `rquickjs::Context::full` is likewise unrestricted by default, and the
    `__host` binding layer built for QJS-M2 doesn't remove or hide any
    global object, and propagates host-side errors (e.g. a failing
    `fleet_call`) through to the script as a real, catchable JS exception
    rather than swallowing or downgrading them
    (`eval::tests::fleet_call_error_surfaces_as_js_exception`) — no
    evidence yet of qjs becoming a second, more-restricted security model.
    Still open: this only covers the host-binding layer built so far
    (`fleet_call`/`args_len`/`arg`/`print`); the fuller `std.*`/network/
    filesystem surface rh exposes hasn't been ported to qjs yet, so this
    can't be called fully resolved.
  - **Version/hash reproducibility for future receipts.** rh AOT/qualify
    receipts bind to source hash (`design-rh-aot.md`). If qjs packs ever
    join the same receipt contract, quickjs-ng's bundled version needs an
    explicit pinning policy (`Cargo.lock` alone may be insufficient if
    receipts need to name the exact engine build, not just "some rquickjs
    0.12.x") — open question, not yet answered.
  - **CI/build-time cost.** Compiling QuickJS's C source (`quickjs.c` is a
    large single translation unit) adds non-trivial time to every clean
    build once wired into the root workspace/CI cache; unmeasured against
    this repo's existing build-time budget concerns (see `plan-v0.1.16.md`
    §0.3, R′ group).

Current non-goals: renaming or deleting the compatibility PE before worker and
REPL ownership moves; npm-style remote rh imports; Cranelift direct codegen
(transpile→rustc remains the production backend until a later gate).

## Explicitly deferred

- npm compatibility, arbitrary remote imports, third-party package lifecycle,
  and Node/Bun binary or API compatibility; an AgenTerm-owned signed
  package/application catalog remains a future optional-component track rather
  than npm emulation inside the script runtime;
- a persistent or automatically started script daemon and cross-invocation
  mutable runtime state;
- raw sockets, UDP, WebSockets, and higher-level network modules beyond the
  shipped unrestricted TCP stream/listener primitives remain planned Script
  Runtime expansion; when shipped, they expose the operating-system authority
  of the invoking user without Script-owned permission gates or endpoint
  allowlists;
- event handlers, watch mode, and durable background scheduling;
- durable REPL state, implicit REPL daemon startup, on-disk command history,
  and concurrent evaluation of one REPL Scope;
- Agent permission, approval, credential, quota, and natural-language policy
  belong to the separate Agent harness;
- GUI command palette delivery beyond its P1 consumption of the shared named
  task catalog.
