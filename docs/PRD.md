# AgenTerm product tree

Status: active development  
Platform: Windows  
Default shell: the real system `cmd.exe`

AgenTerm is a native terminal for people and agents. Human interaction and the
local CLI operate on the same tabs, PTYs, drafts, settings, and observable
state. A process exiting never silently destroys its tab.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Product tree

- AgenTerm
  - Terminal runtime
    - [x] Win32/GDI window without GPU or OpenGL requirements
    - [x] one ConPTY-backed process per tab through `rmux-pty`
    - [x] VT100 parsing, ANSI colors, scrollback, resize, keyboard and mouse
    - [x] dirty-frame rendering and GDI double buffering
    - [x] exited process retains its final screen and exit code
    - [~] robust CJK double-cell layout; broader visual regression is needed
    - [ ] sustained high-throughput and long-output performance qualification
  - Human workspace
    - [x] vertical tabs on the left with stable ID and numeric index
    - [x] line 1: program plus terminal-controlled TITLE
    - [x] line 2: independent user note
    - [x] explicit confirmation before closing a live process
    - [x] dead tabs close only by explicit human or CLI action
    - [x] per-tab external composer with independent draft and Send action
    - [x] settings UI for terminal font family and size
    - [x] embedded AgenTerm icon
    - [ ] configurable shell, colors, working directory, and startup tabs
  - Agent control plane
    - Observation
      - [x] stable active tab `id:name`
      - [x] text capture, raw escaped output, styled cell dumps
      - [x] JSON pane, tab, focus, modal, layout, and protocol snapshots
      - [x] whole-window and selected-pane PNG screenshots
      - [ ] incremental output sequence and event stream
    - Action
      - [x] create, select, rename, annotate, and close tabs
      - [x] send keys and terminal mouse events
      - [x] read, replace, and submit composer content
      - [x] semantic focus and UI actions
      - [x] deterministic waits for output, dead state, focus, and modal state
      - [ ] broadcast input and synchronized panes
    - Protocol
      - [x] loopback-only newline-delimited JSON IPC
      - [x] feature discovery through `protocol-info`
      - [x] explicit errors for unsupported operations
      - [ ] named-pipe transport and stable event subscription
  - tmux/RMUX compatibility
    - [x] common session/window command names, aliases, targets, and formats
    - [x] function-key byte sequences including Byobu F2/F3/F4/F6/F8
    - [x] RMUX status active-marker parsing and clickable window labels
    - [x] Windows native mouse-input bridge for RMUX 0.9.1
    - [x] initial ConPTY grid sizing keeps RMUX status at the bottom
    - [x] minimizing the GUI does not resize PTYs to the iconic rectangle
    - [ ] split panes and layout commands
    - [ ] full compatibility matrix; compatibility claims remain semantic
  - Delivery and quality
    - [x] fast incremental developer build at repository-root `agenterm.exe`
    - [x] release mode and root `agenterm.json` build metadata
    - [x] unit tests for command parsing, protocol, settings, and RMUX status
    - [x] CLI and semantic UX smoke tests through public interfaces
    - [x] one-command fmt, Clippy, test, build, and smoke regression
    - [ ] automated terminal input/resize/ANSI/CJK/long-output matrix
    - [ ] installer, updater, stable PATH location, and signed releases

## Non-negotiable invariants

- Exiting a child process does not remove its tab.
- A live tab is not destroyed without an explicit close and confirmation.
- Tab IDs remain stable for the lifetime of the tab; indexes may change.
- Agent-facing state is machine-readable and actions can be verified without
  arbitrary sleeps.
- tmux/RMUX names are used only where behavior is compatible. Unsupported
  behavior returns an error rather than pretending to succeed.
- AgenTerm does not silently download or bundle fonts. `Sarasa Fixed SC`
  (SIL OFL 1.1) is the recommended optional CJK monospace font.

## Current acceptance gate

Run `.\check.ps1`. A change is ready only when formatting, Clippy with warnings
denied, unit tests, root artifact generation, CLI smoke, and semantic UX smoke
all pass. Rendering changes additionally require `screenshot` or
`screenshot-pane` inspection.
