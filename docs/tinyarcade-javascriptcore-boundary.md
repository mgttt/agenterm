# JavaScriptCore WebAssembly boundary

Status: development research, checked 2026-08-21 against Xcode 26.6 and the
Apple guidelines last updated 2026-06-08.

## Decision

JavaScriptCore WebAssembly is a useful independent development oracle for
tinyvm. It is not the TinyArcade product runtime and does not enter the
nostalgia-arcade iOS package.

The distinction is capability control, not whether JSC can execute Wasm. The
public `JavaScriptCore.framework` exposes `JSContext`, `JSVirtualMachine`,
script evaluation and native object/function bridging. A standard JavaScript
`WebAssembly.Module`/`Instance` can run inside that context. Apple does not
publish a separate Swift/Objective-C Wasm module API, guest instruction fuel,
per-call execution budget, synchronous cancellation contract or VM heap hard
limit. A separate `JSVirtualMachine` enables concurrency and object-space
separation, but its documented API does not make untrusted synchronous guest
execution preemptible.

Sources:

- [Apple JavaScriptCore overview](https://developer.apple.com/documentation/javascriptcore)
- [Apple JSContext](https://developer.apple.com/documentation/javascriptcore/jscontext)
- [Apple JSVirtualMachine](https://developer.apple.com/documentation/javascriptcore/jsvirtualmachine)
- [WebKit: Safari 18.4 Wasm without JIT](https://webkit.org/blog/16574/webkit-features-in-safari-18-4/)
- [WebKit: Safari 26 in-place Wasm interpreter](https://webkit.org/blog/17333/webkit-features-in-safari-26-0/)

## Public and private boundary

Allowed development use stays on the documented framework surface:

```text
JSContext.evaluateScript
  → standard JavaScript WebAssembly globals
  → explicit JS/Swift host functions
  → exact development-only differential evidence
```

WebKit source can be read to understand implementation behavior, but its C++
`JSC::Wasm::*`, tier controls, private headers, `jsc` testing flags and private
entitlements are not app APIs. Do not redeclare or `dlsym` an unlisted symbol,
modify executable-memory/code-signing behavior, or borrow Safari/browser-engine
entitlements.

The installed iOS 26.5 SDK demonstrates why export visibility is not API
permission: `JSContextGroupSetExecutionTimeLimit` and
`JSContextGroupClearExecutionTimeLimit` appear in `JavaScriptCore.tbd`, but
neither is declared anywhere in the public framework headers. They are private
SPI and must not be used. App Review Guideline 2.5.1 requires public APIs.

## Exploratory capability results

One non-shipping probe used only public `JSContext` plus standard JS/Wasm on
this Apple Silicon host. It is discovery evidence, not a compatibility promise:

| Probe | macOS JSContext | iOS 26.5 Simulator JSContext |
|---|---:|---:|
| module/instance + function import | pass | pass |
| memory/grow/maximum | pass | pass |
| table/call_indirect | pass | pass |
| extended constant expressions | pass | not claimed |
| imported numeric globals + const `global.get` | pass | not claimed |
| named table/memory/global exports | pass | not claimed |
| SIMD | pass | pass |
| Wasm exceptions | pass | pass |
| shared memory/atomics | reject | pass |
| multiple memories | reject | not claimed |
| built-in WASI host | absent | absent |
| narrow mock WASI-style import | pass | pass |

The shared-memory disagreement is itself a result: macOS or Simulator behavior
must not be projected onto a physical iPhone. Threads/shared memory, GC,
exceptions, SIMD or any later WebKit feature cannot silently enter the
TinyArcade cartridge baseline. Each standard extension needs an explicit tinyvm
implementation, conformance suite and host-profile version decision.

The checked-in multiple-memory oracle records a narrower capability boundary:
WABT 1.0.41 and tinyvm execute the same standard module with the same result,
while this Mac's public `JSContext` WebAssembly path rejects a module containing
more than one memory. That absence is recorded rather than treated as a tinyvm
failure; WABT remains the independent executable oracle for this proposal.

JSC has no built-in WASI host. Supplying `wasi_snapshot_preview1` functions in
JavaScript can satisfy imports, but that creates a host runtime rather than
discovering an Apple-provided one. TinyArcade deliberately keeps its smaller
`tinyarcade:core/v1` surface and does not expose general files, sockets,
selectors or Apple APIs.

## App Review boundary

The current [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)
say, in relevant part:

- 2.5.1 requires public APIs.
- 2.5.2 generally requires self-contained bundles and restricts downloaded code
  that introduces or changes functionality.
- 4.7 names HTML5/JavaScript mini apps and games, streaming games, chatbots,
  plug-ins and downloadable games for retro console/PC emulators.
- 4.7.2 requires prior Apple permission before extending native platform APIs
  to that software; 4.7.4 requires an index, metadata and universal links.

The text does not expressly name a custom interpreter executing remotely
downloaded standard `.wasm`. Therefore a remote TinyArcade catalog is not
presumed approved merely because JSC or Safari can run the same module. The
shipping default remains bundled-only. External reviewed/private cartridge
features require an explicit Apple approval record and must not be disguised as
resources or H5.

## TinyArcade use

The supported development flow is:

```text
same standard .wasm + same TAR1 snapshot/input/clock/RNG
├── tinyvm (product authority)
└── JavaScriptCore WebAssembly (development oracle)
       ↓
compare exact render/audio length and SHA-256 per step
```

The checked-in oracle implements exactly that flow for Depth Well and Paddle
Guard. It does not compare screenshots, depend on browser timing, or grant JSC
authority over TinyArcade ABI semantics. When engines disagree, reduce the
module and adjudicate it against the WebAssembly and frozen host-ABI contracts.

Physical-device experiments may measure JSC feature support and performance,
but even a fast successful probe would not provide the missing public fuel,
preemption and heap-budget contracts. Those controls remain reasons for tinyvm
to be the cross-platform Wasm VM underneath the application platform.
