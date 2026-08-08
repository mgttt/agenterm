# Phase C prep: retire live `rhai::` namespace

Tip baseline inventory: tip after rev78/79 HE cuts (`f2c375a4` era counts; re-probe before Wave 3).
SSOT for sequencing; do not invent a second living file map.

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

Top surfaces: `json::parse`, `crypto::sha256_file`, `task::sleep`, `runtime::atomic_write`, `json::parse_file`.

`rh::fail` (≈89) is transpile-only — **not** a model for host API rename.

`agenterm-rhai` as operator front door is already clear (0 workflow/task-manifest hits); ~350 remaining mentions are shim PE / compat tests / docs.

## Binding owners (must move together)

| Layer | Owner |
|-------|-------|
| Engine `register_static_module("rhai", …)` | `src/script_stdlib.rs` + clipboard/image/task/http |
| Catalog / shipped surfaces | `src/script_catalog.rs`, `crates/agenterm-rh/src/shipped_surfaces.rs` |
| AOT matchers (~83 `rhai::` strings) | `crates/agenterm-rh/src/transpile.rs` |
| Pack eval fallback | `src/script_rh_host.rs` (`host_eval_snippet` / run-script) |

## Sequenced leaves

1. **Wave 1 — dual alias window:** ✅ shipped at codegen **rev80** — Engine registers `rh` + legacy `rhai`; `host_api_module` / AOT matchers accept both; catalog publishes `rh.*` aliases (`rhai.*` marked Legacy); fixture `rh_host_api_json_task.rh`.
2. **Wave 2 — native emit gaps:** close remaining `rh_host_eval_int("rhai::…")` / `"rh::…"` for json/task/crypto/runtime long tail (clipboard already native at rev79).
3. **Wave 3 — script mass-rename:** ✅ live `scripts/rh/**` has **0** `rhai::` call sites; fixtures mostly `rh::` with intentional dual-alias legacy fixture retained.
4. **Wave 4 — Phase C archive:** drop `rhai` Engine module, eval/run-script Rhai paths, then `agenterm-rhai` PE; scrub residual branding.

**Non-goal:** inventing permission/sandbox policy under Script Runtime.

## Evidence per wave

- Wave 1: ✅ `cargo test -p agenterm-rh` + `script_catalog` + `rh_task_entry_regression` HE ceilings (script-smoke ≤30, remote-ui ≤12); `rh::`/`rhai::` fixtures Native he=1.
- Wave 2: golden transpile asserts no `rh_host_eval_int` for corpus host surfaces that already have native emit.
- Wave 3: ✅ live `scripts/rh` `rhai::`=0; `cargo test -p agenterm-rh` + HE ceilings still green (script-smoke he≤25, remote-ui he≤9).
- Wave 4: zero live `rhai::` / `agenterm-rhai` operator paths outside archive + intentional historical docs.
