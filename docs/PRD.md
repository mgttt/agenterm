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
    - [x] GUI shell appears before the initial ConPTY/cmd process is ready
    - [x] initial terminal loads asynchronously with visible starting feedback
    - [x] exited process retains its final screen and exit code
    - [~] robust CJK double-cell layout; broader visual regression is needed
    - [ ] sustained high-throughput and long-output performance qualification
  - Human workspace
    - [x] window title identifies version and live IPC port
    - [x] vertical tabs on the left with stable ID and numeric index
    - [x] tree starts at the top without a redundant logo/header strip
    - [x] tabs form a visible parent/child tree for agent and program teams
    - [x] tree order is parent-first with indentation and branch connectors
    - [x] closing a parent promotes its children without closing their processes
    - [x] the selected node exposes direct add-child, edit, and close actions
    - [x] add-child immediately opens the new node's name/note editor
    - [x] collapse/expand with persisted node state
    - [ ] drag/drop reparenting and team-level actions
    - [x] line 1: user-defined role/name plus the running program
    - [x] line 2: user note, with terminal-controlled TITLE as fallback
    - [x] explicit confirmation before closing a live process
    - [x] dead tabs close only by explicit human or CLI action
    - [x] per-tab external composer with independent draft and Send action
    - [x] `Settings` and `New` actions grouped below the tree
    - [x] settings UI for terminal font family and size
    - Persistent workspace
      - [x] normal application close preserves the tab tree and active tab
      - [x] names, notes, composer drafts, and original commands are restored
      - [x] restored commands start as new processes; no false process continuity
      - [x] `kill-server` intentionally destroys the saved session
      - [ ] optional terminal screen-history snapshot
    - Status bar
      - [x] full-window bottom status surface, independent of the active terminal
      - [x] semantic bounds exposed through `ui-snapshot`
      - [ ] built-in CPU, disk, clock, active-agent, and token segments
      - [ ] CLI-configurable segment layout and refresh policy
      - [ ] dynamic script/provider segments with timeout and failure isolation
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
  - Command line (`agentermctl.exe`)
    - Shared grammar
      - target: `-t @id`, `-t %id`, `-t index`, or `-t exact-name`
      - format: `-F FORMAT`; supports `#S`, `#I`, `#W`, `#P` and
        `#{session_name}`, `#{window_*}`, `#{pane_*}`, `#{terminal_title}`,
        and `#{tab_parent_id}`; `list-tab-tree` also supports `#{tab_depth}`
        and `#{tab_has_children}`
      - stable IDs are preferred; numeric indexes may change after closing tabs
    - tmux/RMUX-aligned commands
      - Session/server
        - `new-session|new [-s name] [command [args...]]`
        - `attach-session|attach`, `start-server`
        - `list-sessions|ls`, `has-session|has [-t target]`
        - `rename-session|rename name`
        - `kill-session`, `kill-server`
      - Windows mapped to AgenTerm tabs
        - `new-window|neww [-d] [-n name] [--parent target]
          [command [args...]]`
        - `list-windows|lsw [-F format]`
        - `select-window|selectw -t target`
        - `next-window|next`, `previous-window|prev`
        - `rename-window|renamew [-t target] name`
        - `kill-window|killw -t target`
      - Single pane per tab
        - `list-panes|lsp [-t target] [-F format]`
        - `send-keys|send [-t target] [-l] key...`
        - `capture-pane|capturep -p [-t target]`
        - `display-message|display -p [-t target] format`
        - `show-options|show`, `list-commands|lscm`
        - [ ] `split-window|splitw` returns an explicit unsupported error
    - AgenTerm extensions
      - Team tree
        - `list-tab-tree [-F format]`
        - `set-tab-parent -t child --parent parent|root`
        - `show-tab-parent [-t target]`
        - `ui-action new-child [-t parent]`
        - parent cycles fail explicitly
        - closing a parent promotes direct children to its parent
      - State and deterministic waits
        - `active-window|active-tab [-F format]`
        - `inspect|pane-snapshot [-t target]`
        - `dump-cells [-t target] [-r row]`
        - `capture-pane --raw-escaped [-t target]`
        - `wait-pane|expect-pane [-t target] [--contains text|--dead]
          [--timeout-ms ms]`
        - `ui-snapshot`, `protocol-info`
        - `workspace-info`, `save-workspace`, `shutdown`
        - `wait-ui [--active @id] [--focus surface] [-t target
          --tab-state running|dead] [--timeout-ms ms]`
      - Composer and tab metadata
        - `show-composer [-t target]`
        - `set-composer [-t target] text|--stdin|--file path`
        - `send-composer [-t target]`
        - `set-tab-note [-t target] text`, `show-tab-note [-t target]`
      - Semantic UI control
        - `focus terminal|composer|sidebar [-t target]`
        - `ui-action new-tab|new-child|edit-tab|toggle-tree|select-tab|close-tab|confirm|cancel|
          composer-send|open-settings [-t target]`
      - Visual and terminal diagnostics
        - `screenshot [-o path.png]`
        - `screenshot-pane|screenshot-tab [-t target] [-o path.png]`
        - `send-mouse [-t target] -x col -y row [--button
          left|middle|right|wheel-up|wheel-down] [--action press|release]
          [--protocol auto|sgr|native]`
      - Settings
        - `get-settings`
        - `set-setting terminal.font-family FAMILY`
        - `set-setting terminal.font-size 8..36`
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
    - [x] fast incremental developer build under ignored local `dist/`
    - [x] release mode and `agenterm.json` build metadata
    - [x] size-optimized release profile and enforced 4 MiB GUI budget
    - [x] GUI `agenterm.exe` has no startup console flash
    - [x] console `agentermctl.exe` preserves CLI output and exit codes
    - [x] startup regression requires a main window within one second locally
    - [x] version-tagged GitHub Release automation for both EXEs, metadata,
      and ZIP
    - [x] unit tests for command parsing, protocol, settings, and RMUX status
    - [x] CLI and semantic UX smoke tests through public interfaces
    - [x] one-command fmt, Clippy, test, build, and smoke regression
    - [ ] automated terminal input/resize/ANSI/CJK/long-output matrix
    - [ ] installer, updater, stable PATH location, and signed releases

## Non-negotiable invariants

- Exiting a child process does not remove its tab.
- Normal application restart preserves workspace structure and metadata while
  honestly restarting each PTY process.
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
denied, unit tests, `dist/` artifact generation, CLI smoke, and semantic UX
smoke all pass. Rendering changes additionally require `screenshot` or
`screenshot-pane` inspection.
