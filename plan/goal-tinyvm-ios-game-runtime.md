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
├── game host ABI                    [ ]
│   ├── version negotiation           [ ]
│   ├── lifecycle init/tick/suspend   [ ]
│   ├── input snapshot                [ ]
│   ├── bounded render commands       [ ]
│   ├── bounded audio commands        [ ]
│   ├── clock/RNG determinism         [ ]
│   └── storage without guest network [ ]
├── artifact trust                    [ ]
│   ├── manifest + compatibility      [ ]
│   ├── content hash/signature        [ ]
│   ├── atomic cache/rollback         [ ]
│   └── reviewed catalog only         [ ]
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
