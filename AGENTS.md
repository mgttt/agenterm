# AgenTerm agent guide

This is the operational source of truth for coding agents. Product intent and
status live in `docs/PRD.md`; avoid creating additional design documents unless
the information cannot live here or in the PRD.

## Repository map

- `src/lib.rs` — Win32 window, terminal/tab state, rendering, and command
  execution.
- `src/bin/agenterm.rs` — Windows-subsystem GUI entry point.
- `src/bin/agentermctl.rs` — console-subsystem CLI entry point.
- `src/commands.rs` — reusable CLI parsing, command catalog, key mapping, and
  output-path helpers.
- `src/protocol.rs` — serialized local IPC request/response contract.
- `src/rmux_status.rs` — RMUX status-line window parsing and click ranges.
- `src/settings.rs` — persistent user settings.
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
incremental; release-only optimization belongs in `[profile.release]`.

## Runtime control and observation

Discover the live interface instead of duplicating a long command manual:

```powershell
.\dist\agentermctl.exe --help
.\dist\agentermctl.exe list-commands
.\dist\agentermctl.exe protocol-info
.\dist\agentermctl.exe ui-snapshot
.\dist\agentermctl.exe list-windows -F '#{window_id}:#{window_name}'
```

Use a distinct `AGENTERM_IPC_ADDRESS` for isolated tests. Prefer stable tab IDs
(`@N`) over mutable indexes or titles. Use `wait-pane` and `wait-ui`; do not add
fixed sleeps. Rendering investigations should capture both structured state and
PNG evidence.

## Change rules

- Preserve the remain-on-exit and explicit-close invariants in the PRD.
- Keep pure parsing, protocol, and settings logic outside the Win32 state
  machine and cover it with unit tests.
- Exercise behavior through the public CLI in black-box tests.
- Update the PRD tree when capability state changes.
- Keep README human-facing and brief; keep this file agent-facing.
- Do not commit generated binaries. Local artifacts belong in ignored `dist/`;
  downloadable binaries are published by the tag-triggered release workflow.
- Keep `agenterm.exe` as a Windows-subsystem GUI and `agentermctl.exe` as the
  console control client. Both entry points must reuse the library.
- Do not claim full tmux/RMUX compatibility. One AgenTerm tab is currently one
  pane, and unsupported commands must fail explicitly.
