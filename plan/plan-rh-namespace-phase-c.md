# Phase C prep: retire live `rhai::` namespace

Tip baseline: `e6c83c70` era (rev82 Native he=1 for script/remote-ui/working-context;
workbench/unix `pack=ok`). SSOT for sequencing; do not invent a second living file map.

## Outcome

Live `.rh` scripts and AOT packs stop branding host APIs as `rhai::`.
`agenterm-rhai` PE / Engine eval fallback are removed only after native emit covers the corpus.

## Inventory (scripts/rh, non-archive)

| Metric | Count |
|--------|------:|
| `rhai::` call sites (live `scripts/rh`) | **0** after Wave 3 |
| Pre-Wave-3 baseline | ~648 / 65 files |
| Distinct `module::fn` (pre-rename) | 19 |
| Top modules (pre-rename) | json 290, task 122, crypto 121, runtime 82 |
| Live `*.rhai` outside `scripts/archive/rhai/` | **0** |
| `agenterm.tasks.json` `.rhai` entries | **0** |
| Operational `scripts/rhai/` tree | **archived** → `scripts/archive/rhai/` (74 files) |

Top surfaces: `json::parse`, `crypto::sha256_file`, `task::sleep`, `runtime::atomic_write`, `json::parse_file`.

`rh::fail` (≈89) is transpile-only — **not** a model for host API rename.

**Operational scrub (M42f8e, 2026-08-08):** skills/docs no longer present
`scripts/rhai/*.rhai` or `agenterm-rhai` as the live operator front door.
Remaining ~354 `agenterm-rhai` mentions are shim PE / compat tests / historical
docs / policy guards — see [`rhai-trace-scrub-notes.md`](rhai-trace-scrub-notes.md).

## Binding owners (must move together)

| Layer | Owner |
|-------|-------|
| Engine `register_static_module("rhai", …)` | `src/script_stdlib.rs` + clipboard/image/task/http |
| Catalog / shipped surfaces | `src/script_catalog.rs`, `crates/agenterm-rh/src/shipped_surfaces.rs` |
| AOT matchers / dual-prefix | `crates/agenterm-rh/src/{transpile,host_api}.rs` |
| Pack eval fallback | `src/script_rh_host.rs` (`host_eval_snippet` / run-script) |

## Sequenced leaves

1. **Wave 1 — dual alias window:** ✅ shipped at codegen **rev80**.
2. **Wave 2 — native emit gaps:** ✅ through codegen **rev82** — script-smoke / remote-ui / working-context **Native he=1**; working-context AOT pack builds; script-smoke/remote-ui still have **AOT typecheck debt** (`pack=fail`).
3. **Wave 3 — script mass-rename:** ✅ live `scripts/rh/**` has **0** `rhai::` call sites.
4. **Wave 4 — Phase C archive:** drop `rhai` Engine module, eval/run-script Rhai paths, then `agenterm-rhai` PE; scrub residual branding.

**Non-goal:** inventing permission/sandbox policy under Script Runtime.

## Next leaves (Wave 4 gate)

Ordered; 4.1 blocks 4.3; 4.5 follows 4.3; 4.6–4.7 follow 4.5.

| # | Leaf | Exclusive owner(s) | Evidence |
|---|------|--------------------|----------|
| 4.1 | AOT typecheck debt + remaining HE emit for Native packs | `crates/agenterm-rh/src/transpile.rs` (+ `host_api.rs` if helpers) | script-smoke/remote-ui `mode_probe --pack` → `pack=ok`; lock `*_pack_builds` |
| 4.2 | Drop Engine legacy `rhai` module + catalog/shipped aliases | `src/script_stdlib.rs`, `crates/agenterm-rh/src/host_api.rs`, `src/script_catalog.rs`, `crates/agenterm-rh/src/shipped_surfaces.rs` | zero `register_static_module(…"rhai"`; catalog `rh::` only |
| 4.3 | Remove pack Rhai eval/run-script fallback | `src/script_rh_host.rs`, `crates/agenterm-rh/src/{host_api,transpile}.rs` | no prod `host_eval_snippet` / `host_run_script_source` |
| 4.4 | Migrate Engine-root-dependent tests/fixtures | `src/script_{stdlib,task,catalog,http,worker}.rs`, `tests/rh_*.rs`, `crates/agenterm-rh/tests/**` | `cargo test -p agenterm --lib` + `agenterm-rh` green |
| 4.5 | Retire `agenterm-rhai` PE + `ScriptBackend::Rhai` + REPL/worker interpreted path | `src/bin/agenterm-rhai.rs`, `Cargo.toml`, `src/script_{backend,worker,repl}.rs`, `src/client/mod.rs` | five product bins in matrix |
| 4.6 | Packaging / install / bootstrap / smokes | `scripts/artifacts.json`, `install.sh`, `scripts/rh/{check,artifact-verification,*smoke}.rh` | stage-build + artifact-verification |
| 4.7 | Retire PE integration tests + caller-inventory baseline | `tests/rhai_migration.rs`, `tests/script_repl.rs`, `tests/linux_script_cli.rs`, `tests/rh_cli_forward.rs`, `fixtures/rh/caller-inventory-baseline.json`, … | `caller-inventory` / `rh_corpus` green |
| 4.8 | Residual operational trace scrub | `scripts/rh/script-smoke.rh`, `skills/**`, `README.md`, `AGENTS.md`, PRD nodes | intentional historical docs only |

Do not edit `scripts/archive/rhai/**` except as historical reference.

## Evidence per wave

- Wave 1: ✅ rev80 dual-alias; catalog + `agenterm-rh` tests green.
- Wave 2: ✅ rev82; script-smoke/remote-ui/working-context Native he=1; working-context AOT pack builds; script-smoke/remote-ui AOT cargo still fails.
- Wave 3: ✅ live `scripts/rh` `rhai::`=0.
- Wave 4: zero live `rhai::` / `agenterm-rhai` operator paths outside archive + intentional historical docs.
