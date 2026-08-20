# TinyArcade iOS bridge v1

The iOS app links a static TinyArcade library; a cartridge remains data. No
downloaded code is turned into native executable memory. The Rust interpreter,
host ABI and lifecycle owner are compiled into the app binary.

## Delivered surfaces

- `crates/agenterm-tinyvm/include/tinyarcade.h`: versioned C ABI.
- `crates/agenterm-tinyvm/include/module.modulemap`: Swift module `TinyArcade`.
- `crates/agenterm-tinyvm/bindings/swift/TinyArcadeRuntime.swift`:
  `@MainActor` Swift owners, bounded catalog decoding and indexed-2D/audio
  native presentation, plus deterministic replay recording/verification.
- `crates/agenterm-tinyvm/build-xcframework.sh`: device/simulator archive and
  XCFramework builder.
- `crates/agenterm-tinyvm/build-swift-package.sh`: self-contained local Swift
  package builder with one `TinyArcadeRuntime` library product.
- `crates/agenterm-tinyvm/smoke-ios-bridge.sh`: C header, XCFramework and Swift
  simulator-link acceptance gate.

Build and verify from the repository root:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
CARGO="$HOME/.cargo/bin/cargo" \
  crates/agenterm-tinyvm/smoke-ios-bridge.sh
```

The builder uses the dedicated `tinyvm-ios-release` Cargo profile. Its panic
strategy is `unwind`, because every exported operation is fenced with
`catch_unwind` and maps a panic to `TINYARCADE_PANIC`. Building the bridge under
the workspace's abort profile would silently remove that guarantee.

The generated Swift package is the stable app dependency boundary. It contains
both XCFramework slices and the public Swift source, so an app does not compile
Rust or copy wrapper code. The same directory can later be zipped as a
versioned binary release artifact without changing the app-facing product.

The package's `TinyArcadeCatalogV1` decoder implements
`docs/tinyarcade-catalog-transport-v1.md`. It bounds discovery JSON and resolves
same-origin cartridge filenames plus selection-only deep links, but never
performs a request or grants execution trust. This keeps transport policy in
the app while giving sites/converters one interoperable lobby schema.

`TinyArcadeHTTPSClientV1` is the bounded app-owned transport between that
schema and the verified cache. It streams URLSession delegate chunks rather
than buffering an unchecked `data(from:)` result, rejects redirects and
non-200/MIME mismatches, checks declared and received lengths, propagates Task
cancellation, and bounds both active and queued requests. It still exposes no
network import to WASM and never opens or activates content implicitly.

`TinyArcadeReviewedLibraryV1` composes those primitives into one main-actor
installation transaction. It fetches the selected bytes, opens a reviewed
runtime under the live signature/revocation store and native registry as a
preflight, and only then activates the verified cache generation. Thus a
signature-valid cartridge that this app cannot instantiate cannot replace the
last playable active object. Only one install may cross the network `await` at
a time; a second receives `operationInProgress`. Cancellation before activation
leaves selection unchanged, while `openActive` re-verifies current trust before
opening the exact cached bytes.

`TinyArcadeSnapshotStoreV1` owns scene/background persistence independently of
cartridge distribution. It stores one bounded binary envelope per canonical
game id with the host-owned game clock, CRC-32 and the runtime's already
ABI/state-schema-bound snapshot. Writes use atomic replacement, the directory
is excluded from backup and files receive complete-until-first-authentication
protection. Reads reject symlinks and oversized/non-regular files. A corrupt or
incompatible snapshot is removed; the failed candidate runtime is closed and a
second clean runtime is returned with `discardedInvalid`, so save damage cannot
turn into a cartridge launch failure.

## Ownership

ABI v1.5 exposes three non-interchangeable origins: bundled
`tinyarcade_v1_open`, signed `tinyarcade_v1_open_reviewed`, and local
`tinyarcade_v1_open_private`. Every instance retains its immutable origin for
UI/audit queries. Reviewed opening consumes a single-thread-owned trust store;
private opening has no native capability registry and cannot acquire official
provenance.

Bundled and reviewed origins may instead use their corresponding
`*_with_native_modules` open. The host supplies at most 64 exact
namespace/field/i32-signature registrations, with at most 16 parameters, 16
results and 64 calls per lifecycle each. The table and its name pointers are
borrowed only during open;
callback/context pairs remain valid until close. Swift's
`TinyArcadeNativeFunctionV1` owns stable UTF-8 names and strongly retains each
callback box for exactly that runtime lifetime.

Every registration carries `max_calls_per_lifecycle` (`1...64`; Swift defaults
to one). The runtime clears counters before each init, tick, suspend and resume,
then charges the matching function before dispatch. An over-budget call never
enters app code and traps/latches the cartridge. Because at most 64 functions
can be registered, guest-driven native dispatch is globally bounded to at most
4,096 calls in any lifecycle even under the loosest host table.

A callback executes synchronously on the runtime owner thread and receives
borrowed parameter/result buffers plus the complete bounds-checked guest linear
memory. It must return exactly its declared results, must not retain any pointer
or memory view, and must not unwind through C. Throwing, returning a wrong result
count, or returning nonzero from raw C traps and latches only that cartridge.
These callbacks are trusted code already compiled into the app; cartridges
cannot supply native implementations. Private-user opening intentionally has no
variant that grants native modules.

A synchronous callback cannot be safely preempted while it owns borrowed guest
memory and an owner-thread context. Measuring elapsed time after it returns is
not a timeout and would make behavior device-speed-dependent. Therefore each
app-compiled capability implementation must also enforce finite input/work
bounds and must never block on network, file I/O, locks or asynchronous work.
The runtime quota prevents an untrusted guest from amplifying that bounded unit;
it cannot repair an unbounded callback shipped by the app.

An open call creates one opaque handle. The WASM bytes and config are
copied/consumed during the call; caller pointers are not retained. The handle
must be ticked, suspended, resumed, queried and closed on its creating thread.
Wrong-thread calls return `TINYARCADE_WRONG_THREAD` without touching instance
state. The Swift wrapper is `@MainActor`, so ordinary app use enforces this
contract at compile time.

Close is explicit and idempotent at the Swift layer. Its `deinit` is a final
safety release. A raw C consumer must close exactly once on the owner thread.

ABI v1.5 also exposes the existing verified object cache through a distinct
single-thread-owned C handle and `TinyArcadeCartridgeCacheV1` Swift owner. The
app supplies a file URL and a positive per-object WASM byte ceiling. Network
transfer is deliberately absent: only a complete `Data` value may enter
`activate`, which checks current key/content revocation, Ed25519 signature,
length, SHA-256 and embedded manifest before atomically selecting it. Neither a
partial download nor an untrusted object becomes active.

`loadActive` and `rollback` require the matching signed catalog record and
reverify current trust before returning bytes through the same two-stage copy
contract as runtime frames. A failed load clears the handle's prior copied
result, so callers cannot accidentally consume bytes retained from an earlier
successful query. Cache handles reject cross-thread use; Swift confines them to
the main actor and provides explicit, idempotent close.

## Data transfer and errors

Frame, snapshot, replay and metadata outputs use a two-stage copy protocol. A NULL/zero
query writes the required length and returns `TINYARCADE_BUFFER_TOO_SMALL` for
non-empty data. A later copy does not execute the guest again. Bytes are never
NUL-terminated or retained in caller memory.

Every failing call records a static diagnostic in thread-local state. Read it
immediately with `tinyarcade_v1_last_error`; the next ordinary bridge call
clears it. Decode failure, guest trap, failed-instance latch, wrong-thread use,
buffer sizing, trust failure and caught panic have distinct stable status
values. The Swift frame owner validates `grid3d/v1`, `indexed2d/v1` and
`tones/v1` completely before exposing decoded cells, palettes, pixels or tone
events to native rendering/audio code. `tickMedia` returns a discriminated
render frame for either supported visual protocol; the original `tick` remains
a source-compatible `grid3d/v1` convenience for existing Depth Well consumers.

Replay recording is state on the same owner-thread runtime handle. Begin
captures a portable snapshot and clears the previous completed trace; ordinary
tick calls then append monotonic input plus exact media digests. Finish retains
one bounded `.tareplay` for two-stage copy. Suspend/resume and verification are
refused during recording, while cancel discards recording data without changing
the already-advanced game state. Verification compares the trace against the
exact cartridge hash retained at open, restores its initial snapshot and checks
every frame. It consumes runtime state, so Swift documents a disposable fresh
runtime as the preservation-safe verification owner.

`TinyArcadeIndexed2DFrame.rgba8888()` expands only already-validated indices
into canonical row-major RGBA bytes, with a decoder-proven allocation ceiling
below 256 KiB. `makeCGImage()` retains those bytes in an sRGB,
non-premultiplied, non-interpolated image. `TinyArcadeIndexed2DView` is the
minimal UIKit presentation owner: it preserves aspect ratio, applies nearest
filters for magnification and minification, and lets the app choose layout and
compositing. A Metal host remains free to use the palette and index plane
directly; UIKit and Core Graphics never enter the guest ABI.

`TinyArcadeToneSynthesizer.waveData(for:)` converts a validated tone batch into
a bounded 22,050 Hz mono PCM WAV using the event order, pitch, duration and
relative amplitude. `TinyArcadeTonePlayer` is the matching short-feedback
owner. A new batch replaces the old batch, `stop()` clears current feedback,
and `interruptionBegan()` stops without replaying stale game events. The app
forwards its `AVAudioSession.interruptionNotification` began transition and
calls `deactivate()` when the game surface relinquishes audio.

By default the player owns `AVAudioSession` activation with the `.ambient`
category and `.mixWithOthers`, so it follows the silent switch and does not
take exclusive ownership from music or other audio. An app with a centralized
audio coordinator constructs it with `managesAudioSession: false`; in that
mode the player never changes session category or activation. Haptics remain an
app presentation policy and are not inferred by the SDK.

Tick, suspend and resume use a handle-aware panic boundary. If Rust panics after
a handle has been resolved, the boundary first latches that runtime failed,
returns its phase to idle and discards any cached frame/snapshot/replay before returning
`TINYARCADE_PANIC`. The app may inspect/close the handle but cannot execute it
again. A generic `catch_unwind` status without that state transition is not
containment because partially mutated guest state could otherwise be reused.

## Current evidence boundary

The smoke gate builds a real arm64 iOS-device archive and a universal
arm64/x86_64 iOS-simulator archive, assembles both into one XCFramework,
compiles the public C header,
imports the module from Swift, links the Swift ownership wrapper against the
simulator archive, and verifies the output Mach-O platform is `IOSSIMULATOR`.
The optimized linked smoke executable must remain at or below 1.375 MiB; this
measures the dead-stripped consumer result rather than the multi-object static
archive's misleading on-disk size. The earlier 1 MiB gate was raised only when
the exercised Swift consumer added the bounded official-catalog JSON decoder;
the later 1.375 MiB gate accounts for the recoverable snapshot-store owner.
Replay remains within that existing honest ceiling;
the interpreter's separate stripped static-core gate remains below 100 KiB.

The builder pins iOS 14.0 as the deployment target for Rust and Ring C/assembly
objects; the Swift link treats linker warnings as errors. The gate also builds
the generated package as a generic iOS device library and as a universal
arm64/x86_64 simulator library under Swift 6 language mode. With
`TINYARCADE_RUN_BOOTED_SIMULATOR=1`, the smoke additionally runs a standard WASM
cartridge through a Swift-owned `fan:physics/v1` callback and proves i32
parameters/results, guest-memory mutation, generic `indexed2d/v1` decoding,
exact translucent RGBA expansion and native view presentation policy.
It then compiles both reference cartridges and runs the linked executable in an
already-booted iOS Simulator. Depth Well opens through the private origin,
decodes its first frame, suspends/resumes and hard-drops. Paddle Guard executes
600 WASM-owned indexed frames through CGImage/UIKit presentation and crosses a
suspend into a fresh instance during the measured run. Its real launch event is
also synthesized into a WAV, passed through `AVAudioPlayer`, interrupted and
explicitly deactivated on the booted simulator.
The same simulator smoke creates a real cache directory through the Swift v1.5
owner and proves that a cartridge naming an absent trust key cannot activate.
Rust's public C black box separately installs a valid signed cartridge, reloads
its exact bytes, rejects cross-thread access, then proves live revocation clears
the pending copy result and blocks the cached object.
An in-process URLProtocol fixture additionally proves the Swift transport's
catalog/cartridge success path, early declared-length rejection, MIME and
redirect failure, in-flight cancellation, exact active concurrency and typed
zero-queue saturation without relying on an external server.
Another linked simulator executable records four real Paddle Guard inputs,
atomically exchanges the resulting `.tareplay` through a file, verifies all
steps on a fresh runtime, reproduces byte-identical trace bytes, and rejects a
changed output digest plus different WASM bytes carrying the same manifest.

Rust black-box tests drive the C handle through bundled/private/reviewed open,
exact native registration, callback success/failure and failed-instance latch,
signature and revocation, origin query, tick, frame copy,
suspend, snapshot copy, fresh-instance resume, error retrieval, cross-thread
rejection and close. This is build/link/lifecycle evidence, not yet a physical
iPhone launch or frame-time measurement; those remain open physical-device
evidence.
