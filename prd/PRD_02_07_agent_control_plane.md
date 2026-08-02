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
  - [x] this module is the single product owner for local transport,
    endpoint resolution, instance identity, registration migration, peer
    isolation, and stale-endpoint recovery; CLI and executable modules consume
    these contracts instead of defining parallel transport rules
  - [x] freeze three separate typed identities:
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
  - [x] the human labels `{username}_main` and `{username}_dev` are display
    values only. Raw usernames never become socket paths, pipe names, lock
    authority, or security identities; Windows derives scope from the user SID
    and Unix derives it from the effective UID, using a bounded versioned key
  - [~] one OS-user scope may run `main` and `dev` concurrently, but each
    logical instance has at most one live authority. A same-scope launch reuses
    a compatible authority; an incompatible or ambiguously owned endpoint
    fails with a typed result instead of killing it or falling back to another
    instance
  - [~] Unix local endpoint contract:
    - [x] choose a trusted per-UID runtime base, create the AgenTerm instance
      directory with mode `0700`, and create the socket with mode `0600`
    - [x] validate owner, type, permissions, path length, and symlink-free
      components before bind; use a fixed-length derived key rather than a
      truncated username when the platform `sun_path` budget is tight
    - [x] hold a per-endpoint `0600` no-follow regular-file lease under a
      nonblocking exclusive OS lock for the complete listener lifetime; the
      lease records PID plus Linux `/proc` start ticks or macOS
      `proc_pidinfo` start time, so a same-instance concurrent authority fails
      atomically rather than racing the socket probe
    - [x] recover a stale socket only under the same instance lock after a bounded
      connect proves it dead and PID/start identity or lease evidence proves
      the former owner is gone; never unlink a symlink, regular file,
      directory, foreign-owned node, permission failure, timeout, or
      pre-lease socket without a valid predecessor identity
    - [x] Linux `SO_PEERCRED` and macOS `getpeereid` verify both accepted
      clients and connected servers against the effective UID that owns
      `ServerScopeId`; credential lookup failure or mismatch fails closed with
      a typed unsafe-endpoint error
    - [~] six-target CI retains every manifest artifact and native Linux/macOS
      cells now run the public Rhai IPC journey against isolated settings,
      workspace, registration directory, `main` and `dev` Unix authorities.
      Linux/macOS manifests include the standalone `agenterm-server` authority
      consumed by that journey and by transport-neutral clients; it is a
      product artifact rather than a CI-only server substitute.
      The journey proves `0700` runtime-directory and `0600` socket modes,
      typed `server-list` rows, selector separation, bounded duplicate-authority
      rejection, legacy TCP migration, graceful cleanup, and no residual owned
      process. Cross-built cells retain existence proof without attempting to
      execute a foreign architecture.
    - [ ] add destructive black-box coverage for abrupt owner death/stale
      recovery and a deliberately different-UID peer; cfg-gated unit evidence
      continues to own unidentified-stale-node, owner/mode, symlink, and
      different-credential invariants until a suitable isolated CI fixture
      exists
  - [~] Windows local endpoint contract:
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
  - [~] registration schema v2 stores the logical instance,
    `ServerScopeId`, typed endpoint, namespace/schema version, PID plus process
    start identity or lease nonce, server epoch, and existing diagnostic facts.
    Discovery reads v2 native-local records and legacy TCP/address records in
    one bounded pass, deduplicates the same authority, preserves reachable,
    unreachable, incompatible, and owner-unknown states, and never treats
    filename presence as proof of a live server
    - [x] v0.1.12 Windows public evidence binds each new registration to a
      process-start identity, lease nonce, and server epoch before publishing
      it. `server-list` keeps the legacy `running` status for a proven live
      authority while adding canonical `live`, `unreachable`, `incompatible`,
      `owner-unknown`, `stale`, and `stale-test-fixture` classifications.
      `--prune` is explicit, never kills a process, rereads the record, and
      returns a typed per-row receipt; PID reuse or any uncertain identity is
      retained rather than deleted. Main retains its exact settings path;
      dev/custom use a scope-derived path unless the caller explicitly sets
      `AGENTERM_SETTINGS_PATH`, and `protocol-info` reports the effective
      settings path.
    - [ ] retain actual old/new binary matrix, Linux/macOS runtime evidence,
      Windows DACL/remote-client/cancellation proof, and abrupt-owner recovery
      before declaring native IPC migration complete across all platforms
  - [~] migration is staged rather than flag-day:
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
    - the v0.1.11 compatibility bridge first performs a 250 ms bounded,
      exact-endpoint probe of a live schema-v1 default-main registration. A
      new client reuses that authority when present; otherwise ordinary main
      resolves to the platform native endpoint. Main retains the existing
      workspace path, while dev/custom use scoped paths. A native server has
      no fabricated TCP listener, so discovery reports
      `legacy_client_compatibility=unsupported_no_legacy_listener` rather
      than pretending an old TCP-only client can attach. That rollback
      limitation remains a partial migration gate; explicit TCP continues to
      provide the compatibility route when old clients are required.
  - [~] public black-box evidence covers `main/dev` isolation and singleton
    races; Unix permission, length, character, symlink, stale, and owner
    failures; Windows DACL, remote-client rejection, first-instance,
    cancellation, and bounded-I/O failures; schema-v1/v2 mixed discovery;
    upgrade/rollback; explicit TCP compatibility; and truthful structured
    snapshot/diagnostic output without leaking raw SID, home path, or
    credentials
    - [x] public native-IPC smoke proves isolated named-pipe and Unix-socket derivation plus native main/dev authority separation
      together with CLI-over-environment selector precedence, typed selector
      conflicts, schema-v1/v2 mixed discovery with v2 deduplication, truthful
      server-list endpoint facts, explicit typed TCP, and legacy `--address`
      compatibility
    - [x] v0.1.12 Windows no-activate smoke additionally proves main/dev/custom
      settings isolation and explicit override, live owner/epoch facts, visible
      stale test-fixture retention, receipt-based cleanup, and PID-reuse
      protection. A verified process-start mismatch may remove only the stale
      registration generation; it never kills the PID-reusing live process.
      Unknown ownership remains retained. The retained Fleet journey proves
      legacy TCP cannot hijack a native main authority
    - [x] the same Windows journey force-kills an owned named-pipe authority,
      observes its exact nonce-qualified registration as `stale`, then proves
      a same-role replacement keeps endpoint and scope while receiving a new
      PID, nonce, and epoch. It verifies the old generation is removed only
      after replacement and ends with no registration or owned-process residue
    - [x] the Windows published-byte compatibility journey verifies SHA-256
      pinned v0.1.10 and v0.1.11 release archives before execution. It proves
      v0.1.10 default TCP with a HEAD client, v0.1.10 client with explicit
      HEAD TCP, safe old-client rejection beside a native-only HEAD authority,
      v0.1.11 native-server interoperability, and state-preserving
      v0.1.11-to-HEAD upgrade then rollback. It never commits historical bytes.
      Unix v0.1.10 is explicitly skipped because that published package lacks
      `agenterm-server`; v0.1.11 remains the native predecessor there.
    - [x] exact-SHA `b4f1622` ordinary CI run `30724960474` refreshed the
      matching-host matrix: Windows named-pipe and Linux/macOS Unix-socket
      native authority journeys passed together with every applicable published
      predecessor upgrade/rollback journey on both macOS architectures and
      Linux x86_64. Cross-built Windows/Linux ARM64 cells separately retained
      compile and manifest-artifact evidence without pretending foreign runtime
      execution. This closes the stale matching-host evidence gap, but does not
      replace the still-listed destructive different-credential and legacy
      client limitations.
