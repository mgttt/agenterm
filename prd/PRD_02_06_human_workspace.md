# Human workspace

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] window title identifies version and live IPC port
- [~] Linux/macOS human workspace MVP: one window, live POSIX PTY tabs,
  keyboard input, visible VT grid, tab sidebar with New/Tabs/Settings
  toolbar, event journal, shared workspace IPC, composer, settings modal,
  wheel/scrollbar, paste, and word/row/drag selection; status-bar CWD editor,
  window-close confirm, and tabs resize grip on Unix; Win-only proxy editor
  remains a follow-up. The Linux/macOS matching-host gate is now registered
  against public no-activate/activation, renderer-owned snapshot+PNG, native
  clipboard paste, and delayed stale-target cancellation. It remains partial
  until both host-native CI receipts pass. Exact-SHA `78eac9e` run
  `30723737091` passed the complete macOS journey on both architectures;
  Linux reached its native paste but waited for `terminal.pasted` before the
  asynchronous snapshot had converged. The shared journey now first waits for
  typed paste success through public state, then verifies the already-committed
  event, preserving a precise clipboard failure if convergence does not occur.
  Follow-up SHA `d7facf6` run `30724482279` then exposed that precise failure:
  the Script-owned one-shot `xclip` writer had already lost its X11 selection
  owner, so the product adapter truthfully returned `clipboard_backend_error`.
  The Linux fixture now retains a foreground `xclip -silent -loops 2` child:
  one native read proves readiness, AgenTerm consumes the second, and the
  process then exits under the shared orphan-free cleanup contract. Exact-SHA
  `b4f1622` run `30724960474` passed the repaired Linux journey, including
  no-activate/activation, renderer-owned snapshot plus PNG, native clipboard
  paste into the live PTY, delayed stale-target cancellation, and orphan-free
  cleanup. Together with both macOS cells in the same run, this closes the
  matching-host receipt gap; the broader workspace remains partial for the
  explicitly listed follow-up product leaves. Win alignment execution map:
  [`plan/plan-unix-gui-win-parity.md`](../plan/plan-unix-gui-win-parity.md)
- [x] vertical tabs on the left show the numeric index; the stable `@id` is
  exposed through the control plane
- [x] tree starts at the top without a redundant logo/header strip
- [x] tabs form a visible parent/child tree for agent and program teams
- [x] tree order is parent-first with indentation and branch connectors
- [x] closing a parent promotes its children without closing their processes
- [x] the selected node exposes direct add-child and close actions in shared
  row geometry and replaces them with Save/Cancel while that row is being
  edited
- [x] v0.1.11 removes the ordinary-row `Edit` action from visible geometry:
  one click on the row body selects, while a double-click on the name/note
  body enters the same stable-ID inline editor. Disclosure controls, branch
  lines, status lamps, scrollbars and the remaining action area never trigger
  editing. `F2` and the public `edit-tab` UI action remain equivalent
  keyboard/automation entry points.
  Windows and Unix recognizers bind the candidate to the stable tab ID and a
  geometry generation; scrolling, resizing, hiding Tabs, or changing tabs
  invalidates it before a second click can target content that moved under the
  pointer. The owning workbench journey covers single click, excluded hit
  regions, F2/public-action equivalence, Esc/transition cancellation,
  Save/Cancel, no ordinary Edit bounds, and before/after PNG evidence.
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
  - [x] entering edit replaces the ordinary `+`/`Close` actions with
    `Save`/`Cancel`; Cancel restores the persisted name/note without mutation
  - [x] after the v0.1.11 gesture change, ordinary rows expose only the
    contextual add-child and close actions; editing rows expose Save/Cancel,
    and either exit path returns to that simpler ordinary geometry
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
- [x] the terminal grid derives its rows and columns from the exact shared
  terminal viewport, excluding the toolbar, Composer, status bar, Tabs column,
  and terminal scrollbar. The last advertised PTY row remains fully visible
  above the Composer after startup, resize, font changes, and HiDPI scaling;
  constrained windows reduce the grid instead of placing terminal content
  behind another surface. Public evidence is the Unix geometry/unit contract
  plus a real-host screenshot when rendering qualification is available.
  This does not change Composer height or add overlay padding.
- [x] workspace chrome uses a full-height Tabs column and a terminal workbench column containing the top New/Tabs/Settings toolbar, terminal viewport, Composer, and terminal-scoped status bar
- [x] `New`, `Tabs`, `Settings`, language, and terminal font-size actions are
  grouped in the compact toolbar
  above the terminal; the toolbar remains available when Tabs are hidden so
  the same `Tabs` control restores the full-height tree
- [x] toolbar order is Tabs then New, with `[Settings] [En|Zh] [z|Z]` anchored
  at the right; Tabs reads `<Tabs` while the tree is visible and `>Tabs` while
  hidden
- [~] v0.1.11 adds a responsive `Control Center` entry centered within the
  terminal workbench column between the left Tabs/New group and the right
  Settings/En|Zh/z|Z group. It contracts to `CC` before overlapping either
  group, but retains the same accessible name, tooltip, action identity and
  snapshot semantics. The action opens or focuses the independent Control
  Center client owned by
  [the Control Center module](PRD_02_21_control_center.md).
- [~] activating New opens an extensible terminal-creation dialog before
  mutation: Windows ships Default/Command Prompt/PowerShell selection, an
  optional initial command, retained but temporarily inert per-terminal
  HTTP/HTTPS proxy drafts, and Create/Cancel; Unix parity remains follow-up
- [~] the creation dialog still validates proxy-shaped drafts and snapshots
  expose configured booleans without revealing values, but HTTP_PROXY and
  HTTPS_PROXY injection is intentionally paused on every create path. The
  child inherits the caller's proxy environment unchanged until a later
  accepted proxy design defines explicit semantics and evidence.
- [~] built-in control labels come from one declared locale source; English and
  Traditional Chinese can be switched at runtime and persist across UI
  restarts. Semantic snapshots expose the locale and resolved labels. Windows
  replaceable-UI controls are complete; Unix/macOS modal-copy parity remains.
- [x] Windows replaceable-UI labels use one locale source with persistent English and Traditional Chinese switching
- [~] Settings separates persisted default appearance from current-terminal
  overrides for font family, size, and color theme. Every override field can
  independently return to inheritance; `[z|Z]` creates or adjusts only the
  active terminal's size override. Overrides are client-owned and keyed by
  server address plus stable tab ID so the server remains UI-neutral. Windows
  UI and the shared settings model are complete; Unix/macOS modal parity
  remains. Built-in skins extend this contract below; they do not replace
  Apply/Cancel, inheritance, or PTY continuity.
- [ ] the local-IPC migration replaces server-address-derived appearance
  override keys with the stable server scope defined by the
  [Agent control plane](PRD_02_07_agent_control_plane.md), so changing a
  socket path or transport does not orphan terminal UI preferences

## Built-in skins (v1)

Owns the product contract for built-in appearance presets. Execution map:
[`plan/plan-skins-v1.md`](../plan/plan-skins-v1.md). External SkinHub packages
remain a later roadmap leaf (`prd/PRD_02_18_roadmap.md` M14) and must reuse
this theme contract rather than invent a second system.

### Product outcome

- [ ] Users can choose among four built-in presets without interrupting PTYs:
  `classic-day`, `classic-night`, `fancy-day`, `fancy-night`.
- [ ] Skin and luminance are orthogonal: `skin` ∈ {`classic`,`fancy`},
  `luminance` ∈ {`day`,`night`}; the composite id is `{skin}-{luminance}`.
- [ ] Classic inherits today's industrial Dark/Light spirit (migration
  defaults below). Fancy is a more branded refinement (metrics, accent weight,
  optional icon/title branding) — never a second product identity or toy UI.
- [ ] Shared product constitution stays skin-invariant: integer-grid spacing,
  tab-tree semantics, `ui-snapshot` truthfulness, ANSI readability, and
  diagnosable failures.

### Identity and persistence

- [ ] Stable composite ids: `classic-day`, `classic-night`, `fancy-day`,
  `fancy-night`. Display names (en / zh-Hant): Classic Day / 經典白,
  Classic Night / 經典黑, Fancy Day / 華麗白, Fancy Night / 華麗黑.
- [ ] Persist `appearance_preset` (or equivalent `skin` + `luminance`) in
  client settings; keep palette bytes out of settings so built-ins can evolve.
- [ ] Migration: legacy `color_theme: "dark"` → `classic-night`; `"light"` →
  `classic-day`. Unknown ids fall back to `classic-night` (same spirit as
  today's Dark default). Forward-compatible deserialize must not reject
  settings written by a newer build.
- [ ] Per-terminal appearance overrides may select a preset or inherit the
  workspace default, matching today's theme-override model.
- [ ] `ui-snapshot` exposes `appearance_preset`, `skin`, `luminance`, and
  `theme_options` entries with `id`, `label`, and short `description`.
  Legacy `color_theme` may remain as a derived compatibility field
  (`night`→`dark`, `day`→`light`) until consumers migrate.

### Skinable tokens (v1)

| Token | Classic | Fancy | Notes |
|-------|---------|-------|-------|
| Full `ThemePalette` | Current Dark/Light spirit | Distinct accent/surfaces; same struct | Host chrome + terminal defaults + ANSI-16 |
| Brand short name | `AgenTerm` | Same base; optional light ornament only in fancy chrome | Data dirs stay `agenterm` / `AgenTerm` — not skinable |
| Window title template | `{brand} {version}` (+ optional ` — {instance}`) | `{brand} · {version}` (+ instance) | Unify Win/Unix title construction through the template |
| Application icon | Existing `assets/agenterm.*` | `assets/skins/fancy/icon.*` | Linux runtime window icon is part of this leaf |
| Corner radius / border metrics | `0` (right angles) | Small radii (e.g. 4/8) on controls/modals | Metrics live beside palette, not as ad-hoc render magic |
| Scrollbar / selection chrome | Rectilinear, restrained | Softened thumb + stronger hover | Still integer-grid aligned |

Explicit non-goals for v1: user-authored skin packages, hot-reload market,
per-skin full locale catalogs, sound, wallpaper, animated chrome, changing
tab OSC titles, or renaming on-disk config/data roots.

### Settings UX

- [ ] Appearance settings offer a 2×2 preset picker (or skin × luminance) with
  live whole-window preview; Apply persists atomically; Cancel/Esc restores
  the previous preset without PTY disruption.
- [ ] Labels come from the locale source; Unix must not hardcode English-only
  Dark/Light button text once presets ship.
- [ ] Evidence: extend `theme-smoke` / theme render-parity so all four presets
  are selectable, persisted, restart-stable, and PNG-differentiable where
  luminance differs; classic vs fancy must also be distinguishable in chrome
  or snapshot fields.

### Acceptance

- [ ] Choosing any preset updates host chrome and terminal defaults through
  the same palette path used today (no second paint path).
- [ ] Restart restores the persisted preset; unknown/legacy values migrate as
  specified.
- [ ] Window title and icon follow the active skin's branding tokens on each
  host that can set them.
- [ ] Snapshot + PNG evidence agree; typed failures remain diagnosable.
- [ ] SkinHub packaging is out of scope until this built-in contract is green.
- [x] `AGENTERM_SETTINGS_PATH` provides explicit settings isolation; Windows
  retains `%LOCALAPPDATA%\AgenTerm\settings.json`, while Unix uses the stable
  `XDG_CONFIG_HOME/agenterm/settings.json` (or `HOME/.config`) user path
- [x] Unix `main` and `dev` workspaces use stable user-scoped XDG data paths
  instead of inheriting the launcher's current directory; Windows `main`
  retains its legacy `%LOCALAPPDATA%\AgenTerm\workspace.json` migration path
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
- [x] The Proxy workbench is archived. Users currently configure proxy
  variables inside their terminal; create-time `--proxy` and HTTP(S) proxy
  tab-environment overrides are accepted as inert compatibility input and do
  not alter the child environment. No bottom-bar entry, editor, reveal
  control, Prepare, Send Now, or runtime-injection action is advertised.

## Archived tab proxy workbench

- [~] the former create-time proxy drafts remain ephemeral and scoped to one
  stable tab ID, but are intentionally not applied to the child environment;
  workspace persistence never stores their endpoint or credentials
- [x] snapshots expose only bounded redacted launch facts needed for
  diagnostics; pane, event, command-log, retained failure and GUI-stderr
  evidence never reveal the endpoint or credentials
- [x] the former status slot remains zero-width, unavailable and explicitly
  `archived`; every former Proxy editor/application UI action fails explicitly
  with `proxy workbench controls are archived` and changes neither Composer nor
  terminal input
- [x] restarting never restores a transient proxy value or application claim
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
