# TinyArcadeRuntime Swift package

This generated package is the iOS app integration artifact. It contains the
device/simulator `TinyArcade.xcframework` and the main-actor Swift ownership,
media-decoding and signed-catalog wrapper as one `TinyArcadeRuntime` library
product.

The Swift execution default is `TinyArcadeDistributionPolicyV1.appStoreBundledOnly`.
Private and reviewed external runtime/library initializers fail before I/O or
execution unless the app supplies a policy created with
`appleApprovedExternalCartridges(approvalReference:)`. The reference is an
auditable release assertion, not technical proof of permission; keep external
paths absent from App Store UI/builds until Apple has approved this exact custom
WASM use case. SDK black boxes use a non-public test-only policy.

Call `TinyArcadeCartridgeDescriptorV1.inspect(_:)` before presenting an import.
It statically validates the standard WASM manifest, lifecycle exports and exact
function import table without instantiating or executing the cartridge, then
reports identity and required versioned native capabilities. The private
library uses this same descriptor to return
`unsupportedNativeCapabilities` before core-only runtime preflight.

Call `TinyArcadeHostProfileV1.appBuild` with the same config and native
functions as the app runtime to export canonical TAH1 bytes for converters.
`inspectCompatibleCartridge` checks an exact standard import/resource profile
without instantiating the guest or calling handlers. Dynamic fuel/output and
native semantics remain separate reviewed-game gates.

When an official catalog includes `host_profile`, call
`TinyArcadeHTTPSClientV1.fetchHostProfile(_:matching:)` with the locally
generated App-build profile. The request is same-origin and exactly bounded;
success requires byte-for-byte equality with the local profile. Catalog
length/hash fields support discovery and converter content addressing, but do
not authorize a different native module or resource limit in the App.

Use `tickMedia` for the discriminated `grid3d/v1` or `indexed2d/v1` render
frame. Existing Depth Well integrations may keep using the `grid3d/v1`-only
`tick` convenience.

For indexed cartridges, `TinyArcadeIndexed2DFrame.makeCGImage()` provides an
exact sRGB RGBA image and `TinyArcadeIndexed2DView` is a ready-to-layout UIKit
surface with aspect-fit, nearest-neighbour presentation. Apps that own a Metal
renderer can instead consume the validated palette and pixel plane directly.

For audio feedback, pass `TinyArcadeMediaFrame.tones` to
`TinyArcadeTonePlayer.play(_:)`. The default player uses a mixing `.ambient`
audio session; use `TinyArcadeTonePlayer(managesAudioSession: false)` when the
app already owns session policy. Forward interruption-began events, call
`stop()` when feedback should be cut immediately, and call `deactivate()` when
leaving the game surface. The SDK deliberately does not resume interrupted
gameplay tones or choose haptics for the app.

Use `TinyArcadeGameSessionV1` as the foreground gameplay owner. It combines at
most 32 touch/keyboard/controller sources without premature button releases,
advances only a bounded monotonic game clock, rejects background-sized frame
deltas and persists that exact clock through `TinyArcadeSnapshotStoreV1`. Feed
it deltas from `TinyArcadeFramePacerV1` using `CADisplayLink.timestamp` or an
equivalent monotonic source—not `Date`. On scene resignation call
`deactivateAndSave(to:)`; it clears controls and makes further input/ticks fail.
Before foreground presentation resumes, reset the pacer and call `activate()`.

Reviewed downloads should be handed to `TinyArcadeCartridgeCacheV1.activate`
only after the app has received the complete response. The cache verifies the
signed entry and atomically selects it; `loadActive` and `rollback` recheck live
revocations before returning executable bytes. The cache performs no network
request, and private-user imports remain a separate origin and storage policy.

Decode official lobby metadata with `TinyArcadeCatalogV1.decode`. It bounds the
document, game count, strings, localizations, signed-entry encodings and
same-origin `{name}-{version}.wasm` filename. A generated
`tinyarcade://game/<game-id>` URL only selects an existing row; it never
downloads, activates or opens a cartridge. JSON discovery is not a substitute
for cache/trust verification.

`TinyArcadeHTTPSClientV1` streams official catalog and cartridge responses
through strict status, MIME, redirect, timeout, declared-length and received-byte
checks. It defaults to two active plus sixteen queued requests and exposes
smaller bounded limits. Task cancellation stops in-flight work or removes a
queued waiter. The returned cartridge `Data` must still be passed explicitly to
`TinyArcadeCartridgeCacheV1.activate`; transport success grants no provenance.

Use `TinyArcadeReviewedLibraryV1` for the complete official selection path. It
preflights downloaded bytes as a reviewed runtime before cache activation,
serializes installs across `await`, and reopens an active generation only after
live trust/revocation verification. This preserves the last playable cache
state when a signed cartridge needs native capabilities absent from the app.
Construction requires the explicit external-cartridge distribution policy.

Use `TinyArcadeSnapshotStoreV1` for scene/background persistence. It atomically
replaces one bounded file per canonical game id, stores the host-owned game
clock beside the runtime snapshot, applies iOS file protection and excludes the
directory from backup. `openSession` returns a fresh runtime when no save exists,
restores a compatible save, or discards a corrupt/incompatible save and creates
a second clean runtime so failed resume state cannot poison gameplay.

For reproducible bug reports or converter goldens, call
`beginReplayRecording()`, drive the game with ordinary `tick`/`tickMedia`, then
save or upload the bounded `Data` returned by `finishReplayRecording()`. Verify
received bytes on a disposable fresh runtime with `verifyReplay(_:)`; this
checks the runtime's exact loaded-cartridge hash and consumes its gameplay
state. Replay data contains no executable code and grants no native capability
or catalog trust.

Use `TinyArcadePrivateLibraryV1` when a user explicitly imports a cartridge
for personal play. It preflights the exact bytes with the core-only private
runtime before an atomic `game-id@version.wasm` install, excludes the bounded
library from backup, and revalidates canonical identity, size and regular-file
ownership whenever an item is enumerated or opened. It never downloads,
publishes, signs, or grants a native module. Construction requires the same
explicit external-cartridge distribution policy.

Generate a self-contained directory from the repository root:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
crates/agenterm-tinyvm/build-swift-package.sh \
  dist/TinyArcadeRuntimePackage
```

An app may then add that directory as a local Swift package and depend on the
`TinyArcadeRuntime` product. The generated directory is a build artifact; this
template, the Swift source and Rust/C sources remain authoritative.
