# MCP and agentic orchestration (`agenterm-mcp.exe`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- Architecture boundary
  - [ ] run MCP server, MCP client connections, Rhai execution, and
    brain/flow scheduling in sidecar/worker processes; `agenterm.exe`
    remains the fleet authority, renderer, and PTY owner
  - [ ] use the public typed control plane plus Observable Fleet epoch,
    sequence, snapshot, journal, and wait contracts; no component reads or
    mutates GUI internals directly
  - [ ] a worker crash, blocked tool, malformed peer, or script budget
    failure cannot stall terminal rendering, corrupt the workspace, or
    terminate tabs
  - [ ] server and client roles have separate capability profiles,
    credentials, connection allowlists, budgets, audit records, and
    lifecycle controls
- Brain/flow model
  - [ ] `brain` owns bounded decision state and chooses declared tools;
    `flow` owns a persisted, inspectable graph of steps, waits, branches,
    retries, cancellation, and compensation
  - [ ] every run, node, tool call, tab, and child agent has a stable ID;
    transitions are observable and recover from a snapshot plus journal
    without assuming process continuity
  - [ ] MCP tools expose typed AgenTerm operations and return verifiable
    post-state; natural-language output is never the sole success signal
  - [ ] destructive fleet actions retain AgenTerm confirmation and policy
    boundaries, including when initiated by an MCP peer or Rhai flow
- Delivery dependency
  - [ ] no event-driven MCP, Rhai handler, or brain/flow runtime ships before
    the Observable Fleet minimum slice passes ordering, gap, restart, wait,
    and GUI-isolation tests
  - [ ] begin with read-only inventory/snapshot resources and bounded waits,
    then add explicit control tools, then durable flows; MCP client
    federation and autonomous scheduling remain later gates
- v0.1.10 first delivery: public read-only surface
  - Protocol scope
    - [ ] pin the first delivery to the stable MCP `2025-11-25` revision;
      protocol upgrades are explicit catalog/schema changes and draft
      stateless discovery or experimental tasks are not advertised
    - [ ] support only initialize/initialized, ping, resources list/read,
      tools list/call for the single wait tool, and cancellation; stdout is
      newline-delimited UTF-8 JSON-RPC only and bounded diagnostics use stderr
    - [ ] the ordinary AgenTerm GUI adds no MCP panel, connection animation,
      approval surface, or startup work in this read-only delivery
  - Executable and discovery
    - [ ] `agenterm-mcp.exe --help`, `--version`, and
      `capabilities --json` work without starting a GUI or model runtime;
      capability output declares protocol/schema versions, transport,
      resources, tools, limits, and unavailable later-stage roles
    - [ ] `agenterm-mcp.exe serve --stdio` is the only first-delivery MCP
      transport; initialization negotiates a supported protocol version and
      publishes stable server identity without opening a network listener
    - [ ] an absent, stale, restarted, or incompatible AgenTerm server
      returns a typed MCP error with address/session diagnostics and never
      causes the sidecar to create a second workspace authority
  - Inventory and snapshots
    - [ ] `resources/list` advertises versioned read-only resources for
      instance inventory, workspace inventory, tab inventory, and one fleet
      snapshot; stable resource URIs do not encode mutable tab indexes or
      titles
    - [ ] `resources/read` returns the same stable IDs and observable state
      as the public AgenTerm control plane, plus schema version, server
      epoch, and snapshot sequence so a client can establish a verifiable
      event baseline
    - [ ] pane text or other content-bearing fields are absent by default;
      any future content snapshot requires an explicit observe capability,
      bounded output, and a resource distinct from metadata inventory
  - Bounded wait
    - [ ] `tools/list` exposes only one read-only `agenterm_wait` tool in
      the first delivery; no create, send, close, script, process, or
      filesystem tool is advertised
    - [ ] `agenterm_wait` accepts a snapshot epoch/sequence, one allowlisted
      predicate, and a bounded `timeout_ms`; success returns the matching
      event and new position, while restart, journal gap, cancellation, and
      deadline expiry remain distinct typed results
    - [ ] client disconnect, cancellation, or timeout releases capacity
      within a bounded grace period and cannot block the GUI IPC loop or
      another MCP client
  - Failure isolation and deferred roles
    - [ ] malformed JSON-RPC, oversized frames, a killed or hung sidecar,
      backend disconnect, and wait exhaustion cannot stall terminal output,
      mutate workspace state, close tabs, or terminate `agenterm.exe`
    - [ ] sidecar restart reconstructs read-only state from a fresh snapshot
      and epoch/sequence; it never claims uninterrupted subscription or
      process continuity
    - [ ] MCP client federation, network transport, subscriptions, control
      tools, Rhai tool execution, brain/flow, durable scheduling, and
      autonomous actions remain outside this first-delivery gate
