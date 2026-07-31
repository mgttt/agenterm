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
│  ├─ per-worker peak RSS and observed thread-count evidence
│  └─ intentional live-listener cancellation kills and reaps the owned child
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
time, executable size, transport facts, per-worker peak RSS/current thread
samples, block evidence, and graceful plus forced-cleanup state. Linux samples
`VmHWM` and `Threads` from `/proc/self/status`; macOS combines
`getrusage(RUSAGE_SELF)` with `ps thcount`; Windows uses process-memory
counters and a Toolhelp thread snapshot. RSS is an operating-system high-water
mark, while thread count is a point-in-time observation at successful ping.

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
- The original release `self-test`: 54 milliseconds and two graceful children.
  The current contract additionally launches one live listener solely to prove
  forced cancellation and reaping, and reports the measured duration.
- The resolved lock inventory contains 352 packages across supported targets;
  the current Windows normal dependency graph has 229 unique package/version
  lines. This is a significant compile/inventory cost despite the small binary.
- Runtime JSON now reports each worker's OS peak-RSS high-water mark and
  observed thread count plus maxima across the two communicating workers.
  These are small-fixture measurements, not production budgets or load
  qualification; stable integration still requires platform load evidence and
  accepted ceilings.

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
