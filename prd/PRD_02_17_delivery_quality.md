# Delivery and quality

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] fast incremental developer build under ignored local `dist/`
- [x] release mode and `agenterm.json` build metadata
- [x] size-optimized release profile and enforced 4 MiB GUI plus 2 MiB
  per-control-CLI budgets
- [x] GUI `agenterm.exe` has no startup console flash
- [x] console `agenterm-cli.exe` preserves CLI output and exit codes
- [x] startup regression requires a main window within one second locally
- [x] version-tagged GitHub Release automation for all four EXEs, metadata,
  and ZIP
- [x] release automation publishes `agenterm-mux.exe` after its acceptance
  gate
- [ ] `agenterm-bash.exe` remains gated and unpublished
- [x] release metadata reports version, build time, commit, enabled
  features, and SHA-256 for every executable/runtime component
- [x] unit tests for command parsing, protocol, settings, and RMUX status
- [x] PRD alignment lint keeps the public command registry, protocol feature
  flags, mux compatibility output, and declared evidence synchronized
- [~] stable capability/evidence ID contract covers protocol features and
  critical terminal input behavior; rendering, CJK, performance, and the
  remaining shipped leaves still need registered evidence
- [x] CLI and semantic UX smoke tests through public interfaces
- [x] one-command fmt, Clippy, test, build, and smoke regression
- [x] release CI runs the isolated public CLI and fleet smoke suites before
  packaging, even when the redundant GUI smoke suites are skipped
- v0.1.7 fast and trustworthy release gate
  - [ ] `release.ps1` completes local clean-tree/version/tag/remote/auth
    preflight in p95 <= 15 seconds excluding interactive authentication and
    network retry, then atomically pushes `main` and the version tag
  - [ ] the v0.1.6 baseline is approximately 4m20s local release qualification
    plus 4m11s tag workflow; v0.1.7 cache-hit tag-to-Release median over the
    latest three runs is <= 2m30s and at least 35% faster, while a cold run does
    not exceed 4m11s
  - [ ] CI reports queue, cold/cache-hit, job/step and tag-to-Release timing;
    the first actionable failure diagnostic appears within 90 seconds when the
    failing stage can start within that bound
  - [ ] one commit/version/hash provenance manifest identifies the only release
    artifact set; all required tests consume it and the publish job promotes it
    byte-for-byte without rebuilding
  - [ ] the 4,128-write bounded-journal saturation journey runs exactly once
    per release SHA, while desktop GUI journeys remain isolated and
    `no-activate`
  - [ ] Cargo registry, Git sources and compatible build outputs use bounded,
    correctly keyed CI caches; cache miss/corruption cannot alter correctness,
    and developer `target/` cleanup remains explicit
- Scripting public-interface evidence gate
  - Rhai black-box evidence
    - [x] `tests/script_smoke.ps1` drives only public `script check`,
      `script eval`, `script run`, and `script api --json` commands; no test
      links the Rhai host or invokes an internal worker API
    - [ ] fixtures prove deterministic `pure` output, an `observe` snapshot
      and journal position matching `agenterm-cli`, denied mutation and
      ambient authority, stable parse/runtime/limit exit classes, timeout,
      output truncation, worker crash, and subsequent recovery
    - [ ] every Rhai timeout/crash case includes an independent public GUI,
      PTY, and workspace-health assertion; a sidecar error alone is not
      accepted as isolation evidence
  - PRD-command-test alignment
    - [x] evidence IDs `script.rhai-pure`, `script.rhai-observe`,
      `script.rhai-deny-budget`, and `script.rhai-framed` are registered with
      post-assertion emissions
    - [x] changing a shipped script command, capability, API entry, or
      evidence ID must atomically update the public command/API catalog,
      PRD `[x]` leaf, black-box assertion, and alignment contract
    - [~] `tests/prd_alignment.ps1` compares the public command/evidence
      catalog with the PRD contract; exact Rhai API-field comparison remains
      planned
    - [x] `check.ps1` runs `tests/script_smoke.ps1` before the
      safe-scripting release tag
- Autonomous human UX dogfood
  - Latest reproducible findings
    - [ ] P1 target ambiguity: `agenterm-cli new-window -d -n "Research
      Team"` prints mutable index `1` rather than stable ID `@2`; feeding
      that result to `--parent` or `wait-ui --active` can address a
      different tab. Acceptance: creation JSON or a documented format
      returns the stable ID, and a black-box test uses that exact value for
      create-child, select, wait, rename, and close after indexes shift
    - [ ] P1 settings isolation: distinct `AGENTERM_IPC_ADDRESS` and
      `AGENTERM_WORKSPACE_PATH` still share
      `%LOCALAPPDATA%\AgenTerm\settings.json`, so an isolated font test
      changes every running instance. Acceptance: an explicit settings-path
      override scopes read/write/restart tests and leaves the user's file
      byte-identical
    - [ ] P1 window-control gap: `ui-snapshot` observes minimized state and
      geometry, but the public semantic interface cannot resize, minimize,
      maximize, or restore a window; the 2026-07-27 run required Win32
      automation. Acceptance: public actions drive each state, `wait-ui`
      verifies it, minimize preserves the last PTY grid, and restore/resize
      produce the expected new grid
    - [ ] P2 active-tree readability: the three 24-pixel action targets
      reduce the selected child row's note to `child agent wor...` at the
      default 250-pixel sidebar. Acceptance: screenshot fixtures prove
      name/note and actions remain distinguishable at default width, deep
      nesting, long CJK text, and 125%/150% display scaling
    - [ ] P3 language consistency: the default English surface mixes
      `Settings`, `New`, and `Compose input` with `发送`. Acceptance: one
      locale source selects all visible labels and snapshots contain no
      unintended mixed-language controls
  - [ ] add a public-interface dogfood gate that starts the release artifact
    with isolated IPC, workspace, settings, session, and evidence paths;
    fixed sleeps and private state hooks are forbidden
  - [ ] drive first start, root/child creation, stable-ID targeting,
    rename/note, switching, composer edit/send, keyboard/Backspace, terminal
    mouse, viewport scroll, resize/minimize/restore, exit retention,
    dead/live explicit close, normal shutdown/restart recovery, and font
    settings in one deterministic journey
  - [ ] after every transition save `ui-snapshot`, relevant
    pane/workspace/settings JSON, command/exit result, and whole-window or
    pane PNG under one timestamped evidence directory with build metadata
  - [ ] post-assert state rather than command success alone: composer text
    executes once, scroll offsets and PNG viewport agree, dead exit code and
    final screen remain, live close exposes confirmation, tree/name/note/
    active ID survive restart, and settings restore after the test
  - [ ] always shut down the isolated instance, restore any external state,
    detect orphan workers/windows, and fail release qualification for any
    P0/P1 finding; P2/P3 findings require an owned planned leaf and retained
    reproduction evidence
  - [ ] the 2026-07-27 v0.1.3 baseline evidence is under
    `D:\tmp\agenterm-dogfood-v014\`: `01-first`, `02b-tree-corrected`,
    `03/04-composer`, `05/06/07-scroll`, `08/09/10-window-state`,
    `12-settings`, `13-exit-retained`, `14-live-close-modal`, and
    `16/17-restart` JSON/PNG pairs
- [ ] automated terminal input/resize/ANSI/CJK/long-output matrix
- [ ] installer, updater, stable PATH location, and signed releases
