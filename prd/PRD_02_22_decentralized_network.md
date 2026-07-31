# Decentralized network (`agenterm-net`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module owns AgenTerm's independently matured decentralized-network
foundation: process identity, peer transport, content addressing, block
storage, resource controls, diagnostics, and the boundary through which other
products may later consume stable services. It does not make the terminal GUI
or Fleet server a network node.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Product outcome

- [ ] AgenTerm gains a portable, observable libp2p/IPFS foundation that can
  prove identity, peer connectivity, content integrity, bounded storage, and
  cleanup independently before any stable product depends on it.
- [ ] `agenterm-net` remains a separate optional process with its own
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

## Process and integration boundary

- [ ] the first implementation is an independent `agenterm-net` lab process,
  not a library linked into `agenterm.exe`, `agenterm-server`, or
  `agenterm-cc`.
- [ ] process ownership includes peer keys, listeners, connections, streams,
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

- [ ] N1 uses invocation-owned or test-scoped identities and reports peer IDs
  without claiming durable identity continuity.
- [ ] N2 requires an explicit durable-versus-ephemeral identity model, key
  storage, backup, rotation, loss, migration, and multi-device semantics before
  persistent peer identity is advertised.
- [ ] peer addresses are normalized multiaddrs with typed parse and
  compatibility errors. Transport availability is discoverable by platform
  and build rather than inferred from a successful compile.
- [ ] local two-process fixtures prove independent identity, handshake, ping
  receipt, bounded timeout/cancellation, peer exit, and orphan-free shutdown.
  Loopback in deterministic tests is a fixture, not an endpoint authorization
  policy.
- [ ] DHT, mDNS, rendezvous, pubsub, relay, NAT traversal, remote Fleet attach,
  and federation each require a separately advertised capability and resource
  budget; N1 does not claim them.

## IPFS-compatible content foundation

- [ ] CID v1 creation, parsing, codec/multihash identity, byte verification,
  deterministic equality, invalid-input diagnosis, and maximum block size form
  the first content-addressing contract.
- [ ] N1 block `put` and `get` use an invocation-owned temporary store; a
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

- [ ] dependency selection records exact crate features, transitive licences,
  build time, binary-size delta, dynamic requirements, supported targets, and
  maintenance status before entering release artifacts.
- [ ] typed budgets cover listeners, peers, connections, streams, in-flight
  requests, task count, message/block bytes, memory, disk, cache, pins,
  bandwidth, deadlines, output, and shutdown grace.
- [ ] the threat model covers malicious peers, malformed frames and blocks,
  amplification, resource exhaustion, address spoofing, content deception,
  downgrade, key loss, cache corruption, and restart during mutation.
- [ ] partial writes and interrupted promotion preserve the last known-good
  store; corrupt or mismatched blocks are quarantined or rejected with typed
  evidence and never served as verified content.
- [ ] kill, hang, peer disappearance, invalid CID, oversized block, full disk,
  corrupt cache, incompatible protocol, restart, upgrade, and rollback leave no
  orphan process or listener and cannot affect GUI/server/PTY continuity.

## v0.1.11 independent proof gate

- [ ] N0 produces an auditable dependency/licence/feature/size/build-time and
  six-target feasibility decision without casually raising an existing
  sidecar budget.
- [ ] two independent local processes exchange distinct peer identities,
  complete a libp2p handshake and bounded ping, emit machine-readable receipts,
  and exit cleanly on success, deadline, cancellation, or peer loss.
- [ ] identical bytes produce the same CID v1; changed bytes, malformed CIDs,
  unsupported codecs, and oversized blocks fail with stable typed errors.
- [ ] temporary block put/get returns byte-identical content whose hash matches
  its CID; interruption, corruption, and cleanup paths are deterministic.
- [ ] force-killing or hanging the experiment leaves the existing
  `agenterm-server`, GUI, PTYs, workspace, and public CLI usable.
- [ ] tests use isolated identities, ports, stores, and process registries,
  avoid fixed sleeps, and expose bounded structured evidence suitable for CI.
- [ ] until the N2 gate passes, the prototype is not advertised as a stable
  release asset, server capability, public IPFS node, or package-market
  backend.

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
