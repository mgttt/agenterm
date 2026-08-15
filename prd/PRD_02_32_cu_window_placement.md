# `agenterm-cu` window placement

Parent: [Computer-use foundation (`agenterm-cu`)](PRD_02_28_agenterm_cu.md)

This module owns **named window-placement actions** on the computer-use
surface: given a window identity and an action id, compute a destination
rectangle and apply it through the platform accessibility backend. It does
not own transports ([30](PRD_02_30_cu_targets_transports.md)), grants
([31](PRD_02_31_cu_authorization_safety.md)), or the rest of the command set
([29](PRD_02_29_cu_command_surface.md)).

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Why this module exists

- [~] an agent that can click and type still needs to *arrange* a desktop.
  Human window managers (Spectacle, Rectangle, system tiling keys) are a
  distinct skill: move the focused — or a named — window into a predictable
  region without dragging.
- [~] the command surface owns the **action catalog and geometry contract**
  so orchestrators issue the same placements without pixels. The macOS
  daily-driver hotkey host is now `agenterm-cu host` / `AgentermCu.app` (menu bar +
  Spectacle-default shortcuts), not a continued dependency on Spectacle.app.
  Geometry still comes from this module; host TCC/install lessons live in
  `docs/agenterm-rust-cheatsheet.md` (macOS Accessibility trust).
- [~] implementation opened early against the v0.1.19 draft (macOS
  `window-place` + host). Linux/Windows apply and full black-box promotion
  remain open; checkbox rows below stay honest until public gates close.

Provenance (read-only, not a product dependency):
[mgttt/spectacle](https://github.com/mgttt/spectacle) fork of eczarny/Spectacle,
MIT. Catalog: `docs/FEATURE-CATALOG.md` in that repository. Action string
constants are copied as the public id space. JavaScript calculation files and
the ObjC specs are the executable specification; the Rust port is a
clean-room rewrite against those fixtures, not a link of the app.

## Subtree position

```text
agenterm-cu (28)
├── command surface (29)          ← registers the verb only
├── targets / transports (30)
├── authorization / audit (31)    ← actuate + JSONL
└── window placement (32)         ← this module: ids, geometry, apply pipeline
```

## Product outcome

- [~] `agenterm-cu window-place` applies one named action to one window on a `current`
  target (macOS apply path live; remote later) and returns the before/after
  rect plus the resolved action id.
- [~] it succeeds when an agent can tile, third-cycle, move-across-displays,
  and grow/shrink a real window by structured identity, wait on the resulting
  bounds, and see `refused` / `unsupported` / `failed` as distinct outcomes.
  Full catalog + multi-OS black-box promotion still open.

## Command contract

- [ ] verb: `window-place`.
- [ ] required: `--target`, `--grant actuate`, `--action <id>`.
- [ ] optional: `--window HANDLE`. Absent handle means the focused top-level
  window of the frontmost application (Spectacle's historical default).
- [ ] action ids are a closed enum. Two spellings, one meaning:

  | kebab (CLI) | stable constant |
  |-------------|-----------------|
  | `center` | `SpectacleWindowActionCenter` |
  | `fullscreen` | `SpectacleWindowActionFullscreen` |
  | `left-half` | `SpectacleWindowActionLeftHalf` |
  | `right-half` | `SpectacleWindowActionRightHalf` |
  | `top-half` | `SpectacleWindowActionTopHalf` |
  | `bottom-half` | `SpectacleWindowActionBottomHalf` |
  | `upper-left` | `SpectacleWindowActionUpperLeft` |
  | `lower-left` | `SpectacleWindowActionLowerLeft` |
  | `upper-right` | `SpectacleWindowActionUpperRight` |
  | `lower-right` | `SpectacleWindowActionLowerRight` |
  | `next-third` | `SpectacleWindowActionNextThird` |
  | `previous-third` | `SpectacleWindowActionPreviousThird` |
  | `next-display` | `SpectacleWindowActionNextDisplay` |
  | `previous-display` | `SpectacleWindowActionPreviousDisplay` |
  | `larger` | `SpectacleWindowActionLarger` |
  | `smaller` | `SpectacleWindowActionSmaller` |
  | `undo` | `SpectacleWindowActionUndo` |
  | `redo` | `SpectacleWindowActionRedo` |

- [ ] unknown action → typed `invalid_input`. No alias soup beyond the table.
- [x] machine-readable result includes `action`, `window`, `before`, `after`,
  `screen`, and whether quantized / clamped adjustment ran.
- [ ] `wait` already owned by [29](PRD_02_29_cu_command_surface.md) is the
  only legal way to observe completion. Workflows must not sleep.

## Geometry contract (must match Spectacle 1.2)

- [ ] input: window rect, source visible frame, destination visible frame
  (top-origin). Output: destination rect. Pure function; no AppKit in the
  core.
- [ ] half actions cycle `1/2 → 2/3 → 1/3` when the window's mid-line is
  within 1 pt of the candidate; otherwise they snap to `1/2`.
- [ ] corner actions cycle width thirds inside that quadrant the same way.
- [ ] `next-third` / `previous-third` walk horizontal thirds then vertical
  thirds.
- [ ] `center` does not resize. `fullscreen` uses `visibleFrame`, never a
  macOS Space fullscreen.
- [ ] `larger` / `smaller` grow or shrink while keeping edges that already
  touch the visible frame attached.
- [ ] results use rounding that does not leave a 1–2 px gap under the menu
  bar (Spectacle #700).
- [ ] apply pipeline, in order: write size/position/size → quantized shrink
  by 2 pt down to 85% then center in the target → clamp to visible frame.
- [ ] sheet / system-dialog roles refuse with `failed`.
- [ ] application min/max size is honored; the pipeline may undershoot the
  ideal rect but must not report success with a fabricated frame.

The typed preflight is now wired through `agenterm-platform`, libagenterm ABI
1.10 (`agt_window_placement_query`), and CU before the first native write.
Known standard/dialog windows require explicit move/resize support; sheet,
system-dialog, other, and unknown roles fail closed. Explicit min/max/increment
constraints normalize the requested size, application-enforced constraints
publish only the final independent readback, and unknown constraints refuse
resizing. Windows has live UIA + bounded `WM_GETMINMAXINFO` evidence; macOS AX
and Linux X11 adapters have compile/unit evidence, but their real-session
placement black boxes remain open (Linux role intentionally stays unknown
until a trustworthy XID-to-AT-SPI join exists).

`undo` / `redo` now have a bounded per-application history first cut (40
entries, redo truncation, corruption detection and same-directory replacement).
Its deterministic persistence tests are closed, but a public real-window
undo/redo journey remains open. The first compensation saga now revalidates
window identity/state, records only final native readback, rolls back a failed
unpublished history commit only from a known owned rect, and reports structured
`window_place_in_doubt` when it cannot verify ownership or recovery. Process
crash recovery and concurrent-writer CAS remain open; this leaf is still partial.

## Layering

- [ ] `spectacle-core` (name of the geometry crate or module) is pure Rust
  and has no OS imports. Fixture tests are the promotion evidence for the
  math.
- [ ] applying a rect is an `agenterm-platform` mechanism (AX / UIA /
  `_NET_WM` as each backend grows). `agenterm-cu` must not call AX directly.
- [ ] macOS is the first apply backend because that is where the catalog was
  proven. Linux/Windows placement is the same verb on a later backend; the
  command set does not fork.

## Authorization

- [ ] `window-place` is actuation. `observe` is not enough.
- [ ] [31](PRD_02_31_cu_authorization_safety.md) applies in full: no grant →
  `refused`; audit write failure → do not apply; no coordinate fallback.

## Explicit non-goals

- [x] macOS daily-driver host is `agenterm-cu host` (menu-bar extra + Spectacle-default
  global shortcuts). Accessibility is checked only when the menu opens; the
  first item shows status and opens Settings. No popup, no background TCC
  poll, no `Shortcuts.json` editor. Host trust is the launchd process’s
  `AXIsProcessTrusted` against the **current** code signature — not the
  Settings label alone, and not a Terminal-spawned CLI place. Install story
  and evidence: `scripts/install-cu-hotkeys.sh`,
  `docs/agenterm-rust-cheatsheet.md` (macOS Accessibility trust).
- [~] Windows daily-driver host uses the same `agenterm-cu host` mode and
  placement action catalog, presented as a notification-area menu plus global
  shortcuts. It calls the same `Command` / `Executor` path as CLI and macOS;
  Win32 window, input, screenshot, and accessibility mechanisms remain behind
  `libagenterm.dll`. The host owns only lifecycle, menu projection, shortcut
  registration, and dispatch, and must return typed unsupported capability
  status rather than adding product-local native fallbacks.
- [ ] no drag-to-snap, no tile occupancy grid, no batch layout of
  non-addressed windows.
- [ ] no Rectangle-only features (gaps, almost-maximize, custom regions)
  unless they later earn their own ids.
- [ ] no embedding of Spectacle.app, Sparkle, Carthage, or JavaScriptCore.
- [ ] no screenshot/OCR placement.

## Version gate

- [ ] **v0.1.19 starts this module.** Suggested first increment (must-start,
  not must-finish-the-catalog):
  1. freeze ids (this file + Spectacle `FEATURE-CATALOG`);
  2. port geometry for `center` / `fullscreen` / four halves with fixture
     parity;
  3. `agenterm-cu window-place` on `current` + macOS AX set-rect through platform;
  4. grant/audit black-box.
- [ ] later increments on the same module: thirds, corners, display walk,
  larger/smaller, then undo/redo.
- [ ] roadmap ownership:
  [18](PRD_02_18_roadmap.md); execution projection:
  [`plan/plan-v0.1.19.md`](../plan/plan-v0.1.19.md). v0.1.18 remains the
  in-progress unique version plan until it closes. This module must not be
  marked shipped from design text alone.

## Evidence

- [x] pure tests: all 16 stateless frozen actions have exact deterministic
  fixtures, including complete half/corner/third/display cycles; undo/redo have
  separate bounded history, persistence and compensation-saga tests.
- [ ] black-box: real `agenterm-cu` on a real macOS session places a visible window
  and a subsequent `windows` / `wait` observation shows the new bounds.
- [~] unauthorized call is `refused` and does not move the window. The staged
  Windows x86_64 public smoke proves this against its owned fixture; macOS and
  Linux evidence remain open.

## Windows desktop-host checkpoint

- [x] `libagenterm` ABI 1.7 exposes the cross-platform desktop-host contract;
  Windows implements notification-area icon/menu projection, `RegisterHotKey`,
  event polling and deterministic close cleanup.
- [x] CU projects one product-owned catalog containing 18 placement actions and
  Quit. The platform/ABI layer transports numeric actions but does not assign
  their product meaning.
- [x] Native `target/abi-dev` `agenterm-cu host --self-test --json` evidence
  reports `actions=19`, `shared_executor=true`, a refused side-effect-free
  `window-place` dispatch, and `cleaned_up=true`. The same
  `host_actions::execute` function owns Windows menu/global-shortcut and macOS
  callback dispatch, so host modes cannot bypass command authorization/audit.
- [x] The staged `dist/agenterm-cu.exe` beside its exact `dist/agenterm.dll`
  passes `cu-windows-smoke`: version probe, dynamic load, 19 actions,
  observe-only placement refusal with unchanged bounds, authorized
  `left-half` placement through ABI 1.10 role/support/constraint preflight with
  destination-screen reply and independent bounds readback, isolated JSONL
  attempt/outcome audit, and deterministic host cleanup.
- [ ] Candidate qualification and Windows ARM64 evidence remain open. Until
  those gates close, Windows host and this subtree remain partial.
