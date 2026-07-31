# agenterm-net research spike

`agenterm-net` is an isolated, disposable v0.1.12 research package. It is not
part of the AgenTerm workspace, server, GUI, package, or stable release. The
N2-M1 experiment grows the original two-process libp2p handshake into an
explicit, still-experimental sidecar lifecycle and persistent content-addressed
store without putting networking dependencies in the terminal hot path.

## Capability tree

```text
agenterm-net (experimental N2-M1 foundation; 2026-07-31)
├─ identity — explicit ephemeral or durable Ed25519 PeerId
│  └─ durable protobuf key is created once in the caller-selected state dir
├─ node lifecycle — explicit start/status/stop
│  ├─ loopback-only nonce-bound control endpoint
│  ├─ bounded readiness/control waits; no fixed sleep
│  └─ crash evidence is preserved instead of silently starting a second owner
├─ transport — explicit IPv4 loopback TCP only
│  └─ Noise authentication → Yamux multiplexing → Ping evidence
├─ process isolation
│  ├─ self-test starts listener and connector as separate OS processes
│  ├─ stdout JSON is the ready/result protocol
│  ├─ per-worker peak RSS and observed thread-count evidence
│  └─ intentional live-listener cancellation kills and reaps the owned child
└─ content addressing
   ├─ CID v1 / raw codec / SHA-256 multihash
   └─ persistent bounded put/get, pin/unpin, GC, snapshot, corruption rejection
```

The persistent store limits a block to 4 MiB, the store to 32 MiB and 1,024
blocks. Reads always verify content against the requested CID; snapshots report
stored bytes separately from verified bytes so corrupt data still consumes the
budget. Pins survive node restarts and GC removes only unpinned blocks.

It deliberately has no proven DHT, relay, pubsub, public listener, public
bootstrap, NAT traversal, gateway, Remote Fleet attach, terminal input, command
execution, PTY control, server integration, or release claim. The node reports
those network/authority defaults explicitly. DHT/pubsub/relay remain typed
`unavailable`, not implied by the presence of libp2p.

## Commands

```text
cargo run -- capabilities --json
cargo run -- peer-id
cargo run -- self-test --json
cargo run -- node start --state-dir ./.net-state --identity durable --json
cargo run -- node status --state-dir ./.net-state --json
cargo run -- node stop --state-dir ./.net-state --json
cargo run -- store put --store ./.net-store --input ./payload.bin --pin --json
cargo run -- store get --store ./.net-store --cid <cid> --output ./copy.bin --json
cargo run -- store pin --store ./.net-store --cid <cid> --json
cargo run -- store unpin --store ./.net-store --cid <cid> --json
cargo run -- store gc --store ./.net-store --json
cargo run -- store status --store ./.net-store --json
```

Every public result has a schema, request ID, state, deadline and typed SHA-256
receipt (`agenterm-net/receipt/v1`). `self-test` also reports child PIDs,
endpoint, peer identities, elapsed
time, executable size, transport facts, per-worker peak RSS/current thread
samples, block evidence, and graceful plus forced-cleanup state. Linux samples
`VmHWM` and `Threads` from `/proc/self/status`; macOS combines
`getrusage(RUSAGE_SELF)` with `ps thcount`; Windows uses process-memory
counters and a Toolhelp thread snapshot. RSS is an operating-system high-water
mark, while thread count is a point-in-time observation at successful ping.

Node readiness uses a one-shot loopback socket owned by the starting process;
status and stop use the descriptor's loopback address and nonce. The nonce is a
request-binding robustness mechanism, not a user authorization boundary. A
descriptor whose owner cannot be reached is retained as crash evidence and a
second node is refused. Status receipts include the persistent-store snapshot
and the platform process RSS/thread sample. Automated stale-owner recovery,
reconnect evidence and cross-platform load qualification remain N2 gaps.

## Evidence

Run from this directory:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- self-test --json
cargo build --release
```

The current unit and public CLI suites cover durable identity restart,
ephemeral identity rotation, explicit lifecycle, private-by-default facts,
persistent block round-trip, pin/unpin/GC, read-time corruption rejection,
deadline-bounded peer loss, and owned-child cancellation without fixed sleeps.

Original N1 measurements on Windows x86_64 with Rust 1.97.0 on 2026-07-31:

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
