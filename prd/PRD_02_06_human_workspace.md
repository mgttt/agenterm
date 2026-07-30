# Human workspace

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] window title identifies version and live IPC port
- [~] Linux/macOS human workspace MVP: one window, live POSIX PTY tabs,
  keyboard input, visible VT grid, tab sidebar with New/Tabs/Settings
  toolbar, event journal, shared workspace IPC, composer, settings modal,
  wheel/scrollbar, paste, and word/row/drag selection; status-bar CWD editor,
  window-close confirm, and tabs resize grip on Unix; Win-only proxy editor
  remains a follow-up. Win alignment execution map:
  [`plan/plan-unix-gui-win-parity.md`](plan/plan-unix-gui-win-parity.md)
- [x] vertical tabs on the left show the numeric index; the stable `@id` is
  exposed through the control plane
- [x] tree starts at the top without a redundant logo/header strip
- [x] tabs form a visible parent/child tree for agent and program teams
- [x] tree order is parent-first with indentation and branch connectors
- [x] closing a parent promotes its children without closing their processes
- [x] the selected node exposes direct add-child, edit, and close actions in
  shared row geometry and replaces them with Save/Cancel while that row is
  being edited
- [x] add-child immediately opens the new node's name/note editor in the new
  child's own row without borrowing the Composer
- [x] collapse/expand with persisted node state
- [x] high-density two-line rows with compact outer/inner spacing, continuous
  native tree connectors from one renderer-neutral geometry contract on
  Windows and Unix, grid-aligned expand boxes, status lamps, and bordered
  selection; persisted notes remain visible below names, and the optional
  hierarchy-state screenshot in `remote-ui-smoke` captures the connectors
  before parent-promotion mutations
- [x] the full-height Tabs tree owns a visible draggable vertical scrollbar on
  its outer left edge; row content remains to its right, and mouse-wheel, thumb
  drag, row paint, inline editors, selection, disclosure and action hit-testing
  consume the same bounded row offset and translated geometry
- [ ] drag/drop reparenting and team-level actions
- [x] line 1: user-defined role/name
- [x] line 2: user note, otherwise numeric index plus running program;
  terminal-controlled TITLE remains separately observable
- v0.1.8 inline tab editing
  - [x] editing is owned by exactly one stable tab ID and never borrows,
    covers, resizes, or changes the active tab's Composer draft
  - [x] the target row's persisted name and note display surfaces become two
    bounded native single-line edit overlays in place; the row keeps its
    expander, connectors, status lamp, selection, and stable identity
  - [x] normal `+`/`Edit`/`Close` actions become `Save`/`Cancel` for the
    editing row; Save restores normal `Edit`, and Cancel restores the
    persisted name/note without mutation
  - [x] public `set-composer -t @ID "name\nnote"` targets the matching open
    inline editor draft without overwriting that tab's bottom Composer;
    outside an open matching editor it retains the ordinary Composer meaning
  - [ ] Save and `Ctrl+Enter` are the only commit paths; `Tab`/`Shift+Tab`
    move between the two editors and row actions, while `Esc` cancels
  - [ ] a name containing no non-whitespace character fails validation,
    retains both drafts and focus in the row, exposes an inline error, and
    does not partially save the note
  - [x] selecting another tab, hiding Tabs, closing the target through another
    command, reloading the workspace, detaching/stopping/closing the window,
    or otherwise destroying the target row cancels the draft before the
    transition; none of these paths implicitly saves
  - [ ] focus movement inside the same row, including pressing Save or Cancel,
    does not cancel; ordinary window deactivation alone does not commit or
    cancel
  - [x] add-child creates the child with its normal initial persisted values
    and immediately enters that child row's inline editor; Cancel keeps the
    child and restores those initial values rather than deleting it
  - [x] only one row can edit at once; starting another edit predictably
    cancels the first draft before the second editor appears
- v0.1.8 compact Tabs tree
  - [x] root inset and every depth indent use the shared compact geometry;
    paint, connector placement, native editor placement, hit-testing, and
    snapshots consume the same row rectangles
  - [x] each row geometry owns selection, expander/disclosure hit target,
    status lamp, full text, name, note, editors, and normal/editing actions;
    host code contains no `sidebar_width - 72/-48/-24` or `node_x + 24`
    positioning
  - [ ] 180 px Tabs uses accessible compact action glyphs with stable names
    and tooltips; wider Tabs uses full labels
  - [x] responsive indentation preserves a distinct connector anchor for
    every supported depth and reserves at least one CJK glyph plus ellipsis
    beside bounded, non-overlapping actions
- [x] explicit confirmation before closing a live process
- [x] dead tabs close only by explicit human or CLI action
- [x] per-tab external composer with independent draft and Send action
  - [x] compact six-pixel outer spacing gives the native input a three-row
    target at normal window sizes (at least two useful rows when constrained),
    with a persistent native vertical scrollbar for longer drafts
  - [x] native editing shortcuts explicitly support `Ctrl+A` select all,
    `Ctrl+C` copy, `Ctrl+V` paste, and `Ctrl+X` cut
  - [x] submit text and Enter as distinct PTY events so interactive TUIs
    such as Codex execute the draft instead of leaving it in their editor
  - [x] schedule Enter asynchronously beyond paste-burst suppression and
    reject overlapping composer or direct-key input instead of merging
    transactions
  - [ ] automated interactive-TUI fixture that rejects batched paste+Enter
    without requiring a networked Codex session
- [x] workspace chrome uses a full-height Tabs column and a terminal workbench column containing the top New/Tabs/Settings toolbar, terminal viewport, Composer, and terminal-scoped status bar
- [x] `New`, `Tabs`, and `Settings` actions are grouped in the compact toolbar
  above the terminal; the toolbar remains available when Tabs are hidden so
  the same `Tabs` control restores the full-height tree
- [x] toolbar order is Tabs then New, with Settings anchored at the right;
  Tabs reads `<Tabs` while the tree is visible and `>Tabs` while hidden
- [~] activating New opens an extensible terminal-creation dialog before
  mutation: Windows ships Default/Command Prompt/PowerShell selection, an
  optional initial command, separate optional per-terminal HTTP/HTTPS proxy
  inputs, and Create/Cancel; Unix parity remains follow-up
- [x] the creation dialog validates proxy URLs and passes non-empty values only
  as ephemeral child environment; snapshots expose configured booleans but
  never command text, proxy endpoints, credentials, or unsaved input values
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
  - [x] bottom status surface spans only the terminal workbench column, leaving
    the full-height Tabs column visually and structurally independent
  - [x] semantic bounds exposed through `ui-snapshot`
  - [x] the former right-aligned Proxy display/editor entry is archived and
    releases its width to the provider region; its snapshot slot is zero-width,
    unavailable, and explicitly marked `archived`
  - [ ] built-in CPU, disk, clock, active-agent, and token segments
  - [ ] CLI-configurable segment layout and refresh policy
  - [ ] dynamic script/provider segments with timeout and failure isolation
- [x] embedded AgenTerm icon
- [ ] configurable shell, colors, working directory, and startup tabs
- [x] per-tab child environment injection.
- [x] The Proxy workbench is archived. Users configure proxy variables in
  their terminal or pass explicit create-time `--proxy`/tab environment
  values; no bottom-bar entry, editor, reveal control, Prepare, Send Now, or
  runtime-injection action is advertised.

## Archived tab proxy workbench

- [x] create-time proxy environment belongs to one stable tab ID and remains
  ephemeral; workspace persistence never stores its endpoint or credentials
- [x] snapshots expose only bounded redacted launch facts needed for
  diagnostics; pane, event, command-log, retained failure and GUI-stderr
  evidence never reveal the endpoint or credentials
- [x] the former status slot remains zero-width, unavailable and explicitly
  `archived`; every former Proxy editor/application UI action fails explicitly
  with `proxy workbench controls are archived` and changes neither Composer nor
  terminal input
- [x] restarting without explicit create-time proxy configuration restores no
  transient proxy value or application claim
- [ ] any future proxy workbench, remote distribution, fleet/global default or
  persistent profile requires a separately accepted plan covering secret
  storage, identity, policy, revocation and public black-box evidence

## v0.1.8 observation and acceptance

- [ ] `ui-snapshot` identifies the editing target by stable `@id`, reports
  `normal|editing`, validation state, unsaved-change booleans, action
  density, and the row/text/name/note/editor/action bounds used by paint and
  hit-testing
- [ ] snapshot does not disclose an unsaved name/note draft merely to prove
  editing; black-box tests prove Save through persisted post-state and Cancel
  through unchanged post-state
- [ ] public-interface black-box coverage creates a child, observes immediate
  inline editing, saves valid CJK name/note with `Ctrl+Enter`, edits again and
  cancels with `Esc`, rejects an empty name without losing drafts, and proves
  the Composer draft is byte-for-byte unchanged
- [ ] cancellation coverage includes tab switch, Tabs hide, target close, and
  window close/detach; each path proves no hidden save and no orphan native
  edit HWND
- [ ] geometry tests cover normal/editing action replacement, non-overlap,
  deep rows, CJK text reservation, 180/250/480 px Tabs, and degenerate widths
- [ ] screenshot evidence covers normal and editing rows at 180 px and default
  width, including a deep CJK child and the inline validation error

## v0.1.8 non-goals

- no multi-row editing, bulk rename, drag/drop reparenting, or team-level edit
- no autosave on focus loss, tab switch, sidebar hide, close, or shutdown
- no reuse of the Composer as a name/note editor
- no replacement of the native edit controls with a custom text editor
- no change to tab/process close confirmation, parent promotion, stable IDs,
  workspace persistence, or terminal-controlled TITLE semantics
- no global/default/remote proxy policy, persisted proxy profile, or inheritance
  from transient changes inside another tab's live shell
