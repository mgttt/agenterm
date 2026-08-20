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
│   ├── host memory/table budgets     [x]
│   └── trap isolation                [~]
├── game host ABI                    [~]
│   ├── standard WASM cartridge       [~]
│   ├── version negotiation           [~]
│   ├── lifecycle init/tick/suspend   [~]
│   ├── input snapshot                [~]
│   ├── bounded render commands       [~]
│   ├── bounded audio commands        [~]
│   ├── clock/RNG determinism         [~]
│   ├── native capability registry    [~]
│   └── storage without guest network [ ]
├── artifact trust                    [ ]
│   ├── manifest + compatibility      [ ]
│   ├── content hash/signature        [ ]
│   ├── atomic cache/rollback         [ ]
│   └── reviewed catalog only         [ ]
├── cartridge ownership              [ ]
│   ├── official reviewed catalog     [ ]
│   ├── private user import           [ ]
│   ├── converter conformance kit     [ ]
│   └── no public arbitrary execution [ ]
├── iOS native bridge                 [ ]
│   ├── stable C lifecycle ABI        [ ]
│   ├── static library/XCFramework   [ ]
│   ├── Swift ownership/threading     [ ]
│   └── device build + lifecycle test [ ]
├── real-game proof                   [ ]
│   ├── constrained compiler profile  [ ]
│   ├── Depth Well WASM vertical cut  [ ]
│   ├── frame-time/resource evidence  [ ]
│   └── suspend/resume/save evidence  [ ]
└── distribution gate                 [ ]
    ├── fixed app purpose/offline game [ ]
    ├── catalog metadata/deep links   [ ]
    ├── review clarification/probe    [ ]
    └── fail closed on revoked content [ ]
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
JavaScriptCore can therefore serve as a later comparison or experimental
accelerator behind parity tests, but it is not the platform authority and no
game may require it. tinyvm remains the portable, deterministic baseline.

H5, DOM, JavaScript mini-app and WKWebView semantics are excluded. Runtime JIT,
device-side native AOT of downloaded modules, dynamic native-code loading,
WASI, guest network access and arbitrary third-party uploads are excluded.

The cartridge remains an ordinary standards-valid WebAssembly module. The
runtime does not add private opcodes or wrap executable bytes in a proprietary
format. Platform services are standard function imports under versioned module
names: v1 core uses `tinyarcade:core/v1`; future native modules receive their
own versioned namespaces and must be present in a host capability registry.
Unknown namespaces fail closed. Metadata may live in a standard WASM custom
section or adjacent signed manifest, so converters can emit and validate the
same cartridge contract without depending on the interpreter implementation.

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
The registry is partial until native-call time/resource budgets, manifest
capability declarations and the C/iOS registration surface are proven.

Evidence on 2026-08-21:

- Full `agenterm-tinyvm` suite: 143 passed, including six public game-runtime
  black-box tests.
- Clippy with warnings denied: clean.
- iOS device and Apple-silicon simulator library checks: clean.
- Stripped static core: 70,904 bytes; self-test 42.
