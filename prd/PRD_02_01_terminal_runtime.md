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
- [x] one ConPTY-backed process per tab through `rmux-pty` on Windows
- [x] shared PTY backend facade: Windows keeps adapter-private `rmux-pty`; Unix uses
  POSIX `openpty` + fork/exec; `terminal_runtime` consumes one API
- [x] VT100 parsing, ANSI colors, scrollback, resize, keyboard and mouse
- [x] Backspace emits ConPTY VT `DEL` and deletes exactly one input
  character in the default `cmd.exe` line editor
- [~] mouse wheel and a visible draggable scrollbar navigate ordinary terminal
  history; scrollbar track clicks page and dragging to the bottom restores the
  live viewport. Live v0.1.12 dogfood found alternate-screen harnesses whose
  zero local scrollback makes wheel/PageUp ineffective. The v0.1.12 repair now
  publishes additive alternate-screen/application-cursor mode facts and turns
  wheel notches into bounded CSI/SS3 cursor input only when the alternate grid
  has no local history; unit and mixed-version serde evidence pass. The owning
  Windows journey now contains a real alternate-screen PowerShell PTY fixture
  that observes mode facts and maps native wheel messages to RawUI virtual-key
  input while retaining ordinary scrollback assertions; its final integrated
  run remains pending. Future application raw-mouse arbitration also remains
  open, so this capability is not yet marked shipped.
- [~] dragging is intended to select visible terminal cells and Ctrl+C copies
  the selected text while an unmodified click reaches RMUX/native terminal
  input. Live v0.1.12 dogfood found a Windows replaceable-GUI path where a
  selection cannot be established, making Copy unreachable; gesture ownership,
  capture and public clipboard evidence must be requalified before restoring
  shipped status. The repair now preserves a gesture across content generations
  when tab and grid geometry are unchanged, releases capture after completing
  the phase, and caches the completed text. Only a completed non-empty selection
  may claim Ctrl+C; prepared or empty selection state leaves Ctrl+C available as
  PTY interrupt. The owning journey now injects real PTY output after pointer
  down and proves system-menu clipboard text; live highlight/capture observation
  and direct Ctrl+C copy evidence remain before restoring shipped status.
- [x] window-icon system menu exposes focus-aware Copy and Paste: native
  edit controls receive their standard messages, while terminal Copy uses
  the active cell selection and terminal Paste uses the active PTY
- v0.1.8 professional-selection slice (P0), informed by the reviewed PuTTY
  terminal model
  - [ ] selection has explicit about-to-select, dragging, completed, and
    cancelled states; a click that never becomes a drag retains its existing
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
  normalizes newlines, filters unsafe controls, and honors bracketed-paste mode
- [~] dirty-frame rendering and GDI double buffering exist, but live v0.1.12
  dogfood reports sustained terminal-content and native-frame flicker. White-box
  analysis found that the replaceable Windows GUI cleared and repainted directly
  on the window HDC and treated lease heartbeat as visible change. The current
  repair makes lease maintenance non-visual by type and composes a complete
  client frame in a compatible memory DC before one `BitBlt`, with bounded
  allocation and a direct-paint fallback. Same-grid and duplicate in-flight
  resize requests are now suppressed across the typed IPC boundary, keyed by
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
  dogfood on the new binary remains the final human acceptance check before
  restoring shipped status.
- [~] ordinary terminal keys and modifiers are encoded for the active PTY;
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
  contract. A real keyboard Shift+Tab in the latest dogfood binary therefore
  remains the non-substitutable human acceptance check before restoring shipped
  status.
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
  lease/epoch/tab/grid results are discarded.
- [x] GUI shell appears before the initial ConPTY/cmd process is ready
- [x] initial terminal loads asynchronously with visible starting feedback
- [x] exited process retains its final screen and exit code
- [x] explicit tab/server close cancels I/O, closes ConPTY ownership, and
  waits within a 750 ms bound for the process-wait and reader workers; success
  is reported only after both workers finish, while an incomplete shutdown
  returns a typed error instead of pretending the terminal was closed
- [~] robust CJK double-cell layout; broader visual regression is needed
- [ ] sustained high-throughput and long-output performance qualification
