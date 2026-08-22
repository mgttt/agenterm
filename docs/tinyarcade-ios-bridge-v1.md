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
ABI/state-schema-bound snapshot. A save reserves one exact envelope buffer and
writes it to one same-directory, per-game prepared slot. A stale regular file
or symlink in that private slot is reclaimed before reuse; unexpected special
files and directories fail closed, so interrupted saves cannot accumulate
unbounded UUID artifacts or trigger recursive cleanup. Both entry cleanup and
failure cleanup use that same type-checked path; a failed save cannot fall back
to recursive `removeItem` and erase a directory owned by another writer. The
store applies complete-until-first-authentication protection there, and only
then atomically moves or replaces the published generation. Failure before
publication removes the prepared file and leaves the previous snapshot
byte-for-byte intact. The directory is excluded from backup. Reads reject
symlinks and
oversized/non-regular files. A corrupt or
incompatible snapshot is removed; the failed candidate runtime is closed and a
second clean runtime is returned with `discardedInvalid`, so save damage cannot
turn into a cartridge launch failure. A valid mapped envelope passes its bounded
snapshot slice directly to synchronous resume without a full `subdata` copy.

`TinyArcadeGameSessionV1` is the matching foreground owner. It aggregates exact
pressed sets for at most 32 stable input-source ids, so one touch/controller
release cannot clear a button held by another source. Each tick advances the
stored game clock by at most a configurable 1...1000 ms ceiling (250 ms by
default), commits the new clock only after a successful decoded frame, and fails
explicitly before `UInt32` exhaustion. `TinyArcadeFramePacerV1` converts
`CADisplayLink.timestamp` or another monotonic-seconds source to integer deltas,
retains fractional milliseconds to prevent drift, and rejects non-finite,
backwards or oversized samples without changing its baseline. It never accepts
`Date` directly; callers remain responsible for supplying the documented
monotonic source rather than converting wall-clock time to seconds.

`TinyArcadeAppleInputV1` is the main-actor adapter for Apple's public
GameController surface. It discovers connected extended gamepads and the
coalesced hardware keyboard, assigns each controller a non-reused source id,
and sends complete source-local pressed sets to the stable nine-button
contract. D-pad or left stick map to directions; A/B/X/Y map to
primary/secondary/tertiary/start; Menu maps to menu. Arrow keys or WASD map to
directions, Space/Z/X/C to the three actions, Return to start and Escape to
menu. Controller and keyboard handler queues are explicitly main-queue owned.
Unknown keys and controllers without an extended-gamepad profile are ignored.
The keyboard owner tracks the 14 supported physical aliases in one fixed
16-bit mask before deriving portable buttons. Holding both Space and Z, or an
arrow and its WASD alias, then releasing only one therefore preserves the
other key's action without a hash table or an unbounded key collection.

Disconnect, pause, scene resignation and owner deactivation publish an empty
set for every attached source. Reactivation also starts empty: a key held while
the app was inactive is not invented as a new press. The executable SDK smoke
uses two synthetic `GCExtendedGamepad` controllers to prove simultaneous
direction/action union, source-isolated disconnect, inactive-event rejection,
clean reactivation and the exact keyboard map. This is deterministic adapter
evidence, not a claim that Bluetooth latency, physical button feel or a real
keyboard/controller has been tested on hardware.

The real App keeps this full-state device contract outside its edge-triggered
guests. Each route remembers the last set per Apple source, sends only rising
buttons as zero-time press/release ticks, and handles Menu as native pause.
App-target tests execute the bundled Depth Well cartridge to observe one active
piece movement and the bundled Signal Lock cartridge to observe one ring turn;
repeating the same held value changes neither game, and held Menu toggles pause
only once.

On scene deactivation the app calls `deactivateAndSave(to:)`. The session clears
all inputs, becomes inactive before persistence, and rejects further input or
ticks even if storage fails. A runtime/suspend error marks the session failed;
a storage-only error leaves the healthy runtime distinguishable. On foreground
return the app resets its pacer, calls `activate()`, and the first new timestamp
emits a zero delta so background time never enters game time. `save(to:)`
persists runtime state with the exact last successful clock. Rust independently
rejects unknown button bits and backwards clock values before guest execution,
without latching an otherwise healthy runtime. The SDK does not subscribe to
scene notifications itself; lifecycle authority stays with the app.

## Ownership

ABI v1.8 exposes three non-interchangeable origins: bundled
`tinyarcade_v1_open`, signed `tinyarcade_v1_open_reviewed`, and local
`tinyarcade_v1_open_private`. Every instance retains its immutable origin for
UI/audit queries. Reviewed opening consumes a single-thread-owned trust store;
private opening has no native capability registry and cannot acquire official
provenance.

Swift adds a separate release-policy gate above those provenance checks.
`TinyArcadeDistributionPolicyV1.appStoreBundledOnly` is the default for every
private/reviewed runtime and library initializer, which fails before I/O or
guest execution. Enabling an external path requires a policy constructed with a
bounded Apple approval reference. This records an auditable product decision;
it does not manufacture Apple permission. SDK smokes use an internal test-only
policy unavailable to package consumers. Bundled runtime construction remains
unchanged and fully playable offline.

Before any origin opens, `TinyArcadeCartridgeDescriptorV1.inspect` can call the
stateless C descriptor gate. It validates the manifest, standard module,
lifecycle export signatures, core import signatures, native naming/arity and
manifest/import equality without instantiation or guest execution. Its bounded
TAD1 result exposes identity, ABI/state version and every exact import to app UI
or creator tooling. Descriptor success grants no origin, trust or native
capability; actual open remains authoritative.

ABI v1.8 also exports the app side of compatibility negotiation. The
two-stage `tinyarcade_v1_copy_host_profile` converts one exact runtime config
and native function table into callback-free canonical TAH1 bytes;
`tinyarcade_v1_check_cartridge_host_profile` reuses the Rust static validator
without instantiation. Swift exposes the same flow as
`TinyArcadeHostProfileV1.appBuild` and `inspectCompatibleCartridge`, both on
the main actor that owns the native table. Handlers are retained only while
their descriptors are encoded and are never called. The normative format and
limits are in
[`tinyarcade-host-profile-v1.md`](tinyarcade-host-profile-v1.md).
Render, audio and state byte ceilings preserve zero exactly as a disabled or
empty-only channel; zero never expands to a default. Thus the profile generated
by Swift is the same configuration the C runtime enforces dynamically.

ABI v1.11 adds
`tinyarcade_v1_copy_compatible_cartridge_descriptor`. A successful TAH1
preflight now returns canonical TAD1 bytes from the profile-bound inspection
instead of making Swift parse the cartridge again under default limits. The
ordinary check-only and descriptor-only calls remain available for existing C
consumers; the combined call changes neither artifact format nor runtime state.

ABI v1.12 adds `tinyarcade_v1_copy_host_compatibility_report` and Swift's
typed `compatibilityReport(for:)`. The bounded canonical TAC1 result embeds the
profile-bound TAD1 descriptor and every missing or wrong-signature import.
Incompatibility is returned as inspectable data; malformed input and resource
failures remain errors. Neither matching nor incompatible reports instantiate
the guest or invoke app callbacks.

`TinyArcadePrivateLibraryV1` turns that private opening into a complete local
library transaction. It preflights exact bytes under core-only policy before
an atomic canonical install, caps each object at the runtime's 2 MiB ceiling
and the library at 256 cartridges, excludes storage from backup, applies iOS
file protection, and rejects non-regular, symlinked, dangling-symlink,
oversized, corrupt or identity-mismatched objects when they are used. It owns
neither document picking nor public upload; the app must present the explicit
user selection and private provenance.

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
While the callback is active, it must not call a TinyArcade API that takes any
runtime handle, including a different handle. The C boundary rejects that
reentry with `TINYARCADE_INVALID_ARGUMENT` before converting the raw handle into
a Rust reference. An unwind-safe thread-local guard clears when the outer call
leaves; if the callback otherwise succeeds, the successful outer lifecycle call
also clears the nested rejection diagnostic and the cartridge remains healthy.
The Rust/C bridge stages the bounded parameter and result buffers in fixed
16-value stack arrays. Before entering app code, the VM fallibly reserves the
suspended caller's operand stack (or a top-level result vector); nested host
results then append inline without temporary heap staging. Allocator refusal
therefore cannot occur only after a callback has already mutated guest memory
or host state.
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

ABI v1.8 also exposes the existing verified object cache through a distinct
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
The C runtime handle recycles its prior completed render/audio storage through
the next tick and replay-recording tick. It still exposes only the two-stage
copy contract, clears the completed-frame state on error, and never lends Rust
storage across the ABI; steady frames therefore avoid rebuilding the bounded
host buffers without weakening pointer ownership.

An indexed frame may also expose `applicationMetadataSchema` and at most 1,024
opaque `applicationMetadata` bytes after the cartridge negotiates the optional
core extension. The generic SDK does not decode a game's schema. Signal Lock's
App adapter decodes its 64-byte state from the same completed frame, so its
30 Hz render path remains a `.tick` lifecycle and does not allocate a portable
snapshot or call `game_suspend` merely to update native UI/accessibility state.
The C boundary still performs one required copy into Swift-owned immutable
`Data`. `withPixelBytes` and `withApplicationMetadataBytes` then lend scoped,
read-only views into that owner for native decoding and RGBA conversion; their
pointers cannot escape the closure. The zero-indexed `pixels` and
`applicationMetadata` properties remain source-compatible value snapshots and
copy only when a caller explicitly asks for them.
Indexed presentation expands the validated palette plane directly into one
final-size `Data` buffer. It no longer grows a separate `[UInt8]` and then
copies the complete RGBA frame into `Data`; the existing 320 × 200 Simulator
presentation loop exercises the resulting CGImage path under its 16 ms average
budget, including non-opaque palette alpha.

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
owner. A new batch replaces the old batch, while automatic interruption,
old-route-loss and media-services-reset handling stops without replaying or
rerouting stale game events. The next non-empty batch lazily rebuilds playback.
An app with a centralized observer may disable automatic observation and call
the explicit lifecycle methods. The game surface calls `deactivate()` when it
relinquishes audio.

Synthesis writes the WAV header and PCM directly into one final-size `Data`
allocation. The player retains up to eight immutable synthesized batches under
a separate 512 KiB byte ceiling and updates them with LRU access; repeated game
cues therefore avoid trigonometric resynthesis. The cache never retains an
`AVAudioPlayer`: each playback attempt still creates a fresh system object, so
route and media-service lifecycle remain authoritative. Counter exhaustion
clears the bounded cache instead of wrapping its ordering identity.

Apple delivers interruption/reset notifications on the main thread but may
deliver route changes on a secondary thread. The selector boundary therefore
extracts only the scalar reason off-actor and marshals mutation back to the
main actor; a real background-posted route-change black box guards that rule.
After media-services reset, the next `play` call recreates the player and
reapplies category/mode/options before activation, without automatically
restarting the invalidated cue.

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

## Deterministic execution telemetry

C ABI v1.8 adds `tinyarcade_v1_last_execution_stats`; Swift exposes the same
record as `lastExecutionStats()`. It reports the last completed lifecycle
attempt (init/tick/suspend/resume), interpreted Wasm instruction count, current
memory pages and table elements, native dispatches, and render/audio/state byte
counts. A guest trap updates the record before the runtime latches failed, so
diagnostics can distinguish a fuel or host-output problem without executing the
guest again. A host-input rejection that occurs before execution leaves the
previous record unchanged.

The record is deterministic and allocation-free. It deliberately excludes wall
time, process resident memory, thermal state and device scheduling; those remain
platform measurements that the iOS app/test owns. This split lets converters
and replay tests compare guest resource high-water marks across machines without
pretending that elapsed milliseconds are portable.

ABI v1.9 appends `max_call_depth` and `max_activation_slots` to the runtime
configuration. The bridge reads the original 40-byte prefix first, so a v1.8
caller receives the stable defaults (512 and 1,048,576) without an out-of-bounds
read; a full 48-byte configuration owns both ceilings. TAH1 schema 2 publishes
the same values as app-build compatibility metadata. The original 40-byte
execution-stats output remains unchanged. The separate
`tinyarcade_v1_last_execution_stats_v2`/`lastExecutionStatsV2()` record adds the
accepted peak defined-call depth and aggregate live activation slots without
risking overwrite of an older caller's buffer. A limit trap records the highest
admitted usage, never a transient rejected activation.

ABI v1.10 adds app-owned native completion channels without changing the
runtime/config layouts. A channel is created before runtime open, may allocate
a bounded ticket from its synchronous native callback, and is supplied to the
new native-completion open functions. It is single-thread-owned, cannot close
while bound, and is cleared/unbound by runtime close. Payload bytes are copied
at the C call boundary; a late delivery after close fails. The Swift
`TinyArcadeCompletionV1` owner is `@MainActor`, and native handlers are also
main-actor closures bridged through the runtime's proven owner-thread callback.
The companion host-profile function includes the same generated completion
imports used by runtime binding.

ABI v1.11 only appends the combined static compatibility/descriptor export.
It changes no struct layout, callback ownership rule or cartridge ABI.
ABI v1.12 similarly appends only the static typed-report export and Swift
value types; the runtime/config layouts remain unchanged.
ABI v1.13 changes no runtime/config layout. It upgrades generated TAH1 to
schema 4 with an exact-build accepted-Wasm-feature bitmap and TAC1 to the
backward-decodable schema 2 bitmap. Swift exposes that result as
`TinyArcadeWasmFeatureSetV1`; the SIMD bit explicitly means the reviewed signed
PCM subset rather than the complete SIMD proposal.

## Current evidence boundary

The smoke gate builds a real arm64 iOS-device archive and a universal
arm64/x86_64 iOS-simulator archive, assembles both into one XCFramework,
compiles the public C header,
imports the module from Swift, links the Swift ownership wrapper against the
simulator archive, and verifies the output Mach-O platform is `IOSSIMULATOR`.
The optimized linked arm64 smoke executable has an explicit byte ceiling; the
simulator-only x86_64 compatibility slice has a separate ceiling.
This measures the dead-stripped consumer result rather than the multi-object static
archive's misleading on-disk size. The earlier 1 MiB gate was raised only when
the exercised Swift consumer added the bounded official-catalog JSON decoder;
the later 1.375 MiB gate accounted for the recoverable snapshot-store owner;
the arm64 gate additionally includes the app-owned completion channel,
process-lifetime ticket domains, safe late-delivery boundary and automatic
audio interruption/route/reset ownership.
The ceiling advances only in named 16 KiB capability steps. The prepared,
protected snapshot publication transaction receives one such arm64 step; its
black box also proves that a forced replacement failure preserves the old bytes
and removes the prepared artifact.
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
Because some current Simulator filesystems accept file-protection attributes
without returning them from `attributesOfItem`, simulator execution requires an
exact value when the attribute is surfaced; physical-device readback remains
the product evidence gate. `TINYARCADE_RUN_BOOTED_SIMULATOR=simd` separately
runs the focused optional SIMD cartridge, including its scalar/vector lane
bridge, without pretending the default and optional host profiles are the same.
The focused completion executable also runs its independently compiled
511-byte standard cartridge: Swift allocates the request during the native
start callback, renders the pending state, delivers RGBA bytes on the main
actor, and observes guest poll/take plus the decoded pixel. Runtime teardown
then rejects a late result through the still-live unbound channel.
The same simulator smoke creates a real cache directory through the Swift v1.8
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
The session black box combines overlapping input sources, launches and moves
the real Paddle Guard cartridge, converts fractional monotonic timestamps,
rejects invalid/backwards/background samples without baseline mutation,
deactivates/saves/restores/reactivates the exact clock, rejects inactive input
and ticks, distinguishes storage failure from runtime failure, and proves
rejected direct host input leaves the runtime playable.

The performance pass reads telemetry after every measured frame and requires it
to agree with the copied media lengths and configured fuel/page ceilings. In
the current release build, Depth Well's 600-frame run peaks at 13,150 Wasm
steps and 17 pages; Paddle Guard peaks at 37,864 steps and 17 pages. Their p95
values are 0.105 ms and 0.257 ms respectively on the iPhone 17 Pro simulator.
These numbers are regression evidence for this build and host, not a claim
about physical-device latency.

The adjacent Nostalgia Arcade consumer also owns an app-target 600-frame gate.
It measures wall time around the real `BundledDepthWellCartridgeRuntime.tick`,
reads v2 execution telemetry after every frame, enforces an 8 ms p95 host
budget plus fuel/page ceilings, and retains the exact result as an `.xcresult`
attachment. `scripts/test-tinyarcade-on-device.sh` in that repository selects
a connected physical iPhone (or accepts `TINYARCADE_DEVICE_ID`), runs all 11
App runtime tests and the playable UI journey on it, performs the unsigned
arm64 product build, and exports the performance attachment into a timestamped
evidence directory. The same test currently records 0.207 ms p95, 23,203 peak
steps, 17 pages, depth 6 and 62 activation slots on the iPhone 17 Pro simulator;
these remain simulator figures until the physical command is run.

`smoke-nostalgia-consumer.sh` is the runtime-owned closure of that integration
boundary. It rebuilds the package from this checkout, executes 13 App-target
unit tests plus the two-game UI journey, and builds the generic arm64 iOS
product. It then requires the consumer's archive to be byte-identical to the
one emitted under this repository's `target/`, checks that the final executable
contains the ABI v1.10 completion-channel symbol while consuming the current
v1.13 archive, and rejects any implicit
rewrite of the committed cartridges or Xcode project. Evidence on 2026-08-22
has archive SHA-256
`28b510624c430f1c4ce337b8bcb10796c6b1fac3f9e8d1dc75985143d576be7e`;
the App contains only the 6,116-byte Depth Well and 6,040-byte Signal Lock
cartridges, with no WebKit/JavaScriptCore, URLSession, external-library surface
or archived native game engine. A counted App-target test now takes a tone from
the real Depth Well cartridge, plays its exact pitch/duration/amplitude through
`TinyArcadeTonePlayer`, proves playback started and explicitly deactivates it;
both WASM screens preserve that tone path while keeping haptics as App policy.
Those screens now also drive the shared `TinyArcadeGameSessionV1` through
`TinyArcadeFramePacerV1`: the session is the sole game-clock owner, rejects
oversized foreground advances, releases all inputs on pause/background, saves
before scene suspension and ignores background timer/input delivery without
failing the cartridge. The App no longer maintains a wrapping parallel clock.
This closes current-main App consumption, not
the separate physical-iPhone lifecycle and performance requirement.

Rust black-box tests drive the C handle through bundled/private/reviewed open,
exact native registration, callback success/failure and failed-instance latch,
signature and revocation, origin query, tick, frame copy,
suspend, snapshot copy, fresh-instance resume, error retrieval, cross-thread
rejection and close. This is build/link/lifecycle evidence, not yet a physical
iPhone launch or frame-time measurement; those remain open physical-device
evidence.
