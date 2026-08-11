# AgenTerm agent guide

This is the operational source of truth for coding agents. Start product and
repository orientation at `PRD.md`, then follow its links to the owning
`prd/PRD_*.md` module. Machine alignment lives in
`prd/alignment-contract.json`, and public version execution plans live in
`plan/`. Current source layout SSOT is [`plan/ARCHITECTURE.md`](plan/ARCHITECTURE.md);
do not invent a second living file map in prompts or version plans.

Writing or editing a script? Start at
[Script engines](#script-engines--read-the-condensed-manual-before-writing-a-script).

## Document redaction (hard rule · never re-offend)

**Incident (2026-08-08):** a handoff goal under `plan/` shipped host home absolute
paths. That leaks identity. **Never** paste `pwd` / conversation CWD into the tree.

Applies to **every** write: `plan/**`, `prd/**`, `docs/**`, `README*`, examples,
tests/fixtures, commit messages, screenshots, prompts, and goal/handoff markdown
for other agents. Same bar as credentials.

### Path policy (all OS / ISA — one vocabulary)

**Target form is only two kinds:**

1. **Repo-relative** — files inside the clone: `plan/...`, `src/...`, `./check.sh`
2. **`~/...`** — anything under the user home (product data, config, caches)

Never paste `pwd` / expanded CWD. Prefer “from repository root” over `cd` + absolute.

#### Home conversion table (mandatory — memorize this)

Strip the host home root (and the account segment) and rewrite with `~/`.
Use `/` after `~/` even on Windows. Real names and `<name>` placeholders both convert.

| Host form (do not leave in tree) | Write as |
|----------------------------------|----------|
| `/Users/<name>/...` (Darwin) | `~/...` |
| `/home/<name>/...` (Linux) | `~/...` |
| `%USERPROFILE%\...` or `%UserProfile%\...` | `~/...` |
| `$env:USERPROFILE\...` (PowerShell) | `~/...` |
| `C:\Users\<name>\...` (and other drive letters) | `~/...` |
| `$HOME/...` when used as a *documented path* | prefer `~/...` |

Examples:

| Before | After |
|--------|--------|
| `/Users/<name>/.local/share/agenterm` | `~/.local/share/agenterm` |
| `/home/<name>/.config/agenterm` | `~/.config/agenterm` |
| `%USERPROFILE%\AppData\Local\agenterm` | `~/AppData/Local/agenterm` |
| `C:\Users\<name>\.local\share\agenterm` | `~/.local/share/agenterm` |
| absolute path *into this clone* | repo-relative (`src/...`), not `~/...` |

| Class | Action |
|-------|--------|
| Absolute path inside the clone | → repo-relative |
| Absolute path under user home (any row above) | → `~/...` |
| Product data already under home | → `~/...` only |

**Also never write:** real email / phone / token / API key / SMTP pass → RFC 2606 /
`<AUTH_CODE>` / `<API_KEY>` / `<TOKEN>`; real IP / MAC / personal hostname →
`<IP>` / `<HOST>` / generic `station`.

**Exception (code only):** unit tests that assert path *parsers* may use fixed
synthetic absolute strings without a real account name. Docs, PRD, plan, goals,
and handoff prompts **never** get that exception.

**Mandatory pre-commit / post-write self-check:**

```bash
# From repository root; any hit = rewrite (repo-relative or ~/)
./scripts/doc-redact-check.sh path/to/file
```

- Path hit → apply the conversion table above
- Credential-like values → placeholders only
- Handoff prompts for other agents: same scrub; “internal plan” is not exempt

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

`agenterm-rh` is an unrestricted general-purpose local runtime. Never
put Agent permission, approval, path, process, network, credential, or tool
visibility policy into Rhai profiles, API registration, or the Script broker.
Those policies belong to the future Agent harness that chooses how to invoke
the runtime. Deadlines, memory/output/concurrency budgets, typed failures, and
owned-resource cleanup are robustness controls, not permission boundaries.
Raw sockets, listeners, UDP, WebSockets, arbitrary local paths (including
destructive filesystem targets), child processes, and Fleet mutation are valid
Script Runtime capabilities; do not add protected-path, target, process, or
endpoint allowlists while implementing them. In Script Runtime documentation
and catalogs, `capability` means API discovery and compatibility metadata only;
it must never be interpreted as an authorization grant, denial, or sandbox.
An API that has not shipped is a product gap, never permission policy: keep it
truthfully discoverable as planned or unavailable instead of substituting a
loopback-only, safe-path, approved-process, or other policy-reduced variant.
Do not use `safe`, `pure`, `observe`, profiles, or capability tiers as
euphemisms for a reduced Script API. Name robustness limits, native product
data-integrity invariants, and upper-layer caller policy explicitly so they
cannot be mistaken for Rhai authorization.
Legacy task `profile` fields are inert compatibility data and must not change
API registration, visibility, arguments, targets, or execution behavior.

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

Subagents leave handoff files **unstaged** in the shared checkout. The primary
agent stages exact reviewed paths immediately before committing; early or broad
staging can accidentally include another active agent's unfinished work.

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

## Script engines — read the condensed manual before writing a script

Scripts are written in **rh** (`scripts/rh/**.rh`, the AOT-transpiled Rust
subset that all gates and tasks run on) or **qjs** (`scripts/qjs/**.js`,
QuickJS-ng). Neither behaves the way general knowledge of Rust, Rhai, or Node
would suggest, and the two fail in opposite ways: rh rejects constructs it
cannot lower, while qjs accepts all of modern JavaScript and then has almost no
capabilities. Read the relevant manual first — the failure modes below are
recurring, not hypothetical.

| Manual | Use for |
|--------|---------|
| [`docs/agenterm-rh-cheatsheet.md`](docs/agenterm-rh-cheatsheet.md) | rh syntax subset, native-pack vs interpreter semantics, error model, shipped surface index, debug checklist |
| [`docs/agenterm-qjs-cheatsheet.md`](docs/agenterm-qjs-cheatsheet.md) | the four-primitive qjs host surface, module vs classical mode, import resolution, pack format |
| [`docs/agenterm-rh-runtime.md`](docs/agenterm-rh-runtime.md) | the full interface *specification* (reference, not a how-to) |

The two highest-yield rules, stated here so they are unmissable:

- **After editing any `.rh`, run
  `cargo run -p agenterm-rh --example mode_probe -- --root . <entry>` and require
  `mode=native host_eval_int=0`.** A silent fallback to host evaluation changes
  semantics — a missing JSON field becomes a hard failure rather than `()`, and
  objects stop stringifying by concatenation.
- **`rh_fail` records and continues, and `require` inside a helper only returns
  from that helper.** A task can print `PASS: ...` and still fail. Always read
  the *first* recorded failure.

## Development loop

Use PowerShell from the repository root:

```powershell
.\lint.cmd              # fast fail: Rust, JSON, text hygiene, and production Rhai
.\build.bat              # default: release-fast -> .\dist\ (optimized, incremental)
.\build.bat release-fast # same as default; explicit alias
.\build.bat dev          # debug PE -> .\dist\ (also lands in target/debug/)
.\build.bat release      # size-optimized distributable -> .\dist\ (+ target-release/)
.\check.cmd --quick      # static/PRD/fmt + all-target Clippy + lib tests
.\check.cmd --skip-smoke # CI-grade fmt, all-target Clippy/tests, staged artifact
.\check.cmd              # full public-interface regression
.\check.cmd --release    # local release gate; skips event-journal load stress
.\check.cmd --release --include-stress # exact qualification + receipt
.\dist\agenterm-rh.exe task run package-qualified --manifest .\agenterm.tasks.json
.\release.cmd --rehearse # read-only release validation/rehearsal
```

Linux/macOS have matching `./build.sh`, `./check.sh`, `./lint.sh`, and
`./release.sh` aliases over `scripts/bootstrap.sh`. Native Unix `build` emits
the four client binaries; default Unix `check` is the portable Quick lane, and
default Unix `release` is validation-only. Stress qualification, Windows
packaging and exact-byte qualification remain explicit Windows operations.
Do not add an unmatched `.cmd` or `.bat`: prefer a named Rhai task, and when a
Windows bootstrap remains necessary, add the equivalent `.sh` entry and cover
the pair in the cross-platform automation audit and Linux/macOS CI.

Formal delivery is an exact-SHA two-stage GitHub Actions contract.
Before Candidate, Promotion, release authentication, or Actions babysitting,
read `skills/agenterm-release/SKILL.md` and its authentication reference.
`Release Candidate` performs the one stress-inclusive Windows qualification
and builds/seals all six platform artifacts before a tag exists. `Release`
accepts an exact Candidate run plus explicit `publish-vX.Y.Z` confirmation,
revalidates the sealed bytes, creates only the exact tag, and promotes through
a recoverable unpublished draft without Cargo, tests, packaging, signing,
notarization, or overwrite. `release.cmd`
validates or rehearses only and intentionally refuses local publication.
Candidate dispatch is a mechanical action that may be automated after an
explicit exact-SHA request; public Promotion is the human release-authority
boundary. Version 0.1.7 remains an internal qualification baseline and must
never receive a tag or public Release.

## GitHub Actions observation

Treat Actions observation as a bounded read-only operation, not a continuous
15-second public-API poll. A previous multi-agent Candidate watch exhausted
GitHub's anonymous REST allowance because every observer behind the same shared
NAT consumed the same low-rate source; the run itself remained healthy, but
status, jobs, logs, and artifacts became temporarily unobservable. The failure
tree is:

- duplicated observers or lost run identity cause repeated search/list calls;
- fixed short intervals amplify calls while long matrix jobs are unchanged;
- anonymous REST shares an IP-based budget across agents and unrelated users;
- missing rate-limit handling turns temporary observation loss into API
  hammering, misleading failure reports, and unnecessary human intervention.

Use observation channels in this order:

1. the connected GitHub application/connector for authenticated run, job, log,
   and artifact reads;
2. an already authenticated `gh` session, without extracting or printing its
   token;
3. public REST only for a small bounded fallback probe;
4. browser/manual inspection when no programmatic authenticated reader exists,
   or when an explicitly human-authorized mutation such as dispatch is needed.

Resolve the workflow run once, retain its `run_id` and `run_attempt`, and query
that run directly thereafter. Do not repeatedly search by branch, SHA, or
workflow name. Keep one observer owner per run and share its last structured
result with other agents. Cache unchanged job/log/artifact results where the
reader permits it, use conditional requests (`ETag`/`If-None-Match`) for public
REST, and fetch logs or artifacts only after the owning job reaches a relevant
terminal state.

Polling must be finite and state-aware: start no faster than 30 seconds, apply
exponential backoff with jitter up to at least 2 minutes while state is
unchanged, reset only on a meaningful state transition, and stop at an explicit
deadline or terminal conclusion. Honor `Retry-After`,
`X-RateLimit-Remaining`, and `X-RateLimit-Reset`; near exhaustion, stop public
REST calls until reset and switch to an authenticated channel or report that
observation—not the workflow—is temporarily unavailable. Never fan out polling
across subagents.

Git transport authentication and GitHub API authentication are separate
authorities. GCM credentials are for Git fetch/push and must never be queried,
decoded, copied into `GH_TOKEN`, logged, or reused to manufacture API access.
Likewise, connector or `gh` credentials must not be written into Git remotes.
If dispatch or another mutation is unavailable through the connected channel,
pause and give the human the exact workflow, immutable SHA/run identity, fields,
and expected effect; do not probe credential stores or expose tokens to avoid
that human-in-the-loop boundary.

Use a validation ladder instead of running the largest gate after every edit:
run `check.cmd --quick` once after a coherent implementation, then `build.bat`
plus only the directly owning smoke suite, and reserve `check.cmd --skip-smoke`
or full qualification for the integrated pre-push/release boundary. Search all
geometry/protocol consumers before the first black-box run so old assertions
are migrated in the same patch.

The four root Windows batch files and four matching Unix shell files are thin
human aliases. Their shared platform bootstraps perform only generic stage-0
Script worker build/copy/forward/cleanup. Build profiles, testing,
qualification, packaging, cleanup and release policy belong to named Rhai
tasks; do not add task-specific branches or product rules to entry files.

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

For repository-wide rh syntax validation, use the bounded `agenterm-rh
check-many --manifest` path owned by `scripts/rh/lint.rh`, rather than
spawning one Script process per file. It retains a fresh Engine and typed
result for each input, while bounding the manifest, file count, source bytes,
and aggregate deadline. Keep the direct single-file `check` command as the
diagnostic and black-box parity baseline.

`.cargo/config.toml` once forced `jobs = 1` and made clean builds much
slower; that setting was removed. Do not restore a global job limit. The file
itself still exists and is load-bearing — it carries the
`aarch64-unknown-linux-gnu` linker setting the `lnx × aarch64` cell below
depends on. Do not delete it. Default `build.bat` stages **release-fast** into
`dist/` (optimized, no LTO, parallel codegen, incremental under
`target/release-fast/`). Pure debug PE remains `target/debug/` via ordinary
`cargo build` or explicit `build.bat dev`. A final `release` build uses the
dedicated repo-local `target-release/` scratch directory, stages all
distributable files in `dist/`, and then reclaims both `target-release/` and
development `target/`. The reusable bootstrap worker is stored outside Cargo
output so this is safe on Windows. Dev and `release-fast` loops retain target
output for incremental feedback. Release-only size optimization belongs in
`[profile.release]`. The staging path is one named Rhai task; do not split it
back into one interpreter startup per artifact.
Build-identity freezing first reuses an existing compatible Script worker and
falls back to bootstrapping one only when it is absent or incompatible. Do not
restore an unconditional pre-identity worker build: compile-time
`AGENTERM_BUILD_*` values otherwise alternate Cargo fingerprints and add a
redundant shared-library rebuild to warm loops.
All smoke tests inherit `AGENTERM_NO_ACTIVATE=1`; GUI launches and CLI
autostarts must honor it without taking foreground focus. Routine local release
checks may skip the bounded-journal saturation load. A candidate-bound
qualification receipt requires `check.cmd --release --include-stress`; packaging
must consume that exact receipt and must not rebuild.
The release gate enforces explicit budgets of 4 MiB for `agenterm.exe` and
`agenterm-cc.exe`, plus 512 KiB for `agenterm.com` (the synchronous CUI/TUI
forwarder); investigate dependency or feature growth
instead of raising them casually.

## Runtime control and observation

Discover the live interface instead of duplicating a long command manual:

```powershell
.\dist\agenterm cli --help
.\dist\agenterm cli list-commands
.\dist\agenterm cli protocol-info
.\dist\agenterm cli ui-snapshot
.\dist\agenterm cli list-windows -F '#{window_id}:#{window_name}'
```

Use distinct `AGENTERM_IPC_ADDRESS` and `AGENTERM_WORKSPACE_PATH` values for
isolated tests. Prefer stable tab IDs
(`@N`) over mutable indexes or titles. Use `wait-pane` and `wait-ui`; do not add
fixed sleeps. Rendering investigations should capture both structured state and
PNG evidence.

The GUI must expose its native window before starting the initial ConPTY.
`scripts/rh/startup-smoke.rh` guards a one-second local first-window budget
and then waits through public state until the asynchronous terminal becomes
ready.

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

- All agents and subagents work in one shared checkout on `main` (the
  repository root wherever it is cloned — Windows, macOS, and Linux hosts are
  all in active use; never hard-code a personal home path in docs). Do not
  create Git worktrees, task branches, or hidden
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
  downloadable binaries are published only by exact-Candidate Promotion.
- Keep `agenterm.exe` as a Windows-subsystem GUI and keep `agenterm.com` as a
  minimal Console-subsystem forwarder with no business logic. Extensionless
  **`agenterm cli <command>`** / **`agenterm tui`** resolves to `.com`, which
  inherits stdio, synchronously invokes the sibling `agenterm.exe`, and
  propagates its exit code. The GUI PE then attaches the caller's console
  (`AttachConsole(ATTACH_PARENT_PROCESS)`), duplicates the real
  stdin/stdout/stderr via `GetStdHandle` + `DuplicateHandle`, and spawns
  itself as a hidden `__agenterm-internal-cli` worker with those explicit
  handles, waiting and forwarding the exit code. Explorer and explicit
  `agenterm.exe` launches remain no-flash GUI entry points. The CLI
  includes **`agenterm cli mux`** / **`agenterm cli mcp`**; the standalone
  `agenterm-cli`, `agenterm-mux`, and `agenterm-mcp` PEs are removed.
  Preferred headless authority entry is `agenterm server` (separate process
  of the same PE; the old `agenterm-server.exe` binary is removed). All
  entry points must reuse the library.
- Do not claim full tmux/RMUX compatibility. One AgenTerm tab is currently one
 pane, and unsupported commands must fail explicitly.

### Platform crate vs product UI

`crates/agenterm-platform` is the **cross-platform encapsulation library** for
agenterm, wbox, and other embedding apps:

| Layer | Owns | Must not own |
|-------|------|--------------|
| **platform crate** | Typed OS contracts (window, input, clipboard, process, IPC, PTY, …) | Product UI state, Fleet, `ui-action` workbench scripts, server-strip policy, AgenTerm binary names |
| **`src/frontend/*` + `ui_*`** | Product gesture meaning, dialogs, geometry, action ids | Raw `windows_sys` / winit / x11 (boundary tests forbid this) |
| **host adapters** | Present, wake, IME, native controls, IPC wiring | New product policy that only one host implements without catalog/`parity-gap` |

- **Encapsulation success** = OS differences stop in the crate; consumers call
  facades with feature flags (`path` for agenterm; `git`+full SHA for wbox).
- **UX parity success** = shared product semantics + both host adapters wired.
  Platform does **not** “own all UX alignment” by itself.
- Do **not** ship product behavior only in the Windows remote adapter and leave
  “Unix agent later” as the plan unless the action is host-only in
  `ui_action_catalog` with an explicit `parity-gap:`.
- Mechanism leak inventory: `plan/plan-platform-encapsulation-gap.md`.
  Executable goal: `plan/goal-crate-platform.md`.

### Cross-platform UI: shared-first (Win / OSX / Lnx)

Three-host UX parity fails when product semantics land only in the Windows
remote frontend and OSX/Lnx agents re-implement later. Default path:

1. **Product meaning first** in `src/frontend/*`, `src/ui_geometry.rs`, and
   other shared modules (layout math, dialog state, action ids, focus gates,
   snapshot fields). Host adapters (`windows/remote_frontend`,
   `unix/frontend`) present, wake, IME, and native IPC only.
2. **`ui-action` set gate**: `src/frontend/ui_action_catalog.rs` lists
   `SHARED_UI_ACTIONS` plus intentional `WINDOWS_ONLY_*` / `UNIX_ONLY_*`
   allowlists. Unit tests require host inventories to match after allowlists
   and that catalog string literals still appear in the adapter sources.
   Adding a one-host action without updating the catalog/allowlist fails
   `cargo test --lib ui_action_catalog`.
3. **Same change when possible**: for a product gesture both hosts must
   expose, add the id to `SHARED_UI_ACTIONS` and wire both adapters in one
   coherent increment. Do not ship Windows-only product behavior and leave
   "Unix agent later" as the plan unless the entry is explicitly
   host-only with a `parity-gap:` reason in the catalog comment.
4. **Intentional host-only** is allowed for true platform surfaces (e.g.
   current server-strip / instance-picker depth on Windows, Unix new-terminal
   shell verb set). Document in the allowlist; promote to SHARED when the
   peer host gains the surface.
5. Do not grow dual-write match arms for new product policy when a shared
   helper already exists (`new_terminal::dispatch_ui_action`, geometry,
   interaction gates). ARCHITECTURE debt L2 tracks the remaining table-driven
   migration; this catalog is the interim set-diff gate, not the final
   ActionId enum.

## Cursor Cloud specific instructions

The cloud VM is **Linux**, but the native Windows GUI/runtime (`windows-sys`,
ConPTY, MSVC target) and its orchestration (`build.bat`, `check.cmd`, and
`release.cmd`) do not run there;
repository lint and all smoke logic are Rhai-owned. For the authoritative
Windows dev loop see the sections above and `README.md`. On the Linux VM,
build/lint/test Windows targets by cross-compiling
with `cargo-xwin`. The snapshot already has Rust 1.97.0 (pinned by
`rust-toolchain.toml`, with clippy + rustfmt). Cross targets are installed
explicitly by the owning build or CI job so an ordinary host build does not
download all six matrix standard libraries. The snapshot also has `cargo-xwin`,
LLVM `lld`/`llvm-lib`/`llvm-rc`, a `clang-cl` symlink
(`/usr/bin/clang-cl` -> `clang-18`), and Wine.

CI covers all six architecture cells `{x86_64,aarch64} × {win,lnx,osx}`. Local
build commands per cell. `src/bin/` currently holds **four** product binaries
(`agenterm`, `agenterm`, `agenterm-rh`, `agenterm-cc`). Mux/MCP are
**`agenterm cli` subcommands**, not separate PEs. **Prefer building without
`--bin` filters** so new binaries are covered automatically:

| Cell | Host | Build |
|------|------|-------|
| **win × x86_64** | Linux + `cargo-xwin` | `cargo xwin build --target x86_64-pc-windows-msvc` (all four bins) |
| **win × aarch64** | Linux + `cargo-xwin` | `cargo xwin build --target aarch64-pc-windows-msvc` (all four bins) |
| **lnx × x86_64** | Linux native | `cargo build --target x86_64-unknown-linux-gnu` (all bins; no `--bin` filter) |
| **lnx × aarch64** | Linux + `gcc-aarch64-linux-gnu` | `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc cargo build --target aarch64-unknown-linux-gnu` |
| **osx × aarch64** | macOS | `cargo build --target aarch64-apple-darwin` |
| **osx × x86_64** | macOS | `cargo build --target x86_64-apple-darwin` |

Clippy: append `-- -D warnings` to the matching `cargo clippy` or
`cargo xwin clippy` invocation with the same `--target`. Use
`--all-targets` rather than repeating `--bin` filters, so every binary and
test target is linted. On Linux, `cargo fmt --check` runs natively.

**Windows x86_64 on Linux** (primary cloud loop):

- Lint: `cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`
- Build: `cargo xwin build --target x86_64-pc-windows-msvc` →
  `target/x86_64-pc-windows-msvc/debug/`
- Unit tests: `cargo xwin test --target x86_64-pc-windows-msvc` compiles for
  Windows and runs the test exes under Wine. Do not treat any pass count as an
  expected value — the suite grows; read the command's own output.
  Set `WINEPREFIX=$HOME/.wine-agenterm WINEDEBUG=-all` to keep Wine quiet.
- Smoke: `wine target/x86_64-pc-windows-msvc/debug/agenterm-com.exe cli --help`.
  Launching `agenterm.exe` on `DISPLAY=:1` starts a working IPC server:
  `server-list`, `ui-snapshot`, `new-window`, `inspect`, `save-workspace`, etc.
  all round-trip.

**Linux clients on this VM**:

- x86_64: `./scripts/build-linux-clients.sh` (or set `AGENTERM_BUILD_PROFILE=release`)
- aarch64: install `gcc-aarch64-linux-gnu`, then
  `./scripts/build-linux-aarch64-clients.sh` (or the `lnx × aarch64` cargo line above).
  Smoke under QEMU: `qemu-aarch64-static target/aarch64-unknown-linux-gnu/debug/agenterm cli --help`
- Native GUI packages needed for `agenterm` / `agenterm-cc` on X11 (see README):
  `libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libx11-6 libxcb1 libxcb-xkb1`.
  Missing `libxkbcommon-x11-0` panics in `xkbcommon-dl` at window open.
- Native desktop smoke on `DISPLAY=:1` (TigerVNC/XFCE) or CI Xvfb:
  `AGENTERM_NO_ACTIVATE=1 AGENTERM_BOOTSTRAP_TASK=control-center-linux-smoke ./scripts/bootstrap.sh --backend x11`
  and `...=unix-frontend-linux-smoke ./scripts/bootstrap.sh <gui> <cli> --platform linux`.
  If XFCE `Xft/DPI` is `-1` on a VNC screen that reports `0mm×0mm`, winit can
  emit `scale_factor≈0.99` and fail `control_center_linux_renderer_evidence`
  (`scale_factor >= 1.0`). Fix with `echo 'Xft.dpi: 96' | xrdb -merge` and
  `xfconf-query -c xsettings -p /Xft/DPI -s 96` before the smoke.

**Wine / ConPTY limits**: Wine cannot sustain an interactive ConPTY shell — a
tab's `cmd.exe` starts and immediately exits `dead`, so live terminal I/O,
`capture-pane` output, and the GUI smoke suites (`scripts/rh/*-smoke.rh`)
cannot pass on Linux.
Interactive-terminal and rendering work must be validated on a real Windows host
(that is what CI on `windows-latest` covers). Treat Linux Wine here as a fast
Windows-target lint/build/unit-test and control-plane sanity loop; native Linux
GUI/PTY smokes use the real `DISPLAY=:1` desktop or CI Xvfb instead.

Rhai REPL and `agenterm cli script repl` were removed with Phase C Wave 4.5 —
invoke `agenterm-rh` for `.rh` check, eval, task, and run. Instance discovery uses
`~/.local/share/agenterm/instances/` (override with `AGENTERM_INSTANCE_DIR`).
