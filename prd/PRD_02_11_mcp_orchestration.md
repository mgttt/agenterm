# MCP and agentic orchestration (`agenterm-cli mcp`)

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
  - [ ] MCP server and client connection roles have separate advertised-tool
    profiles, credentials, peer/transport policy, budgets, audit records, and
    lifecycle controls; these MCP boundaries never remove, hide, or deny APIs
    in the standalone unrestricted Script Runtime
- Brain/flow model
  - [ ] `brain` owns bounded decision state and chooses declared tools;
    `flow` owns a persisted, inspectable graph of steps, waits, branches,
    retries, cancellation, and compensation
  - [ ] every run, node, tool call, tab, and child agent has a stable ID;
    transitions are observable and recover from a snapshot plus journal
    without assuming process continuity
  - [ ] MCP tools expose typed AgenTerm operations and return verifiable
    post-state; natural-language output is never the sole success signal
  - [ ] destructive fleet actions retain native AgenTerm data-integrity,
    confirmation, receipt, and lifecycle invariants. Any MCP/Agent
    authorization is applied to that caller before it invokes Script Runtime;
    a Rhai flow itself receives the complete unrestricted runtime API
- Delivery dependency
  - [ ] no event-driven MCP, Rhai handler, or brain/flow runtime ships before
    the Observable Fleet minimum slice passes ordering, gap, restart, wait,
    and GUI-isolation tests
  - [ ] begin with read-only inventory/snapshot resources and bounded waits,
    then add explicit control tools, then durable flows; MCP client
    federation and autonomous scheduling remain later gates
- v0.1.10 first delivery: public read-only surface
  - Protocol scope
    - [x] pin the first delivery to the stable MCP `2025-11-25` revision;
      protocol upgrades are explicit catalog/schema changes and draft
      stateless discovery or experimental tasks are not advertised; the
      offline Rust catalog, exact initialize response, and public lifecycle
      fixture freeze this revision and reject lifecycle drift
    - [x] support only initialize/initialized, ping, resources list/read,
      tools list/call for the single wait tool, and cancellation; stdout is
      newline-delimited UTF-8 JSON-RPC only and bounded diagnostics use stderr
      - [x] `initialize`, `notifications/initialized`, and `ping` now enforce
        the stateful lifecycle; pre-ready non-ping requests, duplicate
        initialization, malformed JSON, invalid requests, unknown methods,
        oversized frames, notification response suppression, and EOF shutdown
        have typed coverage
      - [x] negotiated capabilities now include the four metadata resources
        and exactly one tool; tool work runs outside the input loop, has an
        eight-waiter ceiling, and accepts standard cancellation notifications
    - [x] the ordinary AgenTerm GUI adds no MCP panel, connection animation,
      approval surface, or startup work in this read-only delivery
  - Executable and discovery
    - [x] `agenterm-cli mcp --help`, `agenterm-cli mcp --version`, and
      `capabilities --json` work without starting a GUI or model runtime;
      capability output declares protocol/schema versions, transport,
      resources, tools, limits, and unavailable later-stage roles
      - [x] the dependency-free Rust entry point and offline catalog are
        implemented, unit-tested, included in the artifact manifest and
        cross-platform build lists, and deliberately mark protocol methods,
        resources and `agenterm_wait` with shipped method/tool availability
    - [x] `agenterm-cli mcp serve --stdio` is the only first-delivery MCP
      transport; initialization negotiates a supported protocol version and
      publishes stable server identity without opening a network listener
      - [x] the executable reads one bounded UTF-8 JSON-RPC message per line,
        writes only compact JSON-RPC to stdout, flushes each response, permits
        diagnostics only on stderr, and exits successfully when stdin closes
      - [x] a public executable black-box test proves initialize → initialized
        → ping and machine-only stdout
    - [~] an absent, stale, restarted, or incompatible AgenTerm server
      returns a typed MCP error with address/session diagnostics and never
      causes the sidecar to create a second workspace authority
      - [x] resource reads accept explicit `--address`, then
        `AGENTERM_IPC_ADDRESS`, otherwise use fail-closed zero/one/many live
        instance selection; discovery never prunes records or starts a server
      - [x] absent, ambiguous, unreachable, failed, invalid-snapshot, and
        unknown-resource paths return structured JSON-RPC error data
      - [ ] epoch restart during a read and incompatible protocol/schema
        negotiation still need targeted evidence
  - Inventory and snapshots
    - [x] `resources/list` advertises versioned read-only resources for
      instance inventory, workspace inventory, tab inventory, and one fleet
      snapshot; stable resource URIs do not encode mutable tab indexes or
      titles
    - [x] `resources/read` returns the same stable IDs and observable state
      as the public AgenTerm control plane, plus schema version, server
      epoch, and snapshot sequence so a client can establish a verifiable
      event baseline
      - [x] the MCP adapter invokes the existing typed IPC request path
        directly rather than launching or parsing `agenterm-cli`; all four
        resources return versioned JSON text content
      - [x] a live read against the existing 48815 server returned the same
        event epoch/sequence and five stable tabs as its public snapshot
      - [x] an isolated public-executable fixture sends the same typed
        `ui-snapshot` through `agenterm-cli.exe` and `agenterm-cli mcp`, then
        compares server identity, epoch/sequence, workspace/window facts and
        every published tab field while rejecting title, CWD, proxy and
        Composer sentinels
    - [x] pane text or other content-bearing fields are absent by default;
      any future content snapshot requires an explicit observe capability,
      bounded output, and a resource distinct from metadata inventory
      - [x] projection tests and a live encoded-result inspection prove
        terminal title, working context/cwd, proxy, Composer, and pane content
        keys do not enter MCP resources
  - Bounded wait
    - [x] `tools/list` exposes only one read-only `agenterm_wait` tool in
      the first delivery; no create, send, close, script, process, or
      filesystem tool is advertised
    - [x] `agenterm_wait` accepts a snapshot epoch/sequence, one allowlisted
      predicate, and a bounded `timeout_ms`; success returns the matching
      event and new position, while restart, journal gap, cancellation, and
      deadline expiry remain distinct typed results
      - [x] the implementation validates epoch, sequence, stable optional tab
        ID, event-kind allowlist, timeout, unknown fields, duplicate request
        IDs, and concurrency before allocating a waiter
      - [x] live read-only evidence against server 48815 proves a concurrent
        ping completes before a waiting call, and timeout/cancellation return
        distinct structured outcomes without stderr diagnostics
      - [x] an isolated public-executable fixture proves a matched event
        returns its stable tab ID, new sequence and independently fetched
        post-state while omitting the event payload
      - [x] restart, journal-gap and future-sequence remain distinct typed
        outcomes through the public executable; a filtered tab closing is a
        distinct `target_closed` outcome and its private event payload is
        omitted
    - [x] client disconnect, cancellation, or timeout releases capacity
      within a bounded grace period and cannot block the GUI IPC loop or
      another MCP client
      - [x] cancellation during real bounded IPC polling has deterministic
        unit coverage; EOF cancels and joins all active waiter workers
      - [x] the public executable exits within the 1.5-second fixture budget
        when stdin closes during an accepted backend wait
      - [x] an eight-waiter public burst fails the ninth request closed,
        cancellation frees capacity for a replacement, and force-killing the
        isolated MCP client closes its accepted backend connection
  - Offline catalog invariants
    - [x] catalog schema v1 publishes one `stdio` transport, the stable
      `2025-11-25` protocol revision, four metadata-only resources, exactly one
      read-only `agenterm_wait` tool, hard frame/concurrency/wait/error limits,
      and explicit deferred roles
      - [x] instance health discovery publishes a 250 ms per-probe budget,
        1.5-second total deadline, and 32-probe concurrency ceiling; a public
        256-record hanging fixture completes within the deadline envelope and
        reports every unverified candidate fail-closed as unreachable
    - [x] unit tests reject duplicate method/resource/tool identities and prove
      the current implementation slice reports exact shipped handlers
  - Failure isolation and deferred roles
    - [x] malformed JSON-RPC, oversized frames, a killed or hung sidecar,
      backend disconnect, and wait exhaustion cannot stall terminal output,
      mutate workspace state, close tabs, or terminate `agenterm.exe`
      - [x] the public stdio executable recovers from malformed and four-MiB
        oversized frames in the same session
      - [x] a Windows public-executable fixture holds a real bounded wait,
        receives a concurrent ping, force-kills the sidecar, then proves the
        isolated GUI and server processes remain live, PTY output remains
        capturable, and a subsequent typed tab mutation succeeds
    - [ ] sidecar restart reconstructs read-only state from a fresh snapshot
      and epoch/sequence; it never claims uninterrupted subscription or
      process continuity
    - [ ] MCP client federation, network transport, subscriptions, control
      tools, Rhai tool execution, brain/flow, durable scheduling, and
      autonomous actions remain outside this first-delivery gate
