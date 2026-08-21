# TinyArcade converter conformance v1

Fan tools should emit an ordinary standards-valid WebAssembly `.wasm`, not a
TinyArcade bytecode wrapper. The app-specific contract consists only of
standard imports, exports, a standard custom manifest section and versioned
media/state records. The accepted v1 compiler profile includes the scalar MVP
plus the standard bulk-memory proposal for one memory and one MVP funcref table:
copy/fill, passive data/element segments, init/drop and table.copy. It is a
bounded standards profile, not a different VM instruction set.

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
├── unique ordered standard sections with no unconsumed payload
├── at most 262,144 allocation-amplifying decode records
├── exact lifecycle exports and core import signatures
├── indexed2d/v1 output declares indexed2d_version()
├── no private-import native capability namespace
├── bounded init and first tick
├── valid grid3d/v1 or indexed2d/v1 render frame
├── empty audio or valid tinyarcade:tones/v1 batch
├── bounded portable suspend state
├── fresh instance resume
└── byte-identical render/audio replay from the same input, clock and RNG
```

Two compiler-produced reference cartridges own both media branches:
`depth-well-0.1.0.wasm` exercises `grid3d/v1`, while
`paddle-guard-0.1.0.wasm` exercises `indexed2d/v1`. Both are ordinary standard
WASM modules built through the shared `build-rust-cartridge.sh` profile. Their
real Rust output retains `memory.copy`/`memory.fill` and DataCount instead of
lowering bulk work into MVP loops; neither receives a fixture-only loader.

The independent bulk-memory development gate compiles checked-in WAT with
WABT, validates the generated module with `wasm-validate`, then executes those
same bytes in tinyvm and system JavaScriptCore. Run
`crates/agenterm-tinyvm/smoke-wabt-bulk-memory.sh`; both engines must return 143
from a module that exercises passive data and funcref element lifetimes.

Before upload, a converter may additionally consume the exact app-build TAH1
profile defined in
[`tinyarcade-host-profile-v1.md`](tinyarcade-host-profile-v1.md).
`tinyvm cartridge check-profile` compares standard imports and declared
memory/table requirements without executing the guest or native callbacks.
This does not replace the dynamic lifecycle/media/determinism checks below:
step, frame, audio and state ceilings describe failure policy, not statically
provable guest behavior.

Converters should also retain deterministic replay vectors for representative
gameplay and every bug they fix. `tinyvm replay record` turns a bounded
`clock_ms buttons` input plan into a canonical `.tareplay`; `tinyvm replay
check` binds it to the exact cartridge SHA-256 and regenerates every render and
audio digest. The wire format, ceilings, commands and checked-in Depth
Well/Paddle Guard goldens are specified in
[`docs/tinyarcade-replay-v1.md`](tinyarcade-replay-v1.md).

During converter and runtime development, the same replay can also be executed
by a second standards implementation. On macOS the repository's
`smoke-webkit-differential.sh` runs the unmodified `.wasm` in the system
JavaScriptCore WebAssembly engine with the same snapshot, RNG, input and clock,
then compares every render/audio length and SHA-256 with tinyvm. This is a
differential oracle, not a substitute runtime: a match increases confidence in
WASM/ABI semantics, while a mismatch must be reduced and adjudicated against
the WebAssembly and TinyArcade contracts.

The oracle is development-only. It has no DOM, browser UI or network surface;
JavaScriptCore, JavaScript and H5 are not linked into the nostalgia-arcade iOS
runtime. A browser preview may be useful to a cartridge author, but passing one
does not grant App compatibility, catalog trust or Apple distribution approval.

Passing this command establishes technical compatibility for a user's private
library. It does not sign, publish or approve the game for the official catalog.
Official review additionally owns product quality, rights/provenance, metadata,
policy and the signed catalog record.

The normative wire details remain in:

- `docs/tinyarcade-cartridge-abi-v1.md`
- `docs/tinyarcade-media-stream-v1.md`
- `docs/tinyarcade-signed-catalog-v1.md`
- `docs/tinyarcade-catalog-transport-v1.md`
- `docs/tinyarcade-replay-v1.md`
- `docs/tinyarcade-webkit-differential.md`
- `docs/tinyarcade-javascriptcore-boundary.md`
