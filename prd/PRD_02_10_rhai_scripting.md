# Rust host + Rhai scripting

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] pure Rhai run, eval, check, and API discovery execute in a sidecar
- [x] observe Rhai uses typed bounded workspace, tab, snapshot, capture, and event broker methods
  and receives no ambient state or direct IPC authority
- [x] Rhai denies ambient mutation authority and enforces operation budgets
- [x] value limits, parent-enforced wall time, typed limit classification,
  cancellation, worker ceilings, and privacy-bounded audit are enforced
- Product boundary
  - [x] design choice: Rust (`.rs`) implements the host, capability checks,
    and stable APIs; Rhai (`.rhai`) is the user scripting language
  - [x] scripting executes in an optional sidecar/worker; the GUI owns no
    Rhai engine and communicates only through versioned typed contracts
  - [x] observe scripts consume reviewed public tab/workspace operations;
    they receive no direct Win32, PTY, or mutable GUI-state access
  - [x] execution runs off the window thread and never delays first
    window display or terminal painting
  - [x] a script failure is isolated from the GUI, IPC server, terminal
    readers, and workspace persistence
- Runtime architecture
  - [x] scripting lives in `agenterm-script.exe`; the GUI state machine
    remains independent of Rhai engine types and communicates through
    versioned protocol, supervisor, broker, operation, and audit boundaries
  - [x] one engine factory builds each invocation from an immutable
    capability set, source label, API version, and resource budget
  - [x] lazy process isolation ensures no engine construction or script
    directory scan on GUI startup
  - [ ] bounded compiled-AST cache keyed by source fingerprint, API version,
    and capability profile
  - [ ] versioned embedded prelude for shared helpers; module loading stays
    disabled until real use cases justify its size and resolver surface
  - [ ] user scripts default to `%LOCALAPPDATA%\AgenTerm\scripts`; one-shot
    execution also accepts an explicit path or stdin
  - [ ] source loading and runtime authority are separate: selecting a file
    never grants filesystem, process, or terminal-write access
- Capability profiles
  - [x] `pure`: JSON-compatible values, bounded computation, arguments,
    and captured stdout with parent hard limits
  - [x] `observe`: typed workspace, tab list, active tab, UI snapshot,
    bounded pane capture, and journal read/wait broker calls
  - [ ] `control`: `observe` plus create/select/rename/reparent/close,
    composer operations, send keys/mouse, and deterministic waits
  - [ ] separately scoped `fs.read`, `fs.write`, `env.read`, and
    `proc.exec`; roots, variable names, and executables are allowlists
  - [ ] scripts cannot grant themselves capabilities
  - [ ] destructive actions preserve native live-process confirmation
    unless an explicit, visible automation policy authorizes them
- AgenTerm host API
  - [x] typed maps/arrays and stable error codes; scripts never parse
    human-facing CLI output
  - [x] observation: `agent.workspace()`, `agent.tabs()`,
    `agent.active_tab()`, `agent.ui_snapshot()`,
    `agent.capture(tab,max_bytes)`, `agent.events_read(...)`, and
    `agent.events_wait(...)`
  - [ ] action: `new_tab()`, `select_tab()`, `set_parent()`, `set_name()`,
    `set_note()`, `set_composer()`, `send_composer()`, `send_keys()`,
    `close_tab()`, and bounded `wait_*()`
  - [ ] status providers return structured segments; the status bar owns
    layout, refresh, truncation, and error presentation
  - [x] API catalog and version are discoverable without script execution,
    including exact typed signatures, operation IDs, errors, hard ceilings,
    profile availability, and versions
- Resource and security envelope
  - [x] cap source bytes, operations, call/expression depth, collection and
    string sizes, output, wall-clock duration, broker requests/returns,
    capture bytes, event items, wait duration, and concurrent invocations
  - [x] timeout has a stable error/exit code and never blocks the GUI
  - [ ] process execution accepts executable plus argv, never an implicit
    shell string; timeout and output caps are mandatory
  - [ ] canonical root containment and reparse-point/symlink rejection for
    scoped Windows file access
  - [x] audit source fingerprint/label, requested/effective capabilities
    and budgets, broker operation IDs, duration, result class, and denial
    reason without recording source, argv, output, content, or secrets
- Future safe-scripting sidecar contract
  - Process boundary
    - [x] one fresh worker process executes one `run`, `eval`, or `check`
      invocation; the first delivery has no persistent daemon, background
      handler, module resolver, or cross-invocation mutable state
    - [x] the launcher places the worker in a kill-on-close Windows Job
      Object and owns its deadline, cancellation, stdout, stderr, and final
      exit status; a crashed or killed worker cannot affect the GUI server
    - [x] a versioned invocation envelope and result envelope travel over
      inherited anonymous pipes; source text, arguments, capabilities, and
      secrets are not placed in the process command line
    - [x] the worker never connects to GUI IPC directly. A host broker
      validates the profile and supplies only immutable typed inputs over
      the invocation channel
  - Initial profiles
    - [x] `pure` exposes bounded Rhai evaluation, JSON-compatible values,
      invocation arguments, and captured stdout only; it has no clock,
      environment, filesystem, process, network, terminal, or fleet access
    - [x] `observe` adds typed workspace metadata, tab/active-tab views,
      UI snapshots, bounded pane capture, and bounded Observable Fleet
      journal reads/waits; baselines carry epoch and sequence
    - [x] observe callers establish an explicit snapshot/workspace baseline
      and consume validated event envelopes after that sequence; restart
      and journal-gap errors remain typed and are never hidden by an
      implicit resnapshot
    - [x] `control`, filesystem, environment, process execution, network,
      package loading, event handlers, and status providers remain outside
      the first-delivery acceptance gate
  - Public commands and discovery
    - [x] `script run FILE.rhai|- [-- ARGS...]` loads a file or stdin and
      returns script stdout separately from diagnostics
    - [x] `script eval EXPRESSION` evaluates one explicit expression under
      the selected profile without loading user modules
    - [x] `script check FILE.rhai` validates Rhai without execution or a
      live server, including exact API names, profile/capability access,
      version, and static limits
    - [x] `script api --json` reports API/schema versions, profiles,
      exact functions, typed operation parameters/results/errors, defaults,
      hard ceilings, and availability without starting a GUI
    - [x] every command accepts an explicit profile and bounded overrides;
      unknown API versions, profiles, capabilities, or options fail closed
      with stable documented exit codes
  - Budgets, cancellation, and audit
    - [x] versioned nonzero defaults and immutable hard ceilings cover
      source bytes, operations, call depth, collection/string size, output
      bytes, wall time, journal events, wait duration, and worker count;
      `script api --json` exposes the effective values
    - [x] exceeding any budget produces a typed limit result and bounded
      diagnostics; output truncation is explicit and cannot produce a
      successful status
    - [x] cancellation first signals the worker cooperatively, then
      terminates its Job Object after a bounded grace period; CLI
      interruption, timeout, and parent exit cannot orphan a worker
    - [x] one append-only audit record captures invocation ID, timestamp,
      source fingerprint/label, API version, profile, requested/effective
      budgets, duration, exit class, cancellation, and denials, but not
      source, arguments, pane contents, environment values, or stdout
  - Public black-box acceptance
    - [x] tests invoke only released `agenterm-cli script` commands and
      validate stdout/stderr separation, JSON discovery, stable exit codes,
      file/stdin/eval/check behavior, and clean-machine missing-sidecar
      diagnostics
    - [x] pure-profile fixtures prove denied filesystem, environment,
      process, network, fleet, and clock access; observe fixtures prove
      typed snapshots, ordered events, epoch restart, bounded-history gap,
      wait timeout, and absence of mutation APIs
    - [x] adversarial fixtures cover parse/runtime errors, operation and
      output exhaustion, oversized values, cancellation, worker crash,
      parent exit, concurrent worker ceiling, malformed envelopes, and
      unsupported API versions without GUI latency or workspace damage
    - [x] acceptance records GUI/CLI/worker sizes and first-window timing,
      verifies no Rhai code loads during normal GUI startup, and leaves no
      worker or temporary source behind after every result class
- Extension surfaces
  - [x] phase 1 one-shot pure/immutable-observe run/eval/check and API
    discovery shipped with the v0.1.5 minimum sidecar contract
  - [ ] future candidate: versioned local registry and named commands callable
    by people, agents, and IPC through one stable ID
  - [ ] later candidate: read-only status providers with timeout, last-good
    value, visible degraded state, truncation, backoff, and host-owned layout
  - [ ] later candidate: opt-in tab/process/workspace event handlers with
    bounded queues, restart/gap semantics, cancellation, and no re-entrant GUI
    mutation
  - [ ] future scope and version ownership are intentionally reassessed after
    the v0.1.7 internal consolidation rather than inherited as commitments
  - [ ] no network client, package manager, arbitrary import, or general
    async runtime without a concrete reviewed product use case
- Verification and delivery
  - [x] the shipped pure/observe slice records per-binary size, first-window
    timing, no-script startup behavior, budgets, public CLI results, typed
    observation, authority denial, timeout/crash recovery, and worker cleanup
  - [ ] every future authority or provider slice adds
    deny/success/scope-boundary and failure-isolation black-box tests before
    its product status changes
  - [ ] status-provider delivery additionally covers timeout, invalid result,
    reload, truncation, last-known-good value, degraded state, and backoff
  - [ ] every future slice keeps `agenterm.exe` below the 4 MiB release gate; a
    large dependency or Rhai feature must earn its measured cost
