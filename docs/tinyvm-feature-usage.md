# tinyvm standard feature usage

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: implemented static report with real-cartridge evidence

`WasmModule::feature_usage` reports standard post-MVP feature families actually
present in an already decoded module. It does not instantiate the module,
execute its start function, grant a rejected feature or claim that every
instruction in a proposal is supported.

The current report covers:

- bulk memory;
- sign extension;
- nontrapping float-to-integer conversion;
- multi-value;
- reference types;
- multiple tables;
- multiple memories;
- extended constant expressions;
- tail calls.

`tinyvm module validate FILE.wasm` publishes the same information as a stable
comma-separated `standard_features=` row. A baseline scalar module reports
`(mvp-only)`. Independent standard fixtures provide positive evidence for every
reported family, while a minimal fixture protects against blanket “supported
means used” false positives.

## Real cartridge profile

`smoke-cartridge-feature-profile.sh` rebuilds the production Rust cartridges,
validates them and asserts their exact current profiles:

```text
Depth Well    bulk-memory,sign-extension
Paddle Guard  bulk-memory
Signal Lock   bulk-memory,sign-extension
```

This makes the near-term proposal priority evidence-based:

1. Bulk memory is a required baseline for every current production cartridge.
2. Sign extension is required by two of three current cartridges.
3. The remaining implemented families stay valuable for standard compatibility
   and fan tooling, but are not current game-build requirements.
4. SIMD, typed function references, GC, memory64, exceptions and threads have
   no demand from the current production cartridge set. They should enter only
   when a real candidate workload plus independent-engine and size/resource
   evidence justify them.

An exact profile change is not automatically a regression: a compiler or game
may legitimately begin using another standard feature. The smoke gate makes
that change explicit so the PRD, browser oracle and iOS resource evidence are
updated together instead of silently broadening the cartridge baseline.
