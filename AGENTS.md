# AgenTerm agent guide

This is the operational source of truth for coding agents. Product intent and
status live in `docs/PRD.md`; avoid creating additional design documents unless
the information cannot live here or in the PRD.

## Repository map

- `src/lib.rs` — Win32 window, terminal/tab state, rendering, and command
  execution.
- `src/bin/agenterm.rs` — Windows-subsystem GUI entry point.
- `src/bin/agentermctl.rs` — console-subsystem CLI entry point.
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

## Development loop

Use PowerShell from the repository root:

```powershell
.\build.bat             # fast incremental dev build -> .\dist\
.\check.ps1 -SkipSmoke  # fmt, Clippy, unit tests, dev artifact
.\check.ps1             # full public-interface regression
.\build.bat release     # distributable release artifact
.\release.ps1           # validate, tag, push; CI publishes GitHub Release
```

The former `.cargo/config.toml` forced `jobs = 1` and made clean builds much
slower. Do not restore a global job limit. Keep the default dev path
incremental; release-only size optimization belongs in `[profile.release]`.
The release gate enforces explicit budgets of 4 MiB for `agenterm.exe` and
2 MiB each for `agentermctl.exe` and `agenterm-mux.exe`; investigate dependency
or feature growth instead of raising them casually.

## Runtime control and observation

Discover the live interface instead of duplicating a long command manual:

```powershell
.\dist\agentermctl.exe --help
.\dist\agentermctl.exe list-commands
.\dist\agentermctl.exe protocol-info
.\dist\agentermctl.exe ui-snapshot
.\dist\agentermctl.exe list-windows -F '#{window_id}:#{window_name}'
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
  `docs/PRD.md` so it is immediately visible in the repository; the primary
  agent reviews, commits, and pushes small coherent increments.
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
- Keep `agenterm.exe` as a Windows-subsystem GUI, `agentermctl.exe` as the
  native control client, and `agenterm-mux.exe` as the compatibility client.
  All entry points must reuse the library.
- Do not claim full tmux/RMUX compatibility. One AgenTerm tab is currently one
  pane, and unsupported commands must fail explicitly.
