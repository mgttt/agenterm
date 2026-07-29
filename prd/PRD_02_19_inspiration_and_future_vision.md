# Inspiration backlog and future vision

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Chinese title (informal): 灵感采集与未来畅想

This module is the **living idea garden**. It captures product origin,
inspiration, and long-horizon hypotheses before they earn a version gate or an
owning PRD module. Entries here are **not** shipped status, **not**
implementation scope, and **not** release promises until promoted through the
workflow below.

Canonical shipped/partial/planned truth remains in owning `prd/PRD_*.md`
modules, [`PRD_02_18_roadmap.md`](PRD_02_18_roadmap.md), and [`plan/`](../plan/).

## How to use this document

1. **Capture** — add a short idea card under the right lane (template below).
2. **Explore** — link research, sketches, or spikes; keep scope hypothetical.
3. **Promote** — when an idea has a concrete user case, invariant, and
   acceptance evidence, move requirements into exactly one owning PRD module
   and optionally a `plan/plan-v*.md` execution plan.
4. **Archive** — mark rejected or superseded cards; do not delete history.

Legend for idea cards:

| Mark | Meaning |
|------|---------|
| `[idea]` | captured inspiration only |
| `[explore]` | active research or spike |
| `[promoted]` | requirements moved to an owning module; card kept as trace |
| `[deferred]` | valid but explicitly not scheduled |
| `[rejected]` | explored and cut; reason recorded |

## Promotion workflow

```text
Inspiration card (this file)
    → owning PRD module (one canonical owner)
    → plan/plan-*.md (optional version execution)
    → PRD_02_18_roadmap milestone (when version-gated)
    → alignment-contract.json (when shipped evidence exists)
```

Do not duplicate normative requirements here after promotion; link to the owner.

## Platform layers (north star)

Long-term product shape discussed with the product owner. Layers build on the
same fleet contract (tree, Composer, server authority, typed control plane,
Observable Fleet) rather than replacing it.

```text
L0 Fleet kernel     — tree, Composer, server/GUI split, CLI/script/MCP, mux
L1 Orchestration    — workflows, cross-agent coordination, subscriptions/waits
L2 Extensions       — signed packages, plugin market, optional sidecars
L3 Intelligence feeds — news, supply/demand, vertical data (mostly third-party)
L4 Mobile connector — phone as remote client, not a second terminal product
```

## Product origin (why AgenTerm exists)

Captured from product-owner intent; anchors prioritization when evaluating new ideas.

- [idea] Existing terminals (Tabby, ConEmu, Warp, OS-native, and similar) did
  not satisfy:
  - large **team-tree** management for terminals plus long-lived processes and
    agents;
  - an **external Composer** so typing does not fight live message streams and
    long edits stay practical;
  - **lightweight portable** distribution, high stability, fault tolerance, and
    a small memory footprint;
  - **scriptability** and **session interoperability** through a bounded
    tmux/RMUX surface;
  - honest lifecycle semantics (process exit does not erase context; close is
    explicit).
- [promoted] Core responses now live in [Human workspace](PRD_02_06_human_workspace.md),
  [Agent control plane](PRD_02_07_agent_control_plane.md),
  [Fleet multiplexer](PRD_02_05_fleet_multiplexer.md),
  [Delivery and quality](PRD_02_17_delivery_quality.md), and PRD non-negotiable
  invariants.

One-sentence north star:

> Local agent/process **fleet workspace** — tree for organization, terminal as
> viewport, Composer/CLI as control plane; lightweight, durable, verifiable.

## Idea lanes

### Lane A — Fleet workspace and daily UX

| ID | Status | Idea | Depends on | Owning module when promoted |
|----|--------|------|------------|------------------------------|
| A1 | [promoted] | Hierarchical team tree with remain-on-exit and promote-children-on-parent-close | — | Human workspace |
| A2 | [promoted] | Per-tab external Composer (multiline draft, Send) | — | Human workspace |
| A3 | [promoted] | Detach-first window close; server survives hidden GUI | — | Human workspace, Executable family |
| A4 | [idea] | Drag/drop tree reparenting and team-level bulk actions | large-tree UX evidence | Human workspace |
| A5 | [idea] | Scale evidence for 50+ tab trees (scroll, search, focus) | A4 optional | Human workspace |
| A6 | [deferred] | Global/default proxy workbench in GUI | explicit non-goal in v0.1.6+ | Human workspace |

### Lane B — Control plane, automation, and interoperability

| ID | Status | Idea | Depends on | Owning module when promoted |
|----|--------|------|------------|------------------------------|
| B1 | [promoted] | Typed loopback IPC, stable tab IDs, snapshots, waits | — | Agent control plane |
| B2 | [promoted] | Bounded tmux/RMUX subset via `agenterm-mux.exe` | — | tmux/RMUX compatibility, Fleet multiplexer |
| B3 | [explore] | Remote/network transport for non-loopback clients | auth, subscription | Agent control plane, Observable Fleet |
| B4 | [explore] | Stable event subscriptions for push and automation | Observable Fleet minimum | Observable Fleet, MCP orchestration |
| B5 | [idea] | Cross-tab broadcast input and synchronized panes | typed op completeness | Agent control plane |
| B6 | [promoted] | Rhai script runtime and task catalog | — | Rust host + Rhai scripting |
| B7 | [explore] | MCP read-only bridge then governed tools | v0.1.10 gates | MCP orchestration |

### Lane C — Orchestration and multi-agent collaboration

| ID | Status | Idea | Depends on | Owning module when promoted |
|----|--------|------|------------|------------------------------|
| C1 | [explore] | Persisted **workflow/pipeline** graph (steps, waits, branches, retry, cancel) | receipt, journal, typed mutations | MCP orchestration (brain/flow) |
| C2 | [idea] | Cross-agent **team coordination** inside one fleet (delegate task, shared templates, handoff) | C1 partial, stable IDs | Agent control plane, MCP orchestration |
| C3 | [idea] | Workflow recovery from snapshot + journal without assuming process continuity | Observable Fleet | Observable Fleet, MCP orchestration |
| C4 | [deferred] | Federation across machines/users (not chat-first) | B3, security model | Agent control plane |

Non-goals for this lane:

- no Slack/Discord-style general messaging product;
- no natural-language success signal without verifiable post-state;
- no autonomous destructive actions without confirmation and policy.

### Lane D — Extensions and marketplace

| ID | Status | Idea | Depends on | Owning module when promoted |
|----|--------|------|------------|------------------------------|
| D1 | [explore] | Signed optional components as `agenterm-{role}.exe` sidecars | package contract | Optional component lifecycle |
| D2 | [idea] | **Plugin / package market** as discovery + transaction over softmgr | D1, supply-chain gates | Optional component lifecycle |
| D3 | [idea] | GUI never downloads at startup; manifest-only awareness | D1 | Optional component lifecycle, Executable family |
| D4 | [deferred] | Public registry with remote resolution and signing policy | D1, D2 | Optional component lifecycle, Rhai scripting |

### Lane E — Intelligence feeds and on-device assist

| ID | Status | Idea | Depends on | Owning module when promoted |
|----|--------|------|------------|------------------------------|
| E1 | [idea] | **LLM / AI news subscription** — ingest feeds, filter, surface in fleet | HTTP sidecar, notification predicates | New feed connector PRD or Rhai scripting |
| E2 | [idea] | **Supply/demand information services** — same pipeline as E1, different sources | E1 framework | same as E1 |
| E3 | [explore] | On-device **small models** for summarize, triage, suggest Composer text | evidence gates | Specialized intelligence |
| E4 | [deferred] | Governed **LLM gateway** (routing, quota, audit, redaction) | scripting, MCP, event core | LLM gateway |
| E5 | [deferred] | Upload full pane/scrollback to cloud by default | — | **rejected** privacy boundary |

Feeds non-goals:

- AgenTerm is not a media reader app; it **routes signals into actionable fleet context**;
- no auto-execution of trades or commitments from feed content without explicit user confirm.

### Lane F — Mobile connector

Phone as **desktop fleet remote client**, not a standalone mobile terminal.

| ID | Status | Idea | Depends on | Owning module when promoted |
|----|--------|------|------------|------------------------------|
| F1 | [idea] | Mobile app connects to desktop `agenterm-server` over authenticated channel | B3, B4 | Agent control plane (+ future Mobile module) |
| F2 | [idea] | Monitor fleet tree, tab status, bounded output summaries | F1 | Human workspace, Agent control plane |
| F3 | [idea] | Mobile Composer + keyboard; voice-to-text into draft before Send | F1 | Human workspace |
| F4 | [idea] | **Push notifications** for urgent fleet events (dead, wait timeout, keyword, modal) | B4, predicates | Observable Fleet |
| F5 | [idea] | On-phone small model assists monitoring (triage, summarize) without becoming authority | E3, F1 | Specialized intelligence |
| F6 | [deferred] | Full mobile PTY fleet | — | **rejected** — contradicts connector positioning |

Security notes (must be designed before F1 ships):

- pairing, device binding, operation tiers (observe / composer / destructive);
- LAN-first option; remote requires explicit opt-in;
- push payloads stay redacted; deep-link to stable tab `@id`.

### Lane G — Platform and distribution

| ID | Status | Idea | Depends on | Owning module when promoted |
|----|--------|------|------------|------------------------------|
| G1 | [explore] | Multi-platform GUI (Linux/macOS) on shared kernel | — | Executable family, plan-multiplatform-gui |
| G2 | [idea] | Portable no-install distribution as default; installer later | — | Delivery and quality |
| G3 | [promoted] | Strict binary size budgets (4 MiB GUI, 2 MiB sidecars) | — | Delivery and quality, Executable family |
| G4 | [deferred] | Explorer shell replacement / `agenterm-desktop.exe` | high-risk gate | Optional component lifecycle, roadmap |

## Idea card template (copy for new entries)

```markdown
### IDEA-YYYY-MM-DD-short-name

- Status: [idea]
- Lane: (A–G)
- Problem: (user pain in one sentence)
- Sketch: (what it might look like)
- Depends on: (capabilities or gates)
- Non-goals: (what we will not do)
- Promotion target: (owning PRD module)
- Notes: (links, spikes, conversations)
```

## Open inbox

Add uncategorized sparks here; sort into lanes during review.

- [idea] **IDEA-2026-07-29-founder-origin** — capture ongoing "and many more" items
  from product-owner sessions into lane tables or new IDEA cards; this file is
  the pressure-relief valve so the focused roadmap stays readable.

---

Last reviewed: 2026-07-29
