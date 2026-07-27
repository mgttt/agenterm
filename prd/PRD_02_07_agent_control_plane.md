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
    conflicts, deadline rejection, Composer completion, dead writes, and
    stable target identity; full operation-catalog dispatch, alias, result,
    error, and emitted-event contract coverage remains planned
  - [x] one public receipt replay slice proves same-ID same-payload replay and different-payload conflict without repeating the tab-note mutation
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
  - [ ] named-pipe transport and stable event subscription
