# AgenTerm agent guide

This is the operational source of truth for coding agents. Start product and
repository orientation at `PRD.md`, then follow its links to the owning
`prd/PRD_*.md` module. Machine alignment lives in
`prd/alignment-contract.json`, and public version execution plans live in
`plan/`. Discover current source layout from the checkout instead of maintaining
a duplicate file map here.

## Planning and decomposition method

Use tree thinking, divergent thinking, and dependency-aware parallel thinking
as the default planning method:

1. Define one concrete product outcome, then split it into independently
   verifiable capability branches. Split each branch into behavior, evidence,
   delivery, and explicit non-goal leaves.
2. Diverge inside each branch before choosing an implementation: compare
   alternatives, edge cases, user value, authority risk, reuse, complexity,
   and evidence cost. Cut ideas that do not serve the version outcome.
3. Record sequencing, dependencies, risks, and version choices in `plan/`.
   Converge accepted product scope and capability status into the owning PRD
   modules and stable catalogs.
4. Draw the dependency graph, shared prerequisites, hot files, integration
   points, and final serial validation path before assigning implementation.
5. Parallelize independent, exclusively owned leaves; integrate at reviewed
   typed boundaries. Do not confuse a large task list with useful parallelism.

Every shipped leaf must state the user problem, governing invariant or
authority boundary, observable success evidence, safe failure result, public
black-box owner, and excluded scope.

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
.\check.ps1 -Release -IncludeStress # exact candidate qualification + receipt
.\scripts\package-qualified.ps1     # package only the qualified bytes
.\build.bat release     # distributable release artifact
.\release.ps1           # public versions only: validate/tag/push for CI
```

For this repository, `release.ps1` is the authoritative formal-release entry
point. It pushes `main` and the version tag directly through Git/GCM; do not
create a release PR, require a local `gh` installation, or substitute a generic
GitHub publishing workflow. The tag-triggered runner owns GitHub Release
creation and may use its bundled `gh` with `GITHUB_TOKEN`.
Version 0.1.7 is an internal qualification baseline: both `release.ps1` and
the tag workflow reject it. Never create or push `v0.1.7`; qualify it with the
stress-inclusive command above and use only the ignored dry-run package.

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
autostarts must honor it without taking foreground focus. Routine local release
checks may skip the bounded-journal saturation load. A candidate-bound
qualification receipt requires `check.ps1 -Release -IncludeStress`; packaging
must consume that exact receipt and must not rebuild.
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

## Cursor Cloud specific instructions

The cloud VM is **Linux**, but AgenTerm is **Windows-only** (`windows-sys`,
ConPTY, MSVC target). The PowerShell tooling (`build.bat`, `check.ps1`,
`release.ps1`, every `tests/*.ps1` smoke test) is Windows-host-only and does not
run here — for the authoritative Windows dev loop see the sections above and
`README.md`. On the Linux VM, build/lint/test the real
`x86_64-pc-windows-msvc` target by cross-compiling. The snapshot already has
Rust 1.97.0 (pinned by `rust-toolchain.toml`, with the msvc target + clippy +
rustfmt), `cargo-xwin`, LLVM `lld`/`llvm-lib`/`llvm-rc`, a `clang-cl` symlink
(`/usr/bin/clang-cl` -> `clang-18`), and Wine.

- Lint: `cargo fmt --check` runs natively; `cargo clippy --target
 x86_64-pc-windows-msvc --all-targets -- -D warnings` matches CI's gate.
- Build the four exes: `cargo xwin build --target x86_64-pc-windows-msvc`
 (`cargo-xwin` supplies the MSVC CRT/SDK and drives `lld-link`). Output lands in
 `target/x86_64-pc-windows-msvc/debug/`.
- Unit tests: `cargo xwin test --target x86_64-pc-windows-msvc` compiles for
 Windows and runs the test exes under Wine (137 lib + 16 script tests pass).
 Set `WINEPREFIX=$HOME/.wine-agenterm WINEDEBUG=-all` to keep Wine quiet.
- The compiled console exes run under Wine, e.g.
 `wine target/x86_64-pc-windows-msvc/debug/agenterm-cli.exe --help`. Launching
 `agenterm.exe` on `DISPLAY=:1` starts a working IPC server: `server-list`,
 `ui-snapshot`, `new-window`, `inspect`, `save-workspace`, etc. all round-trip.
- **Wine cannot sustain an interactive ConPTY shell**: a tab's `cmd.exe` starts
 and immediately exits `dead`, so live terminal I/O, `capture-pane` output, and
 the `tests/*.ps1` smoke suites cannot pass on Linux. Interactive-terminal and
 rendering work must be validated on a real Windows host (that is what CI on
 `windows-latest` covers). Treat Linux here as a fast lint/build/unit-test and
 control-plane sanity loop, not a full end-to-end terminal environment.
