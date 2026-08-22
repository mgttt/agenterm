# PRD 02.35 — agenterm-tinyvm

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Execution plan: [tinyvm as an iOS game runtime foundation](../plan/goal-tinyvm-ios-game-runtime.md)

## Product definition

`agenterm-tinyvm` 是自有、跨平台、可预算的标准 WebAssembly 解释器，也是
TinyArcade 的运行底层。它接收普通 `.wasm`，在不生成或装载动态机器码的前提下完成
decode、validate、instantiate 和 execution；宿主通过标准 import table 提供版本化能力。

TinyArcade 是第一个 embedding 和持续验收负载，不是 VM 的语言边界。游戏便利能力必须
表现为标准 Wasm 或显式、版本化的 host import，不能演化成私有 opcode、私有卡带字节码
或只服务某一款游戏的解释器分叉。

目标结果是：iOS App 可以装载一枚经过审核的标准 `.wasm` 卡带，创建一个有界、持久的
instance，逐帧驱动游戏，安全 suspend/resume；单枚坏卡带只能失败自身，不能拖死、越界
分配或破坏 App。

## Capability tree

Legend: `[x]` 已有可执行证据 · `[~]` 部分完成 · `[ ]` 规划 · `[–]` 有意排除

```text
agenterm-tinyvm (35)                                      [~]
│
├── product boundary                                     [~]
│   ├── eval(bytes)                                       [x]
│   ├── host                                              [x]
│   ├── <100KiB>                                          [x]
│   └── iOS runtime boundary                              [x]
│       ├── interpret wasm                                [x]
│       ├── JIT native code                               [–]
│       ├── device-side AOT                               [–]
│       ├── dyn native loading                            [–]
│       └── H5/JS/WKWebView                               [–]
│
├── tinyvm engine                                        [x]
│   ├── scalar WebAssembly                                [~]
│   │   ├── control                                       [x]
│   │   ├── parametric                                    [x]
│   │   ├── locals                                        [x]
│   │   ├── memory                                        [x]
│   │   ├── i32                                           [x]
│   │   ├── i64                                           [x]
│   │   ├── f32                                           [x]
│   │   ├── f64                                           [x]
│   │   └── conv                                          [x]
│   ├── standard proposal profile                        [~]
│   │   ├── bulk memory copy/fill                         [x]
│   │   ├── bulk memory passive lifecycle                 [x]
│   │   ├── sign extension proposal                       [x]
│   │   ├── nontrapping float-to-int                      [x]
│   │   ├── multi-value proposal                          [x]
│   │   ├── single-table funcref profile                  [x]
│   │   ├── multiple defined funcref tables               [x]
│   │   ├── multiple internally defined memories          [x]
│   │   ├── extended constant expressions                 [x]
│   │   ├── tail-call proposal                            [x]
│   │   ├── SIMD proposal                                 [ ]
│   │   ├── typed function references                     [ ]
│   │   ├── GC proposal                                   [ ]
│   │   ├── memory64 proposal                             [ ]
│   │   ├── exception handling                            [ ]
│   │   └── threads/shared memory                         [ ]
│   ├── strict load gate                                 [~]
│   │   ├── decode complexity budget                      [x]
│   │   ├── strict declared-memory semantics              [x]
│   │   ├── strict memarg alignment                       [x]
│   │   ├── canonical function expression structure       [x]
│   │   ├── strict i64 signed-LEB range                   [x]
│   │   ├── valid custom-section names                    [x]
│   │   ├── empty memory-section vector                   [x]
│   │   ├── mutable global.set target                     [x]
│   │   ├── strict untyped select value domain             [x]
│   │   ├── export-declared ref.func                       [x]
│   │   ├── imported-global element expressions            [x]
│   │   ├── WABT load-gate oracle                         [x]
│   │   └── static module validation CLI                  [x]
│   └── bounded execution                                [~]
│       ├── persistent instance                           [x]
│       ├── start once                                    [x]
│       ├── per-call fuel                                 [x]
│       ├── explicit guest call stack                     [x]
│       │   ├── host-owned call-depth ceiling             [x]
│       │   ├── host-owned activation-slot ceiling        [x]
│       │   └── fallible execution-stack growth           [x]
│       ├── memory budget                                 [x]
│       ├── table budget                                  [x]
│       └── deterministic execution stats                 [x]
│           └── call/activation peak telemetry            [x]
│
├── standard host and linking                            [~]
│   ├── owned host ABI                                   [~]
│   │   ├── typed standard function imports               [x]
│   │   ├── bounded in-place host dispatch                [x]
│   │   ├── indexed guest-memory callback context          [x]
│   │   └── domain + generation guest resource handles     [x]
│   ├── standard resource imports                        [~]
│   │   ├── standard imported globals                     [x]
│   │   ├── standard imported linear memories             [x]
│   │   └── store-owned imported funcref tables           [x]
│   ├── standard resource exports                        [~]
│   │   ├── linked exported globals                       [x]
│   │   ├── linked exported memories                      [x]
│   │   └── linked exported tables                        [x]
│   └── cross-instance functions                         [~]
│       └── linked exported functions                     [x]
│           ├── numeric value signatures                  [x]
│           ├── store-owned funcref values                [x]
│           ├── opaque externref function/global values   [x]
│           └── standard externref tables                 [x]
│
├── game runtime                                         [x]
│   └── game ABI                                         [x]
│       ├── standard .wasm cartridge                      [x]
│       ├── manifest compatibility                        [x]
│       ├── core v1 imports                               [x]
│       ├── init/tick/suspend/resume                      [x]
│       ├── portable state snapshot                       [x]
│       ├── bounded frame output                          [x]
│       │   ├── recyclable host buffers                   [x]
│       │   └── stable two-pass copy lengths              [x]
│       ├── native module registry                        [x]
│       │   └── atomic resource-table factory             [x]
│       ├── App Store bundled-only gate                   [x]
│       └── machine host profile                          [x]
│           ├── catalog profile binding                   [x]
│           ├── exact zero-budget channel semantics       [x]
│           ├── profile-bound descriptor return           [x]
│           ├── typed compatibility issue report          [x]
│           └── exact-build Wasm feature negotiation      [x]
│
├── native I/O surface                                  [~]
│   ├── indexed2d presentation                           [~]
│   │   ├── bounded app metadata hot path                 [x]
│   │   ├── scoped immutable frame views                  [x]
│   │   └── single-buffer RGBA expansion                  [x]
│   ├── grid3d presentation                              [x]
│   │   └── allocation-free typed cell iteration          [x]
│   ├── tones playback                                   [~]
│   │   ├── bounded PCM tone synthesis                    [x]
│   │   ├── interruption / route / reset owner             [x]
│   │   ├── real App exact tone-event consumption          [x]
│   │   ├── single-buffer WAV + bounded wave LRU            [x]
│   │   └── physical speaker / headphone evidence          [ ]
│   ├── touch/keyboard/controller input                  [~]
│   │   ├── bounded multi-source aggregation              [x]
│   │   ├── Apple keyboard/gamepad adapter                [x]
│   │   ├── overlapping keyboard alias retention          [x]
│   │   ├── real App rising-edge input behavior           [x]
│   │   └── physical keyboard/gamepad evidence            [ ]
│   ├── scene lifecycle + persistence                    [~]
│   │   ├── real App shared session + frame pacer          [x]
│   │   └── protected prepublication snapshot replace     [x]
│   │       └── bounded prepared slot + borrowed restore slice [x]
│   └── physical-device lifecycle/audio/play             [ ]
│
├── evidence                                             [~]
│   ├── Depth Well grid3d                                 [x]
│   ├── Paddle Guard indexed2d                            [x]
│   ├── Signal Lock Swift-to-Wasm migration               [x]
│   ├── deterministic replay vectors                      [x]
│   ├── development JSC + H5 differential                [x]
│   ├── real iOS app consumer                            [x]
│   ├── TestFlight bundled-only candidate                [~]
│   └── physical-device play                             [ ]
│
├── roadmap                                              [~]
│   ├── P0 — close product evidence                      [~]
│   │   ├── TestFlight processing + install               [ ]
│   │   ├── physical device lifecycle                     [ ]
│   │   ├── physical frame-time/resource evidence         [ ]
│   │   └── physical audio-session evidence               [ ]
│   ├── P1 — reusable native modules                     [~]
│   │   ├── generalize memory-zero call-scoped borrowing   [x]
│   │   ├── explicit selected-memory callback context      [x]
│   │   ├── unified host/guest handle lifetimes             [x]
│   │   ├── cross-runtime non-reused table domains          [x]
│   │   ├── native-resource snapshot quiescence             [x]
│   │   ├── versioned native import conventions           [x]
│   │   ├── converter-visible compatibility reports       [x]
│   │   └── platform-neutral host architecture            [~]
│   │       ├── contract / abstraction / backend split     [x]
│   │       ├── internal handles + preopen-only paths       [x]
│   │       ├── optional WASI Preview 1 adapter             [x]
│   │       │   ├── args + environ                           [x]
│   │       │   ├── clock + random                           [x]
│   │       │   ├── preopen discovery + fd_close             [x]
│   │       │   ├── fd read/write/seek/stat                   [x]
│   │       │   ├── path open/unlink                          [x]
│   │       │   └── proc_exit                                 [x]
│   │       └── platform backends outside VM                [~]
│   │           ├── capability-directory std backend         [x]
│   │           ├── iOS Simulator App container wiring        [x]
│   │           └── physical iPhone container evidence        [ ]
│   ├── P2 — accepted standard Wasm coverage             [x]
│   │   ├── proposal priority by real cartridge workload  [x]
│   │   ├── optional SIMD signed-PCM mix subset            [x]
│   │   │   └── signed PCM saturating add/sub               [x]
│   │   ├── independent WABT/JSC differential per leaf    [x]
│   │   └── size/resource budget retained per leaf        [x]
│   ├── P3 — cartridge authoring ecosystem               [~]
│   │   ├── converter/conformance ecosystem               [~]
│   │   ├── fan-authored standard .wasm                   [x]
│   │   │   └── header-only C core v1 declarations        [x]
│   │   └── external distribution after Apple approval    [ ]
│   └── research queue                                   [~]
│       ├── QJWasm ownership + low-copy lessons           [~]
│       ├── unified host/guest handle lifetimes            [x]
│       ├── bounded call/callback/completion channels      [x]
│       ├── event-loop-neutral async completion ABI        [x]
│       │   ├── owner-thread completion queue core           [x]
│       │   ├── versioned guest import protocol              [x]
│       │   ├── C ABI channel ownership + late delivery      [x]
│       │   ├── Swift MainActor owner + host profile         [x]
│       │   ├── standard async cartridge fixture             [x]
│       │   └── booted iOS Simulator completion lifecycle    [x]
│       ├── cross-boundary copy/call benchmarks            [x]
│       └── JavaScriptCore remains development oracle     [~]
│
├── slot-B                                               [ ]
│   └── dynamic native execution                         [–]
│
└── non-goals
    ├── cu                                                 [x]
    ├── dyn                                                [–]
    ├── chassis                                            [x]
    ├── #78                                                [x]
    ├── WASI as implicit/default game host                 [x]
    ├── APE                                                [x]
    └── WAT                                                [x]
```

The first tree is executable product truth: every `[x]` leaf is backed by an owning
integration test or a three-edge golden family. `[~]` means the branch has useful
implementation but still lacks one or more stated acceptance gates; it must not be read
as “almost approved” or “safe to ship externally.”

## Product boundaries and invariants

### 1. Standard Wasm is the executable format

- Input is a standard WebAssembly binary module. WAT is a development source format,
  not a runtime input.
- TinyArcade metadata lives in a standard custom section. Host capability use remains
  ordinary versioned function imports.
- `tinyvm module validate FILE.wasm` validates an ordinary module without requiring a
  TinyArcade manifest, binding imports, instantiating it or running its start function.
- The VM never invents a private opcode to make a game easier to implement.

### 2. Interpretation is the iOS execution boundary

- Downloaded or bundled `.wasm` is interpreted. The runtime does not generate executable
  pages, device-AOT downloaded code or load a cartridge-supplied dynamic library.
- Build-time compilation from game source to standard `.wasm` is expected and is not the
  excluded device-side AOT path.
- `agenterm-dyn` is a separate desktop native door and is never a tinyvm execution backend.
- JavaScriptCore WebAssembly is a development oracle through `JSContext`, not the shipped
  authority, dependency or fallback.

### 3. Untrusted input is bounded before allocation or mutation

- Cartridge bytes are capped by the embedding; every allocation-amplifying section record
  is additionally charged to one decode-complexity budget before reserve.
- Memory pages, table elements, call depth, aggregate live activation slots and per-call
  instructions are host-owned ceilings.
- Guest-sized vectors use fallible growth. A malformed count must return a typed error,
  never abort the process while attempting an infallible allocation.
- Load-time type/structure errors fail before a module becomes invokable. Runtime traps
  remain local to the affected instance.

### 4. VM capability and TinyArcade profile are separate

- The general VM supports typed numeric/reference host calls, imported/exported resources,
  multiple memories and tables, and cross-instance linking.
- TinyArcade core/native v1 deliberately stays i32-only, requires exactly one defined memory
  zero, and currently rejects table imports. Those are embedding rules, not VM language limits.
- New standard proposals enter only after decode, validation, execution, resource-budget and
  independent-engine evidence agree. “A reference engine accepted it” is not sufficient by
  itself.

### 5. Host ownership is explicit

- Imported memories, tables, globals and linked functions belong to an explicit `WasmStore`.
  Clone/re-export/sibling binding must preserve one live identity rather than copy snapshots.
- `funcref` stores a store-owned function address; `externref` stores only an opaque,
  process-unique token. Tinyvm never treats a native object address as a guest value.
- The current host callback receives call-scoped parameter/result slices and a zero-copy
  mutable view of memory zero. Future multi-memory context must identify the selected memory,
  remain synchronous and bounded, and be impossible to retain after callback return; it must
  not use `unsafe` merely to bypass borrow checking.
- Native async work uses an event-loop-neutral bounded completion queue. It reserves request
  count and maximum response bytes before work starts, uses runtime-local generation-checked
  tickets, transfers completed payload ownership without a second copy, and must quiesce before
  portable suspend. Platform scheduling and versioned module imports remain outside the VM.
- The iOS C/Swift boundary owns completion channels separately from runtime handles. A channel
  binds to at most one runtime, cannot close while bound, clears tickets when that runtime closes,
  and rejects wrong-thread or late delivery. Host profiles publish the same completion imports.

### 6. App Store distribution is a separate authority gate

- The production Swift package defaults to `appStoreBundledOnly`; external catalog/private
  cartridge APIs are excluded from the shipping app package unless a release explicitly opts
  into an Apple-approved policy.
- A valid signature, manifest or host profile proves artifact identity/compatibility, not
  Apple permission to download and execute external code.
- Official reviewed catalog, private user import and public fan marketplace are distinct
  product paths. None inherits authority from another.

## Current capability summary

| Layer | What exists now | Safe failure |
|---|---|---|
| Module | Standard binary decode, static validation, custom sections, imports/exports and start | Decode/validation error before execution |
| Execution | Persistent instance, explicit activation trampoline, fuel and resource telemetry | Deterministic trap local to the instance |
| Values | i32/i64/f32/f64, funcref and opaque externref through supported standard locations | Exact type mismatch rejection |
| Resources | Defined/imported/exported globals, memories and funcref/externref tables | Binding/type/limit rejection or borrow-conflict trap |
| Host ABI | Typed arbitrary-arity compatibility callback, fixed 16-value hot path, generation-checked guest resources, bounded native completion queue and optional capability-directory std backend | Callback/platform error propagated without ambient-path reconstruction; saturation/oversize/stale completion fails typed, while persisted snapshots must quiesce native resources |
| Optional WASI P1 | Sixteen exact process/clock/random/preopen/descriptor/path/exit imports over the neutral host contract | Unknown/wrong imports fail binding; bad memory, path, rights or backend absence returns explicit errno/interruption |
| TinyArcade | Manifest, core v1, native registry, lifecycle, deterministic RNG/clock, render/audio/state bounds | Cartridge fails closed; App owner remains alive |
| iOS | C ABI, XCFramework, Swift package, input/frame pacing, persistence, replay and native 2D/3D/audio owners | Main-actor owner latches bad runtime and clears stale output |
| Tooling | Validate, attach-manifest, descriptor, host profile, replay and deterministic catalog publisher | No output publication after failed preflight |

## Game-platform contract

```text
standard .wasm cartridge
    → static module validation
    → TinyArcade manifest + import/export compatibility
    → origin / trust / App Store policy
    → bounded persistent instance
    → init
    → repeated input + monotonic clock → tick → render/audio
    → suspend snapshot + host clock
    → resume or discard-and-create-fresh
```

The core namespace is `tinyarcade:core/v1`. Native extensions use separate,
versioned namespaces compiled into the App and registered with exact signatures and
per-lifecycle quotas. A cartridge declaration requests compatibility; it does not grant
itself a native implementation or distribution authority.

Media is discriminated and bounded:

- `tinyarcade:grid3d/v1` — logical 3D grid records for Depth Well–style scenes;
  Swift owns one immutable frame and iterates typed cells without a second array.
- `tinyarcade:indexed2d/v1` — indexed pixels plus palette for classic 2D games.
- `tinyarcade:tones/v1` — bounded sequential tone events and aggregate duration.

Portable replay stores the exact cartridge hash, manifest identity, initial snapshot,
monotonic input/clock facts and per-frame output length/digest. Execution still uses the
original `.wasm`; replay is evidence, not another executable format.

## Roadmap decisions

### P0 — finish evidence on the product that already exists

The next release gate is not another interpreter feature. It is Apple-side TestFlight
processing/installability followed by physical-iPhone lifecycle, frame-time, resource,
input feel and audio-session evidence. Simulator and archive evidence cannot close these
leaves.

### P1 — native modules without a hidden copy tax

The existing synchronous host door already lends memory zero directly, without copying the
whole guest memory. The next reusable design must generalize that fact for standard
multi-memory modules: specify memory selection, immutable/mutable access, re-entry rejection,
growth exclusion, result reservation before callback, C/Swift pointer lifetime and
deterministic failure. QJWasm is useful here for ownership and low-copy lessons; QuickJS, its
unsafe threading model and its JavaScript product dependency are not adopted.

QJWasm also supplies research targets beyond raw memory access: one lifetime model for host
objects and guest handles, explicit call/callback/async-completion channels, event-loop
integration that does not hard-code QuickJS, and benchmarks that expose boundary copies and
dispatch cost. tinyvm will translate those lessons into bounded native-module ABI work; it
will not make JavaScript, QuickJS or a second runtime part of the iOS product dependency.

The host implementation follows three boundaries. The contract is a versioned Wasm import
namespace: TinyArcade remains the game profile, while a separately enabled
`wasi_snapshot_preview1` subset may serve non-game embeddings. A `no_std` abstraction owns
opaque handles, clocks, random filling, byte I/O, exit state and a virtual preopen table; the
guest never sees OS descriptors or physical paths. Platform backends live outside the VM:
Unix may share an implementation with cfg-gated macOS sandbox handling, while Windows and
iOS require separate path/handle/storage policy. Unsupported operations return explicit
`NOSYS`/`NOTCAPABLE`-equivalent errors.

This remains one published `agenterm-tinyvm` crate. Core, contracts, host abstractions and
optional platform backends may be separate modules and feature gates, but there are no
forked Unix/iOS/Windows VM crates with drifting semantics. The default build stays
`no_std + alloc`; enabling a backend must not alter standard Wasm validation or execution.

`host/` itself is not a standard. Normative inputs are WebAssembly Core imports/exports and
linear memory, WASI Preview 1 when that optional adapter is selected, and—only for a future
component profile—WIT, Canonical ABI and the Component Model. Wasmtime/Wasmi/WAMR/wasm3 and
the Wasm C API are implementation references, not the contract. ISA portability continues
to come from portable guest and host code, never dynamic host machine code.

### P2 — grow standard coverage from workload evidence

Every feature family reported by `WasmModule::feature_usage` now has an authoritative matrix
row connecting a standard fixture, executable WABT/tinyvm/JavaScriptCore differential and a
default or opt-in product-size profile. The signed-PCM SIMD subset additionally runs in H5 as
a development oracle. JavaScriptCore's lack of multiple-memory support is recorded as an
explicit capability boundary rather than weakening the WABT/tinyvm semantic check.

This closes the acceptance workflow for the current reported feature set, not every opcode in
every WebAssembly proposal. Typed function references, GC, memory64, exception handling and
threads remain separately workload-gated proposals. A future family may be reported only after
adding its fixture, independent oracle, executable gate and size profile to the standard-feature
matrix; no decoder-only implementation counts as accepted coverage.

### P3 — authoring and cartridge ecosystem

The converter/conformance path targets standard `.wasm` plus the versioned host profile. A
freestanding C fixture now proves that a non-Rust, non-tinyvm producer can compile, attach the
canonical manifest, execute, snapshot and match tinyvm/JSC/H5 replay output. A usable creator
product and external catalog remain product goals; App Store distribution is enabled only after
a verifiable Apple-approved route exists. Development-time Safari/JSC execution stays useful as
an oracle and never becomes the nostalgia-arcade runtime.

## Acceptance and evidence

The runtime foundation is complete only when all of the following are true:

1. Standard modules are independently validated and every accepted proposal has decode,
   validation, execution, budget and differential evidence.
2. Malformed or hostile modules fail through typed errors without allocator abort, native
   stack overflow, stale output publication or cross-instance corruption.
3. Rust, C and Swift owners agree on lifecycle, lengths, identity and resource accounting.
4. At least two structurally different real games run through the same public ABI and produce
   deterministic replay evidence.
5. The exact current-main runtime is consumed by a real arm64 iOS App target.
6. The TestFlight build is processed, installable and exercised on a physical iPhone across
   foreground/background, interruption, save/restore, input, rendering and audio.

Current evidence owners:

- [Executable goal and incremental evidence](../plan/goal-tinyvm-ios-game-runtime.md)
- [Optional WASI Preview 1 profile](../docs/tinyvm-wasi-preview1.md)
- [Capability-based std host](../docs/tinyvm-std-host.md)
- [Optional iOS WASI host](../docs/tinyvm-ios-wasi-host.md)
- [Selected-memory host callbacks](../docs/tinyvm-selected-memory-host.md)
- [Host resource table](../docs/tinyvm-host-resource-table.md)
- [Native completion channel](../docs/tinyvm-native-completions.md)
- [Optional SIMD audio profile](../docs/tinyvm-simd-audio.md)
- [JavaScriptCore public/private boundary](../docs/tinyarcade-javascriptcore-boundary.md)
- [Converter conformance](../docs/tinyarcade-converter-conformance-v1.md)
- [C cartridge authoring](../docs/tinyarcade-c-authoring-v1.md)
- [Catalog transport](../docs/tinyarcade-catalog-transport-v1.md)
- [Catalog publisher](../docs/tinyarcade-catalog-publisher-v1.md)
- [QJWasm/WA2X source review](../docs/qjwasm-wa2x-source-review.md)
- [Cross-boundary benchmark](../docs/tinyvm-boundary-benchmark.md)
- [Real-cartridge feature usage](../docs/tinyvm-feature-usage.md)
- [Accepted standard-feature matrix](../docs/tinyvm-standard-feature-matrix.md)
- `crates/agenterm-tinyvm/tests/` — public Rust black boxes and independent fixtures
- `crates/agenterm-tinyvm/ios/` — C/Swift package and platform smoke gates

## Explicit non-goals

- H5 mini games, DOM APIs, a WKWebView game shell or JavaScript as the cartridge platform.
- JIT, downloaded native code, cartridge-provided dylibs or device-side AOT on the iOS path.
- WASI as an implicit/default game host surface. An optional, versioned Preview 1 profile may
  be implemented separately; files, network, rendering and storage are never ambient powers.
- WAT parsing in the runtime. Producer tooling may compile WAT to standard `.wasm`.
- Claiming App Review approval from technical validation, TestFlight upload or a release note.
- Replacing `agenterm-dyn`, `agenterm-cu` or `agenterm-chassis`; their product boundaries stay
  separate.
