# ⚠️ Archived: Phase C `rhai::` namespace retirement plan

> Archived on 2026-08-10. Phase C/M42f8 was completed; current Rh execution work and
> evidence are owned by `plan/plan-rh-3.md` and `prd/PRD_02_10_rhai_scripting.md`.
> Unchecked historical rows below were superseded by the completed M42f8 evidence and
> must not be revived as active leaves from this file.

# Phase C prep: retire live `rhai::` namespace (historical plan)

Tip baseline: `a21ac868` — Wave 4.1 landed at codegen **rev83** — script-smoke / remote-ui
Native + AOT `pack=ok` (locked); fresh-clone / workbench / unix / working-context
remain Native with pack locks. SSOT for sequencing; do not invent a second living
file map.

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
2. **Wave 2 — native emit gaps:** ✅ through codegen **rev82** — Native selection for core smokes; pack debt closed in Wave 4.1.
3. **Wave 3 — script mass-rename:** ✅ live `scripts/rh/**` has **0** `rhai::` call sites.
4. **Wave 4 — Phase C archive:** 4.1 ✅ rev83 AOT pack for script-smoke/remote-ui; next drop `rhai` Engine module, eval/run-script Rhai paths, then `agenterm-rhai` PE; scrub residual branding.

**Non-goal:** inventing permission/sandbox policy under Script Runtime.

## Next leaves (Wave 4 gate)

Ordered; 4.1 blocks 4.3; 4.5 follows 4.3; 4.6–4.7 follow 4.5.

| # | Leaf | Exclusive owner(s) | Evidence |
|---|------|--------------------|----------|
| 4.1 | ✅ AOT typecheck debt + remaining HE emit for Native packs | `crates/agenterm-rh/src/transpile.rs` (+ smoke idiom) | script-smoke/remote-ui `mode_probe --pack` → `pack=ok`; `script_smoke_pack_builds` / `remote_ui_smoke_pack_builds` |
| 4.2 | ✅ Drop Engine legacy `rhai` module + catalog/shipped aliases + legacy root (4.2a+4.2b+4.2c) | `src/script_stdlib.rs`, `crates/agenterm-rh/src/host_api.rs`, `src/script_catalog.rs`, `crates/agenterm-rh/src/shipped_surfaces.rs` | zero `register_static_module(…"rhai"`; catalog `rh::` only |
| 4.3 | Remove pack Rhai eval/run-script fallback | `src/script_rh_host.rs`, `crates/agenterm-rh/src/{host_api,transpile}.rs` | no prod `host_eval_snippet` / `host_run_script_source` |
| 4.4 | Migrate Engine-root-dependent tests/fixtures | `src/script_{stdlib,task,catalog,http,worker}.rs`, `tests/rh_*.rs`, `crates/agenterm-rh/tests/**` | `cargo test -p agenterm --lib` + `agenterm-rh` green |
| 4.5 | ✅ Retire `agenterm-rhai` PE + `ScriptBackend::Rhai` + REPL/worker interpreted path | `src/bin/agenterm-rhai.rs`, `Cargo.toml`, `src/script_{backend,worker,repl}.rs`, `src/client/mod.rs` | five product bins in matrix |
| 4.6 | ✅ Packaging / install / bootstrap / smokes | `scripts/artifacts.json`, `install.sh`, `scripts/rh/{check,artifact-verification,*smoke}.rh` | stage-build + artifact-verification |
| 4.7 | ✅ Retire PE integration tests + caller-inventory baseline | `tests/rhai_migration.rs`, `tests/script_repl.rs`, `tests/linux_script_cli.rs`, `tests/rh_cli_forward.rs`, `fixtures/rh/caller-inventory-baseline.json`, … | `caller-inventory` / `rh_corpus` green |
| 4.8 | Residual operational trace scrub | `scripts/rh/script-smoke.rh`, `skills/**`, `README.md`, `AGENTS.md`, PRD nodes | intentional historical docs only |

Do not edit `scripts/archive/rhai/**` except as historical reference.

## Evidence per wave

- Wave 1: ✅ rev80 dual-alias; catalog + `agenterm-rh` tests green.
- Wave 2: ✅ rev82; script-smoke/remote-ui/working-context Native.
- Wave 3: ✅ live `scripts/rh` `rhai::`=0.
- Wave 4.1: ✅ rev83; script-smoke/remote-ui Native AOT `pack=ok` + regression locks.
- Wave 4.2a/b/c: ✅ Engine registers only `rh`; catalog/shipped `rh::` only; `RHAI_LEGACY_HOST_API_ROOT` removed.
- Wave 4.5: ✅ `agenterm-rhai` PE deleted; `ScriptBackend::Rhai` removed; REPL retired.
- Wave 4.6/4.7: ✅ packaging cleaned; integration tests retired.
- Wave 4.8: residual trace scrub (docs/skills/PRD).
