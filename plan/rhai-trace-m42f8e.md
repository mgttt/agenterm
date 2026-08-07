# M42f8e Rhai operational trace audit (read-only)

> Durable copy of the read-only audit handoff. Cleanup (Phase A/B/C) remains gated on Native+pack flips; do not treat this file as permission to flip `agenterm.tasks.json` early.

**Tip SHA (audit snapshot):** `1dea2772521e810269baa8f75cd80c89fabd6a62`
**Persisted at tip:** `096fb157d2819d1c1867a8ba88c461ca618bed83`  
**Branch:** main  
**Audit date:** 2026-08-07  
**Thoroughness:** medium  
**Excludes:** `target/`, `dist/`, `.git/`, binary blobs, `scripts/archive/**`, `plan/archive/**`

## Executive summary

- Total unique hit files (union): **153**
- Live `scripts/rhai/` tree: **39** files
- `agenterm.tasks.json` `.rhai` entries: **30**
- Workflow hits: **1** file (only `release.yml`; other workflows call `agenterm-rh` but still hit `.rhai` via manifest task names)
- `rh_host_run_script` files: **10**
- `compat-delegating` files: **9**

## Counts by category (union)

| Category | Files | Hits |
|----------|------:|-----:|
| workflows | 1 | 2 |
| tests | 22 | 270 |
| tasks | 5 | 71 |
| scripts_rhai_tree | 39 | 83 |
| scripts_other | 20 | 164 |
| code | 24 | 113 |
| docs | 41 | 305 |
| config | 1 | 4 |

## Counts by pattern × category (files)

### `agenterm-rhai`
- code: 13
- config: 1
- docs: 32
- scripts_other: 14
- scripts_rhai_tree: 7
- tasks: 5
- tests: 12
- total: 84 files

### `scripts/rhai/`
- code: 4
- docs: 12
- scripts_other: 8
- scripts_rhai_tree: 26
- tasks: 1
- tests: 9
- workflows: 1
- total: 61 files

### `dot-rhai`
- code: 10
- docs: 24
- scripts_other: 10
- scripts_rhai_tree: 5
- tasks: 2
- tests: 18
- workflows: 1
- total: 70 files

### `rh_host_run_script`
- code: 3
- docs: 3
- tests: 4
- total: 10 files

### `compat-delegating`
- code: 5
- docs: 1
- tests: 3
- total: 9 files

## 1. agenterm-rhai traces outside scripts/rhai/

- `AGENTS.md` (4) [docs]
- `Cargo.toml` (2) [code]
- `PRD.md` (2) [docs]
- `README.md` (3) [docs]
- `agenterm.tasks.json` (6) [tasks]
- `crates/agenterm-rh/src/caller_inventory.rs` (2) [code]
- `crates/agenterm-rh/src/check_many.rs` (3) [code]
- `crates/agenterm-rh/src/main.rs` (2) [code]
- `docs/agenterm-rhai-runtime.md` (7) [docs]
- `docs/index.html` (4) [docs]
- `examples/script-daily-check/README.md` (5) [docs]
- `examples/script-daily-check/agenterm.tasks.json` (1) [tasks]
- `fixtures/rh/caller-inventory-baseline.json` (1) [tasks]
- `fixtures/rh/check-many-rhai-kind.json` (1) [tasks]
- `fixtures/rh/map-set-membership.rh` (1) [tasks]
- `install.sh` (4) [config]
- `plan/ARCHITECTURE.md` (2) [docs]
- `plan/agenterm-rhai-app.md` (5) [docs]
- `plan/design-cc-hyper-control-agent.md` (2) [docs]
- `plan/design-llm-bridge-web-to-api.md` (1) [docs]
- `plan/design-llm-gateway-rhai-logic-pack.md` (8) [docs]
- `plan/design-release-base-vs-apps.md` (3) [docs]
- `plan/design-rh-aot.md` (2) [docs]
- `plan/design-rhai-rust-boundary.md` (5) [docs]
- `plan/design-scripting-boundary-comparison.md` (2) [docs]
- `plan/plan-rh-3.md` (13) [docs]
- `plan/plan-v0.1.15.md` (4) [docs]
- `plan/plan-v0.1.16.md` (5) [docs]
- `plan/platform-ux-parity-evidence-matrix.md` (9) [docs]
- `plan/precision-audit.md` (4) [docs]
- `plan/research-rhai-kernel-depth.md` (3) [docs]
- `prd/PRD_02_02_executable_family.md` (5) [docs]
- `prd/PRD_02_04_optional_components.md` (1) [docs]
- `prd/PRD_02_10_rhai_scripting.md` (16) [docs]
- `prd/PRD_02_13_llm_gateway.md` (1) [docs]
- `prd/PRD_02_18_roadmap.md` (2) [docs]
- `prd/PRD_02_19_inspiration_and_future_vision.md` (2) [docs]
- `research/agenterm-webview/README.md` (3) [docs]
- `research/agenterm-webview/evidence/windows-x86_64.md` (2) [docs]
- `research/agenterm-webview/tools/measure.rhai` (1) [docs]
- `scripts/artifacts.json` (7) [scripts_other]
- `scripts/bootstrap.cmd` (3) [scripts_other]
- `scripts/bootstrap.sh` (3) [scripts_other]
- `scripts/powershell-migration.json` (2) [scripts_other]
- `scripts/rh/artifact-verification.rh` (1) [scripts_other]
- `scripts/rh/check.rh` (5) [scripts_other]
- `scripts/rh/control-center-macos-smoke.rh` (1) [scripts_other]
- `scripts/rh/control-center-smoke.rh` (1) [scripts_other]
- `scripts/rh/lint.rh` (3) [scripts_other]
- `scripts/rh/platform-ux-parity-smoke.rh` (3) [scripts_other]
- `scripts/rh/preflight.rh` (1) [scripts_other]
- `scripts/rh/script-smoke.rh` (4) [scripts_other]
- `scripts/rh/startup-smoke.rh` (1) [scripts_other]
- `scripts/rh/verify-docs-site.rh` (1) [scripts_other]
- `skills/agenterm-local-macos/SKILL.md` (1) [docs]
- `skills/cursor/mailbox.md` (1) [docs]
- `src/bin/agenterm-rhai.rs` (1) [code]
- `src/client/mod.rs` (15) [code]
- `src/platform/policy/paths.rs` (3) [code]
- `src/platform/services/paths.rs` (1) [code]
- `src/platform/services/supervisor_audit.rs` (1) [code]
- `src/script_catalog.rs` (1) [code]
- `src/script_image.rs` (1) [code]
- `src/script_stdlib.rs` (9) [code]
- `src/worker_supervisor/persistent.rs` (2) [code]
- `tests/install_local_macos.rs` (1) [tests]
- `tests/linux_script_cli.rs` (4) [tests]
- `tests/promotion_identity.rs` (1) [tests]
- `tests/release_target_cleanup_policy.rs` (4) [tests]
- `tests/release_workflow_policy.rs` (2) [tests]
- `tests/rh_aot_ci_policy.rs` (8) [tests]
- `tests/rh_cli_forward.rs` (5) [tests]
- `tests/rh_standalone_cli.rs` (3) [tests]
- `tests/rhai_migration.rs` (41) [tests]
- `tests/script_check_many.rs` (3) [tests]
- `tests/script_repl.rs` (2) [tests]
- `tests/target_incremental_prune.rs` (1) [tests]

## 2. scripts/rhai/ references outside tree

- `.github/workflows/release.yml` (1) [workflows]
- `AGENTS.md` (2) [docs]
- `agenterm.tasks.json` (30) [tasks]
- `crates/agenterm-rh/src/caller_inventory.rs` (1) [code]
- `crates/agenterm-rh/src/corpus.rs` (1) [code]
- `plan/design-control-center-ux.md` (4) [docs]
- `plan/design-llm-gateway-rhai-logic-pack.md` (1) [docs]
- `plan/design-rhai-rust-boundary.md` (1) [docs]
- `plan/plan-control-center-ux.md` (1) [docs]
- `plan/plan-rh-3.md` (1) [docs]
- `plan/plan-unix-gui-win-parity.md` (1) [docs]
- `plan/plan-v0.1.15.md` (2) [docs]
- `prd/PRD_02_10_rhai_scripting.md` (30) [docs]
- `prd/PRD_02_17_delivery_quality.md` (3) [docs]
- `scripts/powershell-migration.json` (35) [scripts_other]
- `scripts/rh/cross-platform-automation-audit.rh` (1) [scripts_other]
- `scripts/rh/internal-version-policy.rh` (1) [scripts_other]
- `scripts/rh/lint.rh` (1) [scripts_other]
- `scripts/rh/package-qualified-selftest.rh` (2) [scripts_other]
- `scripts/rh/powershell-migration-audit.rh` (5) [scripts_other]
- `scripts/rh/preflight.rh` (1) [scripts_other]
- `scripts/rh/script-smoke.rh` (3) [scripts_other]
- `skills/agenterm-release/SKILL.md` (1) [docs]
- `skills/agenterm-release/references/github-auth-and-dispatch.md` (1) [docs]
- `src/script_rh_cli.rs` (1) [code]
- `src/script_rh_host.rs` (1) [code]
- `tests/fresh_clone_rehearsal.rs` (1) [tests]
- `tests/promotion_identity.rs` (1) [tests]
- `tests/release_target_cleanup_policy.rs` (1) [tests]
- `tests/release_workflow_policy.rs` (1) [tests]
- `tests/rh_aot_ci_policy.rs` (1) [tests]
- `tests/rh_native_task.rs` (5) [tests]
- `tests/rh_regression.rs` (2) [tests]
- `tests/rhai_migration.rs` (25) [tests]
- `tests/target_incremental_prune.rs` (1) [tests]

## 3. Remaining .rhai operational entrypoints

### agenterm.tasks.json (30 entries)

- `scripts/rhai/build.rhai`
- `scripts/rhai/check.rhai`
- `scripts/rhai/release.rhai`
- `scripts/rhai/harness-cleanup-selftest.rhai`
- `scripts/rhai/diagnostic-bundle-selftest.rhai`
- `scripts/rhai/qualification-selftest.rhai`
- `scripts/rhai/working-context-smoke.rhai`
- `scripts/rhai/theme-smoke.rhai`
- `scripts/rhai/workbench-smoke.rhai`
- `scripts/rhai/control-center-smoke.rhai`
- `scripts/rhai/control-center-macos-smoke.rhai`
- `scripts/rhai/control-center-linux-smoke.rhai`
- `scripts/rhai/unix-frontend-smoke.rhai`
- `scripts/rhai/unix-frontend-smoke.rhai`
- `scripts/rhai/fleet-smoke.rhai`
- `scripts/rhai/server-smoke.rhai`
- `scripts/rhai/native-ipc-smoke.rhai`
- `scripts/rhai/native-ipc-compat-smoke.rhai`
- `scripts/rhai/wake-smoke.rhai`
- `scripts/rhai/startup-smoke.rhai`
- `scripts/rhai/cli-smoke.rhai`
- `scripts/rhai/script-smoke.rhai`
- `scripts/rhai/remote-ui-smoke.rhai`
- `scripts/rhai/remote-ui-upgrade-smoke.rhai`
- `scripts/rhai/platform-ux-parity-smoke.rhai`
- `scripts/rhai/platform-ux-parity-smoke.rhai`
- `scripts/rhai/platform-ux-parity-smoke.rhai`
- `scripts/rhai/build.rhai`
- `scripts/rhai/build.rhai`
- `scripts/rhai/fresh-clone-rehearsal.rhai`

### workflows

- `.github/workflows/release.yml:146` → `scripts/rhai/promotion-identity.rhai` (rh draft: `scripts/rh/promotion-identity.rh`)

## 4. rh_host_run_script / compat-delegating

M42f8: compat-delegating = migration diagnostic only; Phase C removes Engine/compat fallback.

- `crates/agenterm-rh/src/corpus.rs` run_script=0 compat=1 [code]
- `crates/agenterm-rh/src/host_api.rs` run_script=1 compat=0 [code]
- `crates/agenterm-rh/src/transpile.rs` run_script=17 compat=6 [code]
- `crates/agenterm-rh/tests/public_contract.rs` run_script=0 compat=2 [code]
- `plan/design-rh-aot.md` run_script=1 compat=0 [docs]
- `plan/plan-rh-3.md` run_script=2 compat=2 [docs]
- `prd/PRD_02_10_rhai_scripting.md` run_script=1 compat=0 [docs]
- `src/script_rh_host.rs` run_script=1 compat=1 [code]
- `src/script_rh_run.rs` run_script=0 compat=1 [code]
- `tests/rh_backend.rs` run_script=1 compat=0 [tests]
- `tests/rh_native_task.rs` run_script=1 compat=3 [tests]
- `tests/rh_regression.rs` run_script=10 compat=3 [tests]
- `tests/rh_task_entry_regression.rs` run_script=32 compat=33 [tests]

## 5. Live scripts/rhai/ tree

- `scripts/rhai/build.rhai`
- `scripts/rhai/candidate-aggregate.rs`
- `scripts/rhai/check.rhai`
- `scripts/rhai/cli-smoke.rhai`
- `scripts/rhai/control-center-linux-smoke.rhai`
- `scripts/rhai/control-center-macos-smoke.rhai`
- `scripts/rhai/control-center-smoke.rhai`
- `scripts/rhai/cross-platform-automation-audit.rs`
- `scripts/rhai/diagnostic-bundle-selftest.rhai`
- `scripts/rhai/fleet-smoke.rhai`
- `scripts/rhai/fresh-clone-rehearsal.rhai`
- `scripts/rhai/harness-cleanup-selftest.rhai`
- `scripts/rhai/lib/artifact_files.rhai`
- `scripts/rhai/lib/artifact_manifest.rhai`
- `scripts/rhai/lib/bootstrap_timing.rhai`
- `scripts/rhai/lib/build_identity.rhai`
- `scripts/rhai/lib/qualification.rhai`
- `scripts/rhai/lib/release_candidate.rhai`
- `scripts/rhai/lib/script_smoke_helpers.rhai`
- `scripts/rhai/lib/test_harness.rhai`
- `scripts/rhai/native-ipc-compat-smoke.rhai`
- `scripts/rhai/native-ipc-smoke.rhai`
- `scripts/rhai/platform-ux-parity-smoke.rhai`
- `scripts/rhai/promotion-identity.rhai`
- `scripts/rhai/qualification-selftest.rhai`
- `scripts/rhai/release.rhai`
- `scripts/rhai/remote-ui-smoke.rhai`
- `scripts/rhai/remote-ui-upgrade-smoke.rhai`
- `scripts/rhai/script-http-fixture.rhai`
- `scripts/rhai/script-smoke.rhai`
- `scripts/rhai/server-smoke.rhai`
- `scripts/rhai/startup-smoke.rhai`
- `scripts/rhai/target-report.rs`
- `scripts/rhai/theme-smoke.rhai`
- `scripts/rhai/unix-frontend-smoke.rhai`
- `scripts/rhai/verify-script-contract.rhai`
- `scripts/rhai/wake-smoke.rhai`
- `scripts/rhai/workbench-smoke.rhai`
- `scripts/rhai/working-context-smoke.rhai`

## 6. Before archive vs can wait

### Must change before archive (Phase A)

1. `tests/rhai_migration.rs` (98) — Phase A: pins live rhai paths/invocations
2. `agenterm.tasks.json` (66) — Phase A: 30 .rhai entries
3. `scripts/rh/script-smoke.rh` (31) — rh scripts embed rhai fallback/path strings
4. `tests/script_check_many.rs` (14) — Phase A: pins live rhai paths/invocations
5. `scripts/rh/powershell-migration-audit.rh` (12) — rh scripts embed rhai fallback/path strings
6. `scripts/rh/lint.rh` (7) — rh scripts embed rhai fallback/path strings
7. `tests/rh_cli_forward.rs` (6) — Phase A: pins live rhai paths/invocations
8. `scripts/rh/check.rh` (5) — rh scripts embed rhai fallback/path strings
9. `tests/release_workflow_policy.rs` (5) — Phase A: pins live rhai paths/invocations
10. `install.sh` (4) — config baselines name rhai
11. `scripts/rh/package-qualified-selftest.rh` (4) — rh scripts embed rhai fallback/path strings
12. `tests/linux_script_cli.rs` (4) — Phase A: pins live rhai paths/invocations
13. `tests/promotion_identity.rs` (4) — Phase A: pins live rhai paths/invocations
14. `scripts/rh/preflight.rh` (3) — rh scripts embed rhai fallback/path strings
15. `scripts/rh/platform-ux-parity-smoke.rh` (3) — rh scripts embed rhai fallback/path strings
16. `.github/workflows/release.yml` (2) — Phase A: flip workflow entry before archive
17. `scripts/rh/internal-version-policy.rh` (2) — rh scripts embed rhai fallback/path strings
18. `scripts/rh/cross-platform-automation-audit.rh` (2) — rh scripts embed rhai fallback/path strings
19. `scripts/rh/control-center-macos-smoke.rh` (1) — rh scripts embed rhai fallback/path strings
20. `scripts/rh/artifact-verification.rh` (1) — rh scripts embed rhai fallback/path strings
21. `scripts/rh/verify-docs-site.rh` (1) — rh scripts embed rhai fallback/path strings
22. `scripts/rh/prd-alignment.rh` (1) — rh scripts embed rhai fallback/path strings
23. `scripts/rh/control-center-smoke.rh` (1) — rh scripts embed rhai fallback/path strings
24. `scripts/rh/startup-smoke.rh` (1) — rh scripts embed rhai fallback/path strings

### Can wait (Phase B/C)

1. `prd/PRD_02_10_rhai_scripting.md` (84) — Phase B doc sweep
2. `scripts/powershell-migration.json` (72) — Phase B general sweep
3. `tests/rh_task_entry_regression.rs` (66) — Phase B/C test guard updates
4. `plan/plan-rh-3.md` (31) — Phase B doc sweep
5. `plan/design-llm-gateway-rhai-logic-pack.md` (28) — Phase B doc sweep
6. `crates/agenterm-rh/src/transpile.rs` (23) — Phase C compat removal
7. `src/client/mod.rs` (17) — Phase B general sweep
8. `tests/rh_native_task.rs` (17) — Phase B/C test guard updates
9. `plan/plan-v0.1.15.md` (16) — Phase B doc sweep
10. `src/script_project.rs` (16) — Phase B general sweep
11. `tests/rh_regression.rs` (16) — Phase B/C test guard updates
12. `plan/design-control-center-ux.md` (14) — Phase B doc sweep
13. `tests/rh_aot_ci_policy.rs` (10) — Phase B/C test guard updates
14. `plan/platform-ux-parity-evidence-matrix.md` (9) — Phase B doc sweep
15. `src/script_stdlib.rs` (9) — Phase B general sweep
16. `AGENTS.md` (8) — Phase B doc sweep
17. `docs/agenterm-rhai-runtime.md` (8) — Phase B doc sweep
18. `plan/agenterm-rhai-app.md` (7) — Phase B doc sweep
19. `prd/PRD_02_17_delivery_quality.md` (7) — Phase B doc sweep
20. `prd/alignment-contract.json` (7) — Phase B doc sweep
21. `scripts/artifacts.json` (7) — Phase B general sweep
22. `crates/agenterm-rh/src/project_import.rs` (6) — Phase C compat removal
23. `examples/script-daily-check/README.md` (6) — Phase B doc sweep
24. `plan/design-rhai-rust-boundary.md` (6) — Phase B doc sweep
25. `plan/plan-v0.1.16.md` (6) — Phase B doc sweep
26. `research/agenterm-webview/README.md` (6) — Phase B doc sweep
27. `tests/release_target_cleanup_policy.rs` (6) — Phase B/C test guard updates
28. `crates/agenterm-rh/src/check_many.rs` (5) — Phase C compat removal
29. `crates/agenterm-rh/src/corpus.rs` (5) — Phase C compat removal
30. `prd/PRD_02_02_executable_family.md` (5) — Phase B doc sweep
31. `tests/rh_standalone_cli.rs` (5) — Phase B/C test guard updates
32. `docs/index.html` (4) — Phase B doc sweep
33. `plan/precision-audit.md` (4) — Phase B doc sweep
34. `plan/design-release-base-vs-apps.md` (4) — Phase B doc sweep
35. `plan/design-rh-aot.md` (4) — Phase B doc sweep
36. `research/agenterm-webview/evidence/windows-x86_64.md` (4) — Phase B doc sweep
37. `scripts/qualification-gates.json` (4) — Phase B general sweep
38. `tests/fixtures/script-project/agenterm.tasks.json` (4) — Phase B/C test guard updates
39. `README.md` (3) — Phase B doc sweep
40. `crates/agenterm-rh/src/main.rs` (3) — Phase C compat removal
41. `crates/agenterm-rh/src/caller_inventory.rs` (3) — Phase C compat removal
42. `plan/research-rhai-kernel-depth.md` (3) — Phase B doc sweep
43. `prd/PRD_02_18_roadmap.md` (3) — Phase B doc sweep
44. `scripts/bootstrap.cmd` (3) — Phase B general sweep
45. `scripts/bootstrap.sh` (3) — Phase B general sweep
46. `src/script_rh_host.rs` (3) — Phase C compat removal
47. `src/script_rh_cli.rs` (3) — Phase C compat removal
48. `src/platform/policy/paths.rs` (3) — Phase B general sweep
49. `tests/target_incremental_prune.rs` (3) — Phase B/C test guard updates
50. `Cargo.toml` (2) — Phase C compat removal
51. `PRD.md` (2) — Phase B doc sweep
52. `crates/agenterm-rh/tests/public_contract.rs` (2) — Phase C compat removal
53. `examples/script-daily-check/agenterm.tasks.json` (2) — Phase B general sweep
54. `plan/ARCHITECTURE.md` (2) — Phase B doc sweep
55. `plan/plan-control-center-ux.md` (2) — Phase B doc sweep
56. `plan/plan-unix-gui-win-parity.md` (2) — Phase B doc sweep
57. `plan/design-scripting-boundary-comparison.md` (2) — Phase B doc sweep
58. `plan/design-cc-hyper-control-agent.md` (2) — Phase B doc sweep
59. `prd/PRD_02_15_command_line.md` (2) — Phase B doc sweep
60. `prd/PRD_02_19_inspiration_and_future_vision.md` (2) — Phase B doc sweep
61. `research/agenterm-webview/tools/measure.rhai` (2) — Phase B doc sweep
62. `skills/agenterm-release/SKILL.md` (2) — Phase B doc sweep
63. `skills/agenterm-release/references/github-auth-and-dispatch.md` (2) — Phase B doc sweep
64. `src/script_backend.rs` (2) — Phase B general sweep
65. `src/bin/agenterm-rhai.rs` (2) — Phase C compat removal
66. `src/worker_supervisor/persistent.rs` (2) — Phase B general sweep
67. `tests/fresh_clone_rehearsal.rs` (2) — Phase B/C test guard updates
68. `tests/script_repl.rs` (2) — Phase B/C test guard updates
69. `tests/rh_framed_worker.rs` (2) — Phase B/C test guard updates
70. `tests/fixtures/script-project/duplicate.tasks.json` (2) — Phase B/C test guard updates
71. `crates/agenterm-rh/src/bundle.rs` (1) — Phase C compat removal
72. `crates/agenterm-rh/src/host_api.rs` (1) — Phase C compat removal
73. `plan/design-llm-bridge-web-to-api.md` (1) — Phase B doc sweep
74. `plan/plan-cc-automation-cli.md` (1) — Phase B doc sweep
75. `prd/PRD_02_04_optional_components.md` (1) — Phase B doc sweep
76. `prd/PRD_02_13_llm_gateway.md` (1) — Phase B doc sweep
77. `skills/agenterm-local-macos/SKILL.md` (1) — Phase B doc sweep
78. `skills/cursor/mailbox.md` (1) — Phase B doc sweep
79. `src/script_catalog.rs` (1) — Phase B general sweep
80. `src/script_image.rs` (1) — Phase B general sweep
81. `src/script_rh_run.rs` (1) — Phase C compat removal
82. `src/platform/services/paths.rs` (1) — Phase B general sweep
83. `src/platform/services/supervisor_audit.rs` (1) — Phase B general sweep
84. `tests/install_local_macos.rs` (1) — Phase B/C test guard updates
85. `tests/rh_backend.rs` (1) — Phase B/C test guard updates
86. `tests/lua_task_entry_regression.rs` (1) — Phase B/C test guard updates
87. `tests/fixtures/script-project/incompatible.tasks.json` (1) — Phase B/C test guard updates
88. `fixtures/rh/caller-inventory-baseline.json` (1) — Phase B general sweep
89. `fixtures/rh/check-many-rhai-kind.json` (1) — Phase B general sweep
90. `fixtures/rh/map-set-membership.rh` (1) — Phase B general sweep

## 7. Phase B prioritized order (post-flip)

1. **agenterm.tasks.json** — Zero .rhai entries; retarget dist args to agenterm-rh
2. **.github/workflows/release.yml** — promotion-identity → scripts/rh/promotion-identity.rh
3. **`scripts/rhai/**`** — Archive/delete tree after callers flipped
4. **tests/rhai_migration.rs** — Retarget to agenterm-rh + scripts/rh/*
5. **tests/* policy guards** — rh_aot_ci_policy, release_workflow_policy, promotion_identity, etc.
6. **scripts/rh/*** — Scrub rhai path/fallback strings (script-smoke.rh, check.rh)
7. **scripts/*.json configs** — powershell-migration, artifacts, qualification-gates
8. **AGENTS.md, PRD*, plan*, skills/** — Doc/trace sweep
9. **docs/agenterm-rhai-runtime.md, bootstrap.*** — Rename/archive runtime doc references
10. **src/**, **crates/agenterm-rh/** — Phase C: shim, caller-inventory, rh_host_run_script
11. **Cargo.toml, src/bin/agenterm-rhai.rs** — Phase C: drop compat PE

## 8. Top 15 hot files

| # | Hits | File |
|--:|-----:|------|
| 1 | 98 | `tests/rhai_migration.rs` |
| 2 | 84 | `prd/PRD_02_10_rhai_scripting.md` |
| 3 | 72 | `scripts/powershell-migration.json` |
| 4 | 66 | `agenterm.tasks.json` |
| 5 | 66 | `tests/rh_task_entry_regression.rs` |
| 6 | 33 | `scripts/rhai/script-smoke.rhai` |
| 7 | 31 | `plan/plan-rh-3.md` |
| 8 | 31 | `scripts/rh/script-smoke.rh` |
| 9 | 28 | `plan/design-llm-gateway-rhai-logic-pack.md` |
| 10 | 23 | `crates/agenterm-rh/src/transpile.rs` |
| 11 | 17 | `src/client/mod.rs` |
| 12 | 17 | `tests/rh_native_task.rs` |
| 13 | 16 | `plan/plan-v0.1.15.md` |
| 14 | 16 | `src/script_project.rs` |
| 15 | 16 | `tests/rh_regression.rs` |
