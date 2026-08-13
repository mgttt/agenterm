# Computer-use foundation (`agenterm-cu`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module is the root of the `agenterm-cu` product subtree. It owns the
product definition, the boundary against every existing observation/control
surface, the governing invariants, and the promotion gates. Its four child
modules own third-level requirements.

`agenterm-cu` is newly opened scope. Design and implementation are in progress;
**every requirement below is `[ ]` planned.** Nothing in this subtree may be
marked `[x]` or `[~]` without the evidence its owning child module names.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Subtree map

`agenterm-cu` is organized as four child modules under this root, plus the
platform-accessibility backends that live under targets/transports. Platform
accessibility backends are an explicit branch under targets/transports — not a
footnote inside a table.

```text
agenterm-cu (28)
├── command surface (29)
├── targets / transports (30)
│   ├── current / ssh / rdp / vnc
│   └── platform a11y backends (agenterm-platform)
│       ├── Windows: native API + UIA
│       ├── macOS: AX (NSAccessibility)
│       └── Linux: AT-SPI2
├── authorization, safety and audit (31)
└── window placement (32)
```

Structured `tree` observation and `click` / `focus` by node identity are
provided by these native accessibility stacks (see
[30 § Platform accessibility backends](PRD_02_30_cu_targets_transports.md#platform-accessibility-backends)).
Screenshot and coordinate actuation are **degraded fallbacks** with typed
markers in the command result; they are never silent substitutes for a missing
control tree. `cu` consumes `agenterm-platform`; it does not open raw OS APIs or
fork a fifth screenshot stack.

## Subtree index

| # | 子模块 | 一句话 |
|---|--------|--------|
| 29 | [Command surface and layering](PRD_02_29_cu_command_surface.md) | 抽象命令集、洋葱分层契约、结构化控件树与确定性等待 |
| 30 | [Targets and transports](PRD_02_30_cu_targets_transports.md) | `current`/`ssh`/`rdp`/`vnc` 目标族、transport 抽象、**platform a11y backends**（Win UIA / macOS AX / Linux AT-SPI2） |
| 31 | [Authorization, safety and audit](PRD_02_31_cu_authorization_safety.md) | 高危能力面的授权模型、审计、拒绝语义与证据 |
| 32 | [Window placement](PRD_02_32_cu_window_placement.md) | 命名摆放动作（Spectacle 目录）：几何核 + `window-place`；v0.1.19 开工 |

## Product outcome

- [ ] `agenterm-cu` is AgenTerm's own computer-use foundation: one abstract
  command set for observing and controlling a machine — screenshot, window and
  control-tree enumeration, pointer, keyboard, clipboard, file transfer — that
  behaves identically whether the target is this machine or a remote one.
- [ ] it succeeds when an agent can drive a real desktop through one stable
  command surface, address controls by structured identity rather than guessed
  pixel coordinates, wait on observable state instead of sleeping, and have
  every action authorized and auditable.

## Why this product exists

- [ ] AgenTerm's north star is complete interface coverage: an agent must be
  able to control everything a human can and receive the same feedback. The
  terminal surface is largely covered; the machine outside the terminal is not.
  `agenterm-cu` closes that half.
- [ ] the differentiator is direction, not parity. General computer-use tools
  drive a screen through screenshot + OCR + coordinate guessing. AgenTerm
  already publishes exact structured bounds through `ui-snapshot`, so AgenTerm
  can be the first computer-use **target** with a real control tree, not only a
  computer-use client. Both directions belong to this subtree, and the target
  direction must not be dropped in favor of the easier client direction.

## Naming

- [ ] `agenterm-cu` is the accepted product name. It supersedes the
  `agenterm-remote.exe` working name used in
  [`plan/plan-v0.1.15.md`](../plan/plan-v0.1.15.md) §5.6.1. Remote protocol
  support is a transport axis inside this product, not a separate product.

## Product boundary

### Owned here

- The abstract command set and its layering contract ([29](PRD_02_29_cu_command_surface.md)).
- The target family and transport selection ([30](PRD_02_30_cu_targets_transports.md)).
- The authorization, audit and refusal model ([31](PRD_02_31_cu_authorization_safety.md)).
- Named window-placement actions and their geometry contract ([32](PRD_02_32_cu_window_placement.md)).

### Not owned here — must be consumed, not forked

This is the primary risk. AgenTerm already has four surfaces that take
screenshots or inject input. `agenterm-cu` must not become the fifth
independent implementation.

| 已有面 | owning 模块 | cu 的关系 |
|--------|-------------|-----------|
| OS 级 screenshot / window / input / process 机制 | [20 Native platform](PRD_02_20_native_platform.md) `agenterm-platform` | **消费**。cu 不得直调 OS API，新机制先沉入 platform 并带 typed `Unsupported`/`Failed` |
| OS 级 accessibility-tree 机制（观察 + 节点动作） | `crates/agenterm-abi` libagenterm `agt_a11y_*`（里程碑 6）→ `agenterm-platform` 适配器 | **消费**。Linux `current` 的 `tree` / 结构化 `click` / `focus` 经 ABI 机制层，不在 cu 内复刻 AT-SPI/UIA/AX |
| 工作台观察/控制、确定性等待、身份 | [07 Agent control plane](PRD_02_07_agent_control_plane.md) | **不重叠**。cu 不为 agenterm 自己的 tab/PTY 提供第二条控制平面 |
| `agenterm-cc` 的 screenshot/snapshot 投影 | [21 Control Center](PRD_02_21_control_center.md) | **不重叠**。CC 是产品投影，不是通用机器控制面 |
| `agenterm-con cli` 的输入/截图/等待 | [26 con control CLI](PRD_02_26_con_control_cli.md) | **不重叠**。con 是 GUI 生命期内的本进程终端控制 |
| 可选智能 / LLM 网关 | [12](PRD_02_12_specialized_intelligence.md) / [13](PRD_02_13_llm_gateway.md) | **独立**。cu 是工具面，不含模型、推理或提示策略 |

### Explicit non-goals

- [ ] no model, planner, prompt policy or agent loop. `agenterm-cu` provides
  capability, not judgment.
- [ ] no external computer-use framework, runtime or SDK is adopted into the
  product graph. Reference implementations may inform design; they are not
  dependencies. Provenance rules are owned by
  [14 Research provenance](PRD_02_14_research_provenance.md).
- [ ] no unrestricted-by-default authority. The unrestricted local runtime
  posture of [10 script engines](PRD_02_10_rhai_scripting.md) is explicitly
  **not** inherited; see [31](PRD_02_31_cu_authorization_safety.md).
- [ ] no silent capability substitution. An unavailable backend fails typed; it
  never degrades to coordinate guessing while reporting structured success.

## Governing invariants

- [ ] one command set across targets. `current` is the local degenerate tier of
  the same family as `ssh`/`rdp`/`vnc`, not a temporary special case. Adding a
  transport must not change the command surface above it.
- [ ] structured identity outranks pixels. Where a target can expose a control
  tree, commands address controls by structured identity with exact bounds;
  coordinate-only addressing is a declared fallback with an observable marker,
  never an invisible default.
- [ ] observation and action describe the same instant. A screenshot, a control
  tree and a subsequent action must be causally identifiable, so an agent can
  detect that the screen moved under it instead of acting on stale truth.
- [ ] every action is authorized before execution and observable after it. No
  action path exists that bypasses the authorization model or leaves no audit
  record.
- [ ] deterministic waits, never sleeps. Every state change an agent must
  observe is waitable with a bounded typed timeout.
- [ ] failures are typed and local. One target, session or backend failing must
  not corrupt another or abort the host.

## Promotion gates

- [ ] this subtree stays entirely `[ ]` at the root until the `current` tier
  proves the command set end to end on one platform with public black-box
  evidence. Individual child leaves may record `[~]` / `[x]` when their own
  evidence arrives; a partial platform slice does not promote the subtree root.
- [~] Linux `current` has first black-box evidence for AT-SPI2 `tree` and
  structured `click` / `focus` by node path (`scripts/cu-linux-smoke.sh` against
  the real `cu` binary). Windows UIA and macOS AX are not claimed in this slice.
- [ ] the subtree root still has no shipped version. Roadmap ownership is
  [18 Focused product roadmap](PRD_02_18_roadmap.md). **v0.1.19 starts**
  window placement ([32](PRD_02_32_cu_window_placement.md)) as a cu increment;
  that assignment does not promote this root or any other child.
- [ ] no capability may be marked shipped on design documents, reference
  assets, or a passing unit test alone. The evidence standard is the same as the
  rest of the tree: a public black-box journey against the real executable.
- [ ] when a child module's requirements outgrow it, it splits into a further
  module rather than accumulating a monolithic entry. This subtree exists
  precisely so that `agenterm-cu` never lands as one oversized bullet inside an
  unrelated module.

## Execution projection

Design and sequencing live in
[`plan/plan-v0.1.15.md`](../plan/plan-v0.1.15.md) §5.6 (mainline L-CU) and the
current-tier gap input
[`plan/agent-human-parity-audit.md`](../plan/agent-human-parity-audit.md).
Those are execution projections; accepted scope and status belong to this
subtree. Window-placement sequencing lives in
[`plan/plan-v0.1.19.md`](../plan/plan-v0.1.19.md); v0.1.18 remains the
in-progress unique version plan until it closes.
