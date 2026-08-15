# Terminal runtime

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] Win32/GDI window without GPU or OpenGL requirements
- [~] Linux/macOS GUI window without GPU requirements via `winit` +
  `softbuffer` software raster (shared theme/geometry/selection/vt100);
  Linux/macOS share `unix_app`: live POSIX PTY tabs, terminal workbench toolbar,
  composer, settings, wheel/scrollbar, paste, and word/row/drag selection
  with edge autoscroll; status-bar CWD editor, window-close confirm, and tabs
  resize grip on Unix; proxy editor and professional selection remain later
- [x] one ConPTY-backed process per tab on Windows; the platform adapter directly
  owns ConPTY pipes, process/job lifecycle, resize, wait and native console input
- [x] shared PTY backend facade: Windows keeps those implementation details behind
  one adapter; Unix uses POSIX `openpty` + fork/exec; `terminal_runtime` consumes
  one API
- [x] VT100 parsing, ANSI colors, scrollback, resize, keyboard and mouse
- [x] Backspace emits ConPTY VT `DEL` and deletes exactly one input
  character in the default `cmd.exe` line editor
- [x] mouse wheel navigates ordinary terminal history and raw full-screen applications;
  a visible draggable scrollbar navigates ordinary history, track clicks page,
  and dragging to the bottom restores the
  live viewport. Live v0.1.12 dogfood found alternate-screen harnesses whose
  zero local scrollback makes wheel/PageUp ineffective. Byte-level diagnosis on
  pre-passthrough ConPTY proved that `1049h/l` is erased and replaced by an
  indistinguishable full-frame repaint, so `alternate_screen=false` is not an
  authoritative normal-screen fact and repaint/max-scrollback heuristics are
  forbidden. The Windows PTY facade now queries typed child input ownership:
  cooked line input consumes no wheel key, while RawVt/RawNative receives native
  logical Up/Down records and ConHost itself selects CSI/SS3 from its retained
  cursor mode. Linux/macOS keep parser-owned alternate-grid byte input. The
  owning Windows journey retains ordinary scrollbar/wheel evidence and uses a
  real raw full-screen PowerShell PTY; native up/down wheel messages each arrive
  as three complete `ESC O A` / `ESC O B` sequences. The integrated 169.9-second
  journey passed selection, recovery and orphan cleanup as well. Future
  application raw-mouse reporting and Shift local-selection override remain a
  separate professional-input slice rather than weakening this shipped paging
  contract.
- [x] basic Windows visible-cell dragging selects terminal text and a completed
  non-empty selection owns Ctrl+C Copy. Prepared/dragging/completed state,
  exact native capture ownership and paint-owned highlight pixel bounds are
  projected together in `ui-snapshot`; paint consumes the same bounds. The
  owning journey advances the PTY generation after pointer down, observes
  prepared/dragging capture, proves a same-event-position PNG change, releases
  capture on completion, and verifies direct Ctrl+C updates the clipboard
  without adding an ETX byte to the PTY. System-menu Copy remains equivalent.
  Capture acquire/release/query failures clear copyability and surface a typed
  error instead of pretending selection completed. A click that never drags
  remains non-copying and available to existing terminal/RMUX behavior.
- [x] window-icon system menu exposes focus-aware Copy and Paste: native
  edit controls receive their standard messages, while terminal Copy uses
  the active cell selection and terminal Paste uses the active PTY
- [x] physical VT selection semantics are shared by the workbench and
  `agenterm-con` through `agenterm-ui-core`: endpoint normalization, bounded
  visible-row selection, Unicode/path word classes, wide-cell continuation
  handling, Windows CRLF joins, and trailing-space trimming have one kernel.
  Gesture phase, native capture, auto-copy, tab authority, and remote snapshot
  adaptation remain product-owned. Triple-click selects one visible row rather
  than silently crossing soft-wrap boundaries. Shared-kernel, workbench, and
  con tests cover forward/reverse CJK extraction and the multi-click contract.
- v0.1.8 professional-selection slice (P0), informed by the reviewed PuTTY
  terminal model
  - [ ] professional selection extends the shipped basic state machine with
    every tab/modal/shutdown/capture-loss cancellation surface and public
    physical evidence; a click that never becomes a drag retains its existing
    terminal/RMUX click behavior
  - [ ] while an owned drag remains above or below the terminal viewport, the
    GUI-owned timer scrolls at a bounded rate, clamps every endpoint to a valid
    terminal cell, and stops immediately on completion or cancellation
  - [ ] capture loss, tab change, modal opening, terminal replacement, and
    window or server close cancel an unfinished gesture without leaving mouse
    capture, timer activity, input ownership, or suspended rendering behind
  - [ ] double-click selects a Unicode-aware terminal-cell word with an
    explicit punctuation table; triple-click selects one visible terminal row,
    not a logical line joined across automatic wrapping
  - [ ] forward and reverse endpoints, wrapped and multiline text, CRLF copy,
    CJK double-width cells, and wide-cell continuations normalize to the same
    bounded cell selection and clipboard result
  - [ ] physical-input public tests cover drag-outside auto-scroll,
    double-click, triple-click, capture loss, tab change, forward/reverse CJK
    selection, and concurrent PTY output; pure tests own endpoint, word,
    visual-row, continuation-cell, boundary-clamp, and timer progression
  - [ ] input, resize, ANSI, CJK, wide-character, scaling, minimize/restore,
    scrollbar, and long-output qualification proves selection, cell dump,
    bounded capture, `ui-snapshot`, and PNG describe the same visible cells
    and that PTY output continues while selection and auto-scroll are active
- Professional-selection non-goals for v0.1.8
  - [ ] application-requested raw mouse arbitration and its documented Shift
    local-selection override remain a later independently accepted slice
  - [ ] rectangular selection remains later work; v0.1.8 does not infer it
    from word, visual-row, or drag selection
- [x] terminal paste reads bounded Unicode clipboard text off the GUI thread,
  normalizes newlines, filters unsafe controls, and honors bracketed-paste mode.
  On Windows, a human Ctrl+V or system-menu paste opens an owner-modal multiline
  review editor; confirm re-normalizes and revalidates stable server/tab/focus/
  mode identity before the only PTY write, while cancel has no PTY side effect.
  CLI `terminal-paste` remains non-interactive and bypasses review deliberately.
  The Windows public `remote-ui-smoke` proves ordinary asynchronous delivery and
  exact `ESC[200~...ESC[201~` PTY bytes; Unix uses the same framing helper and
  rejects stale tab/focus/modal completions instead of pasting into a new target.
  The reusable Linux/macOS adapters preserve caller deadlines and stable
  `Unsupported`/`Failed` clipboard causes. The matching-host Unix workbench
  journey owns native clipboard-to-PTY delivery, `terminal.pasted`, and delayed
  stale-target cancellation. Exact-SHA `b4f1622` CI run `30724960474` passed
  that complete journey on Linux x86_64 and both macOS architectures.
- [x] shared XRGB rectangle fill preserves one clipped safe API across hosts:
  spans below 64 pixels retain compiler fill, x86-64 long spans use a bounded
  `rep stosd` leaf, and AArch64 long spans use NEON with an exact scalar tail.
  Boundary, clipping, partial-row and guard tests are bit-exact to the scalar
  oracle. A release-mode 200-frame 1920x1080 A/B measured 102.3 ms versus
  210.0 ms for the previous fill (2.05x), while the paired con PE stayed the
  same size and `.text` increased by 48 bytes. Platform adapters continue to
  own surface/present FFI; this pure framebuffer kernel remains in UI Core.
- [x] the Unix HiDPI terminal layer and `agenterm-con` now share
  `RetainedXrgbFrame` for bounded allocation, dimension identity, validity and
  exact host-copy checks. Unix keeps its exact dirty-row mask and
  `TerminalLayerKey` product policy. Allocation/dimension failure propagates as
  a typed pixel-window error (or screenshot error) before a stale or incomplete
  terminal layer is presented; successful raster marks storage valid before
  committing its key.
- [x] dirty-frame rendering and GDI double buffering exist; live v0.1.12
  dogfood reports sustained terminal-content and native-frame flicker. White-box
  analysis found that the replaceable Windows GUI cleared and repainted directly
  on the window HDC and treated lease heartbeat as visible change. The current
  repair makes lease maintenance non-visual by type and composes a complete
  client frame in a compatible memory DC before one `BitBlt`, with bounded
  dimensions. Back-buffer allocation or presentation failure is a typed native
  error that closes the affected replaceable window; it does not silently fall
  back to partial direct painting and reintroduce the flicker path. Same-grid
  and duplicate in-flight resize requests are now suppressed across the typed IPC boundary, keyed by
  server epoch and stable tab ID, and redundant class-wide resize redraw flags
  are removed. White-box comparison with the pre-platform-extraction host then
  found a concrete regression: the new parent window had lost
  `WS_CLIPCHILDREN`, so every full-client `BitBlt` could overwrite native
  EDIT/BUTTON pixels before each child repainted. The platform host again clips
  child HWND regions, and unchanged child bounds/visibility now skip redundant
  `MoveWindow`/`ShowWindow` paint churn. Style and geometry contracts cover both
  invariants. A same-window/same-modal synchronous screenshot A/B measured
  Dark/Light at 528/572 ms and 663/553 ms; Light is not a distinct 4x paint
  path. The full smoke applies Light immediately before its IPC-heavy CWD,
  hierarchy, dense-tab, 80-line scroll, selection, and recovery half, explaining
  a strong visual correlation without dismissing remaining temporal flicker.
  Timestamp reconstruction then confirmed the user's observation precisely:
  three runs spent 15.0--18.3 seconds before Light and 108.6--122.5 seconds
  afterward, while like-for-like snapshot intervals rose about 1.8--2.1x. The
  dominant cause was not the palette but the smoke harness reparsing and
  pretty-rewriting its entire growing `commands.json` after every CLI call, an
  O(n²) recorder whose per-50-command median grew from 213--243 ms to
  815--1000 ms. Command evidence now appends one bounded JSONL record, keeps a
  bounded immediate checkpoint, and seals one compact schema-compatible JSON
  array at cleanup. Explicit observed-sequence barriers replace accidental
  delays the old logger had hidden. The same complete journey now passes in
  36.787 seconds versus 169.9 seconds before, a 4.62x improvement, while still
  applying Light and retaining all 15 evidence IDs.
  Focused structural tests pass. The native host now exposes monotonic redraw,
  parent-paint, child-layout, and child-visibility counters through an explicit
  test-only sample message; the sample is latched into `ui-snapshot` so observing
  it cannot form a repaint feedback loop. The owning Windows journey sampled 19
  native z/Z operations at 23 redraw requests and 8 parent paints, with zero real
  child bounds/visibility updates and increasing no-op coalescing counts. A
  subsequent 500 ms idle observation measured one redraw and one parent paint,
  again with zero child updates. The existing Light-theme 80-line PTY burst now
  waits until the GUI lease has observed the server position and then samples
  after a 250 ms paint-queue settle; it measured four redraw requests and four
  parent paints with zero child updates. This closes the automated idle, zoom,
  and high-output repaint-storm diagnostics; sustained high-output visual
  dogfood on the new binary was accepted by the user as the v0.1.12 visual
  result; future visual regressions remain ordinary maintenance work.
  The clean `78eac9e` dev artifact repeated the complete owning journey in
  63.4 seconds: counterbalanced Dark/Light totals were 1349/1237 ms with
  identical 10 redraws and 8 paints, zoom measured 23/7, 500 ms idle 1/1,
  and the Light-theme high-output burst 3/3. The journey continued through
  selection/copy, ordinary and bracketed paste, GUI detach/reconnect to the
  same server/PTY, server recovery, and orphan-free explicit shutdown. This
  strengthens the automated temporal evidence without substituting for the
  outstanding sustained-output visual acceptance.
- [x] ordinary terminal keys and modifiers are encoded for the active PTY;
  live v0.1.12 dogfood found `Shift+Tab` dropped while terminal focus was
  active. The current repair introduces one shared xterm named-key modifier
  encoder for Tab, navigation, Insert/Delete, paging and F1–F12; Unix preserves
  normalized modifiers and Windows owns the matching virtual-key plus
  WM_KEYDOWN/WM_CHAR de-duplication path. Unit contracts cover shared bytes and
  Windows mapping. The owning Windows journey now sends a Shift+Tab window
  shortcut while terminal focus is active and observes exactly three additional
  bytes at the public pane boundary; the byte contract fixes those bytes as
  `ESC [ Z`. This is deliberately automation rather than physical-key evidence:
  Win32 `GetKeyState` reports the modifier state associated with keyboard input
  retrieved by the target thread, `SetKeyboardState` changes only the caller's
  input-state table, and `SendInput` targets the global foreground input stream.
  Taking foreground focus would violate the smoke-wide `AGENTERM_NO_ACTIVATE=1`
  contract. A real keyboard Shift+Tab in the latest dogfood binary was accepted
  by the user as the v0.1.12 human result. The owning journey now repeats that
  exact GUI Shift+Tab route after
  18 native z/Z operations and settled PTY geometry, requires exactly three
  additional input bytes, and then continues through a live shell marker,
  selection/copy, paste and detach/reconnect. This closes the combined
  focus/resize/GUI-dispatch regression without mislabeling synthetic input as a
  physical-key receipt.
- [~] Windows terminal focus survives immediate native toolbar actions. Live
  dogfood found the font `z/Z` child buttons retained Win32 keyboard focus while
  the terminal input path accepts keys only for the top-level HWND. Font, locale
  and Tabs actions now restore the terminal HWND; actions that open a modal or
  Control Center deliberately do not. Native focus automation reports child
  control focus as neither terminal, composer nor Tabs, and the owning remote UI
  smoke checks focus immediately after the font click. GDI painting restores the
  previously selected font/background mode before an old RAII font can be
  destroyed. The complete replaceable-UI smoke then continues through PTY input,
  font inheritance, GUI detach, same-server/session reconnect and explicit Stop
  Server cleanup. A deeper dogfood failure was also found: one transient native
  PTY resize error used to poison the terminal's fatal I/O state, so all later
  input was rejected while the GUI remained alive, and the server nevertheless
  published a false successful resize. Resize now returns a typed failure,
  commits parser geometry and the resize journal only after native acceptance,
  and leaves the terminal writable after rejection. Remote PTY resize is now
  serialized by an owned worker with a latest-only pending slot, so the Win32
  event thread never waits on the bounded IPC round trip and stale
  lease/epoch/tab/grid results are discarded. Selection PNG evidence also waits
  boundedly for the independently scheduled `WM_PAINT` after structured state
  reaches `dragging`; unchanged pixels for the whole deadline still fail. This
  prevents a published-state/paint race from weakening the requirement that a
  live selection is visibly highlighted.
- [x] GUI shell appears before the initial ConPTY/cmd process is ready
- [x] initial terminal loads asynchronously with visible starting feedback
- [x] exited process retains its final screen and exit code
- [~] `agenterm-con` is a separate lightweight product rather than a mode of
  this runtime. Its PTY ownership, VT damage, present, glyph and ISA behavior,
  workspace/composer chrome, public control CLI, package profiles and artifact
  budget are owned by the [`agenterm-con` subtree](PRD_02_23_agenterm_con.md).
  This module keeps only the kernels both products share.
- [x] explicit tab/server close cancels I/O, closes ConPTY ownership, and
  waits within a 750 ms bound for the process-wait and reader workers; success
  is reported only after both workers finish, while an incomplete shutdown
  returns a typed error instead of pretending the terminal was closed
- [~] robust CJK double-cell layout; broader visual regression is needed
- [ ] sustained high-throughput and long-output performance qualification

Glyph rows and screenshot channel packing use shared bit-exact
architecture pixel kernels; unsupported architectures retain identical scalar
output. Rectangle fills share one clipped stride-aware UI-core contract across
con and the main Unix renderer while relying on compiler-vectorized
`slice::fill`; reducing dirty rows/frames remains the next rendering optimization
rather than maintaining an unmeasured fill-specific ISA fork.

On Windows aarch64, emitted assembly is part of the pixel-kernel evidence.
Rust 1.97 did not inline the small NEON divide-by-255 helper under ordinary
`inline`: each four-pixel iteration made two calls and spilled vector state.
The narrow `inline(always)` exception removes both calls, the helper symbol, and
the stack round-trips while preserving all 33 scalar/ISA parity tests. The
matching optimized `agenterm-ui-core` archive falls from 199,038 to 198,054
bytes. Both Windows aarch64 and Linux aarch64 `agenterm-con` consumer graphs
compile; the Windows x64 executable is intentionally not credited with this
architecture-specific reduction.

Completing a non-empty local terminal selection copies its normalized text to
the system clipboard. A click without a range, application-owned mouse gesture,
scrollbar drag, or tab-divider resize must not mutate clipboard contents.
