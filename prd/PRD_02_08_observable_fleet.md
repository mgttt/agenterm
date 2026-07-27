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
- v0.1.7 causal completion and evidence (P0)
  - [~] event envelopes can carry request and operation correlation, Composer
    submission completion is wired to it, and mutation receipts carry
    before/after event positions; other mutation events are not yet
    comprehensively correlated
  - [~] control receipts define a stable resolved target, server epoch,
    minimum event position, deadline, and typed wait condition for Composer
    completion; existing deterministic wait commands do not yet all freeze
    selectors or bind to server identity, epoch, and a minimum sequence, so
    pre-existing state, target replacement, and restart false-success coverage
    remains incomplete
  - [~] terminal observation now distinguishes input/output counters,
    submission pending and Enter completion, process exit/error, reader EOF,
    parser drain, and finalization; public CLI tests prove a stable finalized
    boundary and typed rejection of writes after finalization, while complete
    receipt coverage for text written, Enter written/failed, terminal output
    observed, process exit, and terminal finalization remains incomplete
  - [x] producer notifications share one atomic wake signal: PTY, process,
    startup, clipboard, and bounded IPC queues coalesce outstanding Win32
    messages, the GUI owner clears before draining and rearms an exhausted IPC
    budget without losing a concurrent wake; public stress proves PTY and 32
    IPC clients progress together and an expired mutation remains a typed no-op
  - [ ] model sequence, render generation, and last-painted event sequence
    make snapshot, bounded capture, cell dump, and PNG evidence causally
    comparable
  - [ ] capture/screenshot machine metadata includes server identity, epoch,
    sampled/rendered sequence, stable tab ID, output position, viewport, and
    explicit truncation without exposing secrets
  - [ ] workspace save uses crash-safe replacement and exposes revision,
    hash, path, commit position, and failure without destroying the previous
    readable workspace; shutdown has a public lifecycle completion wait
  - [ ] timeout results include the unsatisfied predicate, resolved target,
    start/last position, elapsed/deadline, last bounded observation, and a
    typed recovery hint instead of collapsing server/target errors into a
    generic timeout
- Explicitly deferred beyond the minimum slice
  - [ ] durable replay across process restarts, remote/network transport,
    arbitrary user predicates, unbounded terminal byte logging, delivery
    acknowledgements, and exactly-once side effects
  - [ ] Rhai event handlers, MCP subscriptions, brain/flow scheduling,
    status-provider events, and multi-agent orchestration remain consumers
    of this core, not shortcuts around its acceptance gate
