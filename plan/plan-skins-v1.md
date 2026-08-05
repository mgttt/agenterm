# Built-in skins v1 — execution plan

Status: **authorized to start design + scaffolding** (2026-08-05).
Product contract SSOT: [`prd/PRD_02_06_human_workspace.md`](../prd/PRD_02_06_human_workspace.md)
§ Built-in skins (v1). Does not create a tag/Candidate/Release by itself.

## Outcome

Ship four built-in presets — `classic-day`, `classic-night`, `fancy-day`,
`fancy-night` — with skinable palette, brand/title template, icon, and light
metrics, on top of today's Dark/Light theme machinery. External SkinHub
packages stay deferred (roadmap M14).

## Dependency graph

```text
PRD_02_06 contract (done in this plan's first commit)
        │
        ├─► 分身3 设计 tokens / assets/skins/**   (no src/ hot path)
        │         │
        │         └─► hex palettes + brand/title + icon direction
        │
        └─► 分身4 工程 appearance model + settings/snapshot/smoke
                  │
                  ├─ phase A: classic = today's DARK/LIGHT; fancy stub OK
                  └─ phase B: consume 分身3 palettes/icons; Win/Unix chrome
```

Integration owner: **主控2**. Final serial validation on integrated `main`.

## Parallel ownership (exclusive while active)

| Agent | Owns | Must not touch |
|-------|------|----------------|
| **分身3**（皮肤设计） | `assets/skins/**`, design tables under that tree | `src/**`, `scripts/rhai/**`, `prd/**`, settings hot paths |
| **分身4**（皮肤工程） | `src/theme.rs` (or `appearance` split), `src/settings.rs`, `src/frontend/settings.rs`, `src/locale.rs`, `src/ui_snapshot.rs`, theme control wiring in Win/Unix settings UI, `scripts/rhai/theme-smoke.rhai`, related alignment rows | `assets/skins/fancy/**` until 分身3 lands; do not invent final fancy brand art |
| **主控2** | PRD/plan, registry/mailbox, merge order, conflict adjudication | — |

Hot shared files (`src/lib.rs`, `Cargo.toml`, `PRD.md`, `AGENTS.md`): only
主控2 or a single authorized owner after explicit mailbox note.

## Phases

### Phase 0 — Contract (主控2)

- [x] PRD_02_06 built-in skins section
- [x] This plan
- [x] Spawn 分身3 / 分身4; update `skills/cursor/session-registry.md` + mailbox

### Phase 1 — Design freeze (分身3) ✅ 已合 main

Deliver under `assets/skins/`:

1. `classic/manifest.json` and `fancy/manifest.json` with:
   - `id`, display names (en + zh-Hant), `title_template`, `brand_short`,
     `brand_full`, corner-radius metrics, icon paths
2. Four palette tables mapping every `ThemePalette` field + ANSI-16 to hex
   (`classic-day` ≈ today's Light, `classic-night` ≈ today's Dark;
   fancy must pass WCAG AA for text/muted on surfaces and remain ANSI-readable)
3. Icon direction notes (classic = current assets; fancy = new art brief +
   placeholder PNG if final art not ready)
4. Settings 2×2 picker copy (short descriptions for snapshot `description`)

Evidence: files on a short-lived `cursor/skins-design-*` branch; draft PR
optional; 主控2 merges to `main` after review.

### Phase 2A — Engineering scaffold (分身4, parallel with Phase 1) ✅

1. Introduce `SkinId` × `Luminance` (or `AppearancePreset`) with composite ids
2. Map `classic-night`/`classic-day` to existing `DARK`/`LIGHT` const palettes
3. Fancy presets may temporarily alias classic until Phase 2B (must still
   expose distinct ids in settings/snapshot)
4. Persist + migrate `color_theme` → new field; keep derived compatibility
5. Extend locale labels and settings UI beyond two Dark/Light buttons
6. Update `theme-smoke` for four ids + migration cases
7. Small commits; prefer merge to `main` via 主控2 review (avoid long-lived
   orphan branches)

### Phase 2B — Consume design (分身4 after Phase 1 merge) ✅

1. Wire fancy (and any classic tweaks) from `assets/skins` or generated consts
2. Title template unification (Win + Unix)
3. Fancy icons + Linux runtime window icon where feasible
4. Render metrics (radius/border) if the design freeze includes them
5. PNG/render-parity evidence for luminance pairs; snapshot proves fancy≠classic

**Phase 2B deferred (post-merge leaves):**

- Apply 后 Linux window icon 不刷新（仅 startup 设一次）
- Windows `build.rs` 仍 embed `assets/agenterm.ico`，未切 fancy skin icon
- `SkinMetrics` corner radius / border 仅暴露于 `ui-snapshot`，Win/Unix render 仍 rectilinear

**Post-merge cleanup (主控2):**

- [x] Remove dual palette SSOT: delete `DARK`/`LIGHT` consts; `ThemeId::palette`
  and all presets read only `assets/skins/**/palettes/*.json` via
  `embedded_palettes()`
- [x] Shared `appearance_preset_grid` in `src/frontend/settings.rs`; Win/Unix
  settings chrome consume it (paint/native controls stay host-local)

### Phase 3 — Integration (主控2)

1. Rebase/merge both leaves; resolve conflicts serially
2. `./lint.sh` / Quick / owning `theme-smoke` on Linux; Windows path via CI or
   Win agent if needed
3. Update `prd/alignment-contract.json` only when evidence ids change
4. Flip PRD checkboxes when green; delete spent `cursor/*` branches

## Risks

| Risk | Mitigation |
|------|------------|
| Fancy palette drifts from industrial constitution | Design review against PRD non-goals before Phase 2B |
| Settings migration breaks old clients | Forward-compatible deserialize; unknown → classic-night |
| Win/Unix title/icon asymmetry | Explicit host capability; typed unsupported where host cannot set icon |
| Parallel edit of `theme.rs` vs design | Design never edits `src/`; engineering aliases until merge |
| Smoke still assumes two theme_options | Migrate assertions in the same engineering patch |

## Non-goals (this plan)

- SkinHub marketplace / `kind: skin` packages
- User CSS/JSON theme files as a public contract
- Changing workspace/settings directory names
- macOS-only or Windows-only exclusive skins

## Handoff checklist for cloud agents

```text
git fetch && git pull --ff-only origin main
Read: prd/PRD_02_06_human_workspace.md § Built-in skins
      plan/plan-skins-v1.md
      skills/cursor/session-registry.md + mailbox.md
Update mailbox seat block before coding
Small commits; report SHA + evidence; ask 主控2 before touching foreign files
```
