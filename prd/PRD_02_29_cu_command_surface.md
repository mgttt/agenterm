# `agenterm-cu` command surface and layering

Parent: [Computer-use foundation (`agenterm-cu`)](PRD_02_28_agenterm_cu.md)

This module owns the abstract command set every target must honor, the layering
contract that keeps it from rotting, the structured control-tree observation
model, and the determinism rules. It does not own transports
([30](PRD_02_30_cu_targets_transports.md)) or authorization
([31](PRD_02_31_cu_authorization_safety.md)).

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Layering contract

- [ ] the surface is layered and **outer may depend on inner only**:

  ```text
  native primitive   平台机制（agenterm-platform 拥有，cu 不实现）
     ↑
  abstract command   目标无关命令集（本模块拥有）
     ↑
  target selector    选目标/transport（30 拥有）
     ↑
  workflow           组合动作、重试、等待编排
     ↑
  shell command      公开 CLI 入口
  ```

- [ ] a layer never reaches past its neighbor. A workflow may not call a native
  primitive; a shell command may not encode target-specific behavior. A
  violation is a structural defect, not a style preference.
- [ ] the abstract command set is target-agnostic by construction: a command
  whose semantics only make sense for one transport does not belong in it.

## Abstract command set

- [ ] the initial set covers observation and actuation:
  screenshot; window enumeration; control-tree enumeration; pointer
  press/release/move/click/drag; wheel; keyboard text and named keys; clipboard
  read/write; file transfer in both directions; **named window placement**
  (`window-place`, owned by [32](PRD_02_32_cu_window_placement.md)).
- [ ] every command carries an explicit target reference and returns a typed
  result. There is no ambient "current target" that a caller can forget to set.
- [ ] verb spellings converge with the existing AgenTerm surfaces where the
  action is the same (`screenshot`, `send-text`, `send-keys`, `send-wheel`,
  pointer verbs). A shared spelling must mean the same product action; where cu
  cannot honor an existing verb it omits it rather than shipping a weaker
  impostor. The workbench CLI contract is
  [15](PRD_02_15_command_line.md); the con contract is
  [26](PRD_02_26_con_control_cli.md).
- [ ] machine-readable output is the primary interface and the human rendering
  is derived from it, never the reverse.

## Structured observation

- [ ] a control-tree observation returns stable per-node identity, role, label,
  state and **exact bounds** — not a bitmap the caller must interpret.
- [ ] `tree` is sourced from the host's **platform accessibility backend**
  ([30 § Platform accessibility backends](PRD_02_30_cu_targets_transports.md#platform-accessibility-backends)):
  Windows native API + UIA, macOS AX (`NSAccessibility`), Linux AT-SPI2.
  `cu` does not implement these stacks; it consumes `agenterm-platform`.
- [ ] node identity is stable enough to be re-addressed across observations, or
  the instability is reported. An agent must never silently act on a node whose
  identity has been recycled.
- [ ] where a target cannot expose a control tree, the response says so
  explicitly with typed `Unsupported` / `Failed`. Coordinate-only or
  screenshot-only operation is always visible in the result, never inferred by
  the caller.
- [ ] structured `click` / `focus` by node id use the same platform a11y
  backend as `tree`. Coordinate `click` is a separate degraded path requiring
  an explicit marker; it never substitutes silently when structured actuation
  was requested.
- [~] structured `click` / `focus` also accept an accessible name
  (`--window` + `--name` + optional `--role`). Resolution reuses the
  `wait --node-name-contains` matcher (showing/visible, case-insensitive
  substring) and then acts on the existing node-path a11y path. A miss is
  typed `a11y_node_not_found`. Two or more showing matches are typed
  `a11y_node_ambiguous` (with the match count); the command must not pick
  the first. Name addressing must not parse tree dumps, take screenshots,
  or fall through to `--coords`.
- [~] `send-text` accepts the same name addressing (`--window` + `--name` +
  optional `--role`, with `--` ending flag parsing). It focuses the matched
  node through the node-path path, then types; without `--name` it stays the
  plain "type into whatever is focused" verb. A miss or an ambiguous name
  types nothing and fails typed, so a loop-until caller never sprays text
  at the wrong control.
- [~] `send-keys` accepts that same name addressing (`--window` + `--name` +
  optional `--role`, with `--` ending flag parsing). It focuses the matched
  node first, then sends the chord; without `--name` it stays the plain
  "send to whatever is focused" verb. A miss or an ambiguous name sends no
  chord at all.
- [ ] screenshot, control tree and action results are causally identifiable
  against the same observation instant, so a caller can detect that the target
  changed underneath a plan.
- [ ] AgenTerm's own surfaces are first-class observation targets. Making
  AgenTerm a computer-use target with a real control tree is owned here jointly
  with the surfaces that publish `ui-snapshot`; this module owns the cu-side
  contract, not the publishing surfaces themselves.

## Determinism

- [ ] every state transition a caller must observe is waitable with a bounded
  typed timeout. No documented workflow depends on a fixed sleep.
- [ ] a wait failure reports what was observed at the deadline, not only that
  the deadline passed.
- [ ] requests, responses, enumerations and transfers are size and time bounded.
  A pathological target cannot make the host allocate without limit or block
  indefinitely.

## Evidence

- [ ] the command set is proven by public black-box journeys against the real
  executable and a real target, waiting on state rather than sleeping, cleaning
  every process and file it owns.
- [ ] pure tests own command parsing, wire limits, identity/bounds
  normalization, degraded-mode selection and typed failure states.
- [ ] a layering test proves no outer layer links an inner-layer primitive
  directly, in the same spirit as the existing platform boundary gate.
