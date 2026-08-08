# Phase C prep: retire live `rhai::` namespace

Tip baseline inventory: tip after rev78/79 HE cuts (`f2c375a4` era counts; re-probe before Wave 3).
SSOT for sequencing; do not invent a second living file map.

## Outcome

Live `.rh` scripts and AOT packs stop branding host APIs as `rhai::`.
`agenterm-rhai` PE / Engine eval fallback are removed only after native emit covers the corpus.

## Inventory (scripts/rh, non-archive)

| Metric | Count |
|--------|------:|
| `rhai::` call sites | ~647 / 65 files |
| Distinct `module::fn` | 19 |
| Top modules | json 290, task 122, crypto 121, runtime 82 |

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

1. **Wave 1 — dual alias window:** register `rh` + keep `rhai`; centralize transpile/catalog namespace constants; accept both in `uses_host_surface`.
2. **Wave 2 — native emit gaps:** close remaining `rh_host_eval_int("rhai::…")` for json/task/crypto/runtime long tail (clipboard already native at rev79).
3. **Wave 3 — script mass-rename:** mechanical `rhai::` → `rh::` in `scripts/rh/**`, fixtures, embedded eval strings; lint + smoke.
4. **Wave 4 — Phase C archive:** drop `rhai` Engine module, eval/run-script Rhai paths, then `agenterm-rhai` PE; scrub residual branding.

**Non-goal:** script-rename before Wave 1 (breaks check/transpile/pack).
**Non-goal:** inventing permission/sandbox policy under Script Runtime.

## Evidence per wave

- Wave 1: `cargo test -p agenterm-rh`, catalog registration guards, one pack per submodule.
- Wave 2: golden transpile asserts no `rh_host_eval_int("rhai::` for corpus fixtures.
- Wave 3: `agenterm-rh check-many` + owning smokes HostEval/Native unchanged.
- Wave 4: zero live `rhai::` / `agenterm-rhai` operator paths outside archive + intentional historical docs.
