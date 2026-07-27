# Terminal runtime

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] Win32/GDI window without GPU or OpenGL requirements
- [x] one ConPTY-backed process per tab through `rmux-pty`
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
- Professional interaction follow-ups informed by the reviewed PuTTY
  terminal model
  - [ ] application-requested raw mouse reporting wins by default while
    Shift provides a documented local-selection override
  - [ ] dragging a selection beyond the viewport auto-scrolls at a bounded
    rate and capture loss cancels the unfinished gesture cleanly
  - [ ] double-click word, triple-click line, and optional rectangular
    selection use terminal-cell rather than pixel semantics
  - [x] terminal paste reads bounded Unicode clipboard text off the GUI
    thread, normalizes newlines, filters unsafe controls, and honors
    bracketed-paste mode
- [x] dirty-frame rendering and GDI double buffering
- [x] GUI shell appears before the initial ConPTY/cmd process is ready
- [x] initial terminal loads asynchronously with visible starting feedback
- [x] exited process retains its final screen and exit code
- [~] robust CJK double-cell layout; broader visual regression is needed
- [ ] sustained high-throughput and long-output performance qualification
