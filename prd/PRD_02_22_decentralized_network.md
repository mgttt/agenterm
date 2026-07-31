# Decentralized network (`agenterm-net`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module owns AgenTerm's independently matured decentralized-network
foundation: process identity, peer transport, content addressing, block
storage, resource controls, diagnostics, and the boundary through which other
products may later consume stable services. It does not make the terminal GUI
or Fleet server a network node.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Product outcome

- [~] AgenTerm gains a portable, observable libp2p/IPFS foundation that can
  prove identity, peer connectivity, content integrity, bounded storage, and
  cleanup independently before any stable product depends on it. v0.1.12
  starts the N2-M1 controlled full-node vertical slice; it remains
  experimental until its cross-platform evidence gates pass.
- [~] `agenterm-net` remains a separate optional process with its own
  dependency, binary-size, memory, task, connection, disk, packaging, and
  lifecycle evidence; its failure cannot stall the terminal, destroy a tab, or
  corrupt a workspace.
- [ ] later Script, InfoHub, Control Center, and server integration consume one
  versioned typed service contract. They do not link libp2p/IPFS dependencies
  into the terminal or PTY hot path.

## Capability tree and maturity

```text
Decentralized network
├─ N0 selection and contract
│  ├─ dependency / feature / licence inventory
│  ├─ platform, package and binary-size feasibility
│  ├─ protocol, capability and typed-error schemas
│  └─ resource and cleanup budgets
├─ N1 independent local proof
│  ├─ process-scoped peer identity
│  ├─ two-process connect / handshake / ping
│  ├─ CID v1 create / parse / verify
│  └─ bounded temporary block put / get
├─ N2 stable sidecar
│  ├─ durable identity, backup and rotation
│  ├─ discovery / streams / pubsub / relay capabilities
│  ├─ persistent block store / cache / pin / GC
│  ├─ Kubo adapter where interoperability adds value
│  └─ restart, upgrade, downgrade and corruption recovery
├─ N3 product consumers
│  ├─ unrestricted Script APIs / provider calls
│  ├─ InfoHub sources and content references
│  └─ Control Center diagnostics and projections
└─ N4 server service integration
   ├─ on-demand typed facade
   ├─ bounded summaries, routes and events
   └─ no linked networking engine in the Fleet authority
```

Maturity states are monotonic evidence gates, not release marketing. An N1 lab
binary or test fixture remains experimental until the N2 public process,
protocol, recovery, and packaging contracts pass.

## v0.1.12 N2-M1 controlled full-node slice

The accepted v0.1.12 direction is an **independent, explicit, private-mesh
vertical slice**, not an implicit public IPFS service:

```text
N2-M1
├─ node lifecycle: explicit start / status / stop / repair; no install or GUI autostart
├─ durable state: opt-in identity and bounded verified block store / pin / GC
├─ mesh: separately advertised Kademlia DHT, GossipSub and relay capabilities
├─ remote Fleet attach: paired peer-to-peer, bounded read-only snapshot/event projection
└─ evidence: private two-node fixtures; budget/fault/crash/isolation evidence per platform
```

- [~] `research/agenterm-net` now proves explicit deadline-bounded
  `node start/status/stop`, durable-or-ephemeral Ed25519 identity and
  node-resource snapshots. Durable key backup, loss, rotation and migration
  contracts remain open before this identity can be called stable.
- [~] its experimental persistent block store verifies CIDs on every read and
  has a 4 MiB per-block, 32 MiB/1024-block store budget with pin/unpin/GC,
  stored-versus-verified accounting and corruption rejection. Cross-platform
  load, interrupted-mutation and repair evidence remain open.
- [ ] Kademlia DHT, GossipSub and relay have individual capability IDs,
  listener/peer/message/bandwidth/task budgets, bounded I/O, receipts and
  Unsupported results. Public bootstrap, NAT traversal and relay serving are
  off unless the user explicitly configures them.
- [ ] Remote Fleet attach is pair-created and read-only: an expiring invite
  binds expected peer identity, nonce and scope; the projection contains only
  bounded snapshot/event-digest data. It cannot send terminal input, execute a
  command, or become an authority over the remote `agenterm-server`.
- [ ] pairing, transport encryption, replay/wrong-peer/expired-invite failure,
  reconnect and sidecar crash must be observable with typed results. Future
  remote control belongs to the Agent/harness approval and credential layer.
- [ ] N2-M1 tests use a deterministic private mesh and isolated stores; they
  must not require public bootstrap reachability or leave listeners/processes.

## Process and integration boundary

- [x] the first implementation is an independent `agenterm-net` lab process,
  not a library linked into `agenterm.exe`, `agenterm-server`, or
  `agenterm-cc`.
- [x] process ownership includes peer keys, listeners, connections, streams,
  block-store handles, pins, caches, network tasks, receipts, and cleanup. A
  consumer owns only its request and projected result.
- [ ] stable consumers use a versioned, bounded typed protocol with capability
  discovery, request IDs, deadlines, cancellation, receipts, diagnostics, and
  explicit Unsupported results. No consumer parses human logs or assumes a
  particular Rust crate.
- [ ] server integration, if later approved, is an on-demand service facade:
  the stable server may publish bounded availability, summaries, routes, and
  events while the network process retains all heavy runtime and node
  lifecycle.
- [ ] installation and update remain explicit optional-component transactions;
  installing AgenTerm or opening a GUI never silently enables a persistent
  listener, advertises a peer, or starts a full node.
- [ ] network safety and resource limits protect the native product and host,
  but do not become restrictions on the standalone unrestricted Script
  Runtime. Any future Agent permission or credential policy belongs to its
  caller/harness.

## Identity, transport, and discovery

- [x] N1 uses invocation-owned or test-scoped identities and reports peer IDs
  without claiming durable identity continuity.
- [ ] N2 requires an explicit durable-versus-ephemeral identity model, key
  storage, backup, rotation, loss, migration, and multi-device semantics before
  persistent peer identity is advertised.
- [ ] peer addresses are normalized multiaddrs with typed parse and
  compatibility errors. Transport availability is discoverable by platform
  and build rather than inferred from a successful compile.
- [x] local two-process fixtures prove independent identity, handshake, ping
  receipt, bounded timeout/cancellation, peer exit, and orphan-free shutdown.
  Loopback in deterministic tests is a fixture, not an endpoint authorization
  policy.
- [ ] DHT, mDNS, rendezvous, pubsub, relay, NAT traversal, remote Fleet attach,
  and federation each require a separately advertised capability and resource
  budget; N1 does not claim them.

## IPFS-compatible content foundation

- [x] CID v1 creation, parsing, codec/multihash identity, byte verification,
  deterministic equality, invalid-input diagnosis, and maximum block size form
  the first content-addressing contract.
- [x] N1 block `put` and `get` use an invocation-owned temporary store; a
  successful receipt binds the returned bytes to the CID, and interruption or
  corruption cannot report success.
- [ ] persistent block stores, DAG traversal, pinning, garbage collection,
  gateways, clusters, replication, provider discovery, and large caches remain
  behind N2 or later gates with independent disk and recovery evidence.
- [ ] Kubo is an optional interoperability adapter, not the definition of the
  internal product model. Before shipping, the adapter must pin compatible API
  versions, authenticate its local endpoint where applicable, report daemon
  identity and capability, bound requests and responses, and remain
  replaceable by the native sidecar contract.
- [ ] content provenance and trust remain distinct from content addressing: a
  valid CID proves byte identity, not publisher identity, safety, truth, or
  permission to execute.

## Resource, security, and recovery gates

- [~] dependency selection records exact crate features, transitive licences,
  build time, binary-size delta, dynamic requirements, supported targets, and
  maintenance status before entering release artifacts.
- [~] typed budgets cover listeners, peers, connections, streams, in-flight
  requests, task count, message/block bytes, memory, disk, cache, pins,
  bandwidth, deadlines, output, and shutdown grace.
- [ ] the threat model covers malicious peers, malformed frames and blocks,
  amplification, resource exhaustion, address spoofing, content deception,
  downgrade, key loss, cache corruption, and restart during mutation.
- [ ] partial writes and interrupted promotion preserve the last known-good
  store; corrupt or mismatched blocks are quarantined or rejected with typed
  evidence and never served as verified content.
- [~] kill, hang, peer disappearance, invalid CID, oversized block, full disk,
  corrupt cache, incompatible protocol, restart, upgrade, and rollback leave no
  orphan process or listener and cannot affect GUI/server/PTY continuity.

## v0.1.11 independent proof gate

- [~] N0 produces an auditable dependency/licence/feature/size/build-time and
  six-target feasibility decision without casually raising an existing
  sidecar budget.
- [x] two independent local processes exchange distinct peer identities,
  complete a libp2p handshake and bounded ping, emit machine-readable receipts,
  and exit cleanly on success, deadline, cancellation, or peer loss.
- [~] identical bytes produce the same CID v1; changed bytes, malformed CIDs,
  unsupported codecs, and oversized blocks fail with stable typed errors.
- [x] temporary block put/get returns byte-identical content whose hash matches
  its CID; interruption, corruption, and cleanup paths are deterministic.
- [~] force-killing or hanging the experiment leaves the existing
  `agenterm-server`, GUI, PTYs, workspace, and public CLI usable.
- [x] tests use isolated identities, ports, stores, and process registries,
  avoid fixed sleeps, and expose bounded structured evidence suitable for CI.
- [x] until the N2 gate passes, the prototype is not advertised as a stable
  release asset, server capability, public IPFS node, or package-market
  backend.

Current N1 evidence lives in the isolated
`research/agenterm-net/` workspace and the root
`agenterm-net-research` task. It proves two real child processes over
loopback TCP + Noise + Yamux + Ping, ephemeral Ed25519 identities, CID v1
raw/SHA-256 content checks, a 4 MiB temporary block-store bound, corruption
rejection, typed peer-loss, bounded output/deadlines, cancellation, and
orphan-free cleanup. The measured Windows release executable is 1,406,976
bytes; cold/hot release builds were 88.271/25.766 seconds. Peak RSS/thread
evidence is now reported per communicating worker: Windows uses process-memory
counters plus Toolhelp, Linux uses `/proc/self/status`, and macOS uses
`getrusage` plus `ps thcount`. RSS is the OS high-water mark; threads are
observed at successful ping. The self-test also starts a third live listener
and intentionally cancels and reaps it, so forced cleanup is receipt-owned
rather than inferred from a guard being armed. Six-target runtime evidence,
accepted load ceilings, complete malformed-CID coverage, and a live
stable-server/PTY isolation journey remain open, so this evidence does not
promote the research executable into the stable artifact manifest.

## Gates before stable service integration

- [ ] durable identity and key lifecycle are specified and migration-tested
- [ ] capability/protocol/receipt/event/diagnostic schemas are versioned and
  backward/forward compatibility is explicit
- [ ] connection, storage, cache, pin, bandwidth, task, and shutdown budgets
  pass load and fault evidence on Windows, macOS, and Linux
- [ ] restart, reconnect, corruption recovery, upgrade, downgrade, and sidecar
  crash preserve truthful state and isolation
- [ ] SBOM, licence, signature, hash, provenance, package, startup, and
  independent binary-size gates pass on every distributed architecture
- [ ] users explicitly enable persistent networking and can observe, stop,
  repair, reset, and remove it without affecting local terminal use
- [ ] Script and InfoHub adapters pass their own owning black boxes before the
  server exposes even a bounded service facade

## Explicit v0.1.11 non-goals

- [ ] no production DHT, pubsub, relay, NAT traversal, public gateway, remote
  Fleet attach, or cross-machine federation
- [ ] no complete IPFS DAG, pinning, GC, cluster, replication, or persistent
  large-scale cache
- [ ] no always-on full node and no automatic listener caused by installing or
  opening AgenTerm
- [ ] no libp2p/IPFS dependency linked into the terminal GUI, PTY server, or
  Control Center
- [ ] no claim that CID validation establishes provenance, trust, safety, or
  executable permission
- [ ] no Agent permission model and no reduced Rhai network API
