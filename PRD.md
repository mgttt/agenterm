# AgenTerm product tree

Status: active development
Platform: Windows shipped; Linux/macOS GUI and POSIX PTY in active delivery
  (see [`plan/plan-multiplatform-gui.md`](plan/plan-multiplatform-gui.md)).
Current default shell: Windows uses the real system `cmd.exe`; Unix GUI uses
`$SHELL` with `/bin/sh` fallback.
Future default-shell candidate: `agenterm-bash.exe`, only after its
clean-machine gate passes; no release version is committed

AgenTerm is a native terminal and local fleet workspace for people and
AI agents. The Windows GUI is the shipped human surface; Linux and macOS
share the same protocol, clients, and terminal kernel while their GUI
surface is delivered through continuous PTY/render abstraction. Its window is the bridge, the tab tree organizes the fleet, shells
are crew workspaces, and the local control plane lets people and agents observe
and steer the same state. Scripting reuses that public contract rather than
bypassing it. Human interaction and local CLI automation operate on the same
tabs, PTYs, drafts, settings, and observable state. A process exiting never
silently destroys its tab.

The visual language favors industrial confidence over decoration: repeated
integer-grid spacing, solid right-angle connections, strict baseline
alignment, restrained colors, and explicit boundaries should make the fleet
feel precisely assembled and dependable.

AgenTerm competes by keeping the visible product simple and practical while
making the underlying system stable, observable, programmable, and open-ended.
New UI is justified by lower interaction cost, not feature count: advanced
power should prefer discoverable commands and programming interfaces, and
secondary controls should stay contextual or hidden by default when that keeps
the daily workspace quiet.

Terminal durability comes from deterministic two-dimensional state, not from
nostalgia. AgenTerm extends that contract from a character grid to the whole
agent fleet: humans and agents must be able to address, read, wait for, and
control the same tree nodes, focus, input, viewport, process lifecycle, and
rendered evidence precisely.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Documentation contract

This file is the canonical product entry point, product constitution, and
second-level tree. Linked `prd/PRD_*.md` modules own third-level requirements,
decisions, status, and acceptance detail. A requirement has exactly one owning
module; other documents link to it instead of copying it.

Public version plans live under [`plan/`](plan/). They record sequencing,
dependencies, risks, decisions, and delivery history, but remain execution
projections rather than canonical product truth. Every accepted product scope
or capability-status change also belongs in its owning PRD module.

Stable public technical contracts that are too detailed for an owning PRD
module may live under [`docs/`](docs/). Every such specification links back to
exactly one owning PRD module; it defines interface semantics and conformance,
not independent product scope or shipped status.

Machine-readable shipped capability/evidence alignment lives in
[`prd/alignment-contract.json`](prd/alignment-contract.json).

Later executable names, intelligence approaches, and external-runtime
integrations are gated product or research hypotheses unless a roadmap
milestone explicitly assigns them to a release. Their presence in the product
tree does not promise a version or implementation strategy.

## Product tree

- AgenTerm
  - [Terminal runtime](prd/PRD_02_01_terminal_runtime.md) — ConPTY, rendering, input, selection, scrollback, and terminal performance.
  - [Executable family](prd/PRD_02_02_executable_family.md) — Binary roles, boundaries, budgets, and sidecar ownership.
  - [Default shell (`agenterm-bash.exe`)](prd/PRD_02_03_default_shell.md) — Real Bash runtime strategy and compatibility gate.
  - [Optional component lifecycle (`agenterm-softmgr.exe`)](prd/PRD_02_04_optional_components.md) — Signed inventory, installation, update, rollback, and supply-chain safety.
  - [Fleet multiplexer (`agenterm-mux.exe`)](prd/PRD_02_05_fleet_multiplexer.md) — tmux/RMUX-compatible control over the shared AgenTerm authority.
  - [Human workspace](prd/PRD_02_06_human_workspace.md) — Tabs, composer, settings, persistence, status bar, and interaction design.
  - [Agent control plane](prd/PRD_02_07_agent_control_plane.md) — Observation, control, protocol, identity, and deterministic waits.
  - [Observable Fleet event core](prd/PRD_02_08_observable_fleet.md) — Epoch/sequence journal, reads, waits, gaps, restart, and consumers.
  - [Self-hosted development loop](prd/PRD_02_09_self_hosted_development.md) — Building, staging, update visibility, and safe developer iteration.
  - [Rust host + Rhai scripting](prd/PRD_02_10_rhai_scripting.md) — Unrestricted local runtime, supervisor, registry, control, audit, and providers; Agent permissions live above it.
  - [MCP and agentic orchestration (`agenterm-mcp.exe`)](prd/PRD_02_11_mcp_orchestration.md) — Read-only MCP first, then governed tools, flows, and scheduling.
  - [Lightweight specialized intelligence (`agenterm-ai.exe`)](prd/PRD_02_12_specialized_intelligence.md) — Evidence gates for an unassigned optional-intelligence research direction.
  - [Local LLM gateway (`agenterm-llm-gateway.exe`)](prd/PRD_02_13_llm_gateway.md) — Safety gates for an unassigned governed-gateway hypothesis.
  - [Research provenance and clean-room boundary](prd/PRD_02_14_research_provenance.md) — Source review, licensing, provenance, and independent implementation.
  - [Command line (`agenterm-cli.exe`)](prd/PRD_02_15_command_line.md) — Public commands, discovery, output contracts, and lifecycle semantics.
  - [tmux/RMUX compatibility](prd/PRD_02_16_tmux_rmux_compatibility.md) — Compatibility matrix, explicit differences, and conformance evidence.
  - [Delivery and quality](prd/PRD_02_17_delivery_quality.md) — Builds, tests, artifacts, release gates, and regression budgets.
  - [Focused product roadmap](prd/PRD_02_18_roadmap.md) — Version ownership, milestone gates, and future product lanes.
  - [Inspiration backlog and future vision](prd/PRD_02_19_inspiration_and_future_vision.md) — Idea garden, north-star layers, and promotion path into owning modules (not shipped status).
  - [Native platform abstraction](prd/PRD_02_20_native_platform.md) — Shared native-window, input, IME, DPI, clipboard, font, and OS-integration contracts with Windows, macOS, and Linux adapters.
  - [Control Center (`agenterm-cc`)](prd/PRD_02_21_control_center.md) — Independent secondary workspace for Fleet cockpit, workflow, extension, application, and information projections.
  - [Decentralized network (`agenterm-net`)](prd/PRD_02_22_decentralized_network.md) — Independently matured libp2p identity, IPFS content-addressing, storage, transport, and service-integration contracts.

## Non-negotiable invariants

- Exiting a child process does not remove its tab.
- Normal application restart preserves workspace structure and metadata while
  honestly restarting each PTY process.
- A live tab is not destroyed without an explicit close and confirmation.
- Tab IDs remain stable for the lifetime of the tab; indexes may change.
- Agent-facing state is machine-readable and actions can be verified without
  arbitrary sleeps.
- `agenterm-rhai` exposes one unrestricted local runtime with the invoking
  user's operating-system authority. Rhai has no permission tier, approval
  profile, protected-path/process/endpoint list, or restricted substitute for
  an unshipped API; Agent policy belongs to the separate Agent/harness layer.
- tmux/RMUX names are used only where behavior is compatible. Unsupported
  behavior returns an error rather than pretending to succeed.
- AgenTerm does not silently download or bundle fonts. `Sarasa Fixed SC`
  (SIL OFL 1.1) is the recommended optional CJK monospace font.

## Current acceptance gate

Run `.\lint.cmd` for fast local feedback and `.\check.cmd` for ordinary
changes. A change is ready only when repository static lint, formatting,
Clippy with warnings denied, production Rhai checks, unit tests, `dist/`
artifact generation, CLI smoke, and semantic UX smoke all pass. Rendering
changes additionally require
`screenshot` or `screenshot-pane` inspection.

An unpublished v0.1.12 release candidate uses
`.\check.cmd --release --include-stress` on a clean commit and must emit one
complete qualification receipt bound to the exact candidate bytes. The
independent `agenterm-rhai task run package-qualified` step may only copy
those byte-identical qualified artifacts; it does not rebuild. A non-publishing
release rehearsal must validate the candidate, receipt, package manifest, and
remote workflow contract before publication is considered. Creating or
pushing a release tag, or creating a public GitHub Release, still requires the
user's explicit approval. The deleted v0.1.9 GitHub Release and its retained
immutable tag must not be overwritten; the historical server-loss hotfix uses SemVer
`0.1.9+hotfix.1` and tag `v0.1.9+hotfix.1`, with public release title
“AgenTerm v0.1.9.1 Hotfix”. Versions 0.1.7 and 0.1.8 remain historical
baselines; v0.1.7 is internal-only and must never produce a tag or public
GitHub Release.
