# tinyvm accepted standard-feature matrix

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: executable acceptance gate for every feature family reported by
`WasmModule::feature_usage`

The authoritative inventory is
[`standard-feature-matrix.tsv`](../crates/agenterm-tinyvm/tests/fixtures/standard-feature-matrix.tsv).
It currently contains 10 reported feature families, 11 independent fixtures and 10 executable
gates. Reference types deliberately has separate `funcref` and `externref` fixtures.

Each row must name:

1. the exact reported standard feature family;
2. one standard WAT fixture that decodes to that feature usage;
3. an executable `smoke-wabt-*.sh` semantic gate;
4. the independent oracle used by that gate; and
5. the default or optional product-size profile that pays for the capability.

Run the complete acceptance workflow from the repository root:

```sh
CARGO=~/.cargo/bin/cargo \
  ./crates/agenterm-tinyvm/smoke-standard-feature-matrix.sh
```

The workflow compiles and validates every fixture with WABT, executes it in tinyvm and macOS
JavaScriptCore, then runs the default static-core and iOS product budgets. The optional SIMD
row also runs in a real headless H5 browser and pays separate static-core and iOS budgets.
JavaScriptCore currently rejects multiple memories, so that row records the engine capability
boundary while WABT and tinyvm still prove exact execution semantics.

Evidence on 2026-08-22:

- default stripped static core: 101,256 bytes, below the unchanged 100 KiB ceiling;
- opt-in `staticcore,simd`: 117,768 bytes, below its separate 120 KiB ceiling;
- default arm64/x86_64 iOS consumers: 1,663,848 / 1,751,992 bytes;
- opt-in SIMD arm64/x86_64 iOS consumers: 1,681,224 / 1,755,864 bytes;
- focused opt-in SIMD Swift consumer: 1,510,904 bytes.

This matrix proves the accepted subsets implemented and reported by tinyvm. It does not claim
complete coverage of every instruction in the upstream proposals. A future proposal or newly
reported subset is incomplete until its matrix row, independent semantics and product budget
all pass together.
