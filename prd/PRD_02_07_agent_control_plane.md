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
  - [ ] incremental output sequence and event stream
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
  - [ ] direct deterministic wait predicates for modal kind and target
  - [ ] broadcast input and synchronized panes
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
  - [ ] named-pipe transport and stable event subscription
