# Observable Fleet event core (v0.1.5 minimum slice)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- Contract
  - [x] assign a new server `epoch` on every workspace-authority start so a
    consumer can distinguish restart from an in-process event gap
  - [x] assign one strictly increasing `sequence` within an epoch after
    each committed observable state transition in the minimum event schema
  - [x] expose a bounded in-memory event journal whose envelopes contain
    schema version, epoch, sequence, event kind, stable tab ID when
    applicable, and a minimal typed payload
  - [x] the first schema covers tab, composer, terminal, and workspace
    transitions and v0.1.6 adds host window, focus, layout, CWD, and Proxy
    transitions without opening event kinds to unchecked strings
  - [x] a closed event catalog publishes state path, payload, scope, and since metadata
  - [x] snapshot responses include the current epoch and sequence so clients
    can atomically establish a baseline before following the journal
- Read and wait slice
  - [x] add one public bounded read operation for events after
    `(epoch, sequence)`, with explicit gap/restart errors rather than silent
    loss or replay ambiguity
  - [x] add one deterministic wait operation over the same journal for a
    small allowlisted predicate set and deadline; cancellation or timeout
    cannot block the GUI thread
  - [x] bounded event reads and waits follow a snapshot epoch and sequence
  - [x] journal mutation happens only after the corresponding state change
    commits; wired event kinds are snapshot-verifiable
  - [x] black-box tests prove read/wait ordering, timeout,
    snapshot-to-follow handoff, restart, gap, concurrent readers, catalog
    completeness, and server/tab-scoped post-state agreement
- Explicitly deferred beyond the minimum slice
  - [ ] durable replay across process restarts, remote/network transport,
    arbitrary user predicates, unbounded terminal byte logging, delivery
    acknowledgements, and exactly-once side effects
  - [ ] Rhai event handlers, MCP subscriptions, brain/flow scheduling,
    status-provider events, and multi-agent orchestration remain consumers
    of this core, not shortcuts around its acceptance gate
