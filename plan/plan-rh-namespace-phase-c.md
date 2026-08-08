# Phase C prep: retire live `rhai::` namespace

Tip baseline inventory: tip after rev78/79 HE cuts (`f2c375a4` era counts; re-probe before Wave 4).
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
| AOT matchers (~83 `rhai::` strings) | `crates/agenterm-rh/src/transpile.rs` |
| Pack eval fallback | `src/script_rh_host.rs` (`host_eval_snippet` / run-script) |

## Sequenced leaves

1. **Wave 1 — dual alias window:** ✅ shipped at codegen **rev80** — Engine registers `rh` + legacy `rhai`; `host_api_module` / AOT matchers accept both; catalog publishes `rh.*` aliases (`rhai.*` marked Legacy); fixture `rh_host_api_json_task.rh`.
2. **Wave 2 — native emit gaps:** ✅ closed through codegen **rev82** — also `command.stdin_text`, `child.stdout`, `command.args(Json)`; **script-smoke / remote-ui / working-context are Native he=1** with AOT pack locks.
3. **Wave 3 — script mass-rename:** ✅ live `scripts/rh/**` has **0** `rhai::` call sites; fixtures mostly `rh::` with intentional dual-alias legacy fixture retained.
4. **Wave 4 — Phase C archive:** drop `rhai` Engine module, eval/run-script Rhai paths, then `agenterm-rhai` PE; scrub residual branding in code/tests/AGENTS/PRD.

**Non-goal:** inventing permission/sandbox policy under Script Runtime.

## Next leaves (Wave 4 gate)

| Leaf | Owner | Evidence |
|------|-------|----------|
| Native emit long tail (Wave 2 remainder) | `crates/agenterm-rh/src/transpile.rs` | golden transpile: no `rh_host_eval_int` for corpus host surfaces |
| Drop Engine `rhai` module + eval fallback | `src/script_stdlib.rs`, `src/script_rh_host.rs` | zero `register_static_module("rhai"`; pack runs Native-only |
| Retire `agenterm-rhai` PE | `src/bin/agenterm-rhai.rs`, `Cargo.toml`, packaging | shim callers migrated; installer/bootstrap policy updated |
| Residual trace scrub (code/tests) | tests, `scripts/rh/*` policy asserts, bootstrap | `caller-inventory` / workflow policy guards stay green |

## Evidence per wave

- Wave 1: ✅ rev80 dual-alias; catalog + `agenterm-rh` tests green.
- Wave 2: ✅ rev82; script-smoke/remote-ui/working-context Native he=1; working-context AOT pack builds; script-smoke/remote-ui still have AOT typecheck debt (mode Native but cargo fails).
- Wave 3: ✅ live `scripts/rh` `rhai::`=0.
- Wave 4: zero live `rhai::` / `agenterm-rhai` operator paths outside archive + intentional historical docs.
