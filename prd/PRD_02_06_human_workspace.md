# Human workspace

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] window title identifies version and live IPC port
- [~] Linux/macOS human workspace MVP: one window, live POSIX PTY tabs,
  keyboard input, visible VT grid, tab sidebar, event journal, shared
  workspace IPC, composer strip, settings modal, wheel/scrollbar
  scrollback, and basic cell selection/clipboard copy; professional
  selection gestures and Win-only cwd-proxy editors remain follow-ups
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
- [x] compact rows with continuous native tree connectors, grid-aligned
  expand boxes, status lamps, and bordered selection
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
  - [x] the former right-aligned Proxy display/editor entry is archived and
    releases its width to the provider region; its snapshot slot is zero-width,
    unavailable, and explicitly marked `archived`
  - [ ] built-in CPU, disk, clock, active-agent, and token segments
  - [ ] CLI-configurable segment layout and refresh policy
  - [ ] dynamic script/provider segments with timeout and failure isolation
- [x] embedded AgenTerm icon
- [ ] configurable shell, colors, working directory, and startup tabs
- [x] per-tab child environment injection.
- [>] The bottom-bar Proxy convenience is archived. Users configure proxy
  variables in their terminal; the redacted state/application machinery and
  public CLI compatibility remain temporarily available but are no longer
  advertised as GUI workspace controls.

## v0.1.8 P0 tab proxy correctness

- [x] proxy configuration is owned by one stable tab ID, remains ephemeral,
  and is never written to workspace persistence, settings, event payloads,
  receipts, snapshots, diagnostics, or logs
- [x] `Prepare` creates a sensitive Composer draft for the target
  tab and sets proxy state to `Prepared`; it does not write terminal bytes,
  mutate a live shell environment, create a child, or report `On`/`Applied`
- [ ] the prepared draft is visibly sensitive and follows the ordinary
  Composer single-submission contract; unrelated Composer drafts are not
  overwritten without the existing explicit replacement decision
- [x] `Send` or the proxy editor's mouse-accessible `Send Now` submits the
  prepared command exactly once and sets state to `Submitted`; successful
  byte delivery alone never means the proxy is active
- [x] state becomes `Applied` only after a non-secret shell marker is observed
  and public post-state verifies both the shell environment and a real child
  process inherit the intended proxy variables
- [ ] command rejection, marker mismatch, environment/child mismatch, terminal
  exit, process exit, timeout, cancellation, or any other failed application
  moves the attempt to `Failed` with a non-secret reason and never claims `On`
- [>] the former bottom-bar entry to the GUI Proxy editor is archived; its
  implementation remains commented/compatibility-addressable for now rather
  than being deleted in the same change
- [ ] `ui-snapshot` truthfully exposes the stable target, `revealed` boolean,
  `Off|Prepared|Submitted|Applied|Failed`, validation/error category, and
  bounded editor/action geometry, but never the proxy URL, credential,
  prepared command, secret environment value, or Composer text
- [x] real `cmd.exe` and PowerShell qualification proves marker ordering,
  environment application, child inheritance, exit/failure, and no duplicate
  submission
- [x] Bash-compatible preparation sets all four variables consistently:
  `HTTP_PROXY`, `HTTPS_PROXY`, `http_proxy`, and `https_proxy`
- [x] runtime injection is rejected while a direct TUI owns input or when the
  tab was launched non-interactively; the error directs the user to create a
  new tab with `--proxy` instead of typing secret setup into that terminal
- [ ] a new tab does not inherit ad-hoc proxy changes made inside the active
  shell; only explicit create-time `--proxy`/tab environment configuration
  may seed the new tab
- [ ] every transition emits a typed event and receipt bound to request ID,
  stable server/tab identity, epoch/sequence baseline, redacted proxy
  fingerprint, result category, and verified post-state; no transition is
  inferred only from paint text
- [ ] public black-box coverage drives only released GUI/CLI surfaces and
  proves privacy redaction, Prepared-not-On, Submitted-not-Applied, real
  shell/child application, failure/exit, direct-TUI and non-interactive
  rejection, no accidental inheritance, exactly-once submission, and no
  orphan shell, child, worker, native editor, or test-owned server
- [ ] remote proxy distribution, fleet-wide/global defaults, and persistent
  proxy profiles require a separate future plan with identity, secret
  storage, policy, and revocation gates; they are not part of this P0 fix

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
