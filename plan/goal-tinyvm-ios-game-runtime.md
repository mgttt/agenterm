# Goal — tinyvm as an iOS game runtime foundation

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Outcome: an iOS arcade app can load a reviewed standard `.wasm` game, create
one bounded persistent instance, drive it frame by frame through an owned
native ABI, suspend/resume it, and fail one bad game without hanging or
corrupting the app.

Legend: `[x]` proven · `[~]` partial · `[ ]` required

```text
tinyvm iOS game runtime
├── execution kernel                 [~]
│   ├── WASM 1.0 validation/opcodes   [x]
│   ├── persistent instance           [x]
│   ├── start exactly once            [x]
│   ├── per-call instruction budget   [x]
│   ├── VM-owned call activations      [x]
│   ├── host memory/table budgets     [x]
│   ├── decode complexity budget      [x]
│   ├── single-table funcref profile  [x]
│   ├── multiple defined tables       [x]
│   ├── standard tail calls            [x]
│   ├── typed standard host imports     [x]
│   ├── strict declared-memory semantics [x]
│   ├── strict scalar memarg alignment [x]
│   ├── canonical function expressions [x]
│   ├── strict i64 signed-LEB range    [x]
│   ├── valid custom-section names     [x]
│   ├── deterministic execution stats [x]
│   └── trap isolation                [x]
├── game host ABI                    [~]
│   ├── standard WASM cartridge       [x]
│   ├── version negotiation           [x]
│   ├── lifecycle init/tick/suspend   [x]
│   ├── input snapshot                [x]
│   ├── bounded render commands       [x]
│   ├── bounded audio commands        [x]
│   ├── recyclable frame buffers      [x]
│   ├── clock/RNG determinism         [x]
│   ├── native capability registry    [x]
│   ├── bounded in-place host dispatch [x]
│   └── storage without guest network [x]
├── artifact trust                    [x]
│   ├── manifest + compatibility      [x]
│   ├── content hash/signature        [x]
│   ├── atomic cache/rollback         [x]
│   └── reviewed catalog only         [x]
├── cartridge ownership              [~]
│   ├── official reviewed catalog     [~]
│   ├── private user import           [x]
│   ├── App Store bundled-only policy [x]
│   ├── static compatibility descriptor [x]
│   ├── converter conformance kit     [x]
│   ├── canonical manifest authoring  [x]
│   ├── app-build host profile        [x]
│   ├── deterministic catalog publisher [x]
│   └── no public arbitrary execution [~]
├── iOS native bridge                 [~]
│   ├── stable C lifecycle ABI        [x]
│   ├── static library/XCFramework   [x]
│   ├── Swift ownership/threading     [x]
│   ├── input + monotonic clock owner [x]
│   ├── frame pacing + scene state    [x]
│   ├── indexed 2D presentation       [x]
│   ├── device + simulator build      [x]
│   ├── real app target/package link  [x]
│   ├── reviewed install transaction  [x]
│   ├── private atomic library         [x]
│   ├── atomic scene persistence      [x]
│   ├── replay record/verify owner     [x]
│   └── on-device lifecycle test      [ ]
├── real-game proof                   [~]
│   ├── constrained compiler profile  [x]
│   ├── Depth Well WASM vertical cut  [x]
│   ├── Paddle Guard 2D vertical cut   [x]
│   ├── portable replay goldens        [x]
│   ├── development WebKit differential [x]
│   ├── frame-time/resource evidence  [~]
│   └── suspend/resume/save evidence  [x]
└── distribution gate                 [~]
    ├── fixed app purpose/offline game [x]
    ├── catalog metadata/deep links   [x]
    ├── review clarification/probe    [~]
    └── fail closed on revoked content [x]
```

## Dependency path

```text
persistent instance + per-call budgets
    → game host ABI
        → stable C/iOS bridge
            → Depth Well vertical cut
                → device/performance/review evidence

artifact manifest + trust
    → reviewed remote catalog
        → cache/rollback/revocation
            → distribution evidence
```

The execution kernel and artifact-trust branch can mature independently. The
iOS bridge must not freeze a game ABI before the native Rust black-box owner
can drive a persistent instance. A remote catalog must not precede hash,
signature, compatibility, cache and revocation semantics.

## Runtime authority

Apple's JavaScriptCore contains an internal WebAssembly implementation, and
its public JavaScript VM headers mention WebAssembly compilation work. Apple
does not expose a dedicated native `WasmModule` / `WasmInstance` embedding
contract; the public route is JavaScript execution through `JSContext`.
JavaScriptCore therefore serves as a development-only comparison engine behind
exact replay parity tests, but it is not the platform authority and no game may
require it. tinyvm remains the portable, deterministic baseline.

H5, DOM, JavaScript mini-app and WKWebView semantics are excluded. Runtime JIT,
device-side native AOT of downloaded modules, dynamic native-code loading,
WASI, guest network access and arbitrary third-party uploads are excluded.
The tested public/private JavaScriptCore boundary and capability matrix live in
[`docs/tinyarcade-javascriptcore-boundary.md`](../docs/tinyarcade-javascriptcore-boundary.md).

The cartridge remains an ordinary standards-valid WebAssembly module. The
runtime does not add private opcodes or wrap executable bytes in a proprietary
format. Platform services are standard function imports under versioned module
names: v1 core uses `tinyarcade:core/v1`; future native modules receive their
own canonical `authority:module/vN` namespaces and must be present in a host
capability registry. Function names and i32 signatures remain in the standard
import table, which the converter reports without executing the guest.
Unknown namespaces fail closed. Metadata may live in a standard WASM custom
section or adjacent signed manifest, so converters can emit and validate the
same cartridge contract without depending on the interpreter implementation.

Compatibility is defined by the standard module plus that versioned contract,
not by tinyvm internals. Core v1 semantics do not drift when a native module is
added. Each native module advances under its own canonical `/vN` namespace;
module name, field, value signature and finite-work policy are exact. This lets
future fan-facing converters inspect the manifest and import table without
executing a cartridge, emit a capability/compatibility report, and target other
standards-compliant Wasm producers. A capability declaration never grants the
right to load native code.

Official catalog distribution and a user's private cartridge import are two
different policy surfaces. Private import is intended for a user's own app
library and does not silently publish or execute arbitrary uploads for other
users. Both routes share byte validation, resource limits and capability
negotiation; only the official route may enter the reviewed remote catalog.

## First executable increment — proven

One loaded module becomes one persistent instance. Its start function runs
once. Memory and mutable globals survive exported calls. The host selects a
per-call instruction ceiling and a memory-page ceiling that also governs
`memory.grow`. Existing `eval` and `Module::invoke*` remain fresh-instance
convenience APIs. Evidence is public integration tests covering persistence,
start-once, budget exhaustion, memory growth refusal and legacy fresh-call
behavior.

Evidence on 2026-08-21:

- `cargo test -p agenterm-tinyvm --all-targets --locked`: 137 passed.
- `cargo clippy -p agenterm-tinyvm --all-targets --locked -- -D warnings`:
  clean.
- `measure-core.sh`: 70,904-byte stripped macOS core, self-test 42.
- `cargo check -p agenterm-tinyvm --lib --target aarch64-apple-ios --locked`:
  clean.
- Same check for `aarch64-apple-ios-sim`: clean.

## Second executable increment — game ABI v1

The first cartridge boundary consumes an ordinary standard `.wasm` module.
It negotiates `game_abi_version`, runs `game_init` once, and drives persistent
state through `game_tick`. Core services are optional standard function imports
under `tinyarcade:core/v1`: input bits, monotonic game time, deterministic RNG,
and one bounded render/audio submission per lifecycle call. Calls outside init
or tick, duplicate submissions, invalid memory ranges and over-budget output
trap the cartridge without granting another host capability.

Native extensions are not private WASM opcodes. The app explicitly registers
an exact i32 function signature under a versioned namespace such as
`studio:physics/v1`; only then can a cartridge import it. An unknown namespace,
duplicate function import or signature mismatch fails before instantiation.
Manifest declarations, exact C/iOS registration and pre-dispatch lifecycle
quotas are proven. Native callbacks are trusted app code: every future shipped
module must additionally prove its own finite input/work bound and nonblocking
implementation before registration. No native gameplay module ships yet.

Evidence on 2026-08-21:

- Full `agenterm-tinyvm` suite: 143 passed, including six public game-runtime
  black-box tests.
- Clippy with warnings denied: clean.
- iOS device and arm64/x86_64 simulator library checks: clean.
- Stripped static core: 70,904 bytes; self-test 42.

## Third executable increment — manifest and portable state

Every runnable cartridge now carries one canonical
`tinyarcade.manifest.v1` standard WASM custom section. Game id, game version,
ABI/state-schema versions and declared native capability namespaces are parsed
under strict size/UTF-8/canonicality bounds. The declared capability set must
exactly match non-core imports, and all five lifecycle exports must have the
exact `() -> i32` signature before instantiation.

Suspend captures one bounded guest state payload plus host RNG in a canonical
snapshot envelope bound to game id, ABI and state-schema version. Resume into a
fresh instance restores both guest mutable state and deterministic RNG. Wrong
game/schema, truncated bytes and oversized state fail before guest execution.
A guest trap or lifecycle/budget violation latches the instance failed so the
app cannot continue from partially mutated state.

The converter-facing wire contract is
[`docs/tinyarcade-cartridge-abi-v1.md`](../docs/tinyarcade-cartridge-abi-v1.md).

Evidence on 2026-08-21:

- Full `agenterm-tinyvm` suite: 147 passed, including ten public cartridge,
  lifecycle and snapshot black-box tests.
- PRD `[x]` evidence map, Clippy with warnings denied, iOS device build and
  universal arm64/x86_64 simulator build: clean.
- Stripped static core remains 70,904 bytes with self-test 42.

## Fourth executable increment — iOS C/Swift ownership

The versioned C ABI owns open/tick/frame-copy/suspend/snapshot/resume/close,
manifest metadata, failed-state inspection and per-thread error diagnostics.
Opaque handles record their creating thread and reject every cross-thread
operation, including close. Every export has a panic fence and the dedicated
`tinyvm-ios-release` profile preserves unwinding so the fence is real.

The XCFramework builder produces an arm64 iOS-device slice and a universal
arm64/x86_64 iOS-simulator slice with the public header and Swift module map.
The `@MainActor` Swift wrapper owns the handle and exposes Data-valued
frame/snapshot methods.
The Swift-package builder combines those slices and that wrapper into one
self-contained `TinyArcadeRuntime` library product, which is the stable app
dependency boundary and can later be zipped as a binary release artifact.
The bridge smoke gate compiles the C header, builds both slices, assembles the
XCFramework, imports it from Swift, links the wrapper, and verifies an
`IOSSIMULATOR` Mach-O. Physical-device launch remains tied to the Depth Well
vertical cut.

The embedding contract is
[`docs/tinyarcade-ios-bridge-v1.md`](../docs/tinyarcade-ios-bridge-v1.md).

Evidence on 2026-08-21:

- Feature-enabled suite: 151 passed, including C handle lifecycle and the
  macOS-owned XCFramework/Swift-link integration gate.
- Real XCFramework slices: `ios-arm64` and `ios-arm64_x86_64-simulator`, each with C
  header and Swift module map.
- Self-contained Swift package: generic iOS-device and universal simulator
  builds clean under Swift 6; actor-isolated teardown keeps C handles on their
  owner executor.
- Optimized linked Swift simulator smokes: arm64 781,288 bytes and x86_64
  813,024 bytes, both below the 1 MiB consumer footprint gate.
- Feature-enabled Clippy with warnings denied and documentation redaction:
  clean.

## Fifth executable increment — real standard cartridge

Depth Well is now authored as a standalone `no_std` Rust guest rather than a
host-side fixture. The reproducible compiler profile emits a normal `.wasm`,
then lowers compiler-added bulk-memory operations to strict WASM MVP while
preserving the standard TinyArcade manifest custom section. Its original 5 × 5
× 10 falling-polycube rules include a fair five-piece bag, three-axis rotation,
wall kicks, landing ghost, hard drop, full-deck compaction, scoring, level speed
and semantic sound cues.

The first versioned media protocols are allocation-free, strictly decoded
`tinyarcade:grid3d/v1` frames and `tinyarcade:tones/v1` events. The native host
retains camera/material/audio-session authority; cartridges transmit bounded
semantic records rather than platform objects or native commands.

Evidence on 2026-08-21:

- The optimized cartridge is below 16 KiB, contains no absolute developer path
  and loads under a 17-page memory ceiling.
- Init, movement, hard drop, valid 3D frame, valid tone event and portable
  suspend/resume run through the public `GameRuntime` black box.
- Repeating the same hard drop after restore produces byte-identical render and
  audio under a 100,000-instruction per-call ceiling.
- Physical iPhone rendering/input and measured frame-time evidence remain open.

## Sixth executable increment — signed objects and atomic rollback

Official catalog entries now use a canonical Ed25519 message binding game and
schema identity to the exact object length and SHA-256. An app-bundled keyring
supports key rotation, key revocation and content-hash revocation. Verification
also parses the embedded WASM manifest and requires it to match the signed
record before runtime loading.

The app-owned cache stores verified content-addressed objects and atomically
promotes one fixed-size current/previous activation record. Current load and
rollback both re-verify bytes against the current trust/revocation state;
previously valid cached bytes never bypass a later revocation. Cache calls are
owned by the app's single runtime actor and are not a concurrent downloader.

Evidence on 2026-08-21:

- A signed real Depth Well object verifies; one changed byte is rejected.
- Revoking either its key or content hash rejects the otherwise valid object.
- Two valid generations activate and roll back; revoking the previous
  generation prevents reactivation.
- Trust/cache code builds for arm64 iOS device and simulator; the no-feature
  static interpreter core remains independent of the crypto dependency.

## Seventh executable increment — explicit origin and iOS execution

Bundled, official-reviewed and private-user cartridges now have distinct Rust,
C ABI v1.1 and Swift opens. Origin is immutable and queryable. Reviewed opening
requires the live signature/revocation store; private opening always uses an
empty native capability registry and cannot be relabelled as official.

Swift now strictly decodes the versioned 3D frame and tone records before
native consumers see cells/events. The public converter CLI inspects canonical
metadata and checks a cartridge through the same private policy, lifecycle
budgets, media validation and byte-deterministic suspend/resume replay.

Evidence on 2026-08-21:

- C black-box tests prove all three origins, signed reviewed open, live content
  revocation and source query.
- `tinyvm cartridge check` accepts the real 6,076-byte Depth Well artifact and
  reports its bounded frame and 335-byte snapshot.
- A linked 760,656-byte iOS Simulator executable loads Depth Well through the
  Swift private-import API, decodes its frame, suspends/resumes and hard-drops.
- On the booted iPhone 17 Pro simulator, 600 complete tick/copy/decode frames
  measured 0.102 ms average, 0.113 ms p95 and 0.202 ms maximum.
- iOS 14 deployment is pinned for Rust and Ring objects, and linker warnings
  are fatal.

## Eighth executable increment — honest App Review boundary

Current Apple policy was rechecked against the official guidelines dated
2026-06-08. Guideline 2.5.2 generally rejects downloaded/executed code that
changes app functionality. Guideline 4.7 names HTML5/JavaScript mini games,
streaming games, chatbots, plug-ins and retro-emulator game downloads, but does
not expressly name a custom WASM platform; Apple's Mini Apps Partner material
requires approval for another language.

Therefore the initial App Store product gate is a self-contained, fixed-purpose
Depth Well build with its cartridge inside the signed app bundle. Remote
catalog execution and Files/private import remain technical SDK capabilities,
not enabled shipping features, until Apple explicitly clarifies or permits the
use case. This is recorded in
[`docs/tinyarcade-app-review-boundary.md`](../docs/tinyarcade-app-review-boundary.md).

## Ninth executable increment — reusable SDK and real app target

Native extensions remain standard WASM imports. Their canonical namespace is
`authority:module/vN`; exact function fields and i32 arities remain in the
ordinary import table, and `tinyvm cartridge inspect` now reports that table for
converter compatibility checks. Unknown, malformed, undeclared or unregistered
capabilities still fail before instantiation.

The iOS builder now emits an arm64 device slice and one universal arm64/x86_64
simulator slice, then wraps them and the Swift 6 ownership layer in a
self-contained `TinyArcadeRuntime` package. Actor-isolated teardown preserves
the C handle's owner-executor contract. The complete bridge gate builds that
package for generic iOS device and simulator destinations and directly links
both simulator architectures.

The real `nostalgia-arcade` app target now depends on that generated package and
ships the exact 6,076-byte Depth Well cartridge in its signed resources. Its
app-owned adapter exposes only the bundled origin. A hosted iPhone 17 Pro
simulator test proves identity, first frame, suspend, fresh-instance resume and
hard drop; a generic iOS device build proves the package, Rust archive and WASM
resource participate in the final app link.

## Tenth executable increment — WASM-owned playable app route

The live Depth Well route in `nostalgia-arcade` now uses the bundled WASM
cartridge rather than the native game model. Swift owns a fixed orthographic
SceneKit whole-well view, labeled touch controls, tones/haptics, lifecycle and a
versioned local save envelope. All board cells, active/ghost pieces, gravity,
movement, three-axis rotation, hard drop, score, cleared decks, level and
game-over state originate in the guest's standard frame/state protocols.

The host advances a monotonic game clock only while active and unpaused, caps a
single catch-up interval, persists that clock beside the cartridge snapshot and
releases every edge-triggered input at the same clock instant. Background wall
time therefore cannot cause an unexpected drop on resume, and repeated taps do
not become held guest buttons.

Evidence on 2026-08-21:

- App unit tests open the real bundled cartridge and prove hard drop/tone output,
  fresh-runtime restore, paused clock exclusion and score-preserving re-entry.
- The iPhone 17 Pro simulator UI path passes selection, visible 3D frame,
  X/Y/Z rotation, hard drop, pause, settings, exit and restored re-entry.
- The inspected final screen keeps the full 5 × 5 × 10 well, entry piece,
  landing ghost and floor visible under one fixed orthographic camera.
- A generic `iphoneos` build with signing disabled links successfully after the
  UI switch, and the cartridge preparation/conformance script runs in a shell
  where Cargo is installed but absent from `PATH`.
- `nostalgia-arcade` 0.16.4 (29) was archived with automatic signing and
  accepted by App Store Connect for TestFlight processing. Build 28 was first
  rejected as already used, so the build number was advanced and committed
  before the successful archive/upload.
- A physical-iPhone lifecycle/performance session and TestFlight feel check
  remain open; this goal is not complete until that device evidence exists.

## Eleventh executable increment — versioned native import bridge

C ABI v1.2 and the Swift package now let bundled and reviewed cartridges bind
an exact, versioned native function table while private-user cartridges remain
core-only. Each registration fixes namespace, field and i32 arity; unknown or
mismatched imports fail before instantiation. Swift owns stable UTF-8 name
storage and callback contexts until runtime close. Callbacks run synchronously
on the runtime owner thread, borrow guest memory only for the call, and a throw,
wrong result count or raw nonzero return traps and latches that cartridge.

Evidence on 2026-08-21:

- Rust C-ABI tests prove exact binding, i32 parameters/results, guest-memory
  mutation, callback-failure latch, missing registration and arity rejection.
- The C header smoke compiles both new open forms and the callback layout for an
  iOS simulator target with warnings denied.
- The Swift 6 package builds for generic iOS device and universal simulator.
- On the booted iPhone 17 Pro simulator, a standard cartridge calls
  `fan:physics/v1.step_world` through the public Swift API before the same linked
  executable runs Depth Well for 600 frames (0.098 ms average, 0.106 ms p95,
  0.122 ms maximum).
- Native callback wall-time/resource budgets remain open; the physical-iPhone
  lifecycle/performance and TestFlight feel checks also remain open.

## Twelfth executable increment — native dispatch containment

C ABI v1.3 adds an explicit `max_calls_per_lifecycle` to each native function
registration. The value is 1...64 (Swift defaults to one), resets independently
for init/tick/suspend/resume and is charged before callback dispatch. Exceeding
it never enters app code and traps/latches only that cartridge. Combined with
the 64-function table limit, even the loosest host registration has a fixed
4,096-dispatch lifecycle ceiling.

The runtime deliberately does not claim a wall-clock timeout for synchronous
owner-thread callbacks over borrowed guest memory: elapsed-time rejection after
return cannot prevent a hang and would make deterministic game behavior depend
on device speed. Native module implementations are trusted app code and must
prove bounded, nonblocking work before shipping; WASM fuel plus dispatch quota
prevents an untrusted cartridge from amplifying that unit. There are currently
no shipped native gameplay modules.

Evidence on 2026-08-21:

- Public Rust black-box tests prove charge-before-dispatch, rejection without a
  second app callback, failed-instance latch, invalid 0/>64 limits and quota
  reset across successful ticks.
- C ABI tests prove the same over actual callback pointers and guest memory;
  malformed limits null the output handle and fail before runtime creation.
- Swift 6 device/simulator package builds pass. On the booted iPhone 17 Pro
  simulator, one standard cartridge completes two budgeted native calls before
  Depth Well runs 600 frames (0.101 ms average, 0.109 ms p95, 0.117 ms max).
- Physical-iPhone lifecycle/performance and TestFlight feel checks remain open.

## Thirteenth executable increment — panic-latched lifecycle boundary

Tick, suspend and resume now cross one handle-aware unwind boundary. A caught
panic latches the affected runtime failed, restores the host lifecycle phase to
idle and clears cached frame/snapshot output before returning
`TINYARCADE_PANIC`. Subsequent lifecycle execution returns
`TINYARCADE_FAILED_INSTANCE`; callers may still inspect and close the handle.
Ordinary guest traps retain their existing latch, while malformed external
snapshot bytes rejected before guest execution remain non-poisoning.

Evidence on 2026-08-21:

- An injected panic after a live frame and snapshot returns the stable panic
  status, sets `is_failed=1`, removes both outputs and rejects the next tick.
- The same test passes under the exact optimized `tinyvm-ios-release` profile
  whose `panic=unwind` policy is used to build every XCFramework slice.
- Full untrusted byte, stack, recursion, instruction, memory, table, lifecycle,
  callback-failure and native-dispatch-budget tests remain the public trap
  isolation owner.
- Physical-iPhone lifecycle/performance and TestFlight feel checks remain open.

## Fourteenth executable increment — v1.3 consumer-app delivery

The real `nostalgia-arcade` consumer regenerated its app-local Swift package
from the current TinyVM main and linked the C ABI v1.3 XCFramework into both
simulator and generic iOS-device targets. Depth Well remains the same ordinary
6,076-byte WASM 1.0 cartridge with only the seven `tinyarcade:core/v1` imports;
the native-import registry is available to future reviewed cartridges without
granting this cartridge any additional capability.

The WASM-owned route now preserves the product's untimed VoiceOver contract:
automatic gravity stops while assist mode is active, but explicit player input
continues to execute. The new WASM session key also participates in the app's
central UI-test reset, preventing a previous game-over snapshot from leaking
between language, accessibility and navigation scenarios.

Evidence on 2026-08-21:

- The complete iPhone 17 Pro simulator app plan passed 39 tests with zero
  failures; five iPad-viewport-only cases were explicitly skipped (44 total).
- A focused post-pull gate re-proved the real cartridge unit tests plus the
  three repaired language/accessibility UI paths, and a generic `iphoneos`
  build linked successfully with signing disabled.
- The signed `nostalgia-arcade` 0.16.4 (30) archive contains an arm64 app and a
  cartridge whose SHA-256 exactly matches the converter-checked input. Xcode
  accepted the upload and reported that the package entered App Store Connect
  processing.
- App Store Connect processing is not physical-device evidence. The
  physical-iPhone lifecycle/performance session and TestFlight feel check
  remain open, so this goal remains incomplete.

## Fifteenth executable increment — generic indexed 2D frames

The media boundary no longer assumes every cartridge is a 3D Depth Well. The
new `tinyarcade:indexed2d/v1` stream is one ordinary bounded render record: a
fixed header, 1...256 RGBA8 palette entries and an exact row-major byte-index
plane. Dimensions are independently capped at 512, the checked pixel product
at 65,535 and the whole stream at 64 KiB. Full-palette 256 × 240 and 320 × 200
classic frame sizes fit the default host budget. Unknown flags, trailing bytes
and any out-of-palette index fail before native presentation.

An indexed cartridge must declare the feature with the ordinary zero-argument
core import `indexed2d_version() -> i32` and check for version 1. A runtime that
predates the feature rejects that unknown import before instantiation; the new
runtime also traps/latches a `TAI2` submission that omitted the declaration.
This prevents a cartridge from appearing compatible until its first native
render without introducing a proprietary opcode or wrapper.

Rust exposes one discriminated `RenderFrame` decoder used by the converter
gate. Swift exposes the parallel `TinyArcadeRenderFrame` through `tickMedia`,
while the original grid-specific `tick` remains source-compatible for the
existing Depth Well app. The app host still owns scaling, aspect fit,
color-space conversion and Metal/Core Graphics presentation; no GPU command,
platform object or new native capability crosses the guest boundary.

Evidence on 2026-08-21:

- A standard core-only WASM cartridge submits an indexed frame through the
  real `GameRuntime`; allocation-free Rust decoding proves its palette and
  pixel plane. Missing feature declaration, malformed index, flag, length and
  over-budget vectors fail closed.
- The complete all-target/all-feature TinyVM suite passes 168 tests. The PRD
  traceability gate maps `bounded frame output [x]` to the executed core-only
  cartridge test. Clippy with warnings denied and the no-default library check
  are clean.
- The iOS bridge builds device and universal simulator packages, links the
  decoder smoke at 805,992 bytes for arm64 and 844,856 bytes for x86_64, and a
  booted iPhone 17 Pro simulator accepts a valid indexed frame and rejects an
  out-of-palette pixel through Swift before running Depth Well for 600 frames
  (0.110 ms average, 0.117 ms p95, 0.135 ms maximum).
- The stripped static core remains 70,904 bytes with self-test 42. A real 2D
  production cartridge and the physical-iPhone/TestFlight evidence remain
  open; this goal is not complete.

## Sixteenth executable increment — native indexed 2D presentation

The generated iOS SDK now carries the first reusable native presentation path,
not merely a decoded byte container. A validated indexed frame can expand into
canonical row-major RGBA8 bytes and an sRGB `CGImage`; the decoder's pixel
ceiling bounds that temporary allocation below 256 KiB. Alpha remains
non-premultiplied, the image disables interpolation, and the conversion maps
the protocol's explicit R/G/B/A byte order independently of CPU endianness.

`TinyArcadeIndexed2DView` owns the minimal UIKit policy shared by classic
pixel games: aspect-fit layout, clipping and nearest-neighbour magnification
and minification. It does not choose an app layout, frame clock or compositing
scheme. A custom Metal host can still consume the same validated palette and
index plane without using the convenience, and no Apple framework object or
GPU command enters the standard WASM cartridge ABI.

Evidence on 2026-08-21:

- The Swift smoke drives standard core/native-import cartridges through the
  real runtime, verifies exact red and translucent-green RGBA bytes, creates a
  2 × 1 non-interpolated sRGB image backed by those bytes, and presents then
  clears it through the public UIKit view. The same path accepts a full
  classic 320 × 200 / 256-color frame and averages 0.266 ms across 120 native
  presentations, below its 16 ms smoke ceiling.
- Generic iOS-device and universal simulator package builds compile the same
  public source under Swift 6. A booted iPhone 17 Pro simulator executes the
  renderer assertions before the existing 600-frame Depth Well lifecycle
  (0.111 ms average, 0.121 ms p95, 0.129 ms maximum). The optimized linked
  smokes remain below the 1 MiB consumer gate at 834,936 bytes for arm64 and
  872,960 bytes for x86_64.
- A production 2D cartridge and physical-iPhone/TestFlight display evidence
  remain open, so the overall goal is not complete.

## Seventeenth executable increment — second real cartridge

Paddle Guard is an original one-screen paddle game and the first complete
`indexed2d/v1` cartridge. It uses procedural geometry, palette, digit glyphs,
fixed-point physics and tones; it copies no commercial name, image, level,
sound or other asset. Left/right move the shield and primary launches or
restarts. The guest owns a five-by-eight panel field, angle-changing rebounds,
three lives, score, level speed, clear/reset and game-over state.

The 5,280-byte artifact is a strict standard WASM MVP module with no native
capabilities. Its eight imports are ordinary `tinyarcade:core/v1` functions,
including indexed-media negotiation. It emits one 19,248-byte 160 × 120 frame,
generic impact/success/failure tone intent and a 64-byte guest snapshot. A
shared compiler profile now builds both Rust-authored cartridges and performs
the same bulk-memory lowering and path remapping before converter validation;
moving Depth Well onto it preserves the exact 6,076-byte bundled artifact.

Evidence on 2026-08-21:

- Six public black-box tests prove launch/restart, clock-driven movement, a tracked
  shield rebound, unattended life loss, final-panel clear and level rebuild,
  converter acceptance, and byte-identical frame/audio replay through a fresh
  resumed instance. The rare full-field rebuild passes the same 500,000-step
  production ceiling rather than only testing cheap steady-state ticks. Two
  independent cartridge builds are byte-identical and contain no checkout path.
- The complete all-target/all-feature TinyVM suite passes 174 tests. Clippy
  with warnings denied, no-default library compilation and the 70,904-byte
  static-core/self-test gate are clean.
- The generic Swift 6 package builds for iOS device and universal simulator.
  On a booted iPhone 17 Pro simulator, Paddle Guard runs 600 complete
  WASM/copy/decode/CGImage/UIKit frames, crosses suspend into a fresh instance,
  emits gameplay feedback and measures 0.184 ms average, 0.206 ms p95 and
  0.398 ms maximum. Linked smokes remain below 1 MiB at 835,048 bytes arm64
  and 881,256 bytes x86_64.
- A physical iPhone is not connected to this Mac mini, so physical-device and
  TestFlight play evidence remain open and the overall goal is not complete.

## Eighteenth executable increment — bounded native tone playback

The generic tone stream now bounds scheduled work rather than only encoded
bytes: one batch contains at most 16 sequential events and at most 4,000 ms of
total requested duration. Rust and Swift reject the same count, per-event and
aggregate-duration violations before any native presentation. Kinds remain
stable impact/success/failure intent; waveform and timbre remain host choices,
so cartridges and converters do not depend on an Apple implementation.

The iOS SDK now supplies a bounded 22,050 Hz mono PCM/WAV synthesizer and a
main-actor `AVAudioPlayer` lifecycle owner. Its default `.ambient` plus
`.mixWithOthers` session respects the silent switch and other audio; apps with
a central audio coordinator can opt out of SDK session mutation. New batches
replace old feedback, interruption stops without stale replay, and leaving the
game surface has an explicit stop/deactivate path. Haptics remain app policy.

Evidence on 2026-08-21:

- Rust media black boxes accept the exact 16-event/4,000 ms boundary and reject
  a seventeenth event or 4,001 ms aggregate duration. The complete 174-test
  suite, all-feature/all-target Clippy with warnings denied, no-default library
  compile and 70,904-byte static-core/self-test gate are clean.
- The generic Swift package builds for iOS device and universal simulator. On
  a booted iPhone 17 Pro simulator, Paddle Guard's real launch event produces a
  valid WAV, enters `AVAudioPlayer`, crosses interruption and deactivates before
  its 600-frame gameplay run (0.201 ms average, 0.240 ms p95, 4.850 ms maximum).
  Linked smokes remain below 1 MiB at 861,992 bytes arm64 and 899,464 bytes
  x86_64.
- Physical-iPhone speaker, silent-switch and interruption behavior remain open,
  so native I/O and the overall runtime goal remain partial.

## Nineteenth executable increment — iOS verified cartridge storage

ABI v1.4 closes the gap between Rust's signed object cache and an iOS catalog
client. A distinct single-thread-owned cache handle accepts a directory and
per-object byte ceiling, verifies complete cartridge bytes, atomically selects
the current generation, revalidates active/rollback objects under live trust,
and returns only the newly verified bytes through a two-stage copy. Every new
load clears a previous retained result before work begins, so a failed refresh
cannot expose stale executable bytes.

The matching main-actor `TinyArcadeCartridgeCacheV1` gives apps explicit
activate, load, rollback and idempotent close operations. It intentionally owns
no URLSession, download or guest network surface: the app bounds transport and
hands over only a complete response. Private imports remain a separate origin
and never acquire reviewed provenance by entering this store.

Evidence on 2026-08-21:

- The C black box creates a real cache, atomically activates a valid signed
  cartridge, reloads byte-identical WASM through the public copy protocol and
  rejects cross-thread access. After live content revocation, loading returns a
  trust error and the old copied result is no longer available.
- The C header compiles every new symbol and the Swift 6 wrapper builds for
  generic iOS device plus universal simulator. A booted iPhone 17 Pro simulator
  creates the real directory through Swift and proves a cartridge with an
  absent trust key cannot become active. The existing Paddle Guard run remains
  below frame budget at 0.192 ms average, 0.241 ms p95 and 0.992 ms maximum.
- The complete 174-test suite, all-feature/all-target Clippy with warnings
  denied, no-default library compile and 70,904-byte static-core/self-test gate
  are clean. Cache inclusion keeps linked smokes below 1 MiB at 953,032 bytes
  arm64 and 1,012,688 bytes x86_64; the x86_64 margin is now only 35,888 bytes
  and must remain an explicit constraint on later bridge growth.
- Official catalog transport, metadata/deep links and physical-iPhone storage
  remain open, so cartridge ownership, distribution and the overall goal are
  still partial.

## Twentieth executable increment — bounded lobby catalog metadata

`TinyArcadeCatalogV1` defines the converter/site-to-app discovery boundary for
an official lobby. A UTF-8 JSON document is capped at 1 MiB and 256 unique game
IDs. Each row bounds default/localized display text, validates the detached
signed-entry fields, resolves exactly one same-origin HTTPS
`{name}-{version}.wasm` filename and carries no executable authority. The app
may choose a smaller positive cartridge ceiling than the SDK's 8 MiB default.

The selection link `tinyarcade://game/<game-id>` accepts exactly one path
component and no user info, port, query or fragment. It resolves only an
already-decoded row and performs no transport, cache or runtime operation.
Private imports cannot become catalog rows or shareable reviewed links through
this format. Display JSON remains untrusted discovery data: complete downloaded
bytes still require the signed-entry and verified-cache path from increment 19.

Evidence on 2026-08-21:

- The Swift smoke decodes a localized Paddle Guard row using the intended
  `https://partnernetsoftware.com/wasm/` layout, reconstructs its exact signed
  entry, resolves locale fallback and round-trips a selection-only deep link.
  It rejects a traversal filename, a non-ASCII digest without trapping and a
  deep link carrying an auto-run query.
- The bounded JSON/Foundation path increases the exercised linked consumer from
  953,032 to 1,060,872 bytes on arm64 and from 1,012,688 to 1,119,728 bytes on
  x86_64. The honest whole-consumer gate is therefore 1.25 MiB; the separately
  measured interpreter static core remains under its 100 KiB contract.
- The complete 174-test suite, Swift warnings-as-errors compilation,
  all-feature/all-target Clippy with warnings denied, no-default library compile
  and exact 70,904-byte static-core/self-test gate are clean.
- A live hosted/signed catalog, bounded HTTPS client, public per-game universal
  links, moderation/commerce/age metadata, Apple permission and physical-device
  evidence remain open. Distribution and the overall goal are not complete.

## Twenty-first executable increment — bounded app-owned HTTPS

`TinyArcadeHTTPSClientV1` closes the network gap between official discovery and
the verified cache without adding guest network authority. It issues ephemeral
HTTPS GETs, requests identity encoding, clamps timeout to 5...120 seconds,
rejects redirects and non-200 responses, and accepts only the catalog or WASM
MIME types. Declared length is checked before body acceptance, every delegate
chunk is checked against remaining capacity, and final cartridge length must
equal the signed catalog record before `Data` returns.

The client bounds global ownership as well as individual bytes: 1...4 active
requests (default 2), 0...64 queued waiters (default 16), with the active limit
also applied per host. Queue saturation returns `requestQueueFull`. Task
cancellation cancels an in-flight URLSession task or removes a queued waiter;
all completion paths resume exactly once. Transport never activates the cache
or opens a runtime, so HTTPS success cannot create reviewed provenance.

Evidence on 2026-08-21:

- An in-process URLProtocol black box streams a valid catalog and exact 5,280
  byte cartridge, rejects an oversized declared response before body buffering,
  rejects an undeclared oversized body while chunks arrive, rejects a cartridge
  shorter than its signed entry, rejects cross-origin catalog configuration,
  wrong MIME and redirect, and proves in-flight Task cancellation.
- Six concurrent requests through a limit-two client observe an exact peak of
  two. A separate one-active/zero-queue client rejects its second request with
  `requestQueueFull` while the first is visibly in flight.
- The Swift 6 source builds with warnings as errors for generic iOS device and
  universal simulator. On a booted iPhone 17 Pro simulator all transport cases
  pass before the existing 600-frame Paddle Guard run (0.195 ms average, 0.246
  ms p95, 0.973 ms maximum). The fully exercised linked smokes are 1,220,072
  bytes arm64 and 1,280,608 bytes x86_64, within the 1.25 MiB consumer gate;
  x86_64 has 30,112 bytes of remaining headroom.
- The complete 174-test suite, Swift warnings-as-errors compilation,
  all-feature/all-target Clippy with warnings denied, no-default library compile
  and exact 70,904-byte static-core/self-test gate remain clean.
- Live-server TLS/status/MIME evidence, hosted signed metadata, public universal
  links, Apple permission and physical-iPhone behavior remain open. The runtime
  goal is not complete.

## Twenty-second executable increment — deterministic offline publication

The feature-gated `tinyvm catalog build` operator command accepts strict source
metadata, standard `.wasm` cartridges and one raw offline Ed25519 seed. Identity,
version and ABI/state compatibility are derived only from the embedded manifest.
Every cartridge passes module/import validation plus init/tick/media and
byte-deterministic suspend/resume replay before the publisher signs its exact
length and SHA-256. The newly signed record is verified with the derived public
key before any output can be promoted.

Games are sorted by `game_id`; filenames, lowercase hashes, canonical base64 and
JSON formatting are reproducible. The destination must not exist. Work occurs in
a private sibling staging directory and becomes visible with one rename; failure
removes staging. The seed must be an exact 32-byte regular file and, on Unix,
must have no group/other permission bits. It is never emitted or logged. This
catalog key is independent of Apple APNs credentials.

Evidence on 2026-08-21:

- A real compiler-produced Paddle Guard cartridge publishes twice to
  byte-identical catalogs and objects. The generated row derives
  `com.partnernet.paddle-guard`, version `0.1.0`, exact length/hash and a
  decodable 64-byte signature from the cartridge rather than source metadata.
- The black box confirms no raw seed bytes occur in the catalog and an invalid
  source leaves no visible destination. The publisher's own trust store
  re-verifies each signature, object hash/length and embedded manifest.
- The source/output contract is
  [`docs/tinyarcade-catalog-publisher-v1.md`](../docs/tinyarcade-catalog-publisher-v1.md).
  Live hosting, Apple permission and physical-device evidence remain open, so
  official catalog ownership and the overall runtime goal remain partial.

## Twenty-third executable increment — reviewed install transaction

`TinyArcadeReviewedLibraryV1` closes the app-integration gap between discovery,
transport, trust, runtime compatibility and cache activation. One main-actor
transaction downloads the exact selected object, checks cancellation, opens it
as an `officialReviewed` runtime with the current native registry, checks
cancellation again, and only then activates the verified cache. If runtime
preflight or activation fails, no new generation becomes active and any
preflight handle is closed. A single in-flight flag closes Swift actor
reentrancy while URLSession is awaited; parallel installation fails typed rather
than racing two selections.

Evidence on 2026-08-21:

- A booted iPhone 17 Pro simulator fetches a dynamically Ed25519-signed real
  Paddle Guard over the bounded URLProtocol transport, opens it with reviewed
  origin, atomically activates it, renders a 160×120 frame and reopens the
  cached generation under live trust.
- Cancelling an in-flight cartridge request leaves no active cache record; a
  concurrent install receives `operationInProgress`. A valid signed cartridge
  opened with an impossible memory ceiling fails preflight and also leaves no
  active record. Changed downloaded bytes fail trust without replacing the good
  generation, and a later live content revocation rejects cached reopen.
- Swift 6 warnings-as-errors builds remain clean for generic iOS device and the
  universal simulator. The linked consumer is 1,229,256 bytes arm64 and
  1,289,744 bytes x86_64, still below 1.25 MiB. Physical-iPhone lifecycle,
  live-server hosting and Apple permission remain open, so the goal is partial.

## Twenty-fourth executable increment — recoverable scene persistence

`TinyArcadeSnapshotStoreV1` converts the runtime snapshot primitive into an iOS
session owner suitable for backgrounding and process termination. One bounded
binary envelope per canonical game id stores the host-owned game clock and
snapshot under a versioned header and CRC-32. The embedded snapshot remains the
authority for game identity, ABI and state-schema compatibility. The store uses
atomic file replacement, excludes its directory from backup, applies
complete-until-first-authentication file protection and rejects symlinks,
non-regular objects and files outside the configured byte ceiling.

`openSession` never resumes into the runtime ultimately used for a fallback. If
decode or guest resume fails, it closes that candidate, removes the invalid
save and creates a second fresh runtime, returning `discardedInvalid` with clock
zero. This prevents a resume-side failure latch or partially restored guest from
poisoning the playable fallback.

Evidence on 2026-08-21:

- A booted iPhone 17 Pro simulator writes two Paddle Guard generations,
  restores the latest clock and guest state, then proves a changed byte and an
  oversized regular file are discarded into a playable fresh runtime.
- A symlink at the expected per-game path is refused without following it. The
  Swift 6 wrapper and separate snapshot black box build for the generic device
  and arm64 simulator.
- The complete linked consumer grows to 1,289,976 bytes arm64 and 1,337,544
  bytes x86_64. Its honest ceiling is now 1.375 MiB; the interpreter static core
  remains independently gated at 100 KiB and measures 70,904 bytes. Physical
  device background/termination behavior remains open, so the goal is partial.

## Twenty-fifth executable increment — deterministic replay exchange

The feature-gated replay owner turns a real cartridge session into a canonical,
bounded `.tareplay` artifact. It stores no executable code or rendered payload:
the exact cartridge SHA-256, manifest identity, initial portable snapshot,
monotonic button/clock inputs and each render/audio length plus SHA-256 are
enough for the runtime to regenerate and compare complete outputs. Replay
execution verifies the supplied `.wasm` binding internally before it mutates a
runtime, so a caller cannot substitute different bytes with the same manifest.

The v1 decoder proves checked total length before allocation and caps the trace
at 8 MiB, its snapshot at 1 MiB, its steps at 65,536 and each media result at
the ordinary platform ceilings. The CLI records through private core-only
policy and publishes a new trace without overwrite. The Rust API remains
compatible with reviewed runtimes containing future versioned native imports;
the caller must provide the same registered signatures and deterministic native
behavior rather than receiving capability from the trace.

Evidence on 2026-08-21:

- Depth Well and Paddle Guard each record, encode, decode, re-encode and replay
  four real inputs. Together they cover grid3d, indexed2d, movement, rotation,
  hard drop and non-empty tone output.
- Checked-in input plans have stable expected encoded length and SHA-256. A CLI
  black box records the Depth Well vector twice byte-identically, checks all
  four regenerated frames and refuses to overwrite an existing trace.
- Backward clocks, unknown inputs, declared allocation abuse, changed cartridge
  bytes and a changed output digest fail closed. The normative format is
  [`docs/tinyarcade-replay-v1.md`](../docs/tinyarcade-replay-v1.md).
- All 179 package tests, all-feature/all-target Clippy, replay feature isolation,
  no-default library check, exact 70,904-byte static-core gate and iOS
  device/universal-simulator Swift link pass. Linked consumers remain 1,290,216
  bytes arm64 and 1,337,544 bytes x86_64 under the 1.375 MiB ceiling.
- Physical-iPhone execution, live hosted catalog and Apple permission remain
  open, so the overall runtime goal remains partial.

## Twenty-sixth executable increment — iOS replay ownership

C ABI v1.5 and `TinyArcadeRuntimeV1` now own replay recording on the same
single-thread runtime that owns gameplay. The loaded runtime retains the
SHA-256 of its exact construction bytes, closing both a Swift ownership gap and
the earlier core hazard where a caller could pair supplied bytes with a
different runtime carrying the same manifest. Begin captures current state;
ordinary tick records; finish exposes one bounded trace through the existing
two-stage copy pattern; cancel discards trace data without rewinding play.

Verification restores and consumes a candidate runtime, so the Swift contract
explicitly directs apps to a disposable fresh runtime when preserving the live
scene matters. Recording excludes suspend/resume and verification until finish
or cancel. All operations inherit runtime owner-thread enforcement, caught
panic cleanup and the v1 replay allocation/media ceilings. The trace remains
data, grants no native capability and works with any already constructed
bundled, private or reviewed runtime including registered native imports.

Evidence on 2026-08-21:

- A booted iPhone 17 Pro simulator records four real Paddle Guard inputs through
  ordinary `tickMedia`, atomically writes/reads 529 replay bytes, verifies four
  steps on a fresh runtime and reproduces the trace byte-identically.
- The same linked Swift black box rejects a changed digest, duplicate lifecycle
  operations and different WASM bytes with the same manifest. Rust C tests also
  reject cross-thread replay calls and a trace beyond the 8 MiB ceiling.
- A separate Rust black box records and verifies a cartridge importing
  `fan:physics/v1.step`; all eight record/replay callbacks execute through the
  exact registry, while constructing the same cartridge without that registry
  still fails closed before replay.
- All 181 package tests, all-feature/all-target Clippy, replay feature isolation,
  no-default library and exact 70,904-byte static core pass. Generic iOS device
  and universal simulator packages link; ordinary consumers measure 1,311,560
  bytes arm64 and 1,353,416 bytes x86_64, while the replay consumer is
  1,159,864 bytes arm64, all below 1.375 MiB.
- Physical-iPhone lifecycle, live hosting and Apple permission are still open,
  so the overall runtime goal remains partial.

## Twenty-seventh executable increment — private iOS cartridge library

`TinyArcadePrivateLibraryV1` makes explicit user import a bounded local
lifecycle instead of leaving apps to persist arbitrary `Data`. Complete bytes
must first instantiate under the core-only private runtime; only then does the
main-actor owner atomically install the exact module at canonical
`game-id@version.wasm`. The directory is excluded from backup, receives iOS
data protection and holds at most 256 cartridges of at most 2 MiB each.

Enumeration does not execute guest code, but it rechecks canonical identity,
size and regular-file ownership. Open performs those checks again and then
revalidates the loaded manifest identity. Invalid updates cannot replace known
good bytes; corrupt and oversized replacements fail closed; live and dangling
symlinks are never followed. Remove is scoped to an item produced by the same
canonical library. This owner deliberately has no network, catalog, signing,
native-module or public-upload authority.

The compatibility rule is now explicit: the cartridge is standard Wasm and
the platform contract is its versioned manifest, standard import table and
bounded lifecycle records. `tinyarcade:core/v1` is stable; future native
modules advance independently under canonical `authority:module/vN` names.
This lets creator converters statically report capability requirements without
depending on tinyvm internals or turning a declaration into native-code
authority.

Evidence on 2026-08-21:

- A booted iPhone 17 Pro simulator imports real Paddle Guard and Depth Well,
  preserves an installed cartridge across a rejected invalid update, performs
  an atomic same-version update, enumerates deterministically, opens the exact
  private origin, runs a real indexed frame and removes both objects.
- The same black box rejects corrupt and oversized stored bytes plus live and
  dangling symlinks, enforces the 256-cartridge ceiling on import, then proves
  a valid re-import repairs the canonical slot.
- Generic iOS device and universal simulator packages link. Ordinary consumers
  measure 1,342,232 bytes arm64 and 1,395,952 bytes x86_64; replay and private
  library consumers measure 1,191,368 and 1,192,896 bytes arm64 respectively,
  all below the 1.375 MiB linked-consumer ceiling.
- Signing has a valid Apple Development identity, but no physical iPhone is
  attached. The proposed `/wasm/` catalog, catalog JSON and AASA URLs currently
  return 404 and no deployment source was present in the local workspace.
  Physical-device evidence and live hosted distribution therefore remain open.

## Twenty-eighth executable increment — static cartridge compatibility descriptor

Compatibility inspection now has one implementation boundary rather than four
informal interpretations. Rust `CartridgeDescriptor::inspect` parses at most
2 MiB, validates the canonical manifest, standard Wasm module, lifecycle export
signatures, exact core imports, canonical native names/i32 arities, duplicate
rules and manifest/import equality. It does not instantiate the module, run its
start/init functions or require native modules to be registered. Runtime open
reuses the same structural validator and performs registry availability as a
later independent gate; `tinyvm cartridge inspect` also consumes this shared
descriptor.

C ABI v1.7 exposes the result through a stateless two-stage copy as bounded
canonical TAD1 data. The format carries exact inspected byte length, identity,
ABI/state versions, declared native capability namespaces and every function
import's module, field, class and i32 arity. TAD1 is host-side metadata, never a
replacement or wrapper for the standard `.wasm` cartridge. Swift decodes all
lengths, counts, UTF-8, class tags, reserved fields and trailing bytes before
exposing `TinyArcadeCartridgeDescriptorV1`.

`TinyArcadePrivateLibraryV1` now inspects first. A structurally valid fan
cartridge that declares native modules receives the precise typed
`unsupportedNativeCapabilities` result before core-only preflight, while an
official reviewed open may later match the same exact descriptor against
app-compiled registrations. Inspection grants no provenance, catalog trust or
native authority.

Evidence on 2026-08-21:

- Public Rust black boxes inspect a native-importing cartridge without a
  registry, recover exact identity and `fan:physics/v1.step_world (i32,i32) ->
  i32`, then prove private runtime opening still rejects the unavailable
  capability.
- The C black box proves both stages of TAD1 copying, exact inspected byte
  length, native namespace/field presence and malformed-Wasm failure. The C
  header compile gate includes the ABI v1.7 symbols.
- A booted iPhone 17 Pro simulator decodes the same native cartridge through
  Swift, verifies every descriptor field and receives the typed private-import
  rejection before the already-proven native-registered runtime executes it.
- Generic device and universal simulator packages link. Ordinary consumers
  measure 1,370,280 bytes arm64 and 1,435,720 bytes x86_64; replay and private
  consumers measure 1,235,256 and 1,236,912 bytes arm64, all below the existing
  1.375 MiB linked-consumer ceiling. Physical-device and live distribution
  evidence remain open.

## Twenty-ninth executable increment — foreground game session ownership

The ordinary runtime now enforces the deterministic host-input contract that
previously existed only in replay validation. A tick with any bit outside the
nine ABI v1 buttons or a clock below the preceding successful tick fails before
guest execution. It neither latches the cartridge nor advances remembered
time, so a corrected same/later-clock call remains playable. Successful resume
starts a new validation epoch: the portable runtime snapshot deliberately does
not own app time, while the iOS snapshot envelope restores its associated
clock.

`TinyArcadeInputStateV1` accepts complete pressed sets from at most 32 stable
source ids and publishes their union. Touch, keyboard and controller sources
may therefore overlap without one source's release clearing a button still held
by another. Unknown button bits and a thirty-third live source fail without
changing aggregate state.

`TinyArcadeGameSessionV1` owns that input state, one runtime and the monotonic
foreground game clock on the main actor. Each requested delta is capped at
250 ms by default under a configurable 1...1000 ms maximum; background-sized
deltas and `UInt32` exhaustion fail before runtime mutation. Clock state commits
only after a successful decoded media frame. The session saves the exact last
successful clock with `TinyArcadeSnapshotStoreV1` and closes its runtime
explicitly. On scene deactivation the app must release all inputs, save and stop
ticking; stopped sessions do not progress. Snapshot storage now also rejects
dangling symlinks rather than mistaking them for an absent save.

Evidence on 2026-08-21:

- Public Rust and C black boxes tick a real ABI cartridge, reject unknown bits
  and backwards clocks before guest execution, prove the runtime is not failed,
  then successfully tick again at the same valid clock.
- A booted iPhone 17 Pro simulator combines overlapping primary/right sources,
  launches and moves real Paddle Guard, persists clock 16, restores it into a
  fresh runtime, advances to 32, persists again and verifies the second restore.
- The same Swift black box rejects an unknown bit, a thirty-third input source,
  a 251 ms frame delta, clock overflow and use after close. Corrected inputs and
  ticks remain playable. The snapshot black box rejects both live and dangling
  symlinks.
- The complete package has 185 tests; Clippy, replay isolation, no-default and
  the exact 70,904-byte static core remain required. Generic device/universal
  simulator builds link. Consumers measure 1,412,984 bytes arm64, 1,470,008
  bytes x86_64, 1,277,928 replay, 1,279,600 private-library and 1,278,672
  game-session bytes arm64. The honest complete-SDK gate is now 1.5 MiB; the
  interpreter core keeps its independent 100 KiB hard gate.
- No physical iPhone is attached, so real touch/controller, background
  termination, speaker and frame pacing remain open device evidence. The
  overall goal remains partial.

## Thirtieth executable increment — canonical converter manifest authoring

Converters can now begin with a manifest-free standard WebAssembly module from
any producer. `CartridgeManifest::append_to_wasm` validates canonical identity,
versions and sorted versioned capability namespaces, preserves every producer
byte as the output prefix, appends one ordinary custom section reproducibly and
refuses to rewrite an existing manifest.

`tinyvm cartridge attach-manifest` makes that encoder safe to use as a build
step. It parses the input as standard WASM, derives sorted unique native
capabilities exclusively from non-core function imports, appends the manifest,
then runs the complete static descriptor before publishing once through an
atomic no-overwrite path. Authors cannot accidentally maintain a conflicting
second capability list, and a declaration still grants no native authority.

Evidence on 2026-08-21:

- A public Rust black box removes the manifest from a standard game module with
  two native namespaces, authors it twice to byte-identical output, proves all
  original bytes are unchanged, recovers the exact descriptor and rejects both
  a second manifest and noncanonical capability ordering.
- A CLI black box derives `fan:audio/v2,fan:physics/v1` despite reverse import
  order, publishes one inspectable standard `.wasm`, refuses overwrite without
  changing it, refuses an already manifested input and emits no artifact for an
  ordinary WASM module missing the game lifecycle.
- All 187 package tests plus one doctest pass. All-feature/all-target Clippy,
  no-default compile, replay isolation, document redaction and the exact
  70,904-byte static-core/self-test gate are clean.
- A booted iPhone 17 Pro simulator re-proves both real cartridges and every
  reviewed/private/snapshot/replay/session flow. Generic device and universal
  simulator packages link; ordinary consumers measure 1,412,840 bytes arm64
  and 1,469,856 bytes x86_64, with replay/private/session consumers at
  1,277,784, 1,279,440 and 1,278,512 bytes arm64. Physical-device evidence
  remains open.

## Thirty-first executable increment — foreground pacing and scene state

`TinyArcadeFramePacerV1` turns an app-supplied monotonic seconds timestamp into
bounded integer frame deltas while retaining fractional milliseconds across
samples. Its first sample and first sample after reset emit zero. NaN/infinity,
backwards time and more than the configured 1...1000 ms ceiling fail without
changing the accepted baseline. The adapter gives app integrations one reviewed
path for monotonic display timestamps and makes background discontinuities fail
loudly; the app remains responsible for never deriving samples from wall clock.

`TinyArcadeGameSessionV1` now owns explicit active/inactive state.
`deactivateAndSave(to:)` releases all input and becomes inactive before asking
the guest and store to persist; later input/tick calls fail even if storage
fails. Runtime/suspend errors latch the session as failed, while storage-only
errors leave the runtime healthy. `activate()` clears input again and permits
ticks only after the app resets its frame pacer. The SDK deliberately does not
observe scene notifications: the app remains the lifecycle authority.

Evidence on 2026-08-21:

- The real Paddle Guard Swift black box accumulates exact 15/16/15 ms deltas
  from binary-exact fractional timestamps, rejects non-finite, backwards and
  background-sized samples without baseline mutation, and resets to a zero
  first foreground delta.
- The same booted iPhone 17 Pro simulator run deactivates with overlapping held
  inputs, proves inactive input/tick refusal, restores clock 15, advances to 31,
  reactivates at zero delta and restores 31 again. An unsafe snapshot target
  produces a storage error without failing gameplay; a closed runtime during
  save marks the session failed.
- Device and universal simulator packages link. Consumers measure 1,417,768
  bytes arm64, 1,478,928 bytes x86_64, 1,282,712 replay, 1,284,400 private and
  1,283,536 session bytes arm64, all below the 1.5 MiB SDK gate. Physical-device
  pacing/background evidence remains open.
- All 187 package tests plus one doctest, all-feature/all-target Clippy,
  no-default compile, replay isolation, document redaction and the exact
  70,904-byte static-core/self-test gate remain clean.

## Thirty-second executable increment — App Store external-code release gate

Apple's App Review Guidelines dated 2026-06-08 still make self-contained apps
the 2.5.2 baseline and expressly name HTML5/JavaScript mini games, streaming,
chatbots, plug-ins and downloadable games for retro console/PC emulators under
4.7. A custom TinyArcade WASM language is not expressly allowed; the Mini Apps
Partner Program says another language requires Apple approval, and 4.7.2 also
requires prior permission before exposing native platform APIs.

`TinyArcadeDistributionPolicyV1.appStoreBundledOnly` is therefore the default
for every Swift private/reviewed runtime and library initializer. It rejects
before directory creation, network composition, trust checks or guest
execution. An external path requires
`appleApprovedExternalCartridges(approvalReference:)`; the bounded reference is
an auditable release assertion, not technical proof of permission. SDK smokes
use an internal test-only policy that package consumers cannot select. Bundled
runtime construction stays unchanged.

The future creator contract remains deliberately independent of that release
switch. Cartridges are standard `.wasm`; app-native modules are reviewed,
app-compiled host implementations reached only through exact versioned standard
imports. A converter targets a machine-readable host profile rather than tinyvm
internals. A fan upload intended only for the same user's app remains a
private-user transport/install and cannot become a public or official-reviewed
listing by URL or metadata; it stays disabled until the external-code approval
gate is legitimately opened.

Evidence on 2026-08-21:

- The generic-device/universal-simulator Swift warnings-as-errors gate proves
  the new default across direct private opens, private libraries and reviewed
  libraries. A booted iPhone 17 Pro simulator rejects all three bundled-only
  attempts before external work, rejects a malformed approval reference,
  records a bounded approval reference and exercises the existing external
  trust/private flows only through the internal SDK test policy.
- Read-only inspection found the private `mgttt/PartnerNET.Software` GitHub
  Pages source, correcting the earlier local-workspace search. The production
  homepage returns HTTP 200 while `/wasm/`, `catalog-v1.json` and AASA remain
  HTTP 404. No unsigned placeholder catalog was published: choosing and backing
  up the offline catalog trust root plus obtaining Apple permission are release
  authority gates, not defaults for an engineering agent.
- All 187 package tests plus one doctest, all-feature/all-target Clippy,
  no-default and replay-only feature checks, document redaction and the exact
  70,904-byte static-core/self-test gate pass. Generic device and universal
  simulator packages link; ordinary consumers measure 1,442,584 bytes arm64
  and 1,495,376 bytes x86_64, with replay/private/session consumers at
  1,290,600, 1,292,304 and 1,291,424 bytes arm64. Physical-device and Apple
  approval evidence remain open.

## Thirty-third executable increment — exact app-build host profile

TAH1 is a deterministic, callback-free compatibility artifact for one exact
app build. It records game/core/media versions, cartridge and runtime resource
ceilings, plus every app-compiled native module's canonical namespace, field,
i32 signature and per-lifecycle call quota. Native implementations, executable
code, catalog authority and install permission are deliberately absent.

`HostProfileV1`, `NativeModuleRegistry::host_profile`, the converter CLI, C ABI
v1.7 and `TinyArcadeHostProfileV1` share one encoder and static checker. A fan
converter can now reject a standard cartridge with an unavailable or
signature-mismatched native import before upload without instantiating the
guest or calling app code. TAH1 also advertises fuel/output ceilings while
honestly leaving those dynamic behaviors to converter and reviewed-game runs.
The normative bytes and authority boundary are in
[`docs/tinyarcade-host-profile-v1.md`](../docs/tinyarcade-host-profile-v1.md).

Evidence on 2026-08-21:

- Rust round-trips byte-identical TAH1, accepts an exact native import and
  rejects missing/wrong signatures and trailing data. CLI black boxes publish
  a core-only profile without overwrite, inspect it, accept a core cartridge
  and reject a native cartridge without executing it. Tight declared memory,
  duplicate/noncanonical functions and trailing data also fail closed.
- C and Swift export the exact app config/native table, reconsume those bytes
  for static cartridge inspection and prove the registered callback remains
  uncalled. A booted iPhone 17 Pro simulator reconsumes the Swift-produced
  profile before every existing real-game/catalog/storage/replay/session flow.
- All 190 package tests plus one doctest, all-feature/all-target Clippy,
  no-default and replay-only checks, document redaction and the exact
  70,904-byte static-core/self-test gate pass. Device and universal simulator
  packages link; ordinary consumers measure 1,467,784 bytes arm64 and
  1,531,648 bytes x86_64, with replay/private/session consumers at 1,315,608,
  1,317,296 and 1,316,432 bytes arm64. Physical-device, live-hosting and Apple
  approval evidence remain open.

## Thirty-fourth executable increment — catalog-bound host profile discovery

The offline catalog source now requires one canonical TAH1 artifact. The
publisher statically checks every standard cartridge against that exact App
profile before signing, stages the bytes as `host-profile-v1.tahost`, and emits
their bounded length and lowercase SHA-256 at the catalog root. A failed
profile decode or incompatible cartridge leaves no publication directory.

Catalog profile metadata deliberately has discovery authority only. Swift
accepts old catalogs without it, strictly resolves the fixed same-origin
filename when present, downloads under exact length/MIME limits, and then
requires the remote bytes to equal the canonical profile generated from the
local App build. Changing both a catalog profile and its self-reported digest
cannot grant an unavailable native module or larger runtime budget.

Evidence on 2026-08-21:

- The publisher black box proves reproducible profile bytes/length/hash and
  atomic refusal when a cartridge exceeds the supplied profile.
- A dedicated booted iPhone 17 Pro simulator consumer proves traversal
  rejection, bounded HTTPS profile fetch, exact local-profile acceptance and
  same-length mismatch rejection. The existing consumer also proves an older
  catalog without `host_profile` remains readable.
- Generic device and universal simulator packages link. Ordinary consumers
  measure 1,499,384 bytes arm64 and 1,567,128 bytes x86_64; the dedicated
  profile-catalog consumer is 1,372,360 bytes arm64. All remain below the
  unchanged 1.5 MiB SDK gate. Physical-device, live-hosting and Apple approval
  evidence remain open.
- All 190 package tests plus one doctest, all-feature/all-target Clippy,
  default-command, no-default and replay-only checks, document redaction and
  the exact 70,904-byte static-core/self-test gate are clean.

## Thirty-fifth executable increment — bounded untrusted module decoding

The standard WASM loader now owns one 262,144-record complexity budget across
section entries, function types, locals, decoded instructions, element indices
and branch-table targets. Guest counts are charged before reservation,
allocation-amplifying vectors use fallible allocation, and parsed
function/local buffers move into the runtime instead of being cloned. This
closes the gap where a sub-40-byte module could request a multi-billion-entry
allocation before its missing first entry was noticed.

The same load gate now enforces the WebAssembly 1.0 section envelope: standard
sections are unique and ordered, unknown standard ids fail, and every supported
section consumes its exact payload. Custom sections remain freely interleaved,
so the canonical TinyArcade manifest and producer metadata stay ordinary WASM.
No private opcode or wrapper format was introduced.

Evidence on 2026-08-21:

- Public untrusted-byte tests send `u32::MAX` `br_table` and element counts plus
  an over-budget local count through the shipped `eval` API. Each tiny input
  returns `module decode budget`; the same cases pass in a child process rather
  than exiting through allocator `SIGABRT`.
- The public envelope black box rejects duplicate/out-of-order sections,
  trailing section payload and an unknown standard id while accepting custom
  sections before and after an ordinary type section.
- Both compiler-produced Depth Well and Paddle Guard still pass converter and
  gameplay/suspend-resume black boxes under the same strict loader.
- All 193 package tests plus one doctest and all-feature/all-target Clippy pass.
  Device and universal simulator packages link; ordinary consumers measure
  1,500,184 bytes arm64 and 1,567,736 bytes x86_64, while profile-catalog,
  replay, private and session consumers remain below 1.38 MiB. The stripped
  static core is 71,064 bytes with self-test 42, below its unchanged 100 KiB
  hard gate.

## Thirty-sixth executable increment — development-only WebKit differential

A macOS black-box gate now runs the exact same standard cartridge in two
independent engines. tinyvm records a canonical TAR1 trace containing the
cartridge hash, initial portable snapshot, host RNG, monotonic input/clock and
per-frame output evidence. A standalone Swift runner then uses the system
JavaScriptCore WebAssembly implementation, supplies the same frozen
`tinyarcade:core/v1` host semantics, and compares every render/audio length and
SHA-256. It does not reuse tinyvm's decoder or executor.

The reference adapter intentionally has no DOM, canvas, networking or product
UI. It lives under tests, links only into a temporary macOS oracle, and does not
enter the iOS XCFramework or Swift package. This catches interpreter or ABI
drift without turning nostalgia-arcade into an H5/mini-app platform. tinyvm
remains the product runtime and JavaScriptCore remains test evidence only.

Evidence on 2026-08-21:

- Compiler-produced Depth Well and Paddle Guard each match JavaScriptCore for
  four exact replay frames, covering grid3d, indexed2d and tones outputs.
- The public Cargo integration test compiles the Swift oracle with warnings as
  errors, independently verifies the tinyvm trace, then runs both cartridges.
- The test uses the checked-in input plans and generated `.wasm` artifacts; no
  fixture-only guest, H5 page or JavaScript runtime is linked into the app.
- All 194 package tests plus one doctest, all-feature/all-target Clippy, default,
  no-default and replay-only checks, full-repository formatting and document
  redaction pass. The production static core is unchanged at 71,064 bytes with
  self-test 42.

## Thirty-seventh executable increment — deterministic execution telemetry

tinyvm now treats deterministic resource consumption as a first-class Wasm VM
result rather than a simulator log. Every persistent instance retains the
instruction count of its last completed top-level invocation plus current
memory pages and table elements. `GameRuntime` binds that engine evidence to
the completed init/tick/suspend/resume attempt and adds native dispatches and
render/audio/state byte counts. Successful calls and guest traps update the
record; invalid host input rejected before execution does not rewrite it.

C ABI v1.8 exposes one fixed-layout, allocation-free stats record that remains
queryable after a guest latches failed. The Swift main-actor owner validates
the record and returns a typed lifecycle value. Wall time, resident memory,
thermal state and scheduling remain separate device-owned measurements, so a
deterministic replay/converter can compare fuel high-water marks without
claiming milliseconds are portable.

The crate's public identity is also corrected to match the architecture that
now exists: tinyvm is an owned, bounded, cross-platform standard WebAssembly
VM. The early compact `Vm`/`Instr` face remains a compatibility/test API; it
does not define cartridges or the application platform.

Evidence on 2026-08-21:

- Public Rust black boxes prove two identical standard modules report identical
  init/tick stats and bind exact instruction, memory/table and media evidence.
  Suspend/resume state bytes and a quota-trapped native lifecycle are covered.
- C header layout is fixed at 40 bytes; the C owner proves query after tick and
  suspend. Swift 6 device/simulator builds consume the same v1.8 record.
- On the booted iPhone 17 Pro simulator, 600 release frames peak at 13,150
  steps/17 pages for Depth Well and 37,864 steps/17 pages for Paddle Guard.
  Their p95 values are 0.105 ms and 0.257 ms. Every frame's stats agree with
  copied render/audio lengths and configured fuel/page ceilings.
- The complete bridge flow passes with linked consumers of 1,506,184 bytes
  arm64 and 1,581,856 bytes x86_64. The shipping arm64 gate remains 1.5 MiB;
  the simulator-only x86_64 slice has a separate 1.5625 MiB ceiling.
- All 195 package tests plus one doctest, default/all-feature/no-default/replay
  gates, all-target Clippy, formatting, ShellCheck and document redaction pass.
  The stripped static core remains 71,064 bytes with self-test 42.

## Thirty-eighth executable increment — standard bulk memory, not MVP lowering

The real Rust cartridges exposed the next architecture boundary: both contain
standard `memory.copy`/`memory.fill`, but the old publisher lowered those
instructions into MVP loops. tinyvm now owns these standard operations directly
and the shared compiler profile preserves them. This advances an ordinary Wasm
VM rather than growing a game-specific bytecode dialect.

The decoder accepts canonical 0xfc subopcodes 10/11 and only memory index zero;
the validator requires the three i32 operands. Execution implements overlap-safe
copy and low-byte fill. It checks every range and charges one deterministic fuel
unit per 16 bytes before mutation, so an out-of-bounds or fuel trap cannot leave
partial memory changes. DataCount section id 12 is parsed at its spec-defined
position before code and must match the data section; numeric section-id sorting
is no longer incorrectly used as the Wasm ordering rule.

The v1 profile deliberately does not claim the whole bulk-memory proposal:
passive data, `memory.init`/`data.drop` and bulk table operations remain rejected
until their per-instance state and resource contracts are implemented. Standard
features graduate one coherent profile at a time, with unsupported features
failing at load rather than being silently reinterpreted.

Evidence on 2026-08-21:

- Depth Well retains two `memory.copy` and two `memory.fill` instructions;
  Paddle Guard retains one copy and two fills. Both compiler artifacts load and
  pass the ordinary converter/runtime paths without a fixture-only decoder.
- JavaScriptCore and tinyvm match all four exact replay frames for each game
  after the lowering removal, covering grid3d, indexed2d and tones.
- 200 package tests plus one doctest pass across the exercised all-feature
  matrix; default/no-default/replay checks, all-target Clippy, formatting,
  ShellCheck and diff hygiene pass. The new PRD leaf has a public black-box
  integration test, not only an internal decoder test.
- Device/simulator Swift linkage remains below its gates at 1,506,552 bytes
  arm64 and 1,581,976 bytes x86_64. The isolated stripped static core is 87,640
  bytes, below 100 KiB, and its C self-test returns 42.

## Thirty-ninth executable increment — complete bulk-memory segment lifecycle

tinyvm now implements the remainder of the standard bulk-memory proposal that
fits its single-memory, MVP-funcref profile: passive data and element segments,
`memory.init`, `data.drop`, `table.init`, `elem.drop` and `table.copy`. Data and
element definitions remain immutable module data; live/dropped flags, memory
and the funcref table are independent per instance. Consequently a drop or
table mutation in one game instance cannot leak into another instance created
from the same cartridge.

The load gate parses data flags 0/1/2 and index-encoded funcref element flags
0/1/2/3, requires DataCount for data-segment instructions, checks all segment
and function indices, and rejects reference-typed element encodings until the
reference-types proposal is owned. Active/declarative segments are empty after
instantiation. A dropped passive segment permits only the standard zero-length
read at source offset zero.

All init/copy operations preflight source and destination ranges and fuel before
mutation. Memory work costs one deterministic step per 16 bytes; table work
costs one per funcref. Segment-state and table copies use fallible reservation,
so guest-selected segment/table counts cannot turn instantiation into an
allocator abort.

Evidence on 2026-08-21:

- A checked-in standards WAT fixture is compiled by WABT and accepted by
  `wasm-validate`; the exact output runs in both tinyvm and system
  JavaScriptCore and returns 143. It covers passive data and funcref elements,
  all five newly added instructions and `call_indirect`.
- Public black-box tests prove instance isolation, drop semantics, overlap-safe
  table copy and both memory/table fuel atomicity. Invalid DataCount, missing
  segment indices, unsupported memory/table indices and reference-typed segment
  encodings fail loudly.
- The all-feature matrix passes 207 executed package tests plus one doctest;
  the independent WABT/JSC oracle also passes explicitly. Existing Depth Well
  and Paddle Guard JSC replay differentials remain byte-exact.
- Device/simulator Swift linkage remains below its gates at 1,525,160 bytes
  arm64 and 1,591,448 bytes x86_64. The isolated stripped static core is 87,656
  bytes, below 100 KiB, and its C self-test returns 42.

## Fortieth executable increment — standard scalar proposal profile

tinyvm now decodes, validates and executes the completed WebAssembly
sign-extension and non-trapping float-to-integer conversion proposals. This
adds all five `i32`/`i64` sign-extension instructions and all eight
`trunc_sat` conversions. Saturating conversion follows standard NaN, infinity,
signed/unsigned clamp and truncation semantics without turning an out-of-range
input into a trap.

This increment also fixes the architectural rule in the product tree: tinyvm
is developed as a de facto, standards-first WebAssembly VM for cross-platform
extensible applications. TinyArcade v1 is a bounded accepted feature profile
and versioned host ABI, not the VM's permanent capability ceiling. New guest
execution features remain standard Wasm; platform capabilities remain
versioned standard imports rather than private opcodes.

Evidence on 2026-08-21:

- WABT compiles and `wasm-validate` accepts one checked-in WAT fixture covering
  every one of the 13 instructions. The exact generated bytes execute in both
  tinyvm and system JavaScriptCore and return 143.
- The shared Rust cartridge profile enables both proposals. The real optimized
  Depth Well cartridge retains three `i32.extend8_s` instructions and passes
  converter, lifecycle, snapshot and replay tests; Paddle Guard remains valid.
- Public black-box tests cover all five sign extensions and all eight
  saturating conversions, including NaN, infinities, negative unsigned input,
  truncation and integer limits. PRD shipped leaves are mapped to those tests.
- 209 package tests plus one doctest pass under all features; both explicit
  WABT/JavaScriptCore proposal oracles, no-default/replay checks, all-target
  Clippy, formatting, ShellCheck and the two-game WebKit replay differential
  pass.
- Device/simulator Swift linkage remains below its gates at 1,525,560 bytes
  arm64 and 1,591,760 bytes x86_64. The isolated stripped static core remains
  87,656 bytes, below 100 KiB, and its C self-test returns 42.

## Forty-first executable increment — standard multi-value control flow

tinyvm now implements the standard WebAssembly multi-value proposal end to
end. Function bodies and calls validate and return complete heterogeneous
result vectors. Structured `block`, `loop` and `if` decode their standard s33
block type, including non-negative function type indices, parameters and
multiple results. Blocks/ifs branch with results; loop back-edges branch with
parameters; `br`, `br_if`, `br_table`, `return`, explicit else and the implicit
identity else all use the full value-type vector.

The s33 decoder accepts valid sign-extended encodings and distinguishes inline
negative value types from positive type indices; type index 64 therefore uses
`c0 00`. It rejects overlong and incorrectly sign-extended values. Validation
control frames store constant-size views into the already decode-budgeted type
section rather than clone a signature per nesting level, so a large signature
combined with deep control nesting cannot amplify validation memory
quadratically.

Evidence on 2026-08-21:

- WABT compiles and `wasm-validate` accepts a checked-in fixture containing a
  multi-result function, parameterized block/loop/if, loop back-edge values,
  implicit else identity and multi-value `br_if`/`br_table`. The exact bytes
  return 143 in WABT's interpreter, tinyvm and system JavaScriptCore.
- Public black-box tests load a standard heterogeneous two-result export,
  reject a body missing its second declared result and a result-bearing start
  function, execute type index 64 and a valid non-canonical negative s33
  encoding, and reject invalid high bits.
- The shared Rust/Binaryen cartridge profile enables multi-value. Both real
  games rebuild and pass converter, lifecycle, replay and snapshot gates; the
  JavaScriptCore differential still matches all four frames of each game.
- 211 package tests plus one doctest pass under all features; all three explicit
  WABT/JavaScriptCore proposal oracles, no-default/replay checks, all-target
  Clippy, formatting, ShellCheck and diff hygiene pass.
- Device/simulator Swift linkage remains below its gates at 1,526,376 bytes
  arm64 and 1,596,720 bytes x86_64. The isolated stripped static core is 87,672
  bytes, below 100 KiB, and its C self-test returns 42.

## Forty-second executable increment — standard single-table funcref profile

tinyvm now carries standard `funcref` values through function signatures,
locals, mutable/immutable globals and typed select. It decodes, validates and
executes `ref.null`, `ref.is_null`, `ref.func`, table get/set/grow/size/fill and
expression element segment encodings 4 through 7. Function references must be
forward-declared by an element segment, as required by the standard. Table
growth observes both the module maximum and host budget; fill/grow charge
deterministic fuel for every affected element before mutation.

This deliberately closes one useful standards slice rather than claiming the
whole reference-types family. `externref`, multiple tables, typed function
references and GC remain outside the accepted profile and fail at load time.
The boundary is a versioned capability profile of the general-purpose Wasm VM,
not a game-specific opcode set; TinyArcade remains only its first host.

Evidence on 2026-08-21:

- WABT compiles and validates a checked-in fixture covering funcref locals and
  globals, typed select, all reference/table instructions, expression element
  lifecycles, table bulk operations and indirect calls. The exact bytes return
  143 in WABT's interpreter, tinyvm and system JavaScriptCore.
- Public black-box tests prove instance-local table state, declared table
  maxima, null/reference behavior, pre-mutation fuel failure, undeclared
  `ref.func` rejection and explicit flag-6 table-zero initialization. They also
  reject `externref` and a nonzero table index.
- The shared Rust/Binaryen cartridge profile now enables reference types. Both
  real games rebuild and pass converter, lifecycle, snapshot and deterministic
  replay gates; JavaScriptCore still matches all four frames of each game.
- 213 package tests plus one doctest pass under all features. No-default and
  replay-only matrices, all four explicit WABT/JavaScriptCore proposal oracles,
  all-target Clippy, formatting, ShellCheck and diff hygiene pass.
- Device/simulator Swift linkage remains below its gates at 1,527,336 bytes
  arm64 and 1,601,456 bytes x86_64. The isolated stripped static core is 87,688
  bytes, below 100 KiB, and its C self-test returns 42.

## Forty-third executable increment — standard multiple defined tables

tinyvm now represents every internally defined `funcref` table independently.
The table index immediates on get/set/grow/size/fill, `call_indirect`,
`table.init`, active element segments and both sides of `table.copy` are decoded,
validated and executed rather than required to be zero. Cross-table copy first
checks both ranges and fuel, then copies without guest-sized temporary storage.

The host's `max_table_elems` is an aggregate limit across all live tables. It
is checked against the sum of declared minima before allocation and again on
growth; execution statistics report the same aggregate. This prevents a module
from bypassing the iOS memory boundary by splitting elements across many small
tables. Table count also consumes the shared decode-complexity budget.

Export validation now owns all MVP export kinds even though the embedding only
offers function lookup: function/table/memory/global indices are bounded,
unknown kinds fail and names must be unique across kinds. Imported tables still
require a real shared host-store ownership API and remain explicitly outside
this increment rather than receiving copy-on-bind semantics that would violate
standard instance sharing.

Evidence on 2026-08-21:

- WABT compiles and validates a checked-in two-table fixture. The exact bytes
  exercise indexed active segments, get/set, cross-table copy/init, growth,
  fill, size, a table export and indirect calls, returning 143 in WABT's
  interpreter, tinyvm and system JavaScriptCore.
- Public black-box tests prove two-table execution, per-table sizes, aggregate
  statistics, initial and dynamic aggregate host caps, invalid instruction and
  export indices, and duplicate names across function/table export kinds.
- Both real Rust games rebuild without byte or replay-hash changes and retain
  exact four-frame JavaScriptCore parity. No-default and replay-only matrices,
  all five proposal oracles, all-target Clippy, formatting and ShellCheck pass.
- 214 package tests plus one doctest pass under all features. Device/simulator
  Swift linkage remains below its gates at 1,530,600 bytes arm64 and 1,607,792
  bytes x86_64. The isolated stripped static core is 87,704 bytes, below
  100 KiB, and its C self-test returns 42.

## Forty-fourth executable increment — standard tail calls and trampoline

tinyvm now decodes, validates and executes the standard tail-call proposal's
`return_call` and `return_call_indirect` instructions. Validation requires the
target result vector to match the current function result vector exactly and
checks the indirect type/table index before a module can instantiate.

Execution returns a typed tail-call outcome to one `call_any` trampoline. A
defined target replaces the current activation rather than recursively entering
Rust, while a host target dispatches through the same versioned import registry
and returns directly. Ordinary `call` and `call_indirect` remain bounded by the
existing call-depth limit; tail chains remain charged to the same deterministic
instruction budget without consuming additional native stack.

This is a general VM capability, not a Depth Well optimization. It moves the
runtime toward a standards-first cross-platform WebAssembly VM usable by future
extensible hosts, while TinyArcade remains the first embedding and conformance
workload. Imported tables are still deferred: sharing their module-local
function indices would be observably wrong until the runtime has store-level
function identity and cross-instance state ownership.

Evidence on 2026-08-21:

- A checked-in standard WAT fixture performs 100,000 direct self tail calls and
  an indexed indirect tail call, returning 143 in WABT's interpreter, tinyvm
  and system JavaScriptCore from the exact same WABT-produced bytes.
- A public black-box test independently executes the deep direct and indirect
  paths, tail-calls an imported host function, and rejects direct/indirect
  result mismatches plus an unknown indirect table index at load time.
- All 215 non-ignored package tests plus one doctest pass under all features;
  no-default and replay-only matrices, all six explicit WABT/JavaScriptCore
  proposal oracles, both real-game replay differentials, all-target Clippy,
  package formatting, ShellCheck and document redaction pass. Device/simulator
  Swift linkage remains below its gates at 1,531,448 bytes arm64 and 1,612,248
  bytes x86_64. The isolated stripped static core is 87,720 bytes, below
  100 KiB, and its C self-test returns 42. The owning tests are rerun after the
  mandatory main pull before push.

## Forty-fifth executable increment — VM-owned call activations

All guest-defined calls now execute through one explicit activation machine.
`call` and `call_indirect` suspend a caller in a fallibly grown VM vector;
defined returns resume that caller and append results, host calls pass through
the existing typed import door, and tail calls replace the current activation.
No guest call instruction recursively enters Rust or consumes the iOS native
stack.

The former debug/release depth split is gone. Both profiles now accept at most
512 nested defined-call levels and return `Trap("call depth")` at the same exact
boundary. A second aggregate ceiling admits at most 1,048,576 live locals,
operand values and control frames across the current function plus every
suspended caller. The runtime checks that ceiling before allocating a new wide
activation and grows the activation/caller vectors fallibly, so a legal large
locals declaration multiplied by recursion becomes a typed `call stack` trap
rather than an allocator abort.

This is an interpreter architecture invariant, not a game-specific behavior.
It makes ordinary recursion, indirect dispatch, tail calls and versioned native
imports share one bounded cross-platform execution model suitable for small
iOS thread stacks and future non-game Wasm hosts.

Evidence on 2026-08-21:

- Public black-box tests execute 512 levels of both direct and indirect
  non-tail recursion in a debug build and unwind to the exact result 42. The
  next direct level traps deterministically at the documented boundary.
- A separate wide-locals recursion consumes the maximum standard decode-item
  scale and traps on aggregate activation slots before allocating its next
  frame.
- All 217 non-ignored package tests plus one doctest pass under all features;
  no-default and replay-only matrices, both real-game replay differentials, all
  six WABT/JavaScriptCore proposal oracles, all-target Clippy, package
  formatting, ShellCheck and document redaction pass. Device/simulator Swift
  linkage stays below its gates at 1,548,856 bytes arm64 and 1,612,512 bytes
  x86_64. The stripped static core remains 87,720 bytes, below 100 KiB, and its
  C self-test returns 42. Owning tests are rerun after the mandatory main pull
  before push.

## Forty-sixth executable increment — host-owned call resources and ABI evidence

Call containment is now an embedding policy rather than a pair of interpreter
constants. `Limits` owns the maximum simultaneously live guest-defined
activations and aggregate live locals/operand/control slots. The explicit
activation machine enforces both at exact boundaries for direct and indirect
standard calls, while tail calls continue to replace the current activation.
Every persistent instance records the highest admitted call depth and aggregate
slot use for its last top-level invocation, including one that traps.

TAH1 schema 2 publishes both ceilings in its canonical 64-byte header. The
decoder still accepts a canonical schema-1/56-byte artifact and maps its absent
fields to the historical 512/1,048,576 defaults. C ABI v1.9 appends the same
fields to the runtime configuration, but reads the original 40-byte v1.8 prefix
before considering the extension. A separately sized 48-byte execution-stats
V2 record exposes both peaks; the original 40-byte V1 output and function remain
unchanged. Swift owns both configuration fields and the typed V2 query.

This makes the architectural direction explicit: tinyvm is a standards-first,
cross-platform WebAssembly VM whose first embedding is TinyArcade, not a game
script format. App/game facilities remain standard `.wasm` plus explicit,
versioned host imports; no game-specific private bytecode enters the engine.

Evidence on 2026-08-21:

- Public black boxes set smaller host call limits and prove success at the exact
  boundary, deterministic `call depth`/`call stack` traps on the next admitted
  work, and peak telemetry that never reports a rejected transient activation.
- TAH1 schema 2 round-trips custom limits; a hand-built legacy schema-1 profile
  and a real 40-byte C configuration prefix both retain historical defaults.
  Header smoke fixes configuration/V1/V2 layouts at 48/40/48 bytes and Swift 6
  reads V2 stats for every measured Depth Well and Paddle Guard frame.
- All 219 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all-target Clippy, package formatting,
  ShellCheck, all six explicit WABT/JavaScriptCore proposal oracles and the
  four-frame real-game WebKit differential pass. Device/simulator Swift linkage
  remains below its gates at 1,553,000 bytes arm64 and 1,624,568 bytes x86_64.
  The stripped static core remains 87,720 bytes and its C self-test returns 42.
  Physical-device and Apple-review evidence remain open.

## Forty-seventh executable increment — fallible execution-stack growth

Guest execution no longer relies on hidden infallible allocations at the
remaining stack and value-transfer boundaries. Instructions that grow the
operand or control stack preflight the host-owned live-slot ceiling and reserve
fallibly before mutating guest state. Defined/host calls, tail calls, public
invoke conversion, fresh global state and function-result extraction allocate
their complete destination before removing values from the source stack.
Branch-result preservation now copies within the existing operand allocation.

Decoded `br_table` target lists live in one flat immutable arena per function;
an instruction carries only its arena range and default label. Validation and
execution borrow that range, so a loop cannot clone a guest-sized vector and
decode does not perform a secondary heap allocation for every table. The
static-core measurement now asks the platform linker to apply the same
dead-code elimination used by release consumers before stripping; the 100 KiB
threshold and executable selftest remain unchanged.

These are general VM containment rules. They do not introduce a TinyArcade
opcode or game-engine boundary: tinyvm remains the standards-first,
cross-platform WebAssembly VM, while games and future extensible applications
remain versioned host embeddings over ordinary `.wasm` imports.

Evidence on 2026-08-21:

- Public black boxes prove exact host-slot failure for operand and control
  growth before the rejected value/frame appears, and a unit test proves
  branch-result preservation reuses the operand vector allocation.
- All 221 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all six explicit WABT/JavaScriptCore proposal
  oracles, the two-game four-frame WebKit differential, all-target Clippy,
  formatting, ShellCheck, document redaction and both iOS target checks pass.
- Swift linkage remains below its gates at 1,552,856 bytes arm64 and 1,624,112
  bytes x86_64; catalog/replay/private/session consumers are 1,425,688 /
  1,400,120 / 1,418,304 / 1,417,456 bytes. The release-linked stripped static
  core is 86,328 bytes and its C selftest returns 42.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Forty-eighth executable increment — bounded in-place host dispatch

The TinyArcade embedding no longer asks core imports or iOS C native callbacks
to construct a heap `Vec` for every dispatch. Each accepted game/native import
already has at most 16 i32 parameters and results; the VM now stages both in
fixed stack arrays and gives the callback its exact writable result slice.
Input, clock, RNG, indexed2d negotiation, render/audio submission and
save/load-state all use this in-place door. The C bridge writes directly into
the same bounded result slice. The original Rust returning-callback API remains
an explicit compatibility adapter, while new public `register_in_place*`
methods expose the product behavior to other native embeddings.

For a nested host call, the trampoline checks activation/operand limits and
fallibly reserves the suspended caller stack before entering app code. Bounded
results remain in a 16-i32 inline record and append directly into that reserved
stack, without a temporary heap allocation after the callback has mutated
memory or host state. A top-level host call fallibly reserves its owned result
before dispatch. Game import binding now addresses already-validated import
slots directly, eliminating the former infallible cloned-name collection at
runtime open.

This is a host-door property of the cross-platform Wasm VM and versioned
embedding ABI, not a private game opcode. Standard `.wasm` function imports
remain the cartridge-facing contract.

Evidence on 2026-08-21:

- A VM unit test proves nested bounded host results use the inline variant,
  while a public game-runtime black box passes exact `[20, 22]` parameters and
  one writable result slot through `register_in_place`, then consumes result 42
  in guest Wasm. Existing C callback success/failure/latch tests pass on the
  migrated iOS path.
- All 223 non-ignored package tests pass under all features. The two real games
  retain exact JavaScriptCore replay parity, and the complete booted iPhone 17
  Pro simulator smoke passes reviewed/private ownership, UIKit/CGImage, audio,
  snapshot, replay, session and native-callback flows.
- The final simulator performance pass records Depth Well at 0.119 ms average,
  0.165 ms p95 and 0.338 ms max over 600 frames; Paddle Guard records 0.203 ms
  average, 0.253 ms p95 and 1.069 ms max. These remain simulator regression
  evidence, not physical-device claims.
- Swift linkage remains below its gates at 1,552,744 bytes arm64 and 1,624,488
  bytes x86_64; catalog/replay/private/session consumers are 1,425,576 /
  1,400,008 / 1,418,176 / 1,417,344 bytes. The stripped static interpreter core
  remains 86,328 bytes and its C selftest returns 42.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Forty-ninth executable increment — recyclable frame ownership

The cross-platform embedding now has a reusable return path for bounded Wasm
output. Public `GameRuntime::tick_into` clears caller-owned render/audio
vectors, lends whichever buffer has the greater retained capacity to the host,
and swaps completed bytes back after execution. It also recovers and clears
partially written buffers on a guest failure, while invalid host input clears
stale contents without latching the instance. The original `tick` remains a
source-compatible ownership-returning wrapper.

Replay recording and replay verification use the same reusable path. The iOS C
handle now takes its prior completed frame, passes that storage through the next
ordinary or recorded tick, then restores the completed owner for the existing
two-stage C/Swift copy. No Rust allocation pointer crosses the ABI and no guest
format changes: this is a general host-ownership improvement around standard
Wasm imports, not a game-specific VM instruction.

Evidence on 2026-08-21:

- A public runtime black box proves two equal frames retain the exact render and
  audio allocation pointers/capacities, then proves rejected input empties the
  frame, preserves those capacities and leaves the runtime healthy.
- All 224 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all six WABT/JavaScriptCore proposal oracles,
  the two-game four-frame WebKit differential, all-target Clippy, formatting,
  ShellCheck and document redaction pass.
- The complete booted iPhone 17 Pro simulator path passes reviewed/private
  ownership, UIKit/CGImage, audio, snapshot, replay, session and native callback
  flows. Its 600-frame runs measure Depth Well at 0.123 ms average / 0.132 ms
  p95 / 0.165 ms max and Paddle Guard at 0.205 / 0.262 / 0.997 ms; this remains
  simulator regression evidence, not a physical-device claim.
- Swift linkage remains below its gates at 1,552,664 bytes arm64 and 1,624,416
  bytes x86_64; catalog/replay/private/session consumers are 1,425,512 /
  1,399,944 / 1,418,112 / 1,417,280 bytes. The stripped static interpreter core
  remains 86,328 bytes and its C selftest returns 42.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fiftieth executable increment — typed standard host imports

The generic VM host door now preserves the standard value signature instead of
silently inheriting TinyArcade's narrower i32 profile. Public
`bind_import_typed_in_place` exposes exact borrowed `Val` parameter/result
slices for imports with at most 16 values; `bind_import_typed` remains an
explicit arbitrary-arity allocating compatibility form. Both accept i32, i64,
f32, f64 and funcref, validate arguments before app code and validate results
afterwards. `ValueType` position queries expose the exact signature without
duplicating guest-sized type vectors. Non-null function references must name
this module instance's combined function index space. The old `bind_import` now rejects a non-i32
signature at bind time rather than installing a callback that can only trap.

Bounded typed results use their own inline `[Val; 16]` record, while the
existing TinyArcade i32 path retains its smaller `[i32; 16]` record. Nested
callers reserve their destination before callback dispatch; a top-level typed
call reserves its owned return vector first. This advances tinyvm as a
standards-first cross-platform Wasm VM without changing the frozen i32-only
game cartridge/native ABI.

Evidence on 2026-08-21:

- Public black boxes drive mixed i64/f32/f64 parameters and multi-value results
  through both typed APIs, reject wrong result types, and bound funcref output
  to the current instance. A VM unit test proves nested typed results select
  the inline record and top-level results preserve exact types.
- WABT independently compiles and validates the mixed typed-import fixture;
  tinyvm and JavaScriptCore both return the exact `(4.5, 42, 3.5)` tuple.
- All 227 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all six proposal oracles plus the new typed
  host oracle, the two-game WebKit differential, all-target Clippy, formatting,
  ShellCheck and document redaction pass. The stripped static core remains
  86,328 bytes and its C selftest returns 42.
- The complete booted iPhone 17 Pro simulator path remains green. Its 600-frame
  runs measure Depth Well at 0.111 ms average / 0.121 ms p95 / 0.129 ms max and
  Paddle Guard at 0.203 / 0.256 / 1.005 ms; this is simulator regression
  evidence, not a physical-device claim.
- Swift linkage remains below its gates at 1,553,848 bytes arm64 and 1,625,560
  bytes x86_64; catalog/replay/private/session consumers are 1,426,696 /
  1,401,112 / 1,419,312 / 1,418,448 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fifty-first executable increment — strict declared-memory semantics

The standard byte loader no longer grants an undeclared implicit linear
memory. A parsed module with no memory section owns zero pages and exposes an
empty slice to host callbacks. Loads, stores, `memory.size`, `memory.grow`,
`memory.copy`, `memory.fill` and `memory.init` are rejected during decoding;
even a zero-length active data segment is invalid because it still names memory
zero. Passive data remains valid without memory. The programmatic
`Module::new` compatibility builder retains its historical one-page test
convenience, but that default cannot cross the standard binary load boundary.

This correction also repaired the independent MVP golden generator: every
memory case now emits a real standard memory section instead of accidentally
depending on tinyvm's former lenience. It reinforces the architectural rule
that tinyvm is a standards-first cross-platform Wasm VM; game cartridges are
one embedding and cannot redefine core module validity.

Evidence on 2026-08-21:

- The public load gate rejects scalar, size/grow and bulk-memory instructions
  without a declared memory, rejects an empty active segment, and accepts both
  a zero-memory pure-compute module and passive-data-only module with an empty
  live memory view. WABT independently rejects the same undeclared
  `memory.size` boundary.
- The mixed typed-import black box proves that a host callback for a module
  without memory receives an empty slice rather than a synthetic 64 KiB page.
- The regenerated independent MVP memory goldens all declare one standard page
  and continue to cover the same opcode/result facts.
- All 228 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all seven WABT/JavaScriptCore proposal/host
  oracles, the two-game WebKit differential, all-target Clippy, formatting,
  ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,554,008 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,856 / 1,401,288 /
  1,419,456 / 1,418,608 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fifty-second executable increment — strict scalar memarg alignment

Every scalar load/store now validates the standard memarg alignment exponent
against that instruction's natural byte width while decoding. Under-alignment
remains legal and has ordinary unaligned scalar semantics; over-alignment is a
load-time `Decode` failure and never reaches execution. This covers all 23 MVP
load/store opcodes across i32, i64, f32 and f64, including narrow integer
accesses. The runtime still ignores a valid alignment hint during execution,
which is permitted; it no longer confuses that implementation choice with
permission to accept an invalid module.

The stripped-core consumer now invokes the already parsed known export by
function index. Export parsing and public name lookup remain covered by their
own black boxes, while the static size root retains the interpreter rather than
an optional map-lookup facade. This keeps the unchanged 100 KiB product gate
honest after adding strict validation.

Evidence on 2026-08-21:

- A decoder matrix accepts the exact natural exponent and rejects natural + 1
  for every one of the 23 scalar memory opcodes.
- The public byte load gate rejects over-aligned 8-bit, 32-bit and 64-bit loads
  plus a 64-bit store before producing an invokable module.
- WABT independently rejects the same over-aligned `i32.load` module with its
  natural-alignment validation error.
- All 230 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all seven WABT/JavaScriptCore proposal/host
  oracles, the two-game WebKit differential, all-target Clippy, formatting,
  relevant ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,554,168 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,427,016 / 1,401,432 /
  1,419,632 / 1,418,768 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fifty-third executable increment — canonical function expressions

The decoder now owns the outer grammar boundary of every standard function
expression. A function-level `end` must consume the final code-body byte; any
following instruction is rejected instead of being treated as code outside the
function's expression. An `if` may install exactly one `else`; a second one is
rejected rather than overwriting the first branch target and accidentally
passing balanced-stack validation. Both failures occur at the byte load gate,
before a `Module` or invokable instance exists.

Evidence on 2026-08-21:

- Public raw-byte black boxes reject a sized code body containing `end; nop`
  and a balanced `if` containing two `else` opcodes with exact decoder errors.
- The common rejection suite proves the same bytes cannot fall through to a
  run-time trap or produce an invokable module.
- WABT independently rejects both malformed binaries at its expression parser.
- All 231 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all seven WABT/JavaScriptCore proposal/host
  oracles, the two-game WebKit differential, all-target Clippy, formatting,
  relevant ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,554,168 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,427,016 / 1,417,944 /
  1,419,632 / 1,418,768 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fifty-fourth executable increment — strict i64 signed-LEB range

The standard byte loader now validates the unused payload bits in the tenth
byte of every signed 64-bit LEB immediate. Positive and negative encodings
outside the `i64` range fail before a module or instance exists; the exact
minimum and maximum encodings remain legal. The compact check occurs before
the final native shift, so host integer truncation can no longer turn malformed
WebAssembly into an apparently valid `i64.const`.

Evidence on 2026-08-21:

- Public raw-byte black boxes reject both overflow signs with the same typed
  decoder failure and accept/run the exact `i64::MIN/MAX` boundary modules.
- WABT independently rejects both overflowing binaries and accepts both legal
  boundaries.
- All 232 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all seven WABT/JavaScriptCore proposal/host
  oracles, the two-game WebKit differential, all-target Clippy, formatting,
  relevant ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,554,168 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,427,016 / 1,417,960 /
  1,419,632 / 1,418,768 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fifty-fifth executable increment — valid custom-section names

The standard loader no longer treats an entire custom-section payload as
unstructured bytes. It first validates the mandatory length-prefixed UTF-8
name, then leaves every remaining payload byte opaque. Missing names, truncated
name lengths and invalid UTF-8 now fail before a module exists; valid custom
metadata remains repeatable and ignorable. The common borrowed name parser
avoids allocating a `String` for metadata the VM does not retain.

Evidence on 2026-08-21:

- Public raw-byte black boxes reject empty, truncated and invalid-UTF-8 custom
  section names, while a named section with arbitrary opaque payload loads and
  the following standard function still executes.
- WABT independently rejects all three malformed binaries and accepts the
  legal opaque-payload counterpart.
- All 233 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all seven WABT/JavaScriptCore proposal/host
  oracles, the two-game WebKit differential, all-target Clippy, formatting,
  relevant ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,554,008 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,856 / 1,417,800 /
  1,419,456 / 1,418,608 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.
