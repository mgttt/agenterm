# agenterm-net research spike

`agenterm-net` is an isolated, disposable v0.1.11 research package. It is not
part of the AgenTerm workspace, server, GUI, package, or stable release. The
experiment answers one narrow question: can AgenTerm obtain a typed,
resource-bounded two-process libp2p handshake plus minimal content-addressed
storage without putting networking dependencies in the terminal hot path?

## Capability tree

```text
agenterm-net (research-only; designed 2026-07-31)
├─ identity — ephemeral Ed25519 PeerId; no key persistence
├─ transport — explicit IPv4 loopback TCP only
│  └─ Noise authentication → Yamux multiplexing → Ping evidence
├─ process isolation
│  ├─ self-test starts listener and connector as separate OS processes
│  ├─ stdout JSON is the ready/result protocol
│  └─ deadline failure kills and reaps owned children
└─ content addressing
   ├─ CID v1 / raw codec / SHA-256 multihash
   └─ temporary bounded put/get with read-time corruption rejection
```

It deliberately has no DHT, relay, pubsub, public listener, NAT traversal,
gateway, persistent identity, persistent store, server integration, or release
claim.

## Commands

```text
cargo run -- capabilities --json
cargo run -- peer-id
cargo run -- self-test --json
```

Every public result has a schema, request ID, state, deadline and SHA-256
receipt. `self-test` also reports child PIDs, endpoint, peer identities, elapsed
time, executable size, transport facts, block evidence, and cleanup state.

## Evidence

Run from this directory:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- self-test --json
cargo build --release
```

Measured on Windows x86_64 with Rust 1.97.0 on 2026-07-31:

- Hot format + Clippy + six tests: 13.8 seconds. The preceding first test
  compile took 40.6 seconds after dependencies had been checked.
- Cold release-profile build of this package: 88.271 seconds.
- Stripped release executable: 1,406,976 bytes.
- Release `self-test`: 54 milliseconds, two owned child processes, both reaped.
- The resolved lock inventory contains 352 packages across supported targets;
  the current Windows normal dependency graph has 229 unique package/version
  lines. This is a significant compile/inventory cost despite the small binary.
- Runtime memory and thread maxima are not instrumented in this spike. JSON
  reports process count, elapsed wall time, executable bytes and explicit data
  bounds; a later maturity gate must add cross-platform peak RSS/thread
  measurements before considering server integration.

## Dependency and licence note

The deliberately small protocol feature set is `libp2p` TCP + Noise + Yamux +
Ping, Tokio, `cid`, `multihash-codetable`, Serde/JSON, and SHA-256. Direct
dependencies are available under permissive MIT and/or Apache-2.0 licences.
The checked `Cargo.lock` is the exact transitive inventory. Metadata currently
includes MIT, Apache-2.0, BSD, ISC, Unicode, Zlib, MPL-2.0 and compound licence
expressions across all resolved target packages. Distribution work must run the
repository's normal per-target licence/SBOM audit and review ambiguous compound
expressions before this package can be considered for any product manifest.
This research package is not distributed.
