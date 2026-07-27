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
  - [ ] every public control/destructive request has a versioned
    `request_id`, stable `operation_id`, resolved server/tab identity,
    before/after event position, truthful completion phase, and typed
    result/error available through a global machine-readable mode
  - [ ] the server keeps a bounded request deduplication/replay window;
    retrying the same ID and payload cannot repeat a side effect, reusing an
    ID with different input is rejected, and a transport timeout reports
    `outcome_unknown` unless non-execution is proven
  - [ ] expired or cancelled requests cannot execute later after a blocked
    GUI thread recovers
  - [ ] success means committed, accepted asynchronously, or explicit no-op;
    dead/unavailable targets, failed PTY writes, and unresolved selectors
    never return ordinary success
  - [ ] asynchronous receipts publish the exact stable wait predicate and
    baseline cursor needed to observe completion or failure
  - [ ] operation catalog contract tests cover dispatch, result/error schema,
    completion semantics, emitted events, aliases, and stable target identity
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
  - [ ] one canonical structured success/error envelope across commands;
    human text is rendered from it rather than forming a second contract
  - [ ] named-pipe transport and stable event subscription
