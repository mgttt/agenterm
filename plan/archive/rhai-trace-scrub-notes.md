# ⚠️ Archived: Rhai trace scrub notes

> Archived on 2026-08-10 after Phase C/M42f8 completion. Current runtime and migration
> truth lives in `plan/plan-rh-3.md`, `prd/PRD_02_10_rhai_scripting.md`, and automated
> caller-inventory guards. Counts below are historical.

# Rhai trace scrub notes (historical remaining-mention snapshot)

Operational scrub completed **2026-08-08** for skills/docs operator paths.
This file inventories **intentional** residual Rhai branding — not a mass-edit
checklist. Do not rewrite history docs wholesale.

## Scrub outcome (operational)

| Check | Status |
|-------|--------|
| Live `*.rhai` outside `scripts/archive/rhai/` | **0** |
| `agenterm.tasks.json` `.rhai` entries | **0** |
| Live `scripts/rhai/` tree | **archived** (74 files under `scripts/archive/rhai/`) |
| Skills presenting `scripts/rhai/*.rhai` as live operator path | **0** (post-scrub) |
| Skills presenting `agenterm-rhai` as preferred operator entry | **0** (post-scrub; compat shim note in macOS install skill only) |
| Docs claiming live `scripts/rhai/` operator paths | **0** — `docs/agenterm-rhai-runtime.md` correctly marks archive + shim |

## Remaining `agenterm-rhai` by category (~354 hits, tip probe)

Counts exclude `scripts/archive/**`, `target/`, `dist/`.

| Category | Role | Examples |
|----------|------|----------|
| **Shim PE / code** | Wave 4 removal gate | `src/bin/agenterm-rhai.rs`, `src/client/mod.rs` Windows `script` forward, `src/script_stdlib.rs` |
| **Compat tests / policy guards** | Must stay until PE drop | `tests/rhai_migration.rs`, `tests/rh_cli_forward.rs`, `tests/release_workflow_policy.rs`, `tests/rh_aot_ci_policy.rs` |
| **Bootstrap / packaging** | Stages both `agenterm-rh` + shim | `scripts/bootstrap.sh`, `scripts/artifacts.json`, `install.sh` |
| **Live `.rh` policy asserts** | Regression guards | `scripts/rh/check.rh`, `scripts/rh/script-smoke.rh`, `scripts/rh/startup-smoke.rh` |
| **Historical docs / plans** | Intentional — do not mass-edit | `docs/agenterm-rhai-runtime.md`, `plan/agenterm-rhai-app.md`, `plan/rh-3.md`, version plans |
| **PRD / AGENTS** | Product authority — Wave 4 sweep | `prd/PRD_02_10_rhai_scripting.md`, `AGENTS.md` |
| **Public site** | Shim listed as `(compat)` | `docs/index.html` |
| **Parity matrix fallback** | Explicit compat fallback only | `plan/platform-ux-parity-evidence-matrix.md` |

## Remaining `scripts/rhai` path strings (non-archive tree)

These are **historical references** or **audit snapshots**, not live operator paths:

| Location | Why kept |
|----------|----------|
| `plan/rhai-trace-m42f8e.md` | Read-only audit snapshot (header notes archive cutover) |
| `plan/plan-rh-3.md`, design docs | Migration history / M42f8 milestone records |
| `plan/archive/**` | Frozen version plans |
| `tests/rhai_migration.rs`, `tests/rh_*` | Policy guards + migration regression |
| `crates/agenterm-rh/src/caller_inventory.rs` | Baseline inventory guard |

## Operator front door (canonical)

| Surface | Path |
|---------|------|
| Live script tree | `scripts/rh/*.rh` |
| Task/worker CLI | `agenterm-rh` (`task run`, `check-many`, worker) |
| Rhai compat shim (until Wave 4) | `agenterm-rhai` — `.rhai` / `repl` / Windows `agenterm-cli script …` only |

## Related SSOT

- Sequencing: [`plan-rh-namespace-phase-c.md`](plan-rh-namespace-phase-c.md)
- Full audit snapshot: [`rhai-trace-m42f8e.md`](rhai-trace-m42f8e.md)
- Rh migration plan: [`plan-rh-3.md`](../plan-rh-3.md)
