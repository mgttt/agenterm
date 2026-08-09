# Focused product roadmap

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Unscheduled inspiration and long-horizon ideas live in
[Inspiration backlog and future vision](PRD_02_19_inspiration_and_future_vision.md).
That module captures product-owner intent and future lanes; this roadmap owns
version gates and milestone acceptance only.

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
    - [x] one-invocation Rhai sidecar provides API discovery, deterministic
      computation, Fleet observation, and resource-robustness limits
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
    - [ ] event subscriptions, MCP, optional component downloads, Bash,
      intelligence workers, and LLM routing add no binary surface in v0.1.5;
      Agent authorization remains outside Script Runtime
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
    - [x] the terminal-column status bar orders host segments as hidden-Tabs recovery,
      last-known CWD, and flexible provider space. The former right-aligned
      Proxy surface is archived and returns its width to providers
    - [x] the former Proxy eye/editor status entry is archived; users configure
      proxy variables in their shell while only redacted launch-state
      observation remains
    - [x] default window close detaches by hiding the HWND and preserving
      server/PTY state; stop-and-exit saves metadata then ends the server;
      Cancel/Esc changes nothing. No tray icon ships in v0.1.6
    - [x] the first Script Platform slice completed supervised deterministic
      execution and typed Fleet observation; this described its shipped API
      breadth, not a permission boundary. Named script/module loading, Bash
      runtime, and MCP binaries remained outside the release
    - [x] `agenterm cli ui-action` remains the compatibility entry while
      operations gain stable typed IDs internally; new top-level aliases
      are added only when they improve human discovery without duplicating
      semantics
    - [x] all release-core branches must pass before any candidate lane is
      selected; the recommended first candidate is bounded transcript
      capture, not simultaneous scope expansion across all candidates
  - Release core: Settings and built-in themes
    - [x] Dark and Light theme settings preview, apply, cancel, and persist without interrupting PTYs
    - [x] Dark/Light same-window render-cost parity rejects stable 3--5x palette-dependent asymmetry
      by measuring the same synchronous target click, screenshot, and renderer
      activity in counterbalanced ABBA order with a bilateral 2.75x duration
      envelope; the first current-tree receipt measured 1095ms Dark versus
      1203ms Light with identical 10 redraw / 8 paint totals
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
    - [x] partition the terminal-column bottom bar into host-owned Tabs recovery,
      last-known CWD, and flexible provider segments; the archived Proxy slot
      remains zero-width and non-actionable in semantic snapshots
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
    - [x] create-time tab-scoped HTTP(S) proxy context remains ephemeral and redacted across UI, control, persistence, and terminal evidence
    - [x] the former tab-scoped HTTP(S) Proxy GDI eye/editor status surface and
      runtime-application actions are archived and fail explicitly; no proxy
      value, reveal state or application claim persists to workspace
    - [x] the CWD editor remains a keyboard focus trap with typed semantic
      prepare actions; archived Proxy actions do not reuse Composer or mutate
      already-running descendants
  - Release core: adaptive Tabs workspace
    - [x] Tabs collapse, recovery, and resizing share one persisted workspace geometry
    - [x] place Tabs then New in the left side of a compact toolbar above the
      terminal and anchor Settings at the right; `<Tabs` denotes collapse and
      `>Tabs` denotes reveal, while the terminal workbench reclaims the hidden
      tree width and keeps the toolbar available for recovery
    - [~] route New through an extensible creation dialog; Windows selects
      Default/Command Prompt/PowerShell, accepts an optional initial command
      and redacted ephemeral HTTP(S) proxy environment, and mutates only on
      Create, while Unix dialog parity remains follow-up
    - [x] when collapsed, reserve a small host-owned `Tabs` reveal segment
      at the far left of the terminal-column bottom status bar; it is layout
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
      an operation taxonomy for later Rhai/MCP consumers, not an authorization
      boundary or a policy system; discovery labels the catalog
      classification-only and reports no authorization policy
    - [x] expose tabs show/hide/toggle and bounded width adjustment through
      typed semantic actions as well as physical UI, with stable snapshot
      fields for visibility, configured/effective width, grip geometry,
      bounds, and system-menu state; `tabs-show`, `tabs-hide`,
      `tabs-toggle`, and `tabs-set-width --width 180..480` use stable
      `ui.tabs.*` IDs while legacy `toggle-tabs` remains an alias
  - Release core: Script Platform v2
    - [x] repair the shipped v1 contract:
      `script check` rejects unknown APIs, wall-time
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
    - [x] expose discoverable typed workspace/tab/snapshot/bounded-capture/
      event-read/event-wait APIs brokered through the host; deterministic
      computation and Fleet observation are ordinary unrestricted runtime
      uses, while restart, gap, timeout, truncation, and return limits remain
      explicit robustness contracts
    - [x] make `script api --json` the exact typed catalog and make
      `script check` validate API names, API feature IDs, versions,
      and static limits offline rather than only compiling Rhai syntax
    - [x] append privacy-bounded audit records for identity/fingerprint,
      runtime/API facts, available API features and robustness budgets, broker
      operation IDs, duration, result class, failure, cancellation, timeout,
      and crash without source, argv, pane content, environment values,
      stdout, clipboard data, or credentials
    - [x] expose the supervisor, runtime service broker, typed operation adapter,
      and audit sink as Rust boundaries reusable by future Bash and MCP
      executables without making either depend on Rhai types or shipping
      their runtime/transport in v0.1.6
    - [x] public adversarial tests cover malformed/oversized/duplicate
      frames, unsupported versions, every budget class, hard timeout,
      cancel, crash, parent exit, concurrency, restart/gap, audit privacy,
      subsequent recovery, first-window isolation,
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
    - [x] release builds compile in dedicated `target-release/`, stage verified
      artifacts in `dist/`, and clean only that scratch target; development
      builds retain incremental `target/` caching so release cleanup does not
      impose a cold rebuild on the next edit
    - [x] `build.bat release-fast` provides an optimized incremental local
      loop with LTO disabled and parallel codegen, while consolidated
      staging uses one named Rhai task instead of paying one interpreter
      startup per artifact
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
    - [ ] expand typed Rhai control, filesystem, process, network, and
      destructive-operation adapters behind truthful receipts, cancellation,
      and product-state invariants; Script Runtime does not use allowlists as
      Agent permissions
    - [ ] terminal selection auto-scroll plus double-click word and
      triple-click visual-line selection; rectangular selection and raw
      application-mouse arbitration remain later work
  - Explicitly outside v0.1.6
    - [ ] MCP, Rhai event handlers, dynamic status providers, Bash runtime distribution,
      optional-component networking, installer/updater/signing,
      intelligence workers, and LLM routing remain separately gated roadmap
      items
Milestone numbers identify independently gated product tracks, not a strict
serial implementation order. A later track may ship while an unrelated earlier
track remains planned, but every declared dependency must still pass.

- [~] M0 cross-version boundaries and baselines: typed control operations,
  sidecar protocol boundaries, binary size/startup, compatibility corpus, and
  artifact provenance remain prerequisites consumed by later tracks
- [x] M1 fleet CLI: ship `agenterm cli mux` from the existing supported
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
- [ ] M5 / v0.1.8 Programmable Daily Fleet public-ready candidate
  - [ ] the professional-selection slice is owned by
    [Terminal runtime](PRD_02_01_terminal_runtime.md): bounded selection
    auto-scroll, word and visual-row gestures, complete cancellation, and its
    public terminal matrix
  - [ ] the integrated public dogfood and byte-identical candidate,
    qualification, package, and non-publishing release rehearsal are owned by
    [Delivery and quality](PRD_02_17_delivery_quality.md)
  - [ ] every additional v0.1.8 product branch must first enter its own linked
    PRD module with acceptance evidence and explicit non-goals; neither this
    roadmap node nor the public version plan grants inherited implementation
    scope
  - [ ] public-ready does not authorize publication: creating or pushing the
    `v0.1.8` tag and creating a public GitHub Release require explicit user
    approval after the candidate and rehearsal gates pass
- [x] M6 / v0.1.9 General Script Runtime
  - [x] deliver the cross-cutting stable-server/replaceable-GUI migration:
    headless `agenterm server` owns session/PTY truth, current
    `agenterm.exe` reconnects as a versioned UI client, and upgrade/rollback
    evidence preserves server/PTY identity and continuing output
  - [x] establish typed script catalog schema v3 independently of the stable
    Script API v2 protocol version, including reviewed Node.js/Bun research
    analogues, and add an explicit server-independent `local`
    unrestricted local-runtime foundation
  - [x] make unrestricted local execution the ordinary historical
    `agenterm-rhai.exe` behavior (retired Wave 4.5; live path is `agenterm-rh`)
    and deliver the Rust-shaped `std::{fs,path,env,process,time}` subset plus
    `rhai::{task,http,json,bytes,runtime}` extensions without moving future
    Agent approval policy into the runtime
  - [x] add bounded task/stream/cancellation, local modules, versioned
    `agenterm.tasks.json`, named task list/show/check/run, and one machine-readable
    API catalog shared by check, runtime, Fleet tools and future consumers
  - [x] publish that catalog as a stable three-level capability tree with
    shipped/planned machine facts, explicit deferred/out-of-scope specification
    facts, typed degraded reasons, and reviewed purpose-level Rust/Node.js/Bun
    analogues; every entry separates catalog, surface and Rust paths and
    records semantic differences, while the CLI tree and machine matrix render
    from the same source
  - [x] make runtime/module/task identity package-ready through stable
    version, provenance hooks, requirements, capabilities and entry points,
    without adding remote resolution, installation, signature policy or a
    public registry to the runtime; task schema v2 ships runtime/API/catalog
    identity, API/capability requirements, fail-closed compatibility, and
    bounded non-trust origin/provenance hooks
  - [x] systematically expose every public typed Fleet operation or a stable
    degraded reason; mutations return request identity, receipt, correlated
    event and verified post-state through the same unrestricted runtime surface
  - [x] public file/process/loopback-HTTP/task/module/Fleet/privacy/crash/orphan
    journeys prove one complete local automation task; the former low-risk
    PowerShell/Rhai dual-run has completed its caller cutover and source removal
  - [x] implementation sequencing, budgets, risks and release evidence are
    owned by [the v0.1.9 public plan](../plan/archive/plan-v0.1.9.md)
- [x] M7 / v0.1.10 Rhai Self-Hosting and Verifiable Read-Only Agent Bridge
  - [x] complete the evidence-gated replacement of all repository-owned
    PowerShell automation: no tracked `.ps1`, no hidden PowerShell business
    logic in bootstrap/CI, and no PowerShell process in the clean build,
    check, qualification, package, or release-rehearsal process tree
  - [x] use the stable Rhai task catalog and shared modules as the sole source
    of build, quality, black-box, qualification, packaging, and approved
    release semantics; platform entry points only bootstrap and forward
    arguments/exit status
  - [x] ship `agenterm cli mcp` as an on-demand stdio sidecar pinned to one
    stable MCP protocol revision; offline discovery declares exact methods,
    resources, tools, limits, schemas, and unavailable future roles
  - [x] expose only metadata-safe instance, workspace, tab, and causal Fleet
    snapshot resources sourced from the public typed control plane; pane text,
    Composer, environment values, proxy values, credentials, and clipboard
    remain absent
  - [x] expose one read-only bounded `agenterm_wait` tool with epoch/sequence,
    allowlisted predicate, timeout, cancellation, restart, gap, and target-close
    semantics; no create/send/close/kill or other mutation tool is advertised
  - [x] public JSON-RPC lifecycle, same-source resource, wait causality,
    malformed/oversized peer, crash, privacy, concurrency, restart, and orphan
    tests preserve GUI/PTY isolation, no-activate, first-window, binary-size,
    remain-on-exit, and explicit-close gates
  - [x] control tools, MCP client/federation, network transport, subscriptions,
    pane-content resources, embedding the unrestricted Rhai runtime into
    autonomous agent flows, brain/flow, Agent-harness permissions, and
    autonomous scheduling remain later independently approved integration
    gates; these gates never reduce standalone historical `agenterm-rhai.exe`
    APIs (retired Wave 4.5; live automation uses `agenterm-rh`)
  - [x] implementation sequencing, budgets, risks and release evidence are
    owned by [the v0.1.10 public plan](../plan/archive/plan-v0.1.10.md)
- [x] M10 / v0.1.11 Control Center and native local-IPC foundation
  - [x] simplify ordinary tab rows by removing the visible Edit action;
    double-clicking the row text enters the existing stable-ID inline editor,
    while F2 and the public UI action preserve keyboard and automation access
  - [x] add a centered responsive `Control Center` toolbar action and an
    independent `agenterm-cc` client; it reuses or focuses one process per
    user configuration domain and never owns PTYs, workspace truth, package
    transactions, workflow runs or decentralized-network state
  - [x] ship truthful navigation and empty/degraded states for Cockpit,
    Workflows, Extensions (PluginHub/AppHub), and InfoHub; the first accepted
    content slice is read-only Fleet facts over existing typed contracts
  - [x] introduce logical `main` and `dev` instances and a transport-neutral
    endpoint contract; migrate in compatibility stages toward Unix domain
    sockets on Linux/macOS and named pipes on Windows while keeping explicit
    loopback TCP and mixed-version discovery
  - [~] prove an isolated `agenterm-net` libp2p/CID/block-store research loop
    without linking it into the stable server or claiming a stable release
    component
  - [~] define and measure a Tauri-like system-WebView host contract, with
    native Control Center fallback and no WebView dependency in the terminal
    GUI/server hot path
  - [x] sequencing, parallel waves, risks and acceptance evidence are owned by
    [the v0.1.11 public plan](../plan/archive/plan-v0.1.11.md), with canonical product
    boundaries in [Control Center](PRD_02_21_control_center.md),
    [Agent control plane](PRD_02_07_agent_control_plane.md), and
    [Decentralized network foundation](PRD_02_22_decentralized_network.md)
- [~] M11 / v0.1.12 planned, delivered in v0.1.14 — Convergence and fast
  candidate promotion

  > ⚠️ Version attribution corrected 2026-08-05. The v0.1.12 and v0.1.13
  > candidates were built but never publicly released, so the public sequence
  > runs v0.1.11 → v0.1.14. The evidence lines below are unchanged and remain
  > authoritative for the SHAs and runs they name; only the milestone's version
  > label was wrong. Current delivery history is owned by
  > [the v0.1.14 public plan](../plan/archive/plan-v0.1.14.md), and the in-flight
  > version is v0.1.15.
  - [x] converge the v0.1.11 native IPC foundation across Windows named pipes
    and Linux/macOS Unix sockets: logical main/dev isolation, stale authority
    recovery, mixed-schema discovery and one shared resolver remain truthful
    under upgrade and rollback
    - [x] native macOS 26.5 arm64 evidence (2026-07-31) passes the direct
      `native-ipc-smoke --ci-main-dev` public task in 1.60 seconds: logical
      main/dev and duplicate-authority isolation, owner/socket modes,
      `/tmp`/`/private/tmp` canonicalization, typed overlong-path and symlink
      rejection, explicit TCP compatibility, schema-v1/v2 discovery, and
      killed-authority socket/lock/registration recovery. Registration
      takeover removes only a confirmed-dead schema-v2 record for the same
      scope and endpoint; live or differently scoped records remain. Native
      Clean Windows SHA `274f971` subsequently passed both owning tasks:
      current named-pipe/TCP/schema/stale recovery plus exact published
      v0.1.10/v0.1.11 native upgrade, HEAD state write and rollback reads.
      Exact-SHA `b4f1622` ordinary CI run `30724960474` subsequently passed the
      Windows named-pipe and Linux/macOS Unix-socket authority journeys plus all
      applicable published upgrade/rollback journeys in the six-cell matrix.
      This closes the matching-host compatibility receipt gap without erasing
      the separately listed destructive credential and legacy-client limits.
  - [x] close the Linux/macOS main-workbench evidence gap with separate
    matching-host receipts for no-activate launch, native activation truth,
    renderer-owned snapshot/PNG, asynchronous native clipboard paste and stale
    completion cancellation. The shared journey/task/CI ownership is integrated;
    Exact-SHA `b4f1622` run `30724960474` passed the complete journey on Linux
    x86_64 and both macOS architectures, closing this matching-host receipt leaf.
    Broader workspace follow-ups remain governed by the owning PRD.
  - [~] deepen the independent Control Center only through a useful read-only
    Cockpit and complete platform evidence. Windows, Linux X11 and macOS native
    lifecycle/renderer/caller-instance journeys are integrated and have matching-
    host receipts; macOS physical pointer acceptance remains open, and richer
    Workflow, Extensions or InfoHub content is not promoted by this slice
  - [x] split fast feedback, exact-SHA candidate qualification and release
    promotion so a complete stress-inclusive qualification executes once per
    eligible candidate; tag publication verifies and promotes the previously
    qualified six-platform bytes without rebuilding or rerunning the complete
    desktop suite. Delivered by v0.1.14: candidate run `30942173420` at source
    `8ff2b5a` sealed all six platforms, and promotion run `30944087372`
    published without recompiling
  - [x] bind promotion to an exact commit, receipt, platform matrix, artifact
    hashes, SBOM and provenance; missing, stale or tampered candidate artifacts
    fail closed before a GitHub Release exists. Delivered by v0.1.14, which
    published 23 assets bound to `8ff2b5a`. The fail-closed behaviour was
    exercised for real: the first promotion attempts stopped before publication
    on a provenance SBOM mismatch and on manifest identity checks. See
    [Delivery and quality](PRD_02_17_delivery_quality.md) and
    [the v0.1.14 public plan](../plan/archive/plan-v0.1.14.md) for the eight
    release-chain defects this first end-to-end execution exposed
  - [ ] measure queue, cache, compile, test, package, upload and promotion
    stages; correctly keyed Cargo/sccache experiments and optional paid runners
    may change latency but never eligibility, evidence or artifact identity
  - [ ] sequencing, runner experiments, release SLOs, risks and delivery
    history are owned by the current version plan —
    [v0.1.14](../plan/archive/plan-v0.1.14.md) for what shipped and the measured CI
    analysis, [v0.1.15](../plan/plan-v0.1.15.md) for in-flight work; the
    superseded [v0.1.12 plan](../plan/archive/plan-v0.1.12.md) remains the record for
    that candidate. Canonical delivery requirements stay in
    [Delivery and quality](PRD_02_17_delivery_quality.md)
- [ ] M12 / v0.2.0 Control Center content maturity
  - [ ] deepen the independent Control Center beyond its v0.1.11 shell:
    Cockpit operational views, versioned Workflow definitions/runs,
    Extensions catalog backed by softmgr, and InfoHub source/provenance/routes
  - [ ] PluginHub and AppHub remain separate product-class views over one
    catalog/source/install/update/rollback substrate; they do not become two
    incompatible package systems
  - [ ] WebView becomes a production Control Center renderer only after
    platform availability, bridge isolation, offline startup, crash recovery,
    binary-size and six-target evidence pass
  - [ ] durable scheduling, public marketplace transactions and stable
    decentralized service integration remain separately gated even if their
    navigation exists
  - [ ] execution prerequisites and phases (folded from the former
    `plan/plan-v0.2.0.md`, removed 2026-08-04 so `plan/` stays current-version
    only):
    - [ ] prerequisites already shipped: v0.1.11 Control Center shell +
      typed bridge + read-only Cockpit; Platform Facade revision 4;
      shared UX single-point semantics. `agenterm-net` N2-M1 stays in
      `research/` and is not a v0.2.0 prerequisite
    - [ ] Phase A — Cockpit first operable vertical slice (no external
      authority dependency): health/exception/runs/evidence drill-down,
      typed actions with receipt/post-state, epoch-gap rebuild baseline
    - [ ] Phase B — Workflows + Extensions (depend on external authorities
      C1 orchestration and D1/D2 softmgr substrate; until those ship, views
      expose truthful planned/unavailable state, never promote Rhai tasks
      to durable flows)
    - [ ] Phase C — InfoHub + optional WebView (depend on E1 source
      framework; WebView production only after the six-target
      availability/bridge/offline/crash/size gates pass)
    - [ ] non-goals: no second PTY/server/workflow/softmgr/net authority in
      Control Center; no public marketplace transactions, silent installs,
      or libp2p/IPFS embedded in the stable server; no privileged bridge for
      remote web pages; remote package management (agenterm.work) stays a
      later line
    - [ ] future-lane boundaries: InfoHub may reserve a net source type,
      Extensions may reserve a remote catalog concept, but neither embeds a
      node; computer-use (P4) and mobile (plan-mobile.md) are independent
    - [ ] sequencing and delivery history belong to this milestone; the
      former public plan file is deleted (2026-08-04); inspiration lanes
      J1–J5 remain in
      [Inspiration backlog](PRD_02_19_inspiration_and_future_vision.md)
- [ ] Multi-platform GUI track (independent of v0.1.8–v0.1.10 version gates):
  shared PTY backend, Unix IPC server, Linux/macOS `winit`+`softbuffer`
  human window MVP, and release packages that include `agenterm` on
  linux/macos; sequencing in
  [plan-multiplatform-gui.md](../plan/plan-multiplatform-gui.md). Native
  window/input/IME/DPI/clipboard/font integration converges through the
  [Windows, macOS, and Linux platform branches](PRD_02_20_native_platform.md)
  without moving product behavior into OS adapters.
- [ ] M8 evidence-gated optional intelligence: deterministic rules establish
  the baseline; any learned worker advances only after a concrete user case
  and portable Windows CPU evidence beat simpler methods
- [ ] M9 governed LLM gateway hypothesis: local forwarding, routing, quota,
  audit, cost, credential isolation, and redaction remain unassigned until
  scripting, MCP, and event-core gates produce a concrete product need
- [ ] M15 / pre-v0.2.0 rh backend migration and layered pack deployment
  (schedule after v0.1.15; not in v0.1.15 scope)
  - [~] rh AOT track (rh-0→rh-2) ships on `main` for trial:
    subset check, transpile→rustc AOT, pack qualify, fleet native shim, host
    eval, source-hash cache, `AGENTERM_SCRIPT_BACKEND=rh`, `./rh-check.sh`
  - [ ] incremental **Rhai → rh** migration after v0.1.15 delivery:
    automation corpus stays `.rhai` until then; per-script rh-2 validation,
    optional default backend switch, pack-only hot path for task entry points
  - [ ] **layered deployment** (JVM / JAR analogue):
    stable base PE family (host, broker, supervision) vs signed **rh pack**
    application layer loaded in-process via `script_rh_pack`; Control Center,
    gateway sidecar, and task runners share one host C ABI
  - [ ] design SSOT: [`plan/design-rh-aot.md`](../plan/design-rh-aot.md);
    checklist in
    [Rust host + Rhai scripting — Future](PRD_02_10_rhai_scripting.md#future-pre-v02.0-schedule-after-v0115)
  - [ ] non-goals until v0.1.15 ships: 62-task manifest migration, default
    `AGENTERM_SCRIPT_BACKEND=rh`, npm-style remote rh imports, Cranelift direct
    codegen
- [ ] M13 / v0.2.x Distribution surface and one package substrate
  - [ ] `agenterm.work` becomes the single public distribution entry point:
    OS/architecture detection, one copyable install command per platform, a
    `302` artifact redirect to the Release bytes, and a machine-readable
    `releases.json` index derived from the existing per-artifact
    `.provenance.json` and the sealed candidate manifest; no second source of
    release truth and no proxying of Release bytes
  - [ ] the installer surface reaches platform parity: `install.sh` and a new
    `install.ps1` share one channel model (`stable` / `preview`), one
    versioned `releases/<version>` + `current` layout, one SHA-256 gate and
    one `installed.json` record; Windows stays a portable no-admin payload and
    does not acquire an MSI or registry footprint
  - [ ] supply-chain evidence becomes user-visible rather than CI-only: the
    installer verifies the artifact `.provenance.json` against the requested
    version and measured digest, prints commit / tag / build log / signed /
    notarized state, and stores it locally; `provenance.sbom_sha256` is
    populated rather than the empty string it ships as today
  - [ ] macOS publishing is data-driven, not documentation-driven: while the
    Apple Developer enrollment is incomplete, `variant: unsigned-preview` is a
    labeled public preview channel that requires one explicit acknowledgement
    and never installs silently; once `ENABLE_SIGNED_MACOS_RELEASE` is true the
    same clients prefer the signed and notarized artifact with no copy change
  - [ ] `agenterm cli update` owns check / apply / rollback / retention over
    the same verification path as first install; delta updates are an explicit
    non-goal, and applying an update to a running server keeps the existing
    keep-server session semantics while making the disk-versus-live version
    difference understandable without reading documentation
- [ ] M14 / v0.2.x–v0.3.x Hub ecosystem over one substrate
  - [ ] PluginHub, SkinHub, AppHub and InfoHub are product-class views over a
    single softmgr catalog / source / install / update / rollback substrate
    keyed by a `kind` field; skins are `kind: skin` packages over the existing
    theme contract and never become a second extension system. Built-in
    classic/fancy × day/night presets ship first under
    [Human workspace](PRD_02_06_human_workspace.md) /
    [`plan/archive/plan-skins-v1.md`](../plan/archive/plan-skins-v1.md) before any external
    SkinHub package format is frozen.
  - [ ] discovery is cross-kind and renders one signed registry index that both
    the web surface and Control Center consume; the index is a static signed
    document before it is ever a service
  - [ ] trust is tiered and visible: `first-party` / `verified` / `community` /
    `unverified` derive from provenance, SBOM and digest evidence equivalent to
    the product's own release chain; executing kinds (`plugin`, `app`) default
    to `verified` or better and declare a permission manifest, while
    non-executing kinds may relax to `community`
  - [ ] host compatibility is declared per package and pre-checked before a
    host upgrade disables installed content; the plugin API revision reuses the
    Script Runtime and MCP contracts rather than defining a third surface
  - [ ] offline is a first-class state: a stale-labeled local index, fully
    offline operation of installed packages, offline export/import over the
    same verification path, and a self-hosted index URL for restricted networks
  - [ ] curation starts closed and opens on evidence: repository-reviewed
    manifests first, self-service submission with automated provenance and
    permission gates second, and content-addressed distribution over
    `agenterm-net` only after N3; the trust index and its signing key remain
    centrally owned even when hosting is decentralized
  - [ ] non-goals: no public marketplace transactions, no silent installs, no
    privileged bridge for remote pages, and no distribution-native packages
    (deb / rpm / Homebrew / MSI / Store) until one channel is proven worth its
    standing maintenance cost
- [ ] Unscheduled optional-application ecosystem
  - [ ] treat the independently versioned `agenterm-{role}.exe` family as
    future software-distribution units backed by signed inventory,
    compatibility, transactional install/update/repair/remove and rollback
  - [ ] evolve `agenterm-softmgr.exe` only after the local package contract and
    supply-chain gates are proven; a public package/application market is a
    later discovery, trust and distribution service, not v0.1.9 or v0.1.10
    scope
  - [ ] explore `agenterm-desktop.exe` first as an Explorer-coexisting
    companion application; any system-shell replacement requires a separately
    approved minimal watchdog, lease activation, crash rollback and proven
    Explorer recovery, and development builds never directly persist a
    Winlogon shell change
