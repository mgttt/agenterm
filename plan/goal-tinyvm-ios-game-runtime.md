# Goal — tinyvm as an iOS game runtime foundation

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Outcome: an iOS arcade app can load a reviewed standard `.wasm` game, create
one bounded persistent instance, drive it frame by frame through an owned
native ABI, suspend/resume it, and fail one bad game without hanging or
corrupting the app.

Legend: `[x]` proven · `[~]` partial · `[ ]` required

```text
tinyvm iOS game runtime
├── execution kernel                 [x]
│   ├── WASM 1.0 validation/opcodes   [x]
│   ├── persistent instance           [x]
│   ├── start exactly once            [x]
│   ├── per-call instruction budget   [x]
│   ├── VM-owned call activations      [x]
│   ├── host memory/table budgets     [x]
│   ├── decode complexity budget      [x]
│   ├── single-table funcref profile  [x]
│   ├── multiple defined tables       [x]
│   ├── multiple internally defined memories [x]
│   ├── extended constant expressions [x]
│   ├── standard imported globals         [x]
│   ├── named standard resource exports [x]
│   ├── standard imported linear memories [x]
│   ├── standard imported funcref tables [x]
│   ├── linked exported globals/memories/tables [x]
│   ├── linked exported functions    [x]
│   │   ├── numeric value signatures [x]
│   │   ├── store-owned funcref values [x]
│   │   ├── opaque externref function/global values [x]
│   │   └── standard externref tables [x]
│   ├── standard tail calls            [x]
│   ├── typed standard host imports     [x]
│   ├── strict declared-memory semantics [x]
│   ├── strict scalar memarg alignment [x]
│   ├── canonical function expressions [x]
│   ├── strict i64 signed-LEB range    [x]
│   ├── valid custom-section names     [x]
│   ├── empty memory-section vector    [x]
│   ├── mutable global.set target      [x]
│   ├── WABT-valid golden corpus       [x]
│   ├── WABT load-gate oracle          [x]
│   ├── static module validation CLI    [x]
│   ├── deterministic execution stats [x]
│   └── trap isolation                [x]
├── game host ABI                    [x]
│   ├── standard WASM cartridge       [x]
│   ├── version negotiation           [x]
│   ├── lifecycle init/tick/suspend   [x]
│   ├── input snapshot                [x]
│   ├── bounded render commands       [x]
│   ├── bounded audio commands        [x]
│   ├── recyclable frame buffers      [x]
│   ├── clock/RNG determinism         [x]
│   ├── native capability registry    [x]
│   │   └── atomic resource-table factory [x]
│   ├── bounded in-place host dispatch [x]
│   ├── domain + generation native resource handles [x]
│   ├── native-resource snapshot quiescence [x]
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
│   ├── freestanding C authoring       [x]
│   │   └── header-only core v1 declarations [x]
│   ├── app-build host profile        [x]
│   ├── deterministic catalog publisher [x]
│   └── no public arbitrary execution [~]
├── iOS native bridge                 [~]
│   ├── stable C lifecycle ABI        [x]
│   ├── static library/XCFramework   [x]
│   ├── Swift ownership/threading     [x]
│   ├── input + monotonic clock owner [x]
│   ├── frame pacing + scene state    [x]
│   ├── stable two-pass copy lengths  [x]
│   ├── indexed 2D presentation       [x]
│   ├── device + simulator build      [x]
│   ├── real app target/package link  [x]
│   │   └── current-main consumer gate [x]
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

## Queued runtime research — QJWasm / WA2X

After the store-owned funcref increment is complete, review the QJWasm paper
and its published implementation together with the WA2X runtime. This is a
source-level engineering task, not a name-only related-work citation:

- locate the authoritative repositories and pin the exact revisions evaluated;
- audit licenses and provenance before reusing any code or derived structure;
- map QJWasm's cross-runtime ownership graph, reference-count handoff and
  zero/low-copy argument representation against `WasmStore`, linked resources
  and the TinyArcade native capability registry;
- trace its request, callback and Promise/result channels, including queue
  bounds, cancellation, re-entrancy, ordering and event-loop wakeups;
- reproduce the relevant memory and boundary-call benchmarks, separating Wasm
  execution time from scheduling, serialization and large-argument copying;
- record an adopt/adapt/reject decision for each mechanism and add executable
  tinyvm benchmarks before changing the runtime architecture.

QuickJS, a JS event loop and WA2X's AOT execution are not product dependencies:
downloaded cartridges remain standard `.wasm` interpreted by tinyvm on iOS.
The reusable target is ownership, bounded communication and measurement design.
Paper: [QJWasm, Journal of Systems Architecture 179 (2026)](https://www.sciencedirect.com/science/article/pii/S1383762126002444).

The first pinned-source pass is recorded in
[`docs/qjwasm-wa2x-source-review.md`](../docs/qjwasm-wa2x-source-review.md).
It accepts the ownership, no-copy memory, wakeup-coalescing and measurement
lessons; it rejects the current unbounded/unsafe threading implementation and
introduced a decomposed tinyvm/JavaScriptCore boundary benchmark rather than
repeating the paper's aggregate headline.

The pinned-source review and decomposed boundary benchmark are complete. A
later experimental queue remains for mechanisms that need an actual tinyvm
prototype before adoption: a bounded callback/result channel, coalesced host
wakeups, and shared-memory views whose owner and invalidation rules are proven
across the C/Swift boundary. Each experiment must compare against the existing
direct call path and must not introduce QuickJS, AOT/JIT, unbounded queues or
QJWasm code whose repository licensing is not yet explicit.

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
implicit/default WASI, guest network access and arbitrary third-party uploads
are excluded from the game profile. A separately enabled, versioned WASI P1
subset may reuse the same platform-neutral host contract for non-game embedders.
The tested public/private JavaScriptCore boundary and capability matrix live in
[`docs/tinyarcade-javascriptcore-boundary.md`](../docs/tinyarcade-javascriptcore-boundary.md).

The cartridge remains an ordinary standards-valid WebAssembly module. The
runtime does not add private opcodes or wrap executable bytes in a proprietary
format. Platform services are standard function imports under versioned module
names: v1 core uses `tinyarcade:core/v1`; future native modules receive their
own canonical `authority:module/vN` namespaces and must be present in a host
capability registry. Function names and i32 signatures remain in the standard
import table, which the converter reports without executing the guest.
This is deliberately a de facto cross-platform WebAssembly VM for extensible
applications; TinyArcade games are its first embedding and validation workload,
not the boundary of the VM. Standard Wasm semantics remain below host-specific
capability profiles so future non-game embedders reuse the same runtime.
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

## Fifty-sixth executable increment — empty memory-section vectors

The loader now preserves the standard distinction between a memory section
and a memory declaration. A present section whose vector count is zero is
legal and leaves the module with zero memory pages, exactly like an absent
section. Pure computation therefore remains executable, while loads, stores,
memory size/grow, bulk-memory operations and active data still fail the load
gate because no memory was declared. Counts above one remain outside the
current single-memory profile.

Evidence on 2026-08-21:

- Public raw-byte black boxes load and run a pure-compute module containing an
  empty memory vector, observe zero pages/bytes after instantiation, and reject
  the counterpart that executes `i32.load` before producing a module.
- WABT independently accepts the pure-compute binary and rejects the memory
  access binary; it also accepts a non-minimal but in-range LEB encoding of the
  zero vector count.
- All 233 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all seven WABT/JavaScriptCore proposal/host
  oracles, the two-game WebKit differential, all-target Clippy, formatting,
  relevant ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,553,928 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,776 / 1,417,720 /
  1,419,376 / 1,418,528 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fifty-seventh executable increment — mutable global.set targets

The standard validation context now retains each global declaration's
mutability as well as its value type. `global.set` targeting an immutable
global fails the load gate instead of producing a module that traps only when
the instruction executes. The validator borrows the module's canonical global
definitions directly, avoiding a second metadata vector; the interpreter keeps
its immutable check as defense for programmatic builder modules that do not
enter through the standard byte loader.

Evidence on 2026-08-21:

- Public raw-byte black boxes reject an immutable i32 target with a typed
  decode failure, while the otherwise identical mutable module loads, writes
  the global and returns the updated value.
- WABT independently rejects the immutable binary and accepts the mutable
  counterpart.
- The old edge golden that mislabeled an invalid standard module as a runtime
  trap was replaced by a legal mutable i32 roundtrip; its generator reproduces
  all 107 edge rows and the six-per-family threshold remains unchanged.
- All 234 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, all seven WABT/JavaScriptCore proposal/host
  oracles, the two-game WebKit differential, all-target Clippy, formatting,
  relevant ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,553,848 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,696 / 1,417,640 /
  1,419,312 / 1,418,448 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fifty-eighth executable increment — WABT-valid golden corpus

The standard test workflow now validates every generated MVP, family-extra and
family-edge module with WABT before tinyvm executes it. Success and expected
runtime-trap rows share the same requirement: their bytes must first be a legal
standard module. The gate streams fixture hex directly to `wasm-validate`,
reports the exact fixture id on disagreement and remains separate from the
proposal execution oracles.

Evidence on 2026-08-21:

- `smoke-wabt-golden-validity.sh` independently validates all 291 current
  golden modules, including every expected runtime trap, and passes ShellCheck.
- The regenerated corpus remains 174 MVP goldens, 10 extra cases and 107 edge
  cases covering all 172 MVP opcodes; no invalid standard module is hidden by
  tinyvm's own decoder or interpreter.
- All 234 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, the new whole-corpus gate, all seven
  WABT/JavaScriptCore proposal/host oracles, the two-game WebKit differential,
  all-target Clippy, formatting, relevant ShellCheck and document redaction
  pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,553,848 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,696 / 1,417,640 /
  1,419,312 / 1,418,448 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Fifty-ninth executable increment — WABT load-gate oracle

Accepted and rejected raw load-gate cases now have a shared independent oracle
fixture. A Rust black box proves that the fixture is an exact byte-for-byte
mirror of the in-test case arrays, including every id and verdict; a separate
WABT smoke then requires the reference validator to agree with all verdicts.
Cargo remains independent of the external tool, while fixture drift or a
one-sided corpus fails with the responsible case id.

Evidence on 2026-08-21:

- `smoke-wabt-load-gate.sh` agrees with tinyvm on all 33 rejected and 11
  accepted raw modules and passes ShellCheck.
- The public integration test rejects malformed fixture rows, proves the exact
  44-row mirror and keeps every case in the common load/eval/non-invokable
  black boxes.
- All 235 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, both whole-corpus WABT gates, all seven
  WABT/JavaScriptCore proposal/host oracles, the two-game WebKit differential,
  all-target Clippy, formatting, relevant ShellCheck and document redaction
  pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,553,848 bytes arm64 and 1,625,560 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,696 / 1,417,640 /
  1,419,312 / 1,418,448 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Sixtieth executable increment — stable two-pass copy lengths

Every Swift query-then-copy consumer now treats the two C ABI calls as one
consistency transaction. Runtime render/audio/snapshot/replay output and cached
cartridge bytes retain the queried length and reject a successful copy whose
returned length differs, so an ABI drift cannot turn an unwritten zero-filled
tail into valid host data.

Evidence on 2026-08-21:

- The native Swift smoke directly exercises the mismatch branch and requires a
  typed decode failure with the responsible copy context; all ordinary runtime
  and cartridge-cache paths continue to pass through the same centralized
  guard.
- All 235 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, both whole-corpus WABT gates, all seven
  WABT/JavaScriptCore proposal/host oracles, the two-game WebKit differential,
  all-target Clippy, formatting, relevant ShellCheck and document redaction
  pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,553,832 bytes arm64 and 1,629,664 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,680 / 1,417,624 /
  1,419,296 / 1,418,432 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Sixty-first executable increment — static standard module validation

The command-line front door now validates an ordinary standard `.wasm` module
without requiring a TinyArcade manifest. It runs the same bounded load gate as
the embedding, reports function-import and start-function metadata, and never
instantiates the module, binds imports or executes guest code. Cartridge ABI,
media and lifecycle conformance remain separate later gates.

Evidence on 2026-08-21:

- `tinyvm module validate` accepts a structurally valid module whose start
  function contains `unreachable`, proving validation does not execute start;
  the paired malformed module fails with a non-empty diagnostic and nonzero
  status.
- The real Depth Well and Paddle Guard artifacts independently pass the plain
  module command with seven and eight standard function imports respectively,
  without consulting their TinyArcade manifests.
- All 236 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, both whole-corpus WABT gates, all seven
  WABT/JavaScriptCore proposal/host oracles, the two-game WebKit differential,
  all-target Clippy, formatting, relevant ShellCheck and document redaction
  pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,553,832 bytes arm64 and 1,629,664 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,680 / 1,417,624 /
  1,419,296 / 1,418,432 bytes.
- Physical-device play, TestFlight and Apple-review evidence remain open; the
  persistent goal therefore remains active.

## Sixty-second executable increment — current real-app consumer gate

The real Nostalgia Arcade main now owns a repeatable cross-repository consumer
gate instead of relying on an old archive or a synthetic Swift executable. It
rebuilds the local XCFramework/package and bundled cartridge from the adjacent
agenterm main, exercises the App target, and inspects the final device product.

Evidence on 2026-08-21:

- Nostalgia Arcade commit `372fa17` refreshes the deterministic bundled Depth
  Well artifact from 6,076 to 6,022 bytes; its current SHA-256 is
  `8ac3292b354fd5c7f1df05e88ba25e7311f501a8041ad58566ba07e53111899e`.
- Commit `6934f70` adds `scripts/test-tinyarcade-consumer.sh`. The gate requires
  all five selected runtime/App unit tests and the one full playable UI journey
  to execute with zero failures; an Xcode success containing zero selected
  tests is rejected.
- The UI journey enters the television catalog, opens the Wasm-backed Depth
  Well, rotates and hard-drops a piece, pauses, returns to the catalog, reopens
  the cartridge and proves the score persisted. A separate simulator install
  and cold launch visibly reached the bilingual memory-room lobby.
- The no-signing Release device build produces an arm64 iOS 17 Mach-O. Its App
  bundle contains exactly one `.wasm`, byte-identical to the checked-in 6,022
  byte cartridge, and has no WebKit or JavaScriptCore dynamic linkage.
- No physical iPhone is connected to this Mac mini. Physical-device lifecycle,
  sound/input/performance, TestFlight and Apple-review evidence remain open, so
  the persistent goal remains active.

## Sixty-third executable increment — current App Store distribution export

The current Nostalgia Arcade main and current tinyvm package now pass the local
distribution step beyond an unsigned device build or development archive. The
existing App Store Connect export configuration produced an IPA without
uploading it or changing the committed version/build number.

Evidence on 2026-08-21:

- Xcode archives version 0.16.4 build 30 for generic iOS with automatic signing;
  strict codesign verification succeeds and the archive carries the exact
  6,022-byte cartridge SHA-256 from the real-app consumer gate.
- `destination=export` obtains a Cloud Managed Apple Distribution identity and
  Store provisioning profile. The exported arm64 App has
  `get-task-allow=false`, `beta-reports-active=true`, passes strict designated-
  requirement verification and keeps the cartridge byte-identical.
- The resulting IPA is structurally valid and contains the arm64 executable
  plus the single bundled `.wasm`; no TestFlight/App Store upload was attempted.
- Build 30 is already a historical upload number, so a future TestFlight build
  must increment it rather than attempting to reuse this local export.
- Physical-device lifecycle, sound/input/performance and Apple-review evidence
  remain open; the persistent goal therefore remains active.

## Sixty-fourth executable increment — native callback reentrancy guard

The iOS C boundary now enforces the aliasing rule that was previously only
implicit in synchronous native dispatch. While guest lifecycle execution owns a
mutable runtime borrow, a callback cannot reenter an API with the same or any
other runtime handle. Rejection happens before raw-pointer dereference, and an
RAII thread-local guard restores the boundary on normal return or unwind. This
is an embedding-safety property around the general Wasm VM; it does not add a
private cartridge format or game-specific execution semantic.

Evidence on 2026-08-21:

- A native cartridge callback attempts to inspect both its active handle and a
  second healthy handle. Both calls return `TINYARCADE_INVALID_ARGUMENT` without
  touching their output parameters; the callback still returns exact results
  and mutates guest memory as intended.
- The outer tick succeeds, clears the rejected nested-call diagnostic, runs a
  second lifecycle successfully, and leaves both runtime instances healthy and
  closable. The C header, Swift wrapper and iOS bridge contract now state the
  enforced rule.
- All 237 non-ignored package tests plus two doctests pass under all features.
  No-default/replay-only checks, both whole-corpus WABT gates, all seven
  WABT/JavaScriptCore proposal/host oracles, the two-game WebKit differential,
  all-target Clippy, formatting, relevant ShellCheck and document redaction
  pass.
- The stripped static core remains below its unchanged 100 KiB gate at 86,344
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,554,024 bytes arm64 and 1,629,920 bytes x86_64;
  catalog/replay/private/session consumers are 1,426,856 / 1,417,800 /
  1,419,472 / 1,418,624 bytes.
- Physical-device lifecycle, sound/input/performance, TestFlight and
  Apple-review evidence remain open; the persistent goal therefore remains
  active.

## Sixty-fifth executable increment — standard multiple defined memories

The general VM now decodes, validates, instantiates and executes the standard
multiple-memory proposal for internally defined memories. Explicit scalar
memargs, `memory.size`/`memory.grow`, active data, `memory.init`/fill and both
same-memory and cross-memory copy retain their standard indices. Each memory
obeys its declared maximum, while the host page ceiling covers the aggregate
instance so splitting pages across definitions cannot bypass it. Imported
memories remain outside the profile until a store-level binding model exists.

TinyArcade v1 deliberately remains a one-memory embedding. Its load gate now
rejects zero or multiple memories before lifecycle binding because its core
callbacks, snapshots and media ranges address memory zero. This product rule
does not constrain ordinary `WasmModule` users or the standards-facing VM.

Evidence on 2026-08-21:

- WABT 1.0.41 compiles, validates and interprets one shared fixture containing
  two memories, indexed active/passive data, all four scalar value families,
  cross-memory copy, fill, size and grow. WABT and tinyvm both return `1225`;
  tinyvm separately proves aggregate initial rejection and growth refusal.
- This Mac's public JavaScriptCore WebAssembly path rejects the same standard
  module because it does not support more than one memory. The development
  oracle records that capability absence; WABT supplies the independent
  proposal oracle instead of lowering the module or limiting tinyvm to JSC.
- The TinyArcade black box rejects a two-memory cartridge through its own
  profile gate while the underlying module remains valid and executable.
- All 238 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, both whole-corpus WABT gates, all eight
  proposal/host oracles, the two-game WebKit differential, all-target Clippy,
  formatting, ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 101,112
  bytes and its C selftest returns 42. Darwin now removes unreferenced external
  Rust symbols from the fully linked measurement executable instead of
  retaining them under the weaker `strip -x` mode.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,554,952 bytes arm64 and 1,629,896 bytes x86_64;
  catalog/replay/private/session consumers are 1,427,800 / 1,418,728 /
  1,420,432 / 1,419,536 bytes.
- The current Nostalgia Arcade main rebuilds against this VM, passes all five
  selected App/runtime tests and its full two-game UI journey, then produces a
  Release arm64 device App with the byte-identical 6,022-byte Depth Well
  cartridge and no WebKit/JavaScriptCore linkage.
- Physical-device lifecycle, sound/input/performance, TestFlight and
  Apple-review evidence remain open; the persistent goal therefore remains
  active.

## Sixty-sixth executable increment — standard extended constant expressions

tinyvm now evaluates the standard extended-const proposal's wrapping
`i32.add/sub/mul` and `i64.add/sub/mul` inside module constant expressions.
The same typed evaluator owns global initializers, active data offsets and
element offsets, so these sites no longer assume a single `*.const` opcode.
Every expression instruction consumes the shared decode complexity budget;
operand underflow, mixed numeric types and any final arity other than one fail
before a Module is produced.

Constant `global.get` remains outside the accepted profile because tinyvm has
not yet implemented imported globals and their store-level identity/binding.
The VM does not substitute a previously defined guest global as a partial host
model. This keeps the current extension standard, independently testable and
separate from the later imported-resource architecture.

Evidence on 2026-08-21:

- WABT 1.0.41 compiles, validates and interprets one shared fixture with all
  six integer operations across global, data and element initializers. WABT,
  tinyvm and public JavaScriptCore all execute the exact bytes to result `199`.
- Public raw-byte tests prove nested execution and load-time rejection for
  stack underflow, i32/i64 mixing, extra expression results and unavailable
  `global.get`.
- All 239 non-ignored package tests plus one doctest pass under all features.
  No-default/replay-only checks, both whole-corpus WABT gates, all nine
  proposal/host oracles, the two-game WebKit differential, all-target Clippy,
  formatting, ShellCheck and document redaction pass.
- The stripped static core remains below its unchanged 100 KiB gate at 101,112
  bytes and its C selftest returns 42.
- The complete iOS device/universal-simulator bridge and Swift consumers link
  below their gates at 1,555,112 bytes arm64 and 1,629,928 bytes x86_64;
  catalog/replay/private/session consumers are 1,427,944 / 1,418,888 /
  1,420,592 / 1,419,712 bytes.
- Current Nostalgia Arcade main passes all five selected App/runtime tests, its
  full two-game UI journey and a Release arm64 device build against this VM;
  the bundled Depth Well cartridge remains byte-identical at 6,022 bytes.
- Physical-device lifecycle, sound/input/performance, TestFlight and
  Apple-review evidence remain open; the persistent goal remains active.

## Sixty-seventh executable increment — standard imported numeric globals

tinyvm now accepts standard i32/i64/f32/f64 global imports and exposes an
explicit host binding object with exact value-type and mutability matching.
Cloned bindings retain store identity: a guest `global.set`, a host update and
sibling instances all observe the same mutable global rather than copied
initial values. An unbound import traps during instantiation.

Constant `global.get` is now accepted where the standard permits it: the target
must be an imported immutable global. Initializers are validated as typed
programs at module load, then evaluated against bound store values during
instantiation. Active data and element offsets use the same deferred evaluator,
including checked bounds arithmetic.

This is a general-engine capability. TinyArcade v1 inspection and runtime
loading reject global imports because its versioned game ABI intentionally
contains function imports only; widening that embedding remains a separate
profile decision.

Evidence on 2026-08-21:

- WABT 1.0.41 compiles and validates one shared fixture; tinyvm and public
  JavaScriptCore execute those exact bytes to the same combined result
  `878897`.
- Public tests prove exact descriptors, binding type/mutability checks,
  immutable-host rejection, mutable sharing across sibling instances and
  rejection of mutable imported globals in constant expressions.
- TinyArcade descriptor inspection and runtime opening both reject standard
  global imports without restricting the general VM.
- The stripped static core is 101,128 bytes and remains below its 100 KiB
  gate. The arm64 Swift consumer is 1,557,704 bytes under its unchanged
  product gate; the simulator-only x86_64 consumer is 1,640,216 bytes under a
  separate one-linker-bucket-adjusted compatibility ceiling.

## Sixty-eighth executable increment — named standard resource exports

tinyvm now retains validated table, memory and global exports instead of
discarding every non-function export after structural validation. The general
embedding can resolve each resource by its standard export name, inspect table
length, read or mutate exported memory and read or type-safely update a mutable
exported global. Immutable and wrong-typed global writes trap.

This completes the export-side vocabulary needed before imported memory/table
store linking: converters and hosts can use ordinary Wasm resource names, with
no tinyvm-only manifest alias or opcode. TinyArcade v1 remains free to use its
existing memory-zero ABI while the general VM grows independently.

Evidence on 2026-08-21:

- WABT compiles and validates a shared module exporting a funcref table,
  memory, mutable i32 global and immutable i64 global.
- tinyvm and public JavaScriptCore both resolve and mutate those exports and
  execute the same bytes to result `76`.
- Public tests cover missing names, active-data visibility, host memory writes,
  table length, mutable global updates and immutable-global rejection.
- All 241 non-ignored package tests plus one doctest pass with all features;
  the full iOS bridge links at 1,557,656 bytes arm64 and 1,640,400 bytes
  x86_64, and the stripped static core remains 101,128 bytes.

## Sixty-ninth executable increment — standard imported linear memories

tinyvm now accepts standard linear-memory imports and binds them to explicit
host-owned store objects. A cloned `WasmMemory` is the same object: guest
writes, host writes, active data initialization and `memory.grow` are visible
to every importing sibling instance. Import matching implements standard
limits subtyping (current size at least the required minimum, and an actual
declared maximum no wider than a required maximum).

Imported memories use a guarded shared store, while VM-defined memories retain
their direct `Vec` fast path. Loads, stores, scalar memory ops, bulk
init/copy/fill, growth and function host callbacks dispatch through that dual
representation without unsafe aliasing. A conflicting host borrow returns a
deterministic trap rather than panicking. If two distinct import indices bind
the same memory, aggregate page accounting counts the object once and
cross-index overlapping `memory.copy` retains memmove semantics.

The public memory accessors return scoped read/write guards, preventing a slice
from outliving or racing a shared memory mutation. Named re-export of an
imported memory resolves to that same object. TinyArcade v1 rejects memory
imports during both static inspection and runtime opening; this remains a
general VM capability rather than an accidental game-ABI expansion.

Evidence on 2026-08-21:

- WABT validates the shared imported-memory fixture; tinyvm and public
  JavaScriptCore run the exact bytes with two sibling instances to result
  `516`, including shared write and grow visibility.
- A WABT-validated two-index alias fixture proves one-page aggregate budgeting,
  borrow-conflict trapping and overlapping cross-index copy result `593` /
  bytes `aabcdf`.
- Public tests cover unbound instantiation, descriptor order, exact limits
  matching, active data, host mutation and TinyArcade profile rejection.
- All 244 non-ignored package tests plus one doctest pass with all features;
  every WABT/WebKit oracle passes. The stripped static core is 101,160 bytes.
  The iOS bridge and Swift package link at 1,576,776 bytes arm64 and 1,650,496
  bytes x86_64; the host-profile catalog, replay, private-library and session
  consumers link at 1,449,640, 1,424,040, 1,442,256 and 1,441,408 bytes. The
  arm64 product ceiling moved by one explicit 16 KiB bucket for imported-memory
  store identity.
- The real Nostalgia Arcade consumer gate passes five runtime/App unit tests,
  one full UI journey and an arm64 device Release build. Its DEBUG reset now
  removes persisted TinyArcade snapshots as well as defaults, so repeated UI
  runs cannot revive a previously ended cartridge session.

## Seventieth executable increment — imported-table decoding and profile boundary

The general VM now decodes standard funcref-table imports, preserves their
independent import descriptors, places them before defined tables in the
combined standard table index space, validates named exports and accounts for
all declared minima in the aggregate host table budget. Instantiation fails
loudly with `unbound imported table` until a host store object is supplied.
TinyArcade v1 rejects table imports during both static inspection and runtime
opening, so this standards work does not silently widen the shipped game ABI.

This increment deliberately does not claim shared imported-table execution.
Unlike memory bytes, a non-null `funcref` is an instance-bound function
reference, not merely a module-local integer index. Correct sibling/cross-module
sharing therefore needs a first-class function-address representation before
the table binding API can preserve standard store identity. The next increment
will build that representation and the binding/execution oracle instead of
mapping shared entries back onto the caller instance incorrectly.

Evidence on 2026-08-21:

- WABT validates a module with imported table zero, defined table one, active
  elements, named exports and `call_indirect`; tinyvm reports the exact import
  descriptor and indices, then rejects unbound instantiation deterministically.
- Non-ignored tests prove descriptor/limit handling, aggregate host budgeting,
  unbound instantiation and TinyArcade inspection/runtime rejection.
- All 246 non-ignored package tests plus one doctest pass with all features.
  The stripped static core is 101,176 bytes with self-test result 42.
  The iOS bridge links at 1,577,080 bytes arm64 and 1,650,656 bytes x86_64;
  the profile-catalog, replay, private-library and session consumers link at
  1,449,928, 1,424,344, 1,442,528 and 1,441,680 bytes.

## Seventy-first executable increment — host table store and function addresses

The general VM now exposes a cloneable host `WasmTable`, binds it with standard
limits subtyping, applies active elements into the shared object and preserves
that object across importing instances. Imported aliases count once against
the aggregate table budget, and overlapping `table.copy` across two indices
bound to the same object retains memmove order. Defined tables keep a direct
vector path; imported tables use guarded shared storage.

Every live non-null table cell now holds a first-class function address with
its originating instance identity. Defined-table behavior remains unchanged,
but a foreign address can no longer be silently reinterpreted as the caller's
module-local function index: it traps as `cross-instance funcref`. That trap is
an intentional intermediate correctness boundary, not the final standard
behavior. Cross-instance dispatch still needs a store-owned callable instance
record so the target function observes its own globals, memories and tables.

Evidence on 2026-08-21:

- A WABT fixture binds an imported table, initializes it with instance-local
  functions and proves host visibility. Re-instantiating over the same table
  replaces the entry with a sibling address; the older instance detects it as
  foreign instead of calling the wrong local state.
- A second WABT fixture binds two import indices to one six-element host table
  under a six-element aggregate budget. Cross-index overlapping copy produces
  the expected indirect-call sum `16`.
- Public JavaScriptCore executes the sibling fixture to result `4`, proving the
  remaining target semantics: after the second instantiation, a call made by
  the first instance dispatches into the second instance's mutable global.
- The iOS bridge links at 1,579,656 bytes arm64 and 1,656,480 bytes x86_64;
  profile-catalog, replay, private-library and session consumers link at
  1,452,504, 1,443,432, 1,445,136 and 1,444,256 bytes. Arm64 remains within its
  existing product ceiling; x86_64 moves by one explicit 16 KiB simulator-only
  compatibility bucket.
- All 246 non-ignored package tests plus one doctest, every WABT/WebKit gate,
  no-default/replay feature checks, Clippy and formatting pass. The stripped
  static core is 101,208 bytes with self-test result 42. The real Nostalgia
  Arcade gate passes five runtime/App tests, one UI journey and the arm64
  device Release build against this runtime.

## Seventy-second executable increment — explicit table address store

The host table API now has an explicit cloneable `WasmStore`. Multiple distinct
tables intended for one module are created through that store; the convenience
`WasmTable::new` creates a fresh one-table store. Every instantiation receives
a monotonically allocated store-local instance id, and every function address
uses that id rather than an `Rc<()>` token. This supplies the stable lookup key
needed by the next cross-instance dispatcher without global mutable state,
pointer identity tricks or unsafe code.

A module with multiple imported tables must bind all of them from the same
store. Two indices may alias one table object and count once, while two distinct
tables from that store count independently. Bindings from different stores
trap before active elements or start execute. Existing defined-table,
single-table convenience and TinyArcade behavior remain unchanged.

Evidence on 2026-08-21:

- The WABT imported-table gate proves one-object aliases, two distinct tables
  in one store, and deterministic rejection of two foreign stores.
- Public standard-extension tests construct and bind through `WasmStore`; all
  existing funcref/multi-table and JavaScriptCore gates remain green.
- All 246 non-ignored package tests plus one doctest, feature checks, Clippy,
  formatting and the real App 5+1/device gate pass. The stripped static core is
  101,192 bytes. iOS links at 1,579,272 bytes arm64 and 1,656,384 bytes x86_64;
  profile-catalog, replay, private-library and session consumers link at
  1,452,120, 1,443,064, 1,444,720 and 1,443,888 bytes.

## Seventy-third executable increment — cycle-free live table slots

Live instances no longer retain full `WasmTable` handles inside imported table
slots. Each slot carries only its store table id, shared length cell and maximum;
reads, writes, growth, active elements and bulk operations receive the current
store explicitly. This removes the future ownership cycle `Store → Instance →
Table → Store`, allowing the store to own registered instance state without
leaking it.

Defined tables retain their direct vector fast path. Imported alias detection
uses equal table ids inside the already-enforced common store, so aggregate
budgets and overlap order remain unchanged. The existing WABT alias, distinct
same-store, foreign-store rejection, multi-table and JavaScriptCore gates all
exercise the new representation.

Evidence on 2026-08-21: all-feature tests, doctests, no-default/replay checks,
Clippy, rustfmt and shell checks pass; WABT and JavaScriptCore imported-table
and multi-table differentials remain exact. The static core measures 101,192
bytes. iOS links at 1,579,160 bytes arm64 and 1,656,280 bytes x86_64;
profile-catalog, replay, private-library and session consumers link at
1,452,008, 1,442,952, 1,444,624 and 1,443,776 bytes. Nostalgia Arcade consumes
the result with 5 unit tests, 1 UI test and an arm64 device build.

## Seventy-fourth executable increment — owner-resolved function signatures

`WasmStore` now registers the exact combined function-index signature table for
each live persistent instance. An indirect call resolves a table entry as an
`(instance, function)` address and validates it against the address owner's
module type table; it no longer interprets a foreign numeric function index in
the caller's module. Programmatic i32-only functions are normalized to exact
i32 signatures at registration, while decoded functions retain every standard
value type.

Failed start functions and dropped instances unregister their metadata, so a
dangling shared-table address traps as unknown rather than borrowing stale
module data. The imported-table sibling oracle now crosses owner-based type
resolution before reaching the still-explicit execution boundary. The next
increment moves live owner state into the store and removes that boundary.

## Seventy-fifth executable increment — cross-instance table dispatch

Persistent instances now place weakly registered live runtime records in their
common `WasmStore`. `call_indirect` and `return_call_indirect` can therefore
resolve a foreign function address, borrow the address owner's memories,
globals, tables and segment liveness, and execute the correct module. Public
instance memory guards were adapted to the shared runtime record without
copying defined memory or changing imported-memory identity.

The independent WABT fixture now exercises the complete sibling sequence:
instance A returns 1; instantiating B overwrites the shared table; invoking A
dispatches into B and returns 1; invoking B returns 2. TinyVM therefore matches
the JavaScriptCore oracle's aggregate result 4. Exact owner signature checks,
table aliasing, start-once behavior and existing instance APIs remain intact.

This is deliberately an intermediate execution bridge: foreign calls currently
enter the owner's existing trampoline through bounded native recursion, and a
runtime record is weak while its public instance handle lives. Before imported
tables graduate from the experimental profile, replace this bridge with one
store-owned activation trampoline and store-owned instance lifetime, including
cross-instance cycles and aggregate activation accounting.

Evidence on 2026-08-21: 246 non-ignored all-feature tests plus the doctest pass;
Clippy, rustfmt, replay/no-default and the WABT/JavaScriptCore imported-table
differential pass. The static core is 101,208 bytes with selftest 42. iOS links
at 1,582,520 bytes arm64 and 1,667,320 bytes x86_64; profile-catalog, replay,
private-library and session consumers link at 1,455,368, 1,446,296, 1,447,968
and 1,447,120 bytes. Nostalgia Arcade consumes the result with 5 unit tests,
1 UI test and an arm64 device build.

## Seventy-sixth executable increment — store-owned instance lifetime

`WasmStore` now strongly owns every successfully registered runtime record.
Dropping the public `Instance` handle therefore does not invalidate function
addresses already stored in a shared table: the imported-table oracle drops the
second sibling after its counter reaches 2, then invokes through the first
sibling and observes the second sibling's counter advance to 3.

To make that ownership acyclic, instantiation resolves imported `WasmTable`
handles into cycle-free live slots and then clears the decoded module's
binding-only handles before registering the runtime record. Function signature
lookup now reads the owning runtime module directly, removing the duplicate
per-instance signature registry. Failed starts unregister the incomplete record;
successful records live exactly as long as their store.

The remaining imported-table execution hole is structural rather than semantic:
foreign calls still cross a bounded native recursion bridge. The next increment
must make owner switching an explicit state in the activation trampoline so
cross-instance call cycles obey the same guest call-depth and activation-slot
budgets without consuming native stack.

## Seventy-seventh executable increment — explicit foreign-call outcomes

The instruction runner no longer invokes a foreign instance from inside the
`call_indirect` opcode arm. It returns owned `ForeignCall` or
`ForeignTailCall` outcomes carrying the owner address, arguments and (for a
normal call) the suspended defined activation. The existing module trampoline
is now the only place that crosses the temporary owner-runtime bridge and
resumes or tail-unwinds callers with the returned values.

This preserves the passing sibling, tail-call and resource gates while making
the owner switch an explicit interpreter transition. The next step can move
those owned outcomes into a store-level continuation stack without trying to
serialize borrowed locals, operand stacks or control frames out of an opcode
arm.

## Seventy-eighth executable increment — unified store activation trampoline

Persistent invocation and start execution now enter one `WasmStore` driver.
Each module runner executes until values return or a foreign owner is selected;
at that boundary it yields an owned local continuation. The store releases the
current runtime borrow, pushes the continuation, selects the target instance,
and later resumes the original owner with returned values. Normal and tail
indirect calls share this loop, so no cross-instance transition consumes native
stack.

Call depth and activation-slot bases travel with every store continuation.
Module-local callers are checked together with all suspended callers in other
instances, and one instruction counter remains shared across the complete
top-level call. A new independently WABT-compiled fixture installs sibling
functions into two slots and alternates A → B → A for 4,000 calls after the B
handle is dropped. It returns 4,000 with exact peak guest depth 4,001 and exact
aggregate activation usage 12,004, proving cyclic owner re-entry, store-owned
lifetime and non-native recursion in one executable gate.

Evidence on 2026-08-21: all 246 non-ignored all-feature tests plus the doctest,
Clippy, rustfmt, replay/no-default, shell checks and WABT/JavaScriptCore gates
pass. The static core remains 101,208 bytes with selftest 42. iOS links at
1,599,464 bytes arm64 and 1,667,616 bytes x86_64; profile-catalog, replay,
private-library and session consumers link at 1,472,312, 1,446,744, 1,448,416
and 1,447,568 bytes. The arm64 full-runtime ceiling advances by one fixed
16 KiB step to 1,605,632 bytes for the store continuation machinery; the
strict 100 KiB static-core gate and common ceiling for all arm64 consumers
remain enforced. Nostalgia Arcade consumes the unified trampoline build with
5 unit tests, 1 UI test and an arm64 device build.

## Seventy-ninth executable increment — imported-table graduation boundary

Standard imported funcref tables and the execution kernel graduate from partial
to proven. The general VM now covers decode, limits subtyping, common-store
binding, aliases, active/passive/bulk mutation, exact owner signatures,
store-owned lifetime, cross-instance normal/tail dispatch, cyclic re-entry,
shared fuel and aggregate activation budgets without native recursion.

TinyArcade cartridge ABI v1 deliberately continues to reject table imports in
both static inspection and runtime opening. A v1 cartridge is one standard
`.wasm` module and may define multiple internal funcref tables; its app-owned
capability registry grants only exact versioned function imports. Supplying a
shared host function table would be a new multi-module product contract, not a
hidden consequence of engine support, and must arrive under an explicit future
ABI/manifest design if needed.

## Eightieth executable increment — app-owned physical evidence gate

Nostalgia Arcade now exercises the current TinyVM through the real app target
for 600 consecutive Depth Well frames. The XCTest times each host call, reads
v2 execution telemetry after every frame, enforces an 8 ms p95 host budget and
fuel/page ceilings, and retains exact latency, instruction, memory, call-depth
and activation-slot values in the test result bundle. This keeps platform wall
time in the platform test while the VM continues to expose deterministic
resource facts.

The consumer repository also has a one-command physical-device workflow. It
discovers a connected iPhone or accepts an explicit UDID, rebuilds the runtime
and cartridge from adjacent agenterm main, executes all six runtime tests and
the playable UI journey on that device, verifies the arm64 app product, then
exports the retained attachment into a timestamped evidence directory. With no
iPhone connected it fails immediately with an actionable message instead of
silently falling back to a simulator.

Evidence on 2026-08-21: the complete consumer gate passes six unit tests, one
UI test and the arm64 device build. The simulator attachment records 600 frames
at 0.178 ms average, 0.207 ms p95 and 0.386 ms maximum, with 23,203 peak steps,
17 pages, call depth 6 and 62 activation slots. No physical iPhone is connected
to this Mac mini, so the goal's physical lifecycle/performance checkbox remains
open while the executable collection path is ready.

## Eighty-first executable increment — linked standard global exports

A defined standard global no longer lives as an instance-private copied `Val`.
Each live slot owns the same cloneable `WasmGlobal` cell used by imported
globals, and `Instance::exported_global_handle` exposes that exact object for
binding through `Module::bind_global_import`. Mutable writes made by an
importing sibling are immediately visible through the exporting instance;
immutable exports retain their exact type and mutability checks.

The imported-global differential now compiles a separate provider and consumer
with WABT. TinyVM links the provider's two exported global handles into two
consumer instances; JavaScriptCore links the same two `.wasm` files through
ordinary `WebAssembly.Instance` exports/imports. Both produce the unchanged
combined result `878897`, including sibling mutation and a later host update.

Evidence on 2026-08-21: 247 non-ignored all-feature package tests plus the
doctest pass; the linked WABT/JavaScriptCore oracle, no-default and replay
checks, Clippy, rustfmt and all shell checks pass. The stripped static core is
101,240 bytes with selftest 42. iOS links at 1,599,752 bytes arm64 and
1,667,864 bytes x86_64; profile-catalog, replay, private-library and session
consumers link at 1,472,600, 1,447,032, 1,448,704 and 1,447,856 bytes.

## Eighty-second executable increment — lazy linked memory exports

`Instance::exported_memory_handle` now resolves a standard defined-memory
export as the exact object another module can bind through
`Module::bind_memory_import`. Resolution moves the existing allocation into a
cloneable `WasmMemory` without copying its bytes; active data, host writes,
guest writes and growth are thereafter visible through provider, consumer and
handle. Imported memory re-exports return their existing object.

Promotion is deliberately lazy. A module that never requests a linkable export
keeps the direct `Vec` memory slot and its existing instruction hot path. This
includes current TinyArcade games even though Rust emits a standard memory
export, so general multi-module support does not impose per-load `RefCell`
dispatch on the game runtime.

The imported-memory differential now WABT-compiles a provider module and links
its exported memory into two consumer instances. TinyVM and JavaScriptCore run
the same two `.wasm` files to result `516`, including active initialization,
sibling writes, host mutation and shared growth. Evidence on 2026-08-21: 248
non-ignored all-feature package tests plus the doctest pass; WABT/JSC,
no-default/replay, Clippy, rustfmt and shell gates pass. The stripped static
core remains 101,240 bytes with selftest 42. All iOS linked sizes are unchanged:
1,599,752 arm64, 1,667,864 x86_64, and 1,472,600 / 1,447,032 / 1,448,704 /
1,447,856 bytes for profile-catalog / replay / private-library / session.

## Eighty-third executable increment — linked table exports

`Instance::exported_table_handle` now resolves a standard defined funcref-table
export as an object another module can bind through
`Module::bind_table_import`. Like linked memory, promotion is lazy: the
existing element vector moves without copying into the instance's current
`WasmStore`; modules that never request the handle retain direct table storage.
An imported-table re-export reconstructs a handle to its existing store table.

The stronger WABT fixture places a provider function in its defined exported
table. A separate consumer imports that table and invokes the function after
the provider's public `Instance` handle is dropped. Store ownership preserves
both the table entry and its function owner; the unified store trampoline
returns 42 without native recursion. The existing sibling overwrite, alias,
foreign-store rejection and 4,000-call cross-instance cycle cases remain in
the same oracle. JavaScriptCore executes the provider/consumer pair and sibling
sequence from the same WABT-produced bytes.

Evidence on 2026-08-21: 249 non-ignored all-feature package tests plus the
doctest pass; WABT/JSC, no-default/replay, Clippy, rustfmt and shell gates pass.
The stripped static core remains 101,240 bytes with selftest 42. All iOS linked
sizes remain unchanged at 1,599,752 arm64, 1,667,864 x86_64, and 1,472,600 /
1,447,032 / 1,448,704 / 1,447,856 bytes for profile-catalog / replay /
private-library / session.

## Eighty-fourth executable increment — linked numeric function exports

`Instance::exported_function_handle` now resolves a standard function export,
including an imported function re-export, into a cloneable store-owned address
with its exact parameter and result types. `Module::bind_function_import`
accepts that handle only for an identical standard signature and selects the
provider's store at consumer instantiation. Multiple linked resources must
belong to one common store; mismatched stores fail before a runtime record is
registered.

Linked signatures currently admit the four standard numeric value types.
Reference-valued direct links fail explicitly until a later increment gives
standalone funcref values store-owned identity; an instance-local function
index must never be reinterpreted in a consumer instance.

Direct linked calls use the same explicit store activation trampoline already
proven for foreign `call_indirect`. Both ordinary `call` and `return_call`
release the current instance borrow, switch owner without native recursion,
retain shared fuel/depth/activation accounting, and resume or tail-unwind the
consumer. Binding-only store handles are cleared before strong store
registration, preserving acyclic runtime ownership; dropping provider and
consumer public handles does not invalidate a function retained by a later
linked instance.

Three checked-in WAT fixtures are independently compiled and validated by WABT.
TinyVM and public JavaScriptCore link the same provider, consumer and relay
bytes and both produce `4242424` from a normal call, foreign tail call,
re-exported function call and mixed i32/i64/f32/f64 arguments/results. TinyVM
additionally proves exact signature/reference rejection, different-store
rejection and provider lifetime after public-handle drop.

Evidence on 2026-08-21: all 249 non-ignored all-feature package tests plus the
doctest pass; every WABT/JavaScriptCore oracle, replay/no-default, Clippy,
rustfmt and shell gate passes. The stripped static core remains 101,240 bytes
with selftest 42. iOS links at 1,599,960 bytes arm64 and 1,671,976 bytes x86_64;
profile-catalog, replay, private-library and session consumers link at
1,472,808 / 1,447,224 / 1,448,896 / 1,448,048 bytes. The simulator-only x86_64
ceiling advances by one explicit 16 KiB linker bucket; the arm64 product
ceiling is unchanged. Nostalgia Arcade consumes current main with 6 unit tests,
1 UI test and an arm64 device build.

## Eighty-fifth executable increment — store-owned funcref values

Non-null funcref values now carry one process-unique opaque 64-bit token. The
owning Store maps that token to its instance and combined function index;
another Store cannot resolve it, including after an old Store is dropped and
allocator addresses are reused. Local `ref.func` values are canonicalized only
when they leave their owner through a top-level result or cross-instance call,
so ordinary single-instance execution retains the compact local
representation. `table.get/set/grow/fill`, globals, typed arguments and results
all preserve the canonical owner; a value presented to another store traps
before guest execution or table/global mutation.

Function-reference globals now participate in common-store selection. A
registered instance's global cells carry canonical values but no Store handle;
exported funcref-global handles retain the Store strongly, while imported
binding-only handles are cleared before instance registration. This preserves
provider lifetime for a linked reference without introducing
`Store → InstanceState → Global → Store` cycles. The general decoder now
accepts the standard funcref global import type in addition to the four numeric
types.

The WABT/JSC provider-consumer oracle now round-trips a consumer `ref.func`
through a provider function, stores it in a table and calls it indirectly to
42. It also imports a provider's exported funcref global, places that reference
in the consumer table and calls the original provider function to 43 after the
provider's public instance handle is dropped. Cross-store value and global
bindings trap explicitly; JavaScriptCore runs the same standard bytes.

Evidence on 2026-08-21: the 249 non-ignored all-feature package tests and the
no-default-feature suite pass, as do Clippy with warnings denied, rustfmt, the
PRD leaf map and the independently compiled WABT/JavaScriptCore linked-function
oracle. The stripped static core is unchanged at 101,240 bytes with selftest
42; the store-reference machinery is absent from that deliberately minimal
profile.

## Eighty-sixth executable increment — decomposed boundary benchmark

The QJWasm review now has an executable consequence rather than a borrowed
headline multiplier. `smoke-boundary-benchmark.sh` compiles and validates one
standard WAT fixture, then runs those exact bytes through release tinyvm and
public JavaScriptCore. Both engines emit compatible CSV rows for empty calls,
mixed numeric arguments, borrowed linear-memory reads, intentional host copies
and a constant-work guest memory touch.

Payload points are 0, 64, 1,024, 65,536 and 76,800 bytes. This exposes the
copy-size curve independently of interpreter/call cost and includes one full
TinyArcade-sized frame. The benchmark validates results but never gates on
elapsed time, so host load cannot create a false conformance failure. A
monotonic host clock gives JavaScriptCore sub-millisecond observations without
adding browser/H5 runtime code to the app.

Evidence on 2026-08-21: WABT validation, the ignored release Rust benchmark and
the optimized Swift/JavaScriptCore benchmark all pass through the one command.
The low-iteration smoke confirms all five cost dimensions and payload sizes;
normal research runs default to 20,000 operations per point.

## Eighty-seventh executable increment — exported-resource lifetime closure

The resource-linking oracles now exercise every exported handle after its
public provider instance is dropped. Linked table and function execution
already retained their provider Store; the memory oracle now mutates and reads
the exported allocation after provider drop, and the global oracle links,
mutates and shares exported cells after provider drop.

The funcref oracle adds the stronger stale-token case: it creates a function
reference, drops every handle to the original Store, constructs another Store
and submits the old value there. The new Store traps it as foreign rather than
accepting an allocator-reused address. Together with live wrong-store tests,
this makes the process-unique token invariant executable.

Evidence on 2026-08-21: the focused memory lifetime test, independently
compiled WABT/JavaScriptCore global and function-linking oracles, rustfmt and
all-target/all-feature Clippy with warnings denied pass.

## Eighty-eighth executable increment — explicit media-version imports

The core v1 import surface now negotiates every media schema instead of only
indexed 2D. A cartridge that submits `TAG3`, `TAI2` or `TAT1` must import the
matching `grid3d_version`, `indexed2d_version` or `tones_version` function.
Older hosts reject a new version import during loading; the current host traps
an undeclared media submission before native rendering or audio scheduling.

Depth Well checks grid3d and tones version 1 during initialization, while
Paddle Guard checks indexed2d and tones. The development JavaScriptCore oracle
exposes the same declarations, and one black-box test proves that undeclared
grid/audio formats trap while correctly declared formats run. Exact replay
hashes advance because the standard guest Wasm bytes now include these imports;
the deterministic behavior and converter/trust checks remain unchanged.

Evidence on 2026-08-21: the focused runtime and both real-cartridge suites,
the WebKit differential, rustfmt, all-target/all-feature Clippy, the PRD leaf
map, no-default-feature tests and the full all-feature package suite pass.

## Eighty-ninth executable increment — opaque standard externref values

The general VM host door now preserves standard nullable `externref` values in
function parameters/results, locals, globals, typed select, `ref.null` and
`ref.is_null`. A non-null `WasmExternReference` is a process-unique monotonic
token: tinyvm never dereferences it or exposes a native pointer, while the host
owns any bounded token-to-object registry and its lifetime. The public token is
opaque but hashable/orderable, so a native module can use it as a safe registry
key without relying on allocation addresses.

This capability deliberately remains below TinyArcade cartridge ABI v1, whose
native/core functions stay i32-only. It also does not claim externref tables;
the loader rejects those until a separately budgeted generic-reference table
model exists. An ordinary all-feature test proves typed host identity, null,
mutable exported-global behavior, mismatched-result rejection and the table
boundary. A separately WABT-compiled fixture passes the same host object through
function/global/function in tinyvm and JavaScriptCore.

Evidence on 2026-08-21: the focused standard-extension test, independent
WABT/JavaScriptCore externref differential, full all-feature and no-default
suites, iOS device/universal-simulator XCFramework link, rustfmt and Clippy all
pass. `Val` remains at most 16 bytes, the arm64 linked consumer grows only 16
bytes, and the stripped static core remains 101,240 bytes with selftest 42.

## Ninetieth executable increment — current-main real-app consumer closure

The real Nostalgia Arcade consumer gate rebuilt the Swift package and Depth
Well cartridge directly from agenterm main, then executed the generated runtime
inside the app target. This caught a real integration drift rather than merely
reconfirming the SDK: the consumer repository still carried a 6,022-byte guest
from before explicit Grid3D/Tones version imports. The reproducible build now
commits the current 6,116-byte cartridge with both declarations and a stable
SHA-256 of `d1a61599e6877da2b27bd4859f49ba9e0bc0fd4b80df8f66145893ebb2317e6f`.

Evidence on 2026-08-21: six real-app runtime tests, including the 600-frame
latency/resource gate, and the complete playable UI journey pass on the iPhone
17 Pro simulator. A generic arm64 `iphoneos` Release build contains exactly the
rebuilt cartridge, links current tinyvm, and has no WebKit/JavaScriptCore
dependency. Re-running the preparation after commit leaves the consumer
worktree clean, proving byte reproducibility. Nostalgia Arcade main contains
commit `6ff3262`; physical-device execution remains the separate open gate.

## Ninety-first executable increment — current runtime TestFlight candidate

Nostalgia Arcade 0.16.4 build 31 packages current tinyvm and the refreshed
6,116-byte Depth Well cartridge in a signed arm64 archive. Before upload, the
archive passed strict code-signature verification, contained exactly one Wasm
file with the committed SHA-256, targeted iOS, and had no WebKit/JavaScriptCore
dynamic dependency. Xcode then completed App Store Connect analysis/SPI
analysis and recorded `Upload succeeded` with no upload warnings or errors.

The consumer repository binds the candidate to both source commits, a canonical
archive content-tree digest, executable and `Info.plist` hashes, dSYM UUID,
cartridge hash, toolchain and Apple upload event in
`docs/releases/0.16.4-31-testflight.md`. Its retained simulator attachment is
also decoded rather than inferred from the test name: 600 frames measure
0.175 ms average, 0.205 ms p95 and 0.351 ms maximum with 23,203 fuel, 17 pages,
call depth 6 and 62 activation slots.

Apple-side processing/availability is not yet independently observed because
this Codex session has no connected browser surface. The archive itself records
upload event `c1ca0832-ad3b-4c3c-b4c9-57161afd2d5b` at
2026-08-21T15:45:26Z with `state=success`. This is distribution evidence, not
the still-open physical-iPhone lifecycle/performance/feel result.

## Ninety-second executable increment — physically bundled-only Swift surface

The first archive audit caught a release-policy mismatch that runtime tests did
not: build 31 never called an external cartridge path, but its monolithic Swift
wrapper still linked `URLSession` and compiled catalog, reviewed-download and
private-import APIs. That contradicted the stated compile/link-time exclusion
gate and was not acceptable as the next App Review candidate.

The generated Swift package now compiles those surfaces only under the explicit
`TINYARCADE_EXTERNAL_CARTRIDGES` condition. Default App package generation omits
the external distribution policy, HTTP/catalog, trust/cache, reviewed-library
and private-import Swift APIs. SDK black boxes define the condition and continue
testing the future mechanisms, so the review-safe product boundary does not
erase the research path.

Evidence on 2026-08-21: the default package builds for generic iOS and universal
simulator with warnings denied and contains no external API marker strings. The
explicit research build still compiles every existing direct Swift black box;
the complete iOS bridge gate passes with arm64/x86_64 consumers at 1,601,272 and
1,676,856 bytes, and catalog, replay, private-library and session consumers all
remain inside their release budgets. A replacement app archive must now prove
the final executable has no `URLSession`, catalog/private-import markers,
WebKit or JavaScriptCore before superseding TestFlight build 31.

## Ninety-third executable increment — bundled-only TestFlight replacement

Nostalgia Arcade 0.16.4 build 32 now supersedes build 31. The exact signed
archive passes the new reusable archive gate: one byte-identical bundled Wasm,
arm64 iOS, strict code signature, no WebKit/JavaScriptCore dependency, no
`NSURLSession` import and no external catalog/reviewed/private Swift marker.
The real-App gate also keeps those checks on every generic-device Release build.

Xcode completed App Store Connect package and SPI analysis and reported
`Upload succeeded`; Apple accepted the upload for processing at
2026-08-21T16:03:33Z with event
`fabc2496-8c84-49dd-9d9b-1e01ed01a386`, no warnings or errors. The consumer
repository records source/artifact hashes, dSYM UUID and the supersession reason
in `docs/releases/0.16.4-32-testflight.md`; build 31 is explicitly barred from
selection. Apple-side processing/installability and physical-iPhone lifecycle,
performance and owner-feel evidence remain open external gates.

## Ninety-fourth executable increment — standard externref tables

The general VM now carries opaque host `externref` identities through standard
defined, imported and exported tables. Table element type is explicit in the
decoded module, public import descriptor, host `WasmTable` handle and live table
slot; funcref/externref bindings cannot be confused. `table.get`, `table.set`,
`table.grow`, `table.size`, `table.fill`, same/cross-table `table.copy`, passive
`table.init` and `elem.drop` use the table's declared reference type. The load
validator rejects mixed-type copy/init and indirect calls through externref
tables before execution.

The host creates externref tables without storing native pointers. `get` and
`set` preserve only the process-unique opaque token, exported tables retain
their store allocation after the provider instance is dropped, and sibling
instances observe the same cells. Aggregate table-element and per-table maximum
budgets remain shared with funcref tables. TinyArcade v1 still rejects table
imports and remains i32-only; this standards increment does not widen the
shipping game ABI.

Evidence on 2026-08-22: one WABT-compiled and independently validated fixture
passes the same identity, import/export, provider-drop, grow, fill, copy and
passive-init behavior in tinyvm and JavaScriptCore. The normal all-feature test
suite covers defined tables, host type mismatch and mixed-table validation;
all 127 library tests, every integration suite, the development WebKit replay
differential, generic-device/universal-simulator Swift linkage and doctest pass.
The iOS arm64 consumer remains below its gate at 1,602,104 bytes. The stripped
static core is 101,256 bytes, below the unchanged 100 KiB ceiling, and its C
selftest returns 42. The current-main nostalgia-arcade consumer also passes six
runtime unit tests, the complete cartridge-hall UI test and an unsigned arm64
device Release build against this exact worktree.

## Ninety-fifth executable increment — two real WASM games in the iOS consumer

Signal Lock is now a second standard cartridge rather than a Swift game engine.
Its rules, deterministic state, indexed-2D frame and tones live in the same
`.wasm` exercised by tinyvm, JavaScriptCore and a real browser-WASM oracle. The
shipping App target retains only the native shell, controls, lifecycle and
atomic snapshot owner; the old Signal implementation and the other three native
prototype games are excluded from compilation and linkage.

Evidence on 2026-08-22: nostalgia-arcade main `f420ff8` rebuilds both cartridges
from agenterm main, passes ten real-App runtime tests and one two-cartridge UI
journey, and produces an arm64 device Release App containing exactly the two
byte-identical reviewed `.wasm` files. The executable contains no archived game
engine markers, WebKit/JavaScriptCore dependency, URLSession import or external
cartridge surface. Physical-iPhone play remains open.

## Ninety-sixth executable increment — platform-neutral host contract

The unified crate now exposes a default-`no_std + alloc` host boundary beneath
future import adapters. `HostBackend` owns native clock, random, descriptor,
relative-path and exit mechanisms; `HostContext` owns bounded guest descriptor
numbers, opaque backend handles, process strings, rights and virtual preopens.
Guest paths can only be relative to a registered preopen and never reveal an OS
descriptor, drive letter or physical path. Missing platform operations fail
with an explicit typed result instead of being invented by the VM.

The contract is not itself WASI and does not change TinyArcade imports. A future
optional WASI P1 adapter will translate standard imports/errno into this layer;
Unix, Windows and iOS backends remain separately gated work inside the same
crate.

Evidence on 2026-08-22: five public host-contract tests cover opaque mapping,
rights, preopen path escape, transactional process values, explicit unsupported
results and capacity rejection before a platform open. The no-default-feature
library check, all-target Clippy with warnings denied, PRD leaf map and complete
all-feature/all-target suite pass. The real iOS XCFramework/Swift link remains
green, the three-game tinyvm/JSC/H5 replay differential agrees exactly, and the
stripped static core remains 101,256 bytes with selftest 42.

## Ninety-seventh executable increment — optional WASI P1 adapter

The `wasi-p1` feature now binds a deliberately small standard
`wasi_snapshot_preview1` subset over the neutral host contract. The implemented
surface is args/environ sizes and copies, clock time, random fill, preopen
metadata/name and descriptor close. Every present field and complete value type
signature is checked before instantiation; unknown fields fail binding, while a
missing platform mechanism behind an implemented field returns canonical errno.

The adapter validates guest memory before host mutation and exposes only virtual
preopen names. It does not enter the default feature graph or TinyArcade ABI.
File I/O, path operations and a non-returning `proc_exit` outcome remain open
leaves rather than simulated success.

Evidence on 2026-08-22: a standards-shaped binary module executes all nine
imports through one persistent tinyvm instance and proves exact guest-memory
layouts plus backend close. A second black box rejects an unknown field and a
wrong standard signature before instantiation. The isolated no-default-feature
WASI test/Clippy gate and the complete all-feature/all-target suite pass;
the isolated `no_std` arm64 iOS feature build, generic-device/universal-simulator
Swift linkage and the three-engine game differential remain green.

## Ninety-eighth executable increment — bounded WASI descriptor I/O

The optional `wasi-p1` adapter now implements the standard `fd_read`,
`fd_write`, `fd_seek` and `fd_filestat_get` signatures over `HostContext`.
Guest descriptors remain rights-checked mappings to opaque backend handles.
Vectored I/O caps each call at 64 records and validates the entire iovec table,
all data ranges and the result pointer before its first backend call. It rejects
backend over-reporting and checked-count overflow instead of trusting platform
code. Seek and the 64-byte Preview 1 filestat layout likewise preflight output
before host mutation.

Evidence on 2026-08-22: a standards-shaped binary fixture performs read, write,
seek and stat through four separately rights-scoped guest descriptors and proves
the resulting memory layouts and backend bytes. An adversarial fixture passes
65 iovecs and receives `INVAL` without a backend write; another proves that an
output overlapping the iovec table cannot corrupt later records because the
adapter snapshots the bounded table first. The focused no-default-feature WASI
tests, warnings-denied Clippy and arm64 iOS `no_std` feature check pass. The
complete all-feature/all-target suite passes all 127 library tests and every
non-ignored integration test, including the real iOS XCFramework/Swift link and
the three-game tinyvm/JSC/H5 differential. Linked sizes remain 1,602,104 bytes
arm64 and 1,677,136 bytes x86_64; the stripped static core remains 101,256 bytes
with selftest 42.

## Ninety-ninth executable increment — preopen-relative WASI paths

The optional adapter now implements the exact Preview 1 `path_open` and
`path_unlink_file` signatures. It validates the output slot, guest UTF-8,
relative path, open flags and requested rights before dispatch. `HostContext`
then enforces that the directory is a virtual preopen with both path authority
and every delegated descriptor right; the backend receives only its opaque
directory handle and relative path. The resulting native handle is published
as a bounded guest fd, preserving cleanup if publication fails.

The initial subset supports create/directory/truncate plus read/write/seek/stat
rights. Lookup flags, exclusive open, descriptor flags, inheriting rights and
unrepresented Preview 1 rights fail explicitly instead of being discarded.

Evidence on 2026-08-22: a standards-shaped binary opens `save.bin`, receives
guest fd 1 and unlinks through virtual root `/save`; the fixture backend sees
only opaque handle 77 and the relative path. A second binary attempts `../x`,
receives `NOTCAPABLE`, and proves neither backend operation ran. The owning
focused no-default-feature tests, warnings-denied Clippy and arm64 iOS `no_std`
feature check pass. The complete all-feature/all-target suite passes all 127
library tests and every non-ignored integration test, including the real iOS
XCFramework/Swift link and three-game tinyvm/JSC/H5 differential. Linked sizes
remain 1,602,104 bytes arm64 and 1,677,136 bytes x86_64; the stripped static core
remains 101,256 bytes with selftest 42.

## One-hundred-fourth executable increment — converter compatibility report

The existing TAH1 host profile now exposes a structured, non-executing
`HostCompatibilityReportV1`. A standards-valid cartridge produces all native
import issues in deterministic import order: wholly missing module/function
pairs are distinct from same-name parameter/result signature mismatches. Parse,
resource-limit and malformed-profile failures remain typed errors, while the
old fail-fast `inspect_cartridge` API is preserved as a wrapper.

The converter CLI now prints stable key/value rows for cartridge identity,
issue count, each exact required/available signature and final compatibility.
An incompatible result remains a failing process status, but creators no longer
receive only an opaque “unavailable” message. This formalizes the already-
enforced lowercase major-versioned native namespace convention without changing
TAH1 bytes, cartridge bytes, runtime authority or App Store policy.

Evidence on 2026-08-22: library black boxes distinguish missing
`fan:physics/v1.step_world` from its wrong-arity form and preserve the legacy
fail-fast error. The CLI black box targets a core-only app profile with a native
cartridge and verifies the exact missing row plus `compatible=false`. The
30-test game-runtime suite and warnings-denied all-feature/all-target Clippy
pass. The complete all-feature/all-target package passes all 128 library tests
and every non-ignored integration test, including both iOS XCFramework gates
and the three-game tinyvm/JSC/H5 differential. Default linked sizes are
1,602,424 bytes arm64 and 1,681,600 bytes x86_64; all remain inside their
product gates. The stripped static core remains 101,256 bytes with selftest 42.

## One-hundred-fifth executable increment — measured selected-memory boundary

The existing QJWasm-inspired benchmark now measures the guest-to-host direction
it previously omitted. One standard WABT fixture calls three imports: the
legacy memory-zero view, `WasmHostMemories::memory(0)` and the same indexed path
with an explicit copy into a preallocated host buffer. Those rows join the
existing host-to-guest, direct view, copy and guest-touch dimensions at five
payload sizes through both release tinyvm and public JavaScriptCore.

The smoke gate now verifies that both engines emit the same complete 32-row
metric/payload matrix, positive timing observations and valid iteration counts;
it never fails on relative speed. Public JSContext rejected the attempted
two-memory shared fixture, so the cross-engine benchmark truthfully stays on
memory zero. Tinyvm's independent standard multi-memory and selected-memory
tests remain the authority for nonzero indexes.

Evidence on 2026-08-22: the full 20,000-iteration development run passes. On
this host, tinyvm's indexed view remains close to its legacy memory-zero path
across payload sizes, while explicit 64 KiB and 76,800-byte copies show the
expected size-dependent cost. JavaScriptCore exhibits the same qualitative
separation. The complete all-feature/all-target package passes all 128 library
tests and every non-ignored integration test, including both iOS XCFramework
gates and the three-game tinyvm/JSC/H5 differential. Warnings-denied Clippy,
shellcheck, rustfmt, document redaction and the 97-leaf PRD trace gate pass.
Default linked sizes remain 1,602,424 bytes arm64 and 1,681,600 bytes x86_64;
the stripped static core remains 101,256 bytes with selftest 42.

## One-hundred-sixth executable increment — real-cartridge feature priority

Decoded modules now expose a static `WasmFeatureUsage` report for nine accepted
post-MVP standard families. The CLI prints the same deterministic
`standard_features=` row without instantiation or start execution. A minimal
module proves that engine support is not falsely reported as module use, while
independent standard fixtures exercise every positive flag.

A new production-cartridge smoke rebuilds Depth Well, Paddle Guard and Signal
Lock and gates their exact current profiles. All three require bulk memory;
Depth Well and Signal Lock additionally require sign extension. None currently
requires SIMD, typed function references, GC, memory64, exceptions or threads.
Those proposals therefore remain behind real-workload, independent-engine and
resource/size evidence rather than entering because another VM implements them.

Evidence on 2026-08-22: the three production builds and static CLI reports pass
with the profiles above; the independent nine-family usage test and minimal
false-positive test pass. The complete all-feature/all-target package passes all
128 library tests and every non-ignored integration test, including both iOS
XCFramework gates and the three-game tinyvm/JSC/H5 differential. The no-default
library passes all 115 tests; warnings-denied Clippy, shellcheck, rustfmt,
document redaction and the 98-leaf PRD trace gate pass. Default linked sizes
remain 1,602,424 bytes arm64 and 1,681,600 bytes x86_64; the stripped static
core remains 101,256 bytes with selftest 42.

## One-hundredth executable increment — non-returning WASI process exit

The optional Preview 1 subset now includes the exact `(i32) → ()` `proc_exit`
import. It clears any stale outcome, asks `HostContext`/`HostBackend` to accept
the unsigned exit code, records that typed code in the clonable adapter owner,
then interrupts execution through the stable exported `WASI_PROC_EXIT_TRAP`
marker. It never returns an empty success that would let guest instructions
after `proc_exit` execute. Embedders can inspect or consume the code through
`exit_code()` and `take_exit_code()`; backend rejection remains a separate trap
and does not publish an outcome.

Evidence on 2026-08-22: a standards-shaped binary calls `proc_exit(7)` before a
would-be return of 99. The backend receives exactly 7, invocation ends with the
dedicated non-returning marker, and both typed adapter accessors behave as
specified. A separate import-only binary binds all 16 exact signatures so the
completed parent profile has direct executable coverage, not only child-leaf
claims. The focused no-default-feature tests, warnings-denied Clippy and arm64
iOS `no_std` feature check pass. After the PRD traceability gate caught and then
verified that new parent mapping, the complete all-feature/all-target suite
passes all 127 library tests and every non-ignored integration test, including
the iOS XCFramework/Swift link and three-game tinyvm/JSC/H5 differential.
Linked sizes remain 1,602,104 bytes arm64 and 1,677,136 bytes x86_64; the
stripped static core remains 101,256 bytes with selftest 42.

## One-hundred-first executable increment — capability-based std host

The unified crate now has an optional `std-host` backend rather than separate
Unix, Windows and iOS VM forks. `StdHostBackend` owns a bounded table of
`cap-std` directories/files, realtime and monotonic clocks, system random,
sleep and recorded exit outcomes. An embedding performs one explicit ambient
open for each chosen preopen; all guest file operations thereafter stay
relative to that directory capability. Real paths never enter `HostContext`,
WASI or guest memory. Specific native failures now survive the neutral layer as
not-found, exists, directory-kind or access errors and map to canonical WASI
errno values.

The backend is separately enabled and does not change the default
`no_std + alloc` core. On iOS it requires the App to pass an owned
Documents/Caches directory; it does not discover ambient storage or expose the
container root.

Evidence on 2026-08-22: macOS black boxes execute real preopen/create/write/
seek/read/stat/close/unlink behavior, backend handle exhaustion, clocks, random
and exit outcome. A sibling-directory escape attempt preserves an external
sentinel. An independently WAT-compiled standard module drives the same real
file lifecycle end to end through tinyvm, the 16-import WASI adapter,
`HostContext` and `StdHostBackend`, with exact guest-memory and host-filesystem
assertions. The `std-host,wasi-p1` library graph compiles for arm64 Linux musl,
Windows GNU/LLVM and arm64 iOS, while the default no-feature library remains
`no_std`. The complete all-feature/all-target suite, warnings-denied Clippy,
real iOS XCFramework/Swift link and three-game tinyvm/JSC/H5 differential pass.
Linked sizes remain 1,602,104 bytes arm64 and 1,677,136 bytes x86_64; the
stripped default static core remains 101,256 bytes with selftest 42.

## One-hundred-second executable increment — iOS container WASI host

The crate now has a separately built `ios-wasi-host` C ABI for one-shot standard
WASI commands. Swift supplies an App-owned UTF-8 directory and explicit VM,
guest-descriptor and backend-handle limits. Rust opens that directory once,
publishes only virtual `/save`, binds the exact Preview 1 subset and invokes
`_start`. Normal empty return and accepted `proc_exit` are distinct typed
outcomes; decode, trap, storage, argument and caught-panic failures remain
separate status values.

This surface is deliberately absent from the default TinyArcade artifact. The
optional builder selects a separate Cargo feature, header directory and Clang
module named `TinyWasiHost`; the normal `ios-c-api`/`TinyArcade` XCFramework and
Swift package stay unchanged.

Evidence on 2026-08-22: an independently compiled standard command runs inside
a booted iPhone 17 Pro Simulator. Swift creates and supplies a fresh temporary
container directory; guest `_start` writes `hello` to `/save/slot.bin`, closes
the descriptor and calls `proc_exit(7)`. Swift verifies the file bytes, exit
presence and exact code. The optional builder also produces an arm64 device and
universal simulator XCFramework, passes its C header check and links the Swift
owner with warnings denied. The unchanged default builder also passes its full
XCFramework, C/Swift/package and negative header/archive-surface gates. The
complete all-feature/all-target suite passes all 128 library tests and every
non-ignored integration test, including the Simulator container run and
three-game tinyvm/JSC/H5 differential; warnings-denied Clippy passes. Linked
default sizes remain 1,602,104 bytes arm64 and 1,677,136 bytes x86_64, while the
stripped default static core remains 101,256 bytes with selftest 42. Physical-
iPhone container behavior remains open.

## One-hundred-third executable increment — selected-memory host callbacks

The typed bounded host door now has an opt-in multi-memory form. Its public
`WasmHostMemories` context resolves the module's standard memory index space to
call-scoped read or mutable guards. It owns and copies no guest bytes; the host
cannot retain a guard after the synchronous callback returns, and a mutable
guard must release its exclusive context borrow before another index is
accessed. Existing memory-zero callbacks remain source-compatible for profiles
that deliberately require one memory.

This closes the abstraction gap between tinyvm's existing standard multi-memory
engine and future versioned native modules. Defined memories remain distinct;
multiple imported indexes bound to one `WasmMemory` preserve shared identity and
runtime borrow checks rather than becoming snapshots or unsafe raw pointers.

Evidence on 2026-08-22: one independently WAT-compiled module exposes two
defined memories to a typed import, selects memory one, rejects an absent index
and returns the exact typed result. A second module binds two imported memory
indexes to the same host object; mutation through index zero is observed
through index one and by the external owner. The 23-test standard-extension
suite, 94-leaf executable PRD trace map and warnings-denied all-feature/all-
target Clippy pass. The complete all-feature/all-target package passes all 128
library tests and every non-ignored integration test, including both iOS
XCFramework gates and the three-game tinyvm/JSC/H5 differential. The no-default
library passes all 115 tests and warnings-denied Clippy. Default linked sizes are
1,602,104 bytes arm64 and 1,677,312 bytes x86_64; the stripped static core
remains 101,256 bytes with selftest 42.

## One-hundred-seventh executable increment — native resource-handle ownership

The unified crate now exposes `HostResourceTable<T>` for native modules that
must name host-owned objects through a standard Wasm `i32`. Its nonzero token
encodes a module domain, bounded slot and generation; distinct domains reject
cross-module collisions, while close advances the generation before
reuse, while final-generation exhaustion permanently retires the slot instead
of wrapping an ancient token onto a new object. Insert failure drops its owned
input, and clear/table drop own deterministic cleanup. This is a lifecycle
primitive for any host, not a platform backend, executor or permission layer.

Evidence on 2026-08-22: six public black boxes prove exact i32 bit round trips,
bounded capacity, mutable access, close, clear, deterministic drop, cross-domain
and stale-token rejection, and all 4,095 generations of one slot. A seventh
black box drives an
ordinary TinyArcade cartridge through versioned create/read/close imports and
leaves no live host resource. The complete all-feature/all-target package
passes all 128 library tests and every non-ignored integration test, including
both iOS XCFramework gates and the three-game tinyvm/JSC/H5 differential.
Warnings-denied all-feature and isolated no-default Clippy, arm64 iOS device and
Simulator checks, rustfmt, document redaction and the 100-leaf PRD trace gate
pass. Default linked sizes remain 1,602,424 bytes arm64 and 1,681,600 bytes
x86_64; the stripped static core remains 101,256 bytes with selftest 42.

The real consumer independently passed ten runtime tests, one two-cartridge UI
journey and its arm64 release audit. Nostalgia Arcade `0.16.4 (33)`, the first
TestFlight candidate containing both Depth Well and Signal Lock standard WASM
cartridges, was uploaded successfully and entered Apple processing. Physical
iPhone installation, lifecycle, frame-time and audio evidence remain open.

## One-hundred-eighth executable increment — registry-owned resource domains

`NativeModuleRegistry` now owns resource-domain assignment instead of asking
each native callback to invent an integer. The first explicit claim or function
registration for a canonical versioned module receives the next nonzero domain;
repeated claims and every later function reuse it, while sibling modules receive
different domains. A domain claim alone does not add a function to the
converter-visible host profile or grant a cartridge an import.

Evidence on 2026-08-22: the real resource-handle cartridge obtains its texture
table domain from the registry, observes stable reassignment, claims a distinct
audio domain, and completes versioned create/read/close calls. The lower-level
six-test table suite proves that tokens at otherwise identical slot/generation
positions are rejected across those domains. The 102-leaf executable PRD trace
binds both registry leaves to this black-box owner. The complete all-feature/
all-target package passes all 128 library tests and every non-ignored
integration test, including both iOS XCFramework gates and the three-game
tinyvm/JSC/H5 differential. Warnings-denied Clippy, arm64 iOS `no_std`, rustfmt
and document-redaction gates pass. Default linked sizes are 1,602,872 bytes
arm64 and 1,681,888 bytes x86_64; the stripped static core remains 101,256
bytes with selftest 42.

## One-hundred-ninth executable increment — cross-runtime resource identity

The previous module-number model was insufficient: two replacement runtime
registries could both assign domain one, allowing the first resource created in
the new runtime to accept an old token with the same slot and generation. The
handle layout now spends 12 bits on a resource-table-instance domain, 10 bits
on generation and 10 bits on `slot + 1`. A shared
`ResourceDomainAllocator` issues 4,095 domains without reuse or wrap; each table
supports 1,023 live slots and permanently retires a slot after 1,023
generations. `NativeModuleRegistry::resource_table` validates and reserves
before claiming a domain, atomically records one table per canonical native
module, and rejects duplicate configuration. Ordinary function registration
does not consume resource identity.

The token is deliberately runtime-local rather than a native pointer or a
durable identity. The allocator prevents aliases among replacement runtimes in
one process lifetime. Persisted guest snapshots must quiesce native resources
and reconstruct them explicitly; enforcing that product-level boundary remains
an open PRD leaf. A speculative C/Swift resource-table ABI is therefore deferred
until a real platform module can prove its owner and restore protocol; the
existing callback ABI remains sufficient for current iOS cartridges.

Evidence on 2026-08-22: seven public resource-table black boxes prove exact bit
round trips, bounded ownership/drop, generation retirement, cross-table
rejection, first-token rejection across replacement runtimes, all 4,095 unique
domain claims and explicit exhaustion. The real native-import cartridge proves
invalid and duplicate table requests do not consume allocator state, ordinary
function registration remains identity-free, sibling texture/audio tables are
distinct, and versioned create/read/close still completes. The 102-leaf
executable PRD trace, all 128 library tests and every non-ignored integration
test pass, including both iOS XCFramework gates and the three-game tinyvm/JSC/H5
differential. Warnings-denied all-feature and isolated `no_std` Clippy, arm64
iOS `no_std`, rustfmt and document-redaction gates pass. Default linked sizes
are 1,602,808 bytes arm64 and 1,681,792 bytes x86_64; the stripped static core
remains 101,256 bytes with selftest 42.

## One-hundred-tenth executable increment — snapshot resource quiescence

`NativeModuleRegistry` is now consumed when constructing its one
`GameRuntime`, closing the API path that could bind one runtime-local table and
domain into multiple instances. Every registry-created `HostResourceTable`
shares a private, type-erased live counter with that runtime. Insert/remove,
clear and table drop update the counter without exposing the platform object or
adding a platform backend to the VM.

`GameRuntime::suspend` first lets the guest execute its normal cleanup and state
submission, then checks all tracked tables. Any live native resource clears the
candidate state, latches the runtime failed and returns `native resources not
quiescent`; no portable snapshot becomes observable. Standalone resource tables
remain reusable by non-game embeddings, while only registry-created tables
participate in this game lifecycle contract.

Evidence on 2026-08-22: the real native-resource WAT fixture creates, reads and
closes a host object, then successfully emits a zero-byte guest snapshot. A
replacement runtime using a distinct table leaves its init-created object live;
the same guest suspend path is rejected and every later tick remains latched.
The executable PRD trace now binds all 103 completed leaves. The complete
all-feature/all-target package passes all 128 library tests and every
non-ignored integration test, including both iOS XCFramework gates and the
three-game tinyvm/JSC/H5 differential. All 115 isolated `no_std` library tests,
warnings-denied all-feature and isolated `no_std` Clippy, arm64 iOS `no_std`,
rustfmt and document-redaction gates pass. Default linked sizes are 1,603,208
bytes arm64 and 1,682,184 bytes x86_64; the stripped static core remains
101,256 bytes with selftest 42.

## One-hundred-eleventh executable increment — independent C cartridge authoring

The authoring boundary now has a producer independent of Rust and tinyvm. A
generic `build-c-cartridge.sh` uses a normal LLVM wasm32 backend, freestanding
C17 and `wasm-ld` to emit a standard module with no libc, WASI, JavaScript glue
or runtime library. The existing converter attaches the canonical manifest only
after linking. The checked-in 32×16 indexed2d fixture imports five ordinary
`tinyarcade:core/v1` functions, exports the complete lifecycle and persists four
bytes of guest state.

Evidence on 2026-08-22: LLVM 22.1.8 emits a 708-byte, one-page, MVP-only final
cartridge. Static validation reports five core function imports, zero resource
imports, no start function and no native capability. The independent black box
proves init, bounded rendering, input movement and fresh-instance snapshot
restore. A four-step replay agrees frame-for-frame in tinyvm, public
JavaScriptCore WebAssembly and a real headless H5 WebAssembly engine. This is a
development conformance fixture, not another nostalgia-arcade product game and
does not change the App Store bundled-only gate.

The executable PRD trace now binds all 104 completed leaves. The complete
all-feature/all-target package passes all 128 library tests and every
non-ignored integration test, including both iOS XCFramework gates and the new
four-cartridge tinyvm/JSC/H5 differential. All 115 isolated `no_std` library
tests, warnings-denied all-feature and isolated `no_std` Clippy, arm64 iOS
`no_std`, rustfmt and document-redaction gates pass. Linked runtime sizes remain
1,603,208 bytes arm64 and 1,682,184 bytes x86_64; the stripped static core
remains 101,256 bytes with selftest 42.

## One-hundred-twelfth executable increment — header-only C guest SDK

The C authoring path no longer asks each cartridge to duplicate Clang-specific
import attributes. `tinyarcade_guest_v1.h` defines fixed-width freestanding ABI
types, compile-time width assertions, every core v1 import declaration and one
lifecycle export macro. It contains no implementation, allocator, libc, WASI,
JS or tinyvm runtime dependency; unused declarations do not become imports.
`build-c-cartridge.sh` supplies only this include directory to the normal wasm32
compiler.

Evidence on 2026-08-22: the independent fixture now includes the public header
instead of declaring imports itself and still emits the exact 708-byte
MVP-only cartridge with only its five referenced functions. Its runtime,
fresh-instance snapshot restore and four-engine replay differential remain
green. The executable PRD trace now binds all 105 completed claims; shell
syntax, rustfmt, document redaction and diff checks pass. Runtime/iOS/static-core
artifacts are unchanged because the header is guest authoring source only.
