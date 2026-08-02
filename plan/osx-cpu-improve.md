# macOS CPU usage investigation and improvement plan

Status: P0, P2, P3 shipped; P1 open
Date: 2026-08-02
Owner module: [`prd/PRD_02_01_terminal_runtime.md`](../prd/PRD_02_01_terminal_runtime.md)
(rendering performance) — this file is an execution projection, not product truth.

## Symptom

The macOS GUI (`agenterm`) shows 20–25% CPU during ordinary interactive use and
50–80%+ while a tab streams output (for example an attached byobu session with
an active agent window), on a MacBook Air M1 with a large (3360×1976 physical)
window.

## Measurements

All numbers from `release-fast` builds on macOS 26.5 / M1, using `sample(1)`
and the `AGENTERM_FRAME_LOG=1` diagnostic (added in commit `18a6cab`; prints
presented frames per 5 s window to stderr).

| Scenario | CPU | Present rate |
| --- | --- | --- |
| Fresh isolated instance, idle tab, 960×600 | 0.7% | ~0/s |
| Same instance, `while true; do date; done` in the tab | 54% | 19–26/s |
| User instance, 4K window, byobu + streaming agent output | 82% | not instrumented |

Conclusions from the numbers:

- The event loop itself is honest: idle cost is negligible. `Wait`/`WaitUntil`
  control flow, cursor-blink scheduling, and wake plumbing are not the problem.
- CPU scales with *presents per second × cost per present*. Streaming PTY
  output drives a redraw for essentially every drained output batch.
- Per-frame cost is the real defect: ~25 ms per present at 960×600 logical
  (≈54% / 22 fps), which extrapolates to 60–100 ms per present at the user's
  4K window — one core saturated below 20 fps.

## Where a frame goes (sampled call stacks)

Each present repaints and converts the *entire* frame, three full-buffer
passes over ~26 M pixels at 4K:

1. `render_terminal_grid_hidpi` → `render_terminal_grid` → `draw_cell` —
   the terminal region is repainted cell-by-cell at native (physical)
   resolution every frame. There is no dirty-row tracking; an output batch
   that touched two rows still repaints every cell. (Glyph rasters are
   cached in `font.rs`; the cost is the per-cell blit and styling, not
   rasterization.)
2. `scale_frame_nearest` — the logical frame (sidebar, toolbar, composer,
   status bar, and the already-obsolete logical terminal pixels) is upscaled
   to the physical framebuffer with a full-frame nearest-neighbour pass.
   The terminal region is therefore effectively rendered twice.
3. softbuffer present → CoreGraphics `convert_using_vImageConverter` under
   `CA::Transaction::commit` — macOS converts the whole XRGB buffer through
   vImage lookup tables on every commit (~19% of main-thread samples during
   load). Cost is proportional to full buffer size regardless of how little
   changed.

## Fix plan, ranked

- **P0 — Dirty-row repaint. [DONE]** Shipped: `TerminalGrid` tracks per-row
  damage during `sync_from_screen` (cell compare plus cursor rows); a
  persistent physical-resolution terminal layer repaints only dirty rows and
  is blitted per present; `scale_frame_nearest` skips the layer rectangle.
  Measured on the `date`-loop load at 960×600: 54% CPU @ 22 fps → 33% CPU @
  29 fps, with per-frame cost ~3× lower; larger windows gain more because the
  removed passes scaled with area. Rendered evidence (colors, CJK wide cells,
  scrollback up/down, streaming tail) verified ghost-free via `screenshot`.
  Original scope: The vt100 screen knows which rows changed
  between frames. Repaint only dirty rows plus the cursor cells into a
  persistent physical-resolution terminal layer; leave clean rows untouched.
  Streaming output typically touches the last few rows only, so this should
  remove 80–90% of terminal-region work. This also directly serves the
  dirty-frame regression debt already recorded in PRD_02_01.
  Acceptance: `AGENTERM_FRAME_LOG` unchanged rate but CPU under streaming
  load drops accordingly; `screenshot-pane` evidence shows no ghosting after
  scroll, resize, theme change, and alternate-screen switches.
- **P1 — Render once at physical resolution.** Draw chrome and terminal
  directly into the physical-resolution buffer and delete the
  `scale_frame_nearest` full-frame pass (and the double terminal render).
  Halves the remaining fixed cost per present.
- **P2 — Present pacing. [DONE]** Shipped: PTY-output-driven redraws are
  coalesced to ~30 presents/s (`request_output_redraw`); interactive paths
  still redraw immediately. Without this, cheap frames let the present rate
  balloon (measured 113 fps) and ate the P0 win. Original scope: Coalesce output-driven redraws to a ~30 fps cap
  (schedule via `WaitUntil` instead of immediate `request_redraw` when the
  last present is recent). Bounded win today because frames are currently
  CPU-bound below that rate, but it protects the win from P0/P1.
- **P3 — Present-path conversion. [DONE]** Shipped: vendored softbuffer
  (`third_party/softbuffer`, one-line change) tags frames with the display's
  color space, so Core Animation blits without the per-frame vImage pass.
  date-loop load at 960×600: 33% → 16%. Original scope: Investigate a CALayer-compatible pixel
  format/colorspace for the softbuffer surface so `CA::Transaction::commit`
  stops running vImage `AnyToAny` over the full buffer each present.

## Diagnostics kept in tree

- `AGENTERM_FRAME_LOG=1` — frames per 5 s window on stderr, zero cost when
  unset (`src/platform/adapters/unix/frontend/mod.rs`,
  `note_frame_for_diagnostics`).
- Reproduction load: run `while true; do date; done` in one tab; idle
  baseline: fresh isolated instance via `AGENTERM_WORKSPACE_PATH` +
  `AGENTERM_IPC_ENDPOINT` overrides.

## Related but separate

- Resize crash (SIGABRT during window resize) was traced to a vt100 wrap
  underflow on one-row grids and guarded separately (grid minimum 2 rows,
  window minimum inner size 320×240); it is not a performance item.
