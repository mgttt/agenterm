# TinyArcade converter conformance v1

Fan tools should emit an ordinary WebAssembly 1.0 MVP `.wasm`, not a TinyArcade
bytecode wrapper. The app-specific contract consists only of standard imports,
exports, a standard custom manifest section and versioned media/state records.

Run the same black-box gate used by the runtime repository:

```sh
cargo run -p agenterm-tinyvm --bin tinyvm -- \
  cartridge inspect path/to/game.wasm

cargo run -p agenterm-tinyvm --bin tinyvm -- \
  cartridge check path/to/game.wasm
```

`inspect` parses canonical identity/schema metadata and the normal WASM import
table without executing guest code. It reports every import's namespace,
function field, i32 signature and core/native classification so a converter can
give an exact compatibility report. `check` uses the private-import policy and
therefore rejects every native module import. It then enforces a 2 MiB file
ceiling, 64 memory pages, 1,024
table elements, one million interpreted instructions per lifecycle call and
the ordinary frame/audio/state byte budgets.

```text
converter check
├── regular non-empty file within 2 MiB
├── standard WASM envelope and canonical TinyArcade manifest
├── exact lifecycle exports and core import signatures
├── no private-import native capability namespace
├── bounded init and first tick
├── valid tinyarcade:grid3d/v1 frame
├── empty audio or valid tinyarcade:tones/v1 batch
├── bounded portable suspend state
├── fresh instance resume
└── byte-identical render/audio replay from the same input, clock and RNG
```

Passing this command establishes technical compatibility for a user's private
library. It does not sign, publish or approve the game for the official catalog.
Official review additionally owns product quality, rights/provenance, metadata,
policy and the signed catalog record.

The normative wire details remain in:

- `docs/tinyarcade-cartridge-abi-v1.md`
- `docs/tinyarcade-media-stream-v1.md`
- `docs/tinyarcade-signed-catalog-v1.md`
