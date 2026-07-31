# Agent control plane

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- Observation
  - [x] stable active tab `id:name`
  - [x] text capture, raw escaped output, styled cell dumps
  - [x] JSON pane, tab, focus, modal, layout, and protocol snapshots
  - [x] whole-window and selected-pane PNG screenshots
  - [x] non-intrusive bounded transcript capture by stable tab ID, with
    explicit visible-vs-scrollback range, truncation metadata, and no
    viewport mutation; this requires a versioned protocol addition rather
    than automating `scroll-pane`
  - [~] incremental output sequence and event stream: terminal output has
    bounded epoch/sequence events and byte counters, but capture and waits
    cannot yet require a minimum output/event position
- Action
  - [x] create, select, rename, annotate, and close tabs
  - [x] launch Codex agent tabs with stable tab/session/IPC context and
    optional tab-scoped proxy settings
  - [x] send keys and terminal mouse events
  - [x] scroll a selected terminal viewport by rows, pages, top, or bottom
    while keeping screenshots and capture aligned with the human view
  - [x] read, replace, and submit composer content
  - [x] semantic focus and UI actions
  - [x] deterministic waits for output, composer completion, dead state,
    active tab, and focus
  - [x] direct deterministic wait predicates for modal kind and target
  - [ ] broadcast input and synchronized panes
- v0.1.7 self-feedback command contract (P0 before expanding script control)
  - [~] CLI-to-server requests now carry a versioned
    `request_id`, stable `operation_id`, resolved server/tab identity,
    before/after event position, truthful completion phase, and typed
    result/error through `--receipt-json`; representative control and dead-PTY
    paths are black-box tested, but resolved-target and typed-result coverage
    is not yet complete across every public control/destructive command
  - [~] the server keeps a bounded in-memory request deduplication/replay
    window and black-box tests cover same-ID replay plus different-payload
    rejection;
    retrying the same ID and payload cannot repeat a side effect, reusing an
    ID with different input is rejected, but a client-side transport timeout
    does not yet recover a receipt proving `outcome_unknown` versus
    non-execution
  - [~] mutation deadlines are checked on the GUI thread before execution so
    an expired request is rejected without reserving or running it; explicit
    cancellation and a blocked-GUI recovery black-box remain planned
  - [~] receipts distinguish committed, accepted, no-op, and unknown outcome,
    and the dead-PTY write regression returns a typed no-op;
    dead/unavailable targets, failed PTY writes, and unresolved selectors
    still require a command-wide false-success audit
  - [~] asynchronous Composer submission receipts publish a resolved tab,
    epoch/sequence baseline, deadline, and submission-complete wait descriptor;
    other asynchronous paths and descriptor/event-name conformance remain
    unproven
  - [~] unit and public CLI tests cover receipt serialization, replay,
    conflicts, deadline rejection, Composer completion, dead writes, stable
    target identity, and destructive terminal shutdown; full
    operation-catalog dispatch, alias, result, error, and emitted-event
    contract coverage remains planned
  - [x] public receipt replay proves same-ID same-payload replay and different-payload conflict without repeating a tab-note mutation, and proves retried `new-window`/`kill-window` create and close exactly one stable tab
- v0.1.8 typed-operation readiness for Fleet consumers (P0 prerequisite)
  - [ ] every public typed operation has one stable catalog identity,
    classification, canonical aliases, parameter/result/error schema, target
    resolution contract, availability, and version
  - [ ] catalog-to-dispatch conformance proves each entry either reaches its
    canonical implementation or returns a typed unsupported/degraded reason;
    no consumer must infer availability from missing commands or help text
  - [ ] every mutation exposed to another public consumer has resolved
    server/tab identity, request ID, deadline, replay behavior, truthful
    receipt outcome, before/after event position, and correlated post-state or
    an explicit reason that the correlation is unavailable
  - [ ] destructive operations preserve native confirmation and documented
    noninteractive lifecycle semantics, remain-on-exit, explicit close,
    tree-cycle safety, and exactly-once replay behavior
  - [ ] generated or catalog-driven public black-box coverage checks dispatch,
    aliases, target resolution, typed results/errors, deadlines, replay,
    emitted events, unsupported degradation, and restart/target-close failure
    without private GUI-state access
  - [ ] this module owns catalog and control correctness. The unrestricted
    local runtime, module/task, tool-schema, and script-facing mapping acceptance is
    owned by [Rust host + Rhai scripting](PRD_02_10_rhai_scripting.md) and is
    not duplicated here
  - [ ] Agent permissions, approvals, credential/path/network policy, and tool
    visibility are enforced by the future Agent harness before it invokes
    Script Runtime; they are never implemented by removing or denying Rhai APIs
- Protocol
  - [x] loopback-only newline-delimited JSON IPC
  - [x] feature discovery through `protocol-info`
  - [x] registered multi-instance discovery with PID, address, version,
    session, workspace, tab count, active tab, and liveness
  - [x] one ordinary user launch owns the predictable default
    `127.0.0.1:48815` server; another default launch reuses that authority,
    while additional servers require an explicit loopback `--address`
  - [x] explicit `--address` targeting; discovery automatically removes a
    record only when Windows definitively reports its PID dead, retains a
    live but temporarily unreachable process for diagnosis, and keeps
    `--prune` as the explicit override
  - [x] bounded discovery probes and clean-machine-safe explicit-address
    GUI autostart that returns as soon as IPC becomes ready
  - [x] explicit errors for unsupported operations
  - [~] IPC responses now carry optional structured error fields and a
    versioned receipt while preserving legacy fields; many command branches
    still originate human error text and ordinary CLI mode does not yet render
    every message from one canonical typed envelope
  - [ ] stable event subscription
- v0.1.11 native-local IPC and logical instances
  - [ ] this module is the single product owner for local transport,
    endpoint resolution, instance identity, registration migration, peer
    isolation, and stale-endpoint recovery; CLI and executable modules consume
    these contracts instead of defining parallel transport rules
  - [ ] freeze three separate typed identities:
    - `LogicalInstance`: the user-facing role and lifecycle class
      `main | dev | ephemeral | custom`; v0.1.11 defaults ordinary launches to
      `main`, reserves `dev` for isolated development, and keeps
      `ephemeral/custom` explicit rather than silently allocating random ports
    - `IpcEndpoint`: a versioned transport value
      `unix:<path> | pipe:<name> | tcp:<host>:<port>`; Linux/macOS derive a
      Unix domain socket for ordinary local instances, Windows derives a named
      pipe, and explicit loopback TCP remains a compatibility/diagnostic
      transport
    - `ServerScopeId`: a stable, opaque identity derived from the trusted OS
      user scope, logical instance, and namespace version; registration,
      connection handshake, singleton ownership, workspace defaults, epoch,
      and receipts must agree on it
  - [ ] the human labels `{username}_main` and `{username}_dev` are display
    values only. Raw usernames never become socket paths, pipe names, lock
    authority, or security identities; Windows derives scope from the user SID
    and Unix derives it from the effective UID, using a bounded versioned key
  - [ ] one OS-user scope may run `main` and `dev` concurrently, but each
    logical instance has at most one live authority. A same-scope launch reuses
    a compatible authority; an incompatible or ambiguously owned endpoint
    fails with a typed result instead of killing it or falling back to another
    instance
  - [ ] Unix local endpoint contract:
    - choose a trusted per-UID runtime base, create the AgenTerm instance
      directory with mode `0700`, and create the socket with mode `0600`
    - validate owner, type, permissions, path length, and symlink-free
      components before bind; use a fixed-length derived key rather than a
      truncated username when the platform `sun_path` budget is tight
    - recover a stale socket only under the same instance lock after a bounded
      connect proves it dead and PID/start identity or lease evidence proves
      the former owner is gone; never unlink a symlink, regular file,
      directory, foreign-owned node, permission failure, or timeout
    - where the OS exposes peer credentials, verify the peer UID against
      `ServerScopeId`; ownership uncertainty fails closed with a typed error
  - [ ] Windows local endpoint contract:
    - create the named pipe with an explicit DACL scoped to the current user
      SID and only separately justified system principals; do not inherit a
      broadly writable ACL
    - set `PIPE_REJECT_REMOTE_CLIENTS`, use overlapped bounded connect/read/
      write operations, and make cancellation and owner shutdown release every
      pending operation without blocking the GUI
    - use `FILE_FLAG_FIRST_PIPE_INSTANCE` or an equivalent atomic first-owner
      primitive so concurrent launches cannot create two authorities for the
      same `ServerScopeId`
    - validate the connected server identity against registration and
      handshake facts; stale registration, PID reuse, access denial, timeout,
      and namespace mismatch remain distinguishable typed outcomes
  - [ ] registration schema v2 stores the logical instance,
    `ServerScopeId`, typed endpoint, namespace/schema version, PID plus process
    start identity or lease nonce, server epoch, and existing diagnostic facts.
    Discovery reads v2 native-local records and legacy TCP/address records in
    one bounded pass, deduplicates the same authority, preserves reachable,
    unreachable, incompatible, and owner-unknown states, and never treats
    filename presence as proof of a live server
  - [ ] migration is staged rather than flag-day:
    - first ship the common resolver, schema-v2 writer/reader, mixed discovery,
      and explicit native endpoint support while the shipped TCP default
      remains usable
    - then make named pipe/Unix socket the ordinary `main` and `dev` defaults
      only after new-client/old-server and old-client/new-server compatibility,
      upgrade, rollback, stale recovery, and concurrent-start evidence passes
    - retain explicit loopback TCP and the legacy registration reader through
      a documented compatibility window; non-loopback TCP remains outside this
      local-transport change and requires its own authenticated remote-control
      threat model
    - treat `AGENTERM_IPC_ADDRESS` as a legacy explicit TCP selector during
      transition, add `AGENTERM_IPC_ENDPOINT` and `AGENTERM_INSTANCE` as the
      typed endpoint/instance environment representation, and keep all GUI,
      CLI, Control Center, Script, MCP, and mux consumers on one resolver
  - [ ] public black-box evidence covers `main/dev` isolation and singleton
    races; Unix permission, length, character, symlink, stale, and owner
    failures; Windows DACL, remote-client rejection, first-instance,
    cancellation, and bounded-I/O failures; schema-v1/v2 mixed discovery;
    upgrade/rollback; explicit TCP compatibility; and truthful structured
    snapshot/diagnostic output without leaking raw SID, home path, or
    credentials
