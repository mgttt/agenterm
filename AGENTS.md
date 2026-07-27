# AgenTerm agent guide

This is the operational source of truth for coding agents. Product intent and
status live in the product set rooted at `PRD.md`; third-level detail lives in
the linked `prd/PRD_*.md` modules, and machine alignment lives in
`prd/alignment-contract.json`. Avoid creating additional design documents
unless the information cannot live here or in that product set.

## Repository map

- `src/lib.rs` — Win32 window, terminal/tab state, rendering, and command
  execution.
- `src/bin/agenterm.rs` — Windows-subsystem GUI entry point.
- `src/bin/agenterm-cli.rs` — console-subsystem CLI entry point.
- `src/bin/agenterm-mux.rs` — tmux/RMUX compatibility CLI entry point.
- `src/bin/agenterm-script.rs` — one-invocation constrained Rhai worker.
- `src/commands.rs` — reusable CLI parsing, command catalog, key mapping, and
  output-path helpers.
- `src/event_journal.rs` — bounded observable-event ordering and gap detection.
- `src/instances.rs` — multi-server registration and discovery records.
- `src/protocol.rs` — serialized local IPC request/response contract.
- `src/rmux_status.rs` — RMUX status-line window parsing and click ranges.
- `src/script_protocol.rs` — versioned host/worker scripting contract.
- `src/settings.rs` — persistent user settings.
- `src/tab_tree.rs` — pure hierarchy ordering and cycle detection.
- `src/workspace.rs` — versioned tab-workspace persistence.
- `tests/` — black-box tests that drive only the public AgenTerm executable.
- `assets/` — application icon sources.
- `scripts/` — build metadata tooling.
- `prd/` — detailed product-tree modules linked from the canonical `PRD.md`
  index; do not duplicate scope between modules.
- `plan-0.1.7.md` — version execution projection for dependencies, milestones,
  risks, and sequencing; it must link to, not replace, the owning PRD nodes.

## Parallel execution discipline

Before editing, sketch the task's dependency graph: identify independent work,
shared prerequisites, integration points, and the final validation path. Use
subagents by default for genuinely independent branches such as:

- changes whose owned file sets do not overlap;
- read-only code or reference audits that can inform an implementation;
- isolated black-box test investigations with distinct IPC and workspace paths;
- documentation or test work that does not depend on an unfinished interface.

Give every subagent a bounded deliverable, an explicit file-owner list, and the
evidence it must return. Ownership is exclusive while the task is active. The
subagent must report changed files, tests run, findings, and any assumptions,
then hand control of those files back to the primary agent. The primary agent
owns cross-cutting decisions, reviews every handoff, resolves integration
issues, and commits only small coherent increments.

All agents share one checkout and see edits immediately. Never concurrently
edit a hot/shared file such as `src/lib.rs`, `PRD.md`, `Cargo.toml`, build
scripts, or this guide. Split work at stable file boundaries where possible; if
two tasks must touch the same file, serialize them under one owner. Do not run
competing Cargo builds against the same target directory, and do not let a test
agent rebuild or replace an artifact another agent is actively validating.

Parallelism is a latency tool, not a goal. Keep tightly coupled changes,
one-file refactors, quick inspections, and tasks dominated by a shared
prerequisite on the primary path. After parallel work returns, integrate and
review it before validation. Run the final formatting, Clippy, unit-test,
artifact, and full public-interface gates serially on the integrated tree so the
result represents one reproducible source state.

## Development loop

Use PowerShell from the repository root:

```powershell
.\build.bat             # fast incremental dev build -> .\dist\
.\build.bat release-fast # optimized incremental local-test build -> .\dist\
.\check.ps1 -SkipSmoke  # fmt, Clippy, unit tests, dev artifact
.\check.ps1             # full public-interface regression
.\check.ps1 -Release    # local release gate; skips event-journal load stress
.\build.bat release     # distributable release artifact
.\release.ps1           # validate, tag, push; CI publishes GitHub Release
```

For this repository, `release.ps1` is the authoritative formal-release entry
point. It pushes `main` and the version tag directly through Git/GCM; do not
create a release PR, require a local `gh` installation, or substitute a generic
GitHub publishing workflow. The tag-triggered runner owns GitHub Release
creation and may use its bundled `gh` with `GITHUB_TOKEN`.

The former `.cargo/config.toml` forced `jobs = 1` and made clean builds much
slower. Do not restore a global job limit. Keep the default dev path
incremental and let Cargo use the machine's logical CPUs. Use `release-fast`
for repeated optimized local testing: it disables LTO, uses parallel codegen,
and retains incremental state. After staging all distributable files in
`dist/`, the final `release` build deliberately runs `cargo clean` so `target/`
cannot grow without bound. Release-only size optimization belongs in
`[profile.release]`. The staging path is intentionally one PowerShell process
and prefers `pwsh` when available; do not split it back into one interpreter
startup per artifact.
All smoke tests inherit `AGENTERM_NO_ACTIVATE=1`; GUI launches and CLI
autostarts must honor it without taking foreground focus. Local release
qualification skips the bounded-journal saturation load. Only the clean release
CI runner should opt back in with `check.ps1 -Release -IncludeStress`.
The release gate enforces explicit budgets of 4 MiB for `agenterm.exe` and
2 MiB each for `agenterm-cli.exe` and `agenterm-mux.exe`; investigate dependency
or feature growth instead of raising them casually.

## Runtime control and observation

Discover the live interface instead of duplicating a long command manual:

```powershell
.\dist\agenterm-cli.exe --help
.\dist\agenterm-cli.exe list-commands
.\dist\agenterm-cli.exe protocol-info
.\dist\agenterm-cli.exe ui-snapshot
.\dist\agenterm-cli.exe list-windows -F '#{window_id}:#{window_name}'
```

Use distinct `AGENTERM_IPC_ADDRESS` and `AGENTERM_WORKSPACE_PATH` values for
isolated tests. Prefer stable tab IDs
(`@N`) over mutable indexes or titles. Use `wait-pane` and `wait-ui`; do not add
fixed sleeps. Rendering investigations should capture both structured state and
PNG evidence.

The GUI must expose its native window before starting the initial ConPTY.
`tests/startup_smoke.ps1` guards a one-second local first-window budget and then
waits through public state until the asynchronous terminal becomes ready.

## Terminal interaction engineering

PuTTY is the local professional-terminal reference implementation. The reviewed
baseline is `D:\dev\putty` commit
`61574e2e98f7d262dea4ff6380e167541518aedf` (2026-07-25). Use it to check
interaction invariants and edge cases, not as a source for blind code copying.
Its permissive licence still requires preserving its notice with any substantial
copied portion; prefer independent Rust implementations based on observed
behavior.

- Treat mouse input as an explicit arbitration between local selection and
  application-requested raw mouse reporting. A selection gesture must keep
  ownership through release; a future raw-mouse path should support a documented
  Shift override for local selection and must never send an unmatched release.
- Keep selection states distinct: button-down/about-to-select, dragging, and
  completed. A click that never becomes a drag must retain its terminal/RMUX
  click behavior. Capture loss, modal menus, tab changes, and shutdown must
  cancel an unfinished drag instead of leaving input or rendering suspended.
- Store and compare selection endpoints as terminal-cell positions. Normalize
  forward/reverse selections, skip wide-cell continuations when copying, use
  Windows CRLF in clipboard text, and test CJK plus wrapped/multiline content.
  Dragging beyond the viewport should eventually auto-scroll without inventing
  out-of-range cells.
- Accumulate high-resolution wheel deltas until `WHEEL_DELTA`; do not discard
  partial input. Wheel events go to scrollback unless an application raw-mouse
  mode owns them. Scrollbar thumb positions need full-width arithmetic and the
  viewport, capture, screenshots, and structured snapshot must agree.
- Respect Win32 clipboard ownership: allocate movable NUL-terminated UTF-16,
  transfer ownership only after `SetClipboardData` succeeds, and free on every
  pre-transfer failure. Clipboard reads for future terminal paste must not block
  the GUI thread; normalize newlines, filter unsafe control characters, and
  honor bracketed-paste framing.
- Minimize must not resize the PTY to the iconic client rectangle. Resize,
  maximize/restore, font metrics, DPI changes, scrollbar geometry, and terminal
  rows/columns form one contract and need state plus PNG evidence.

## Change rules

- All agents and subagents work in the single shared `D:\dev\agenterm`
  checkout on `main`. Do not create Git worktrees, task branches, or hidden
  planning copies. Material planning progress must be written incrementally to
  the applicable `PRD.md`/`prd/PRD_*.md` product node so it is immediately
  visible in the repository; the primary agent reviews, commits, and pushes
  small coherent increments.
- Preserve the remain-on-exit and explicit-close invariants in the PRD.
- Preserve tree safety: parent cycles are rejected and closing a parent promotes
  its direct children instead of terminating them.
- Keep pure parsing, protocol, and settings logic outside the Win32 state
  machine and cover it with unit tests.
- Exercise behavior through the public CLI in black-box tests.
- Update the PRD tree when capability state changes.
- Keep README human-facing and brief; keep this file agent-facing.
- Do not commit generated binaries. Local artifacts belong in ignored `dist/`;
  downloadable binaries are published by the tag-triggered release workflow.
- Keep `agenterm.exe` as a Windows-subsystem GUI, `agenterm-cli.exe` as the
  native control client, and `agenterm-mux.exe` as the compatibility client.
  All entry points must reuse the library.
- Do not claim full tmux/RMUX compatibility. One AgenTerm tab is currently one
  pane, and unsupported commands must fail explicitly.
