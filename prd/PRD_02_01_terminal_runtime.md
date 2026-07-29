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
- [~] shared PTY backend trait: Windows keeps `rmux-pty`; Unix uses
  POSIX `openpty` + fork/exec; `terminal_runtime` consumes one API
- [x] VT100 parsing, ANSI colors, scrollback, resize, keyboard and mouse
- [x] Backspace emits ConPTY VT `DEL` and deletes exactly one input
  character in the default `cmd.exe` line editor
- [x] mouse wheel and a visible draggable scrollbar navigate terminal
  history; scrollbar track clicks page and dragging to the bottom restores
  the live viewport
- [x] dragging selects visible terminal cells and Ctrl+C copies the selected
  text; an unmodified click still reaches RMUX/native terminal mouse input
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
- [x] dirty-frame rendering and GDI double buffering
- [x] GUI shell appears before the initial ConPTY/cmd process is ready
- [x] initial terminal loads asynchronously with visible starting feedback
- [x] exited process retains its final screen and exit code
- [x] explicit tab/server close cancels I/O, closes ConPTY ownership, and
  waits within a 750 ms bound for the process-wait and reader workers; success
  is reported only after both workers finish, while an incomplete shutdown
  returns a typed error instead of pretending the terminal was closed
- [~] robust CJK double-cell layout; broader visual regression is needed
- [ ] sustained high-throughput and long-output performance qualification
