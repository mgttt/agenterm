# Human workspace

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] window title identifies version and live IPC port
- [x] vertical tabs on the left show the numeric index; the stable `@id` is
  exposed through the control plane
- [x] tree starts at the top without a redundant logo/header strip
- [x] tabs form a visible parent/child tree for agent and program teams
- [x] tree order is parent-first with indentation and branch connectors
- [x] closing a parent promotes its children without closing their processes
- [x] the selected node exposes direct add-child, edit, and close actions
- [x] add-child immediately opens the new node's name/note editor
- [x] collapse/expand with persisted node state
- [x] compact rows with continuous native tree connectors, grid-aligned
  expand boxes, status lamps, and bordered selection
- [ ] drag/drop reparenting and team-level actions
- [x] line 1: user-defined role/name
- [x] line 2: user note, otherwise numeric index plus running program;
  terminal-controlled TITLE remains separately observable
- [x] explicit confirmation before closing a live process
- [x] dead tabs close only by explicit human or CLI action
- [x] per-tab external composer with independent draft and Send action
  - [x] native editing shortcuts explicitly support `Ctrl+A` select all,
    `Ctrl+C` copy, `Ctrl+V` paste, and `Ctrl+X` cut
  - [x] submit text and Enter as distinct PTY events so interactive TUIs
    such as Codex execute the draft instead of leaving it in their editor
  - [x] schedule Enter asynchronously beyond paste-burst suppression and
    reject overlapping composer or direct-key input instead of merging
    transactions
  - [ ] automated interactive-TUI fixture that rejects batched paste+Enter
    without requiring a networked Codex session
- [x] `Settings` and `New` actions grouped below the tree
- [x] built-in control labels come from one declared English locale;
  semantic snapshots expose the locale and resolved labels
- [x] settings UI for terminal font family and size
- [x] `AGENTERM_SETTINGS_PATH` provides explicit settings isolation while
  the default remains `%LOCALAPPDATA%\AgenTerm\settings.json`
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
- [x] per-tab child environment injection with ephemeral proxy convenience;
  values are never persisted to the workspace
