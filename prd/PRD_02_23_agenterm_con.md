# Lightweight terminal host (`agenterm-con`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module is the root of the `agenterm-con` product subtree. It owns the
product definition, boundary, invariants, cross-cutting evidence, and the safe
failure result. Its four child modules own third-level requirements, status, and
measured evidence; a requirement stated in a child is not restated here.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Subtree index

| # | 子模块 | 一句话 |
|---|--------|--------|
| 24 | [Terminal and rendering](PRD_02_24_con_terminal.md) | PTY、VT、行级 damage、present、字形、ISA、渲染性能 |
| 25 | [Workspace and input](PRD_02_25_con_workspace.md) | Tab 树、chrome、composer、滚动条、选择、剪贴板、焦点 |
| 26 | [Control protocol and public CLI](PRD_02_26_con_control_cli.md) | `agenterm-con cli`、`ATC1` 帧、JSON 契约、快照/截图证据 |
| 27 | [Package, budget and delivery](PRD_02_27_con_delivery.md) | 独立 package、`con-*` profile、严格小于 1 MiB、独立 CI、体积历史 |

## Product outcome

- [~] `agenterm-con` is the lightweight, green, GUI replacement for a system
  console host. It provides several independently owned local terminals without
  the workbench authority, background server, Fleet, mux, MCP, or script engines
  of `agenterm`. Its sustained-throughput qualification is shipped in
  [24](PRD_02_24_con_terminal.md), and the strict sub-1-MiB artifact budget in
  [27](PRD_02_27_con_delivery.md) is recovered. The product stays partial until
  the native Chinese-IME keyboard acceptance in [25](PRD_02_25_con_workspace.md)
  is complete.

The product succeeds when a user can launch one small executable, organize
terminals in a left-side tree, interact through a dedicated external input
area, and automate observation/input through a bounded public CLI while every
tab and the whole GUI remain stable under malformed output, native callback
failure, resize storms, process exit, and interaction races.

## Product boundary

### Shipped and owned here

- One GUI process containing multiple independent PTY sessions.
- Left tree tabs with stable IDs, parent-cycle rejection, active-tab selection,
  and child promotion when a parent closes.
- A terminal viewport with CJK double-cell layout, selection, clipboard,
  scrollbar, font zoom, IME, keyboard, mouse, wheel, resize, and DPI behavior.
- A persistent external input area whose editing keys remain local while it is
  focused and whose explicit send action writes to the active terminal.
- On Linux, a real AT-SPI child tree for that chrome (not only the X11
  window-title frame) so `agenterm-cu` can address inner controls by name.
- `agenterm-con cli` for tab listing/lifecycle, content capture, screenshots,
  text/paste/key/mouse/wheel input, deterministic waits, resize, performance
  evidence, and bounded shutdown.
- GUI-lifetime local control only. Closing the GUI ends its control endpoint.

### Explicit non-goals

- [x] no background server, Fleet authority, persistent workspace, remote mux,
  MCP, script runtime, task engine, plugin host, or Agent permission policy.
  The no-script boundary is enforced in the binary rather than by convention:
  `--script`, its JSON command decoder, command queue, wait scheduler, and
  script-only screenshot state are absent from the product graph, and automation
  uses the public control protocol exclusively.
- No claim of tmux/RMUX compatibility.
- No product navigation or workbench policy shared with `agenterm` merely to
  reduce code size.

## Governing invariants

- A child process exit retains its tab, final screen, and exit status until an
  explicit close.
- Closing a parent promotes direct children; it does not terminate them.
- One tab's PTY, parser, screenshot, malformed escape sequence, control request,
  panic, or resource exhaustion cannot corrupt another tab or abort the host.
- Native callbacks never unwind across FFI. Official artifacts use the `con-*`
  unwind profiles and contain callback panics as typed failures.
- Input focus has one owner. Composer editing never leaks Space, selection,
  copy, cut, paste, or navigation keys into the PTY.
- Pointer selection, dirty rasterization, native present, structured snapshot,
  and PNG evidence describe the same frame. A click cannot erase or scramble
  pixels outside its local feedback region.
- PTY output, control frames, waits, queues, dimensions, screenshots, and
  allocations are bounded and fail without blocking the GUI indefinitely.
- Product code consumes platform/UI-core contracts; raw OS APIs and ISA kernels
  remain in their owning shared mechanism layers.

## Observable success evidence

- Pure tests own tree safety, geometry, VT damage, Unicode width, selection,
  control parsing/wire limits, and safe failure states.
- A real one-shot child black-box journey proves `child_alive` transitions to
  false, its numeric exit code and final screen remain observable, and the GUI
  stays alive until an explicit close rather than treating child exit as a host
  exit.
- No single tab can monopolize the host. Concurrent-flood, oversized-CSI,
  select/capture-race and four-tab teardown journeys each prove an unaffected
  sibling stays interactive and controllable while one tab is saturated,
  closing, or being captured. The owning contracts are session teardown and
  scheduling in [24](PRD_02_24_con_terminal.md), and endpoint queues, pending-wait
  cancellation and screenshot ownership in
  [26](PRD_02_26_con_control_cli.md).
- Public black-box tests launch the real executable with isolated endpoints,
  use `agenterm-con cli`, wait on state rather than fixed sleeps, and clean all
  owned processes and files.
- Rendering journeys drive real window pixel coordinates, capture the native
  client area before and after input, and assert preservation of visible
  content as well as structured selection state.
- Composer focus isolation has its own physical journey, owned by
  [25](PRD_02_25_con_workspace.md).
- Windows x86_64 runs the matching custom-std `con-release-fast` Clippy, unit,
  public GUI black-box, panic-containment, and artifact build path.
- `{x86_64,aarch64} x {win,lnx,osx}` compile cells prove the product and selected
  platform adapters remain portable. Candidate remains the final sealed-byte
  qualification authority. Workflow ownership, the exact-SHA rule and the
  artifact budget belong to [27](PRD_02_27_con_delivery.md).

## Safe failure result

An invalid command receives a bounded error response; an unavailable tab or
native operation remains local; an allocation/present/PTY failure retains the
last valid state where possible and closes only the affected owner when
continuation is unsafe. No failure silently falls back to a server, script, mux,
or reduced permission model.

## Delivery ownership

Package identity, `con-*` unwind profiles, artifact budget, the independent
`.github/workflows/ci-agenterm-con.yml` feedback owner, and the exact-SHA
two-workflow Candidate precondition are owned by
[27](PRD_02_27_con_delivery.md). The cross-product release contract itself
remains owned by
[delivery and quality](PRD_02_17_delivery_quality.md).

## Shared improvement boundary

Stable, product-neutral lessons should move into `agenterm-platform`,
`agenterm-ui-core`, or shared frontend contracts only after both consumers and
their evidence agree. UI composition may differ. PTY lifecycle, VT/CJK rules,
font rasterization, retained frame mechanics, input normalization, clipboard,
IME, native window contracts, and bounded failure types should converge rather
than fork.

Shared kernels consumed by both products keep their requirement in the owning
shared module and are only referenced from this subtree:
[terminal runtime](PRD_02_01_terminal_runtime.md) owns physical VT selection
semantics, `RetainedXrgbFrame`, and the architecture pixel kernels;
[native platform](PRD_02_20_native_platform.md) owns the window, input, IME,
DPI, clipboard and font adapter contracts.
