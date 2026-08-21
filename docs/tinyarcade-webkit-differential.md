# TinyArcade development WebKit differential

This gate answers one narrow question: given the same ordinary `.wasm` game and
the same deterministic TinyArcade host facts, do tinyvm and another WebAssembly
implementation emit the same observable frame bytes?

It is a development oracle, not an alternate product runtime. Nothing in this
directory authorizes a cartridge, replaces the iOS interpreter, adds browser
semantics to the ABI or ships JavaScript/H5 inside nostalgia-arcade.

## Contract

```text
same cartridge.wasm
├── tinyvm + core/v1 host → TAR1 replay evidence
└── JavaScriptCore WASM + reference core/v1 host
                              ↓
                 exact per-step comparison
                 ├── render byte length + SHA-256
                 └── audio byte length + SHA-256
```

The TAR1 trace binds the exact cartridge SHA-256 and contains the initial
portable TGS1 snapshot, host RNG state and monotonic input/clock steps. The
JavaScriptCore adapter restores those facts before its first compared tick. Its
host imports enforce the same input mask, monotonic clock, xorshift32 RNG,
single render/audio submission and media/state byte ceilings.

Raw output evidence is authoritative. Screenshots or perceived similarity can
hide palette, command, audio or stale-frame differences and are therefore not
the differential criterion.

## Run

On macOS with Xcode command-line tools:

```sh
crates/agenterm-tinyvm/smoke-webkit-differential.sh
```

The script builds the real Depth Well and Paddle Guard cartridges, records and
checks their canonical replay with tinyvm, compiles a temporary Swift/
JavaScriptCore oracle with warnings denied, and checks every frame again in
JavaScriptCore. All temporary binaries and traces are removed on exit.

The same adapter can be loaded by a Safari development harness later, provided
the harness preserves this contract. DOM timing, `requestAnimationFrame`,
keyboard events and canvas rendering must stay outside the comparison core;
normalize them into the recorded input/clock stream first.

## Interpreting a mismatch

A mismatch does not automatically mean tinyvm is wrong: the WebAssembly spec,
the frozen TinyArcade ABI and the replay bytes remain the authorities. Reduce
the failing cartridge and step, distinguish guest execution from host-import
semantics, and add the minimal case to the permanent regression suite. Never
change core/v1 merely to make two implementations agree.
