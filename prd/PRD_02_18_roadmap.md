# Focused product roadmap

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- v0.1.5 Control, Terminal & Bounded Automation
  - Shipped interaction slice
    - [x] offline command help and malformed global options fail locally
      without probing or autostarting a GUI
    - [x] zero, one, and multiple healthy instances produce structured,
      deterministic target selection instead of silently choosing a fleet
    - [x] high-resolution mouse-wheel scrolling and a visible draggable
      scrollbar share the same viewport state as capture and screenshots
    - [x] terminal-cell drag selection, visible highlighting, CJK-safe text
      extraction, and Windows clipboard copy preserve plain-click RMUX input
    - [x] composer and settings edits explicitly support `Ctrl+A/C/V/X`
  - Shipped bounded automation slice
    - [x] snapshot-positioned bounded event reads and predicate waits expose
      typed epoch/gap/timeout failures
    - [x] one-invocation Rhai sidecar provides pure and immutable-observe
      profiles with API discovery and resource limits
  - Remaining release work
    - [x] creation output offers a documented stable-ID format and a
      black-box journey reuses that exact ID after mutable indexes shift
    - [x] `AGENTERM_SETTINGS_PATH` isolates settings tests and instances
      without changing the default `%LOCALAPPDATA%` contract
    - [x] public semantic actions resize, minimize, maximize, and restore
      the window; waits verify post-state and minimize preserves the PTY grid
    - [x] all built-in English controls use one locale source; the composer
      button no longer mixes `发送` with English labels
    - [x] release metadata, `--version`, Cargo lock state, and README report
      `0.1.5`; the full release gate passes the existing size and one-second
      first-window budgets
  - Explicitly deferred
    - [ ] event subscriptions, Rhai control authority, MCP, optional
      component downloads, Bash, intelligence workers, and LLM routing add
      no authority or binary surface in v0.1.5
    - [ ] raw application mouse arbitration, selection auto-scroll,
      and word/line/rectangular selection retain the professional-terminal
      follow-up gates above; bounded terminal paste shipped through the
      focus-aware window system menu
- [x] v0.1.6 Observable & Adaptable Workspace
  - Frozen implementation defaults
    - [x] Settings uses `Dark`/`Light` labels with stable `dark|light` IDs,
      live preview, atomic Apply, and Cancel/Esc rollback; dark remains the
      migration default and custom theme files remain unfrozen
    - [x] Terminal `Ctrl+Down` focuses Composer and Composer `Ctrl+Up`
      returns to Terminal; source-inapplicable directions pass through.
      Terminal `Ctrl+Left` shows/focuses Tabs when hidden and Tabs
      `Ctrl+Right` returns to Terminal, but no native Edit control loses
      standard Ctrl+Arrow word navigation
    - [x] Tabs recovery appears in the status bar only while hidden; system
      menu recovery is always available. Width defaults to 250 px, clamps
      around 180..480 px while retaining a usable terminal, double-click
      resets it, and visibility plus configured width persist
    - [x] the status bar orders host segments as hidden-Tabs recovery,
      last-known CWD, flexible provider space, and right-aligned Proxy.
      CWD/proxy edits default to safely quoted Composer preparation;
      immediate injection is explicit and never offered for unknown shells
    - [x] Proxy closed-eye reveals only on/off, open-eye reveals sanitized
      scheme/host/port, and credential material requires a second temporary
      reveal inside the editor; all reveal state is ephemeral and secret
      values remain absent from persistence, snapshots, events, and audit
    - [x] default window close detaches by hiding the HWND and preserving
      server/PTY state; stop-and-exit saves metadata then ends the server;
      Cancel/Esc changes nothing. No tray icon ships in v0.1.6
    - [x] Script Platform v2 completes supervised pure/typed-observe only;
      Rhai control remains a post-core candidate and named script/module
      loading, Bash runtime, and MCP binaries remain outside the release
    - [x] `agenterm-cli ui-action` remains the compatibility entry while
      operations gain stable typed IDs internally; new top-level aliases
      are added only when they improve human discovery without duplicating
      semantics
    - [x] all release-core branches must pass before any candidate lane is
      selected; the recommended first candidate is bounded transcript
      capture, not simultaneous scope expansion across all candidates
  - Release core: Settings and built-in themes
    - [x] Dark and Light theme settings preview, apply, cancel, and persist without interrupting PTYs
    - [x] redesign Settings as a keyboard-accessible draft dialog with
      Appearance and Terminal sections plus explicit Apply and Cancel;
      theme selection previews the complete window, Apply atomically saves,
      and Cancel/Esc restores the configuration from dialog open
    - [x] ship stable built-in `dark` and `light` theme IDs, preserving dark
      as the migration default; themes own host surfaces, controls, terminal
      defaults, selection, scrollbar, and basic ANSI 16 colors while
      explicit RGB and the standard 256-color cube retain their values
    - [x] use an internal theme registry and persist only `color_theme` so
      later custom save/load/import can extend the model without freezing a
      premature external theme-file contract in v0.1.6
    - [x] expose theme ID through settings and snapshots; public UX evidence
      covers preview, Apply, Cancel/Esc rollback, restart persistence, PTY
      continuity, Dark/Light screenshots, and readable focus/contrast
  - Release core: keyboard-first surface navigation
    - [x] keyboard-first Ctrl+Arrow surface navigation preserves native Edit word movement and suppresses cross-focus repeat
    - [x] directionally map Terminal `Ctrl+Down` to Composer and Composer
      `Ctrl+Up` to Terminal while retaining `Ctrl+Up` in the PTY,
      `Ctrl+Down` in native Edit, existing `Ctrl+Shift+I`, and Esc
    - [x] fire surface navigation once per physical press and suppress
      auto-repeat crossing into the newly focused surface; modal focus traps
      and unavailable surfaces fail safely
    - [x] Terminal `Ctrl+Left` shows and focuses Tabs, including when hidden,
      and Tabs `Ctrl+Right` returns to Terminal; Composer, note, and Settings
      Edit controls retain native `Ctrl+Left/Right` word navigation
    - [x] route keyboard and semantic focus through one typed operation,
      retain `ui-snapshot.focus.surface` as the fact source, and black-box
      direction, native Edit pass-through, repeat, and hidden recovery
    - [x] physical Win32 evidence covers live PTY focus and native Edit
      word-navigation arbitration; routing is host-surface based rather
      than shell-name based, and the RMUX compatibility CLI adds no second
      in-terminal key layer
  - Release core: working-context status segments
    - [x] partition the bottom bar into host-owned Tabs recovery,
      last-known CWD, flexible provider, and right-aligned Proxy segments;
      narrow layouts preserve interactive recovery targets and Dark/Light
      states, and CWD/Proxy geometry, editors, reveal controls, and
      semantic snapshots share the same layout
    - [x] truthful working-context CWD uses launch and OSC 7 provenance with safe Composer preparation
    - [x] report CWD honestly with `launch|osc7|user_requested|unknown`
      provenance; support OSC 7 and future shell integration, but never
      inspect remote process PEBs or parse prompt pixels to pretend that a
      last-known path is authoritative
    - [x] a CWD editor safely quotes known cmd/PowerShell/future-Bash
      commands and defaults to preparing them in Composer; explicit
      non-default Send Now is unavailable for unknown shells because the
      host cannot prove that a foreground terminal is waiting at a prompt
    - [x] CWD preparation never silently overwrites a Composer draft:
      empty-only is the default, append/replace are explicit typed actions,
      Prepare performs no PTY write, and a request remains
      `user_requested`/pending until a valid bounded local OSC 7 confirms
      the path; invalid OSC does not replace the last-known value
    - [x] tab-scoped HTTP(S) proxy context remains ephemeral and redacted across UI, control, persistence, and terminal evidence
    - [x] after CWD is accepted, show tab-scoped HTTP(S) proxy state with a
      GDI eye/eye-slash toggle and editor; closed-eye shows only on/off,
      open-eye shows sanitized scheme/host/port, and credential/query/
      fragment values require a second editor reveal and remain redacted
      from snapshots, events, audits, logs, and semantic screenshot data
    - [x] CWD/proxy editors are keyboard focus traps with typed semantic
      prepare actions; proxy values and reveal state remain ephemeral,
      never persist to workspace, and never falsely claim to mutate the
      environment of already-running arbitrary descendants
  - Release core: adaptive Tabs workspace
    - [x] Tabs collapse, recovery, and resizing share one persisted workspace geometry
    - [x] place a `Tabs` button immediately left of `Settings`; activating
      it collapses the complete tab tree and its controls so terminal and
      composer reclaim the width
    - [x] when collapsed, reserve a small host-owned `Tabs` reveal segment
      at the far left of the existing bottom status bar; it is layout
      chrome, not a dynamic provider, and therefore remains available when
      future status scripts fail, time out, or have no value
    - [x] add an always-available, checked-state `Toggle Tabs` item to the
      window-icon system menu; the hidden status segment and system menu
      prevent a persisted collapsed state from trapping the user
    - [x] make the tab/terminal boundary a draggable horizontal resize grip
      with a resize cursor, pointer capture, live terminal/composer
      relayout, and double-click reset to the default width
    - [x] central geometry clamps tab width around a proposed 180 px
      minimum, 250 px default, and 480 px maximum while preserving a
      usable terminal floor on narrow windows; exact values require visual
      and CJK-label evidence rather than scattered constants
    - [x] persist `tabs_visible` and the last expanded width as user layout
      preferences; hiding never discards the width, and restoring uses the
      last valid clamped value
    - [x] hiding while focus is in the tab tree moves focus safely to the
      terminal; Settings, close confirmation, composer, scrollbars,
      selection, screenshots, PTY sizing, and hit testing all consume the
      same effective content origin
  - [x] `agenterm.exe --no-activate` shows or starts the workspace without activation and behind the current foreground window; `--not-foreground` remains an alias
  - Release core: detach-first server lifecycle
    - [x] detach-first window close preserves the live server and explicit stop creates a fresh runtime
    - [x] replace unconditional `WM_CLOSE` destruction with a host-owned
      three-choice close confirmation: `Keep Server Running` is the default
      and hides the window while preserving the same server, epoch, IPC,
      live PTYs, scrollback, and drafts; `Stop Server & Exit` saves
      workspace metadata then ends the server and PTYs; `Cancel` and Esc
      return without changing state; all three button labels are centered
      horizontally and vertically
    - [x] treat the default choice as detach rather than false process exit:
      a later `agenterm.exe`, `start-server`, or `attach-session` invocation
      re-shows and focuses the same hidden HWND and server process
    - [x] keep explicit automation noninteractive: `shutdown` performs the
      save-and-stop path, while `kill-server`/`server-kill` retain their
      stronger destructive saved-session semantics; Windows logoff/shutdown
      saves and exits without blocking the OS on the interactive modal
    - [x] expose close-modal, visible/hidden, detach, reattach, and shutdown
      state through typed snapshots, waits, events, and `server-list --json`
      without claiming continuity after a real server stop; the discovery
      view publishes visible/detached/window state, modal kind, and current
      event position beside PID, address, tabs, session, and workspace
  - Release core: Observable Fleet completion
    - [x] audit every declared event kind against its committed state and
      fill any missing transition coverage without expanding into durable
      replay or unbounded terminal logging; the compile-time closed catalog
      and public server/tab post-state checks prevent string-only drift
    - [x] add public black-box restart, bounded-history gap, and concurrent
      reader/waiter journeys, including snapshot-to-follow handoff and
      cancellation cleanup
    - [x] make modal kind/target directly waitable so close-confirmation and
      Settings automation no longer require client-side polling
  - Release core: typed operation foundation
    - [x] typed operation catalog shared by CLI validation, IPC dispatch, capability discovery, stable errors, and event attribution
      replaces UI-specific branching incrementally, beginning with adaptive
      Tabs operations rather than claiming every legacy command is migrated
    - [x] classify operations as observe, control, or destructive; this is
      an honest authority boundary for later Rhai/MCP consumers, not yet a
      policy system that grants autonomous control; discovery labels the
      catalog classification-only and reports no authorization policy
    - [x] expose tabs show/hide/toggle and bounded width adjustment through
      typed semantic actions as well as physical UI, with stable snapshot
      fields for visibility, configured/effective width, grip geometry,
      bounds, and system-menu state; `tabs-show`, `tabs-hide`,
      `tabs-toggle`, and `tabs-set-width --width 180..480` use stable
      `ui.tabs.*` IDs while legacy `toggle-tabs` remains an alias
  - Release core: Script Platform v2
    - [x] repair the shipped v1 contract before adding authority:
      `script check` rejects unknown/profile-inaccessible APIs, wall-time
      exhaustion returns the typed limit class, invocation input is bounded,
      and the host validates result envelope/API/invocation identity plus
      stable success/script/configuration/limit/host exit classes
    - [x] extract a Rhai-independent worker supervisor with kill-on-close
      Windows Job Object, parent-enforced deadline, bounded cooperative
      cancellation then forced termination, protocol/output limits,
      concurrency ceilings, and no orphan after timeout, crash, CLI
      interruption, or parent exit
    - [x] replace the stdin-to-EOF/final-stdout-only worker exchange with a
      versioned inherited-pipe frame protocol for invoke, broker request/
      response, cancel, and result; script stdout remains captured data and
      can never corrupt protocol frames
    - [x] keep `pure` ambient-authority-free and upgrade `observe` from one
      raw snapshot variable to discoverable typed workspace/tab/snapshot/
      bounded-capture/event-read/event-wait APIs brokered through the host;
      restart, gap, timeout, truncation, and return limits remain explicit
    - [x] make `script api --json` the exact typed catalog and make
      `script check` validate API names, profiles, capabilities, versions,
      and static limits offline rather than only compiling Rhai syntax
    - [x] append privacy-bounded audit records for identity/fingerprint,
      requested/effective profile, capabilities and budgets, broker
      operation IDs, duration, result class, denial, cancellation, timeout,
      and crash without source, argv, pane content, environment values,
      stdout, clipboard data, or credentials
    - [x] expose the supervisor, capability broker, typed operation adapter,
      and audit sink as Rust boundaries reusable by future Bash and MCP
      executables without making either depend on Rhai types or shipping
      their runtime/transport in v0.1.6
    - [x] public adversarial tests cover malformed/oversized/duplicate
      frames, unsupported versions, every budget class, hard timeout,
      cancel, crash, parent exit, concurrency, restart/gap, authority
      denial, audit privacy, subsequent recovery, first-window isolation,
      binary budgets, and absence of orphan workers or temporary source
  - Quality gate
    - [x] pure geometry tests cover visible/hidden, narrow-window clamps,
      resize/reset, and terminal origin; settings tests cover defaults,
      migration, invalid widths, and isolated persistence
    - [x] public UX black-box tests click all recovery entrances, perform a
      physical boundary drag, verify live PTY column changes, restart the
      isolated GUI, and prove terminal selection/scrollbar/modal behavior
      remains aligned
    - [x] lifecycle black-box tests exercise all three close choices and
      keyboard defaults; detach must preserve PID, epoch, tab IDs, PTYs,
      scrollback, drafts, and server discovery across reattach, while
      stop-and-exit must create a new epoch/PTY on the next start and CLI
      shutdown/kill paths must never wait for a modal
    - [x] release qualification adds Observable Fleet restart/gap/
      concurrent-reader evidence while preserving the 4 MiB GUI,
      per-sidecar size, one-second first-window, remain-on-exit, and
      explicit-close gates
    - [x] release builds stage verified artifacts in `dist/` and then run
      `cargo clean`; development builds retain incremental `target/`
      caching so disk cleanup does not impose a rebuild on every edit
    - [x] `build.bat release-fast` provides an optimized incremental local
      loop with LTO disabled and parallel codegen, while consolidated
      staging uses one PowerShell process (preferring `pwsh`) instead of
      paying one interpreter startup per artifact
    - [x] every local smoke-test GUI launch and CLI autostart inherits
      `AGENTERM_NO_ACTIVATE=1` and must remain behind the user's foreground
      work; local release qualification skips the 4,128-write bounded-event
      saturation load, which runs explicitly on the clean release CI worker
    - [x] after the final v0.1.6 visual surface is stable, capture a
      deterministic privacy-safe Dark-theme demonstration as
      `assets/screendump0.png` and place it near the top of README with
      descriptive alt text; transient test evidence remains under ignored
      output paths
  - Candidate enhancement lanes after the core is green
    - [x] non-intrusive bounded transcript capture by stable tab ID, with
      visible/scrollback ranges and explicit truncation metadata
    - [ ] an explicit Rhai `control` preview may expose only individually
      allowlisted, non-destructive typed operations after broker/capability/
      audit gates pass; send-keys, close, kill, shutdown, filesystem,
      process, and network authority remain denied
    - [ ] terminal selection auto-scroll plus double-click word and
      triple-click visual-line selection; rectangular selection and raw
      application-mouse arbitration remain later work
  - Explicitly outside v0.1.6
    - [ ] MCP, default/destructive Rhai control authority and event handlers,
      dynamic status providers, Bash runtime distribution,
      optional-component networking, installer/updater/signing,
      intelligence workers, and LLM routing remain separately gated roadmap
      items
Milestone numbers identify independently gated product tracks, not a strict
serial implementation order. A later track may ship while an unrelated earlier
track remains planned, but every declared dependency must still pass.

- [~] M0 cross-version boundaries and baselines: typed control operations,
  sidecar protocol boundaries, binary size/startup, compatibility corpus, and
  artifact provenance remain prerequisites consumed by later tracks
- [x] M1 fleet CLI: ship `agenterm-mux.exe` from the existing supported
  tmux/RMUX command surface and generated compatibility matrix
- [ ] M2 shell gate: prototype `agenterm-bash.exe`, select and license the
  real Bash runtime strategy, then pass clean-machine terminal tests
- [ ] M3 optional components: ship signed-manifest inventory/install/update/
  rollback foundations and independently gated SSH, HTTP, and SQLite
  sidecars without adding GUI network authority
- [~] M4 / v0.1.7 internal Control-Plane Integrity & Delivery Reset
  - [x] product truth is split into owned PRD modules and command, operation,
    event, executable, and evidence registries have drift checks; the
    integrated inventory and status audit are qualification gates
  - [~] close the command feedback loop with versioned receipts, stable
    resolved targets, bounded idempotency, truthful completion, causal events,
    epoch-bound waits, and false-success regression coverage: receipt replay,
    Composer completion, terminal finalization, and dead-write paths are wired,
    while command-wide and wait-wide coverage remains incomplete
  - [x] an isolated shared test harness retains privacy-bounded
    first-failure evidence and proves bounded cleanup of identity-matched
    owned processes/windows/workers/registrations, including injected CLI,
    GUI, and script-worker failures
  - [x] qualification has a versioned required-gate manifest, provenance
    validation, fail-closed receipt logic and self-tests, while an independent
    dry-run packager accepts only the exact qualified executable/SBOM bytes
  - [~] running and staged identities expose
    `same|stale|incompatible|unknown` with public fleet evidence, while
    lifecycle actions and the final GUI/server compatibility decision remain
    gated
  - [~] command, receipt/error, terminal lifecycle, observation, upgrade
    identity, scripting protocol, test-harness, and qualification boundaries
    are extracted incrementally without a Win32/renderer/ConPTY rewrite;
    bounded IPC transport, ConPTY runtime, and lossless wake signaling now
    have owned modules, while the remaining Win32 state-machine decomposition
    is intentionally deferred until a concrete change needs it
- [ ] M5 next public product version: scope intentionally remains unassigned
  until the v0.1.7 internal qualification and dogfood review; registry, named
  commands, providers, Rhai control, and event handlers are candidates rather
  than inherited commitments
- [ ] M6 later controlled agentic bridge: ship MCP read-only resources,
  then explicit control tools, Rhai control, brain/flow orchestration, and
  agent/token status without weakening close safety
- [ ] M7 evidence-gated optional intelligence: deterministic rules establish
  the baseline; any learned worker advances only after a concrete user case
  and portable Windows CPU evidence beat simpler methods
- [ ] M8 governed LLM gateway hypothesis: local forwarding, routing, quota,
  audit, cost, credential isolation, and redaction remain unassigned until
  scripting, MCP, and event-core gates produce a concrete product need
