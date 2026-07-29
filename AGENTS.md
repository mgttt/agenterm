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

`agenterm-script.exe` is an unrestricted general-purpose local runtime. Never
put Agent permission, approval, path, process, network, credential, or tool
visibility policy into Rhai profiles, API registration, or the Script broker.
Those policies belong to the future Agent harness that chooses how to invoke
the runtime. Deadlines, memory/output/concurrency budgets, typed failures, and
owned-resource cleanup are robustness controls, not permission boundaries.

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
.\lint.ps1              # fast fail: Rust, PowerShell, JSON, and production Rhai
.\build.bat             # fast incremental dev build -> .\dist\
.\build.bat release-fast # optimized incremental local-test build -> .\dist\
.\check.ps1 -Quick      # static/PRD/fmt + all-target Clippy + lib tests
.\check.ps1 -SkipSmoke  # CI-grade fmt, all-target Clippy/tests, dev artifact
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

Use a validation ladder instead of running the largest gate after every edit:
run `check.ps1 -Quick` once after a coherent implementation, then `build.bat`
plus only the directly owning smoke suite, and reserve `check.ps1 -SkipSmoke`
or full qualification for the integrated pre-push/release boundary. Search all
geometry/protocol consumers before the first black-box run so old assertions
are migrated in the same patch.

Treat build and test latency as a continuously measured product constraint.
When parallel delegation is authorized, a read-only background observer may
profile the active development loop while the primary agent keeps shipping; it
must not edit feature files, contend for the Cargo lock, or launch foreground
GUI tests. Record cold build, hot incremental build, Quick, SkipSmoke, owning
smoke, and release timings separately. Accept an optimization only when
before/after evidence proves the gain and coverage remains owned elsewhere.

Run cheap lint, formatting, JSON/catalog checks, and Rhai `check` before
expensive compilation or black-box journeys so deterministic mistakes fail
early and consume less developer time and agent context. Every expensive
behavior has one authoritative gate: broad test lanes skip wrappers already
owned by a dedicated smoke or qualification gate. Do not hide GUI, network,
stress, packaging, or release work inside a lane whose name says it skips that
work.

The former `.cargo/config.toml` forced `jobs = 1` and made clean builds much
slower. Do not restore a global job limit. Keep the default dev path
incremental and let Cargo use the machine's logical CPUs. Use `release-fast`
for repeated optimized local testing: it disables LTO, uses parallel codegen,
and retains incremental state. A final `release` build uses the dedicated
repo-local `target-release/` scratch directory, stages all distributable files
in `dist/`, and cleans only that scratch directory; it must not erase the
development `target/` cache. Release-only size optimization belongs in
`[profile.release]`. The staging path is intentionally one PowerShell process
and prefers `pwsh` when available; do not split it back into one interpreter
startup per artifact.
Build-identity freezing first reuses an existing compatible Script worker and
falls back to bootstrapping one only when it is absent or incompatible. Do not
restore an unconditional pre-identity worker build: compile-time
`AGENTERM_BUILD_*` values otherwise alternate Cargo fingerprints and add a
redundant shared-library rebuild to warm loops.
All smoke tests inherit `AGENTERM_NO_ACTIVATE=1`; GUI launches and CLI
autostarts must honor it without taking foreground focus. Routine local release
checks may skip the bounded-journal saturation load. A candidate-bound
qualification receipt requires `check.ps1 -Release -IncludeStress`; packaging
must consume that exact receipt and must not rebuild.
The release gate enforces explicit budgets of 4 MiB for `agenterm.exe` and
2 MiB each for `agenterm-cli.exe`, `agenterm-mux.exe`, and
`agenterm-mcp.exe`; investigate dependency
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
`README.md`. On the Linux VM, build/lint/test Windows targets by cross-compiling
with `cargo-xwin`. The snapshot already has Rust 1.97.0 (pinned by
`rust-toolchain.toml`, with clippy + rustfmt). Cross targets are installed
explicitly by the owning build or CI job so an ordinary host build does not
download all six matrix standard libraries. The snapshot also has `cargo-xwin`,
LLVM `lld`/`llvm-lib`/`llvm-rc`, a `clang-cl` symlink
(`/usr/bin/clang-cl` -> `clang-18`), and Wine.

CI covers all six architecture cells `{x86_64,aarch64} × {win,lnx,osx}`. Local
build commands per cell (all four binaries: `agenterm` GUI plus
`agenterm-cli`, `agenterm-mux`, `agenterm-script`):

| Cell | Host | Build |
|------|------|-------|
| **win × x86_64** | Linux + `cargo-xwin` | `cargo xwin build --target x86_64-pc-windows-msvc` (all four bins) |
| **win × aarch64** | Linux + `cargo-xwin` | `cargo xwin build --target aarch64-pc-windows-msvc` (all four bins) |
| **lnx × x86_64** | Linux native | `cargo build --target x86_64-unknown-linux-gnu --bin agenterm --bin agenterm-cli --bin agenterm-mux --bin agenterm-script` |
| **lnx × aarch64** | Linux + `gcc-aarch64-linux-gnu` | `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc cargo build --target aarch64-unknown-linux-gnu --bin agenterm --bin agenterm-cli --bin agenterm-mux --bin agenterm-script` |
| **osx × aarch64** | macOS | `cargo build --target aarch64-apple-darwin --bin agenterm --bin agenterm-cli --bin agenterm-mux --bin agenterm-script` |
| **osx × x86_64** | macOS | `cargo build --target x86_64-apple-darwin --bin agenterm --bin agenterm-cli --bin agenterm-mux --bin agenterm-script` |

Clippy (all four bins unless noted): append `-- -D warnings` to the matching
`cargo clippy` or `cargo xwin clippy` invocation with the same `--target` and
`--bin` flags. On Linux, `cargo fmt --check` runs natively.

**Windows x86_64 on Linux** (primary cloud loop):

- Lint: `cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`
- Build: `cargo xwin build --target x86_64-pc-windows-msvc` →
  `target/x86_64-pc-windows-msvc/debug/`
- Unit tests: `cargo xwin test --target x86_64-pc-windows-msvc` compiles for
  Windows and runs the test exes under Wine (137 lib + 16 script tests pass).
  Set `WINEPREFIX=$HOME/.wine-agenterm WINEDEBUG=-all` to keep Wine quiet.
- Smoke: `wine target/x86_64-pc-windows-msvc/debug/agenterm-cli.exe --help`.
  Launching `agenterm.exe` on `DISPLAY=:1` starts a working IPC server:
  `server-list`, `ui-snapshot`, `new-window`, `inspect`, `save-workspace`, etc.
  all round-trip.

**Linux clients on this VM**:

- x86_64: `./scripts/build-linux-clients.sh` (or set `AGENTERM_BUILD_PROFILE=release`)
- aarch64: install `gcc-aarch64-linux-gnu`, then
  `./scripts/build-linux-aarch64-clients.sh` (or the `lnx × aarch64` cargo line above).
  Smoke under QEMU: `qemu-aarch64-static target/aarch64-unknown-linux-gnu/debug/agenterm-cli --help`
- GUI `agenterm` builds on Linux/macOS; CI only checks the binary exists (no DISPLAY smoke).

**Wine / ConPTY limits**: Wine cannot sustain an interactive ConPTY shell — a
tab's `cmd.exe` starts and immediately exits `dead`, so live terminal I/O,
`capture-pane` output, and the `tests/*.ps1` smoke suites cannot pass on Linux.
Interactive-terminal and rendering work must be validated on a real Windows host
(that is what CI on `windows-latest` covers). Treat Linux here as a fast
lint/build/unit-test and control-plane sanity loop, not a full end-to-end
terminal environment.

On Linux/macOS, `agenterm-cli script` hosting is Windows-only for now — invoke
`agenterm-script` directly. Instance discovery uses
`~/.local/share/agenterm/instances/` (override with `AGENTERM_INSTANCE_DIR`).
