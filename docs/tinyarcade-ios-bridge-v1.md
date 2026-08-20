# TinyArcade iOS bridge v1

The iOS app links a static TinyArcade library; a cartridge remains data. No
downloaded code is turned into native executable memory. The Rust interpreter,
host ABI and lifecycle owner are compiled into the app binary.

## Delivered surfaces

- `crates/agenterm-tinyvm/include/tinyarcade.h`: versioned C ABI.
- `crates/agenterm-tinyvm/include/module.modulemap`: Swift module `TinyArcade`.
- `crates/agenterm-tinyvm/bindings/swift/TinyArcadeRuntime.swift`:
  `@MainActor` Swift owner plus indexed-2D native presentation.
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

## Ownership

ABI v1.3 exposes three non-interchangeable origins: bundled
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

## Data transfer and errors

Frame, snapshot and metadata outputs use a two-stage copy protocol. A NULL/zero
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

`TinyArcadeIndexed2DFrame.rgba8888()` expands only already-validated indices
into canonical row-major RGBA bytes, with a decoder-proven allocation ceiling
below 256 KiB. `makeCGImage()` retains those bytes in an sRGB,
non-premultiplied, non-interpolated image. `TinyArcadeIndexed2DView` is the
minimal UIKit presentation owner: it preserves aspect ratio, applies nearest
filters for magnification and minification, and lets the app choose layout and
compositing. A Metal host remains free to use the palette and index plane
directly; UIKit and Core Graphics never enter the guest ABI.

Tick, suspend and resume use a handle-aware panic boundary. If Rust panics after
a handle has been resolved, the boundary first latches that runtime failed,
returns its phase to idle and discards any cached frame/snapshot before returning
`TINYARCADE_PANIC`. The app may inspect/close the handle but cannot execute it
again. A generic `catch_unwind` status without that state transition is not
containment because partially mutated guest state could otherwise be reused.

## Current evidence boundary

The smoke gate builds a real arm64 iOS-device archive and a universal
arm64/x86_64 iOS-simulator archive, assembles both into one XCFramework,
compiles the public C header,
imports the module from Swift, links the Swift ownership wrapper against the
simulator archive, and verifies the output Mach-O platform is `IOSSIMULATOR`.
The optimized linked smoke executable must remain at or below 1 MiB; this
measures the dead-stripped consumer result rather than the multi-object static
archive's misleading on-disk size.

The builder pins iOS 14.0 as the deployment target for Rust and Ring C/assembly
objects; the Swift link treats linker warnings as errors. The gate also builds
the generated package as a generic iOS device library and as a universal
arm64/x86_64 simulator library under Swift 6 language mode. With
`TINYARCADE_RUN_BOOTED_SIMULATOR=1`, the smoke additionally runs a standard WASM
cartridge through a Swift-owned `fan:physics/v1` callback and proves i32
parameters/results, guest-memory mutation, generic `indexed2d/v1` decoding,
exact translucent RGBA expansion and native view presentation policy.
It then compiles Depth Well,
runs the linked executable in an already-booted iOS Simulator, opens it through
the private origin, decodes its first frame, suspends/resumes and hard-drops.

Rust black-box tests drive the C handle through bundled/private/reviewed open,
exact native registration, callback success/failure and failed-instance latch,
signature and revocation, origin query, tick, frame copy,
suspend, snapshot copy, fresh-instance resume, error retrieval, cross-thread
rejection and close. This is build/link/lifecycle evidence, not yet a physical
iPhone launch or frame-time measurement; those remain open physical-device
evidence.
