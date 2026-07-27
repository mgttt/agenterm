# AgenTerm product tree

Status: active development  
Platform: Windows  
Current default shell: the real system `cmd.exe`.
Planned default shell: `agenterm-bash.exe` after its clean-machine gate passes

AgenTerm is a native super-fleet terminal for people and AI agents. Its window
is the bridge, the tab tree is the fleet, shells are crew workspaces, and the
local control plane lets people and agents observe and steer the same state.
The planned scripting plane will reuse that public contract rather than bypass
it. Human interaction and local CLI automation operate on the same tabs, PTYs,
drafts, settings, and observable state. A process exiting never silently
destroys its tab.

The visual language favors industrial confidence over decoration: repeated
integer-grid spacing, solid right-angle connections, strict baseline
alignment, restrained colors, and explicit boundaries should make the fleet
feel precisely assembled and dependable.

Terminal durability comes from deterministic two-dimensional state, not from
nostalgia. AgenTerm extends that contract from a character grid to the whole
agent fleet: humans and agents must be able to address, read, wait for, and
control the same tree nodes, focus, input, viewport, process lifecycle, and
rendered evidence precisely.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

<!-- agenterm-alignment-contract
{
  "schema_version": 2,
  "planned_command_roots": ["agenterm-bash"],
  "capabilities": [
    {
      "id": "terminal.backspace-del-one",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "Backspace emits ConPTY VT `DEL` and deletes exactly one input",
      "evidence_ids": ["cli.backspace-del-one"]
    },
    {
      "id": "terminal.mouse-scrollback",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "mouse wheel and a visible draggable scrollbar navigate terminal",
      "evidence_ids": ["ux.mouse-scrollback"]
    },
    {
      "id": "terminal.text-selection-copy",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "dragging selects visible terminal cells and Ctrl+C copies the selected",
      "evidence_ids": ["ux.terminal-selection-copy"]
    },
    {
      "id": "settings.path-isolation",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "`AGENTERM_SETTINGS_PATH` provides explicit settings isolation",
      "evidence_ids": ["ux.settings-isolation"]
    },
    {
      "id": "control.stable-create-id",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "`new-window -F` can return the new tab's stable ID",
      "evidence_ids": ["cli.stable-create-id"]
    },
    {
      "id": "workspace.locale-consistency",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "built-in control labels come from one declared English locale",
      "evidence_ids": ["ux.locale-consistency"]
    },
    {
      "id": "workspace.semantic-window-control",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "semantic actions control window state and client size without corrupting the PTY grid",
      "evidence_ids": ["ux.semantic-window-control"]
    },
    {
      "id": "runtime.remain-on-exit",
      "protocol_feature": "remain_on_exit",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "exited process retains its final screen and exit code",
      "evidence_ids": ["cli.remain-on-exit"]
    },
    {
      "id": "workspace.live-close-confirmation",
      "protocol_feature": "live_close_confirmation",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "explicit confirmation before closing a live process",
      "evidence_ids": ["ux.live-close-confirmation"]
    },
    {
      "id": "compat.rmux-status-click-bridge",
      "protocol_feature": "rmux_status_click_bridge",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "unit-source-partial",
      "prd": "RMUX status active-marker parsing and clickable window labels",
      "evidence_ids": ["unit.rmux-status-parser"]
    },
    {
      "id": "control.semantic-ui-automation",
      "protocol_feature": "semantic_ui_automation",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "semantic focus and UI actions",
      "evidence_ids": ["ux.semantic-ui-automation"]
    },
    {
      "id": "workspace.hierarchical-tabs",
      "protocol_feature": "hierarchical_tabs",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "tabs form a visible parent/child tree",
      "evidence_ids": ["ux.hierarchical-tabs"]
    },
    {
      "id": "workspace.persistence",
      "protocol_feature": "persistent_workspace",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "normal application close preserves the tab tree and active tab",
      "evidence_ids": ["ux.persistent-workspace"]
    },
    {
      "id": "fleet.tab-environment",
      "protocol_feature": "tab_environment",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "per-tab child environment injection",
      "evidence_ids": ["fleet.tab-environment"]
    },
    {
      "id": "fleet.codex-launcher",
      "protocol_feature": "codex_launcher",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "launch Codex agent tabs",
      "evidence_ids": ["fleet.codex-launcher"]
    },
    {
      "id": "compat.mux-frontend",
      "protocol_feature": "mux_frontend",
      "kind": "architecture",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "tmux/RMUX-compatible fleet control entry point",
      "evidence_ids": ["fleet.mux-frontend"]
    },
    {
      "id": "control.instance-discovery",
      "protocol_feature": "instance_discovery",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "registered multi-instance discovery",
      "evidence_ids": ["fleet.instance-discovery"]
    },
    {
      "id": "control.observable-events",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "bounded event reads and waits follow a snapshot epoch and sequence",
      "evidence_ids": ["cli.observable-events"]
    },
    {
      "id": "scripting.rhai-pure",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "pure Rhai run, eval, check, and API discovery execute in a sidecar",
      "evidence_ids": ["script.rhai-pure"]
    },
    {
      "id": "scripting.rhai-observe",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "observe Rhai receives one brokered immutable UI snapshot",
      "evidence_ids": ["script.rhai-observe"]
    },
    {
      "id": "scripting.rhai-deny-budget",
      "kind": "behavior",
      "status": "shipped",
      "evidence_mode": "black-box",
      "prd": "Rhai denies ambient mutation authority and enforces operation budgets",
      "evidence_ids": ["script.rhai-deny-budget"]
    },
    {
      "id": "scripting.rust-host-rhai-language",
      "kind": "decision",
      "status": "accepted",
      "evidence_mode": "decision",
      "prd": "design choice: Rust (`.rs`) implements the host",
      "evidence_ids": []
    }
  ]
}
-->

## Product tree

- AgenTerm
  - Terminal runtime
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
    - Professional interaction follow-ups informed by the reviewed PuTTY
      terminal model
      - [ ] application-requested raw mouse reporting wins by default while
        Shift provides a documented local-selection override
      - [ ] dragging a selection beyond the viewport auto-scrolls at a bounded
        rate and capture loss cancels the unfinished gesture cleanly
      - [ ] double-click word, triple-click line, and optional rectangular
        selection use terminal-cell rather than pixel semantics
      - [ ] terminal paste reads the clipboard off the GUI thread, normalizes
        newlines, filters unsafe controls, and honors bracketed-paste mode
    - [x] dirty-frame rendering and GDI double buffering
    - [x] GUI shell appears before the initial ConPTY/cmd process is ready
    - [x] initial terminal loads asynchronously with visible starting feedback
    - [x] exited process retains its final screen and exit code
    - [~] robust CJK double-cell layout; broader visual regression is needed
    - [ ] sustained high-throughput and long-output performance qualification
  - Executable family
    - [x] `agenterm.exe`: Windows-subsystem GUI, PTY owner, workspace authority,
      renderer, and IPC server
    - [x] `agentermctl.exe`: native AgenTerm observation and automation client
    - [x] `agenterm-mux.exe`: tmux/RMUX-compatible fleet control entry point
    - [x] `agenterm-script.exe`: optional one-invocation Rhai scripting worker
      for the v0.1.5 safe scripting contract
    - [ ] `agenterm-mcp.exe`: MCP server/client and agentic orchestration
      sidecar
    - [ ] `agenterm-ai.exe`: CPU-first lightweight specialized-intelligence
      sidecar
    - [ ] `agenterm-llm-gateway.exe`: governed local LLM forwarding and routing
      sidecar
    - [ ] `agenterm-bash.exe`: AgenTerm-owned default Bash entry point
    - [ ] `agenterm-ssh.exe`: AgenTerm-owned SSH entry point
    - [ ] `agenterm-curl.exe`: AgenTerm-owned HTTP transfer entry point
    - [ ] `agenterm-sqlite.exe`: AgenTerm-owned SQLite entry point
    - [ ] `agenterm-softmgr.exe`: signed optional-component lifecycle manager
    - [ ] an AgenTerm-owned executable means a stable product contract,
      discovery, diagnostics, policy, and fleet integration; it does not imply
      rewriting mature Bash, SSH, HTTP, or SQLite protocol engines
    - [x] all control frontends reuse shared request/target/format libraries;
      they do not duplicate GUI state or start a second workspace authority
    - [~] each binary has independent release size reporting and an enforced
      budget (4 MiB GUI, 2 MiB per CLI); per-binary startup reporting remains
      planned, and adding a frontend must not inflate `agenterm.exe`
  - Default shell (`agenterm-bash.exe`)
    - Product contract
      - [ ] stable AgenTerm-owned executable path, terminal integration, error
        messages, version output, and backend discovery
      - [ ] backed by a real Bash runtime; AgenTerm will not label a partial
        home-grown parser as Bash
      - [ ] remains usable outside AgenTerm as a normal console executable
      - [ ] inside a tab receives scoped `AGENTERM_IPC_ADDRESS`, stable tab ID,
        session name, and workspace metadata without embedding credentials
      - [ ] becomes the new-tab default only after the clean-machine acceptance
        gate passes; `cmd.exe` remains the honest fallback before that point
    - Runtime strategy gate
      - [ ] compare an installed-runtime resolver, a redistributable minimal
        Bash bundle, and a native compatibility implementation for license,
        fresh-machine reliability, process model, startup, update size, CJK,
        Ctrl-C/signals, path translation, and security
      - [ ] prefer a small launcher plus verified real Bash distribution unless
        measurements show that deployment or process behavior is unacceptable
      - [ ] runtime resolution is explicit and inspectable through
        `agenterm-bash --runtime-info`; never silently substitute `cmd.exe`
      - [ ] runtime installation/update is checksum-verified, version-pinned,
        transactional, and separate from GUI startup
    - Compatibility acceptance
      - [ ] interactive editing, history, completion, UTF-8/CJK, resize,
        bracketed paste, Ctrl-C/Ctrl-D, and correct exit status
      - [ ] quoting, variables, functions, command substitution, pipelines,
        redirection, conditionals, loops, traps, and representative `.sh` files
        execute in the selected real Bash runtime
      - [ ] Windows path and executable launching rules are documented and
        tested without pretending POSIX and Win32 paths are identical
      - [ ] shell exit leaves the AgenTerm tab visible and explicitly closable
  - Optional component lifecycle (`agenterm-softmgr.exe`)
    - Product boundary
      - [ ] `agenterm.exe` never downloads, updates, or resolves packages during
        startup; optional network and installation work stays in an explicit
        console sidecar
      - [ ] Bash, SSH, HTTP, SQLite, MCP, scripting runtimes, and future tools
        remain independently versioned sidecars so capability growth does not
        inflate GUI size or first-window latency
      - [ ] the GUI consumes only a small installed-component manifest and
        reports missing or incompatible components without silently fetching
        them
    - Supply chain and transaction
      - [ ] a signed, versioned manifest declares platform, component version,
        URLs, byte size, SHA-256, signer/key identity, dependencies, and minimum
        AgenTerm protocol/API version
      - [ ] downloads use a staging directory, bounded size/time, signature and
        hash verification, and no execution before verification
      - [ ] install and update use same-volume atomic promotion; interrupted or
        failed activation preserves the last known-good version
      - [ ] rollback, repair, inventory, provenance, and garbage collection are
        explicit commands with machine-readable results
      - [ ] user-scoped installation is the default; elevation is never
        implicit, and PATH or file-association mutation requires explicit
        consent
    - Verification
      - [ ] black-box fixtures cover clean install, offline cache, corrupt
        archive, bad signature, incompatible manifest, interrupted promotion,
        rollback, concurrent invocation, and locked executable behavior
      - [ ] release metadata and size/startup gates report each sidecar
        independently; an optional component is not counted as GUI capability
        until its integration and failure-isolation tests pass
  - Fleet multiplexer (`agenterm-mux.exe`)
    - Architecture
      - [x] thin console frontend over AgenTerm IPC; `agenterm.exe` remains the
        only server and PTY/workspace owner
      - [x] automatically discovers the live AgenTerm instance from the tab
        environment, with explicit `--address` and `--session` overrides
      - [x] server bind, inherited addresses, and explicit client overrides are
        centrally restricted to numeric loopback IPs
      - [x] if no server exists, server-start behavior is explicit and mirrors
        supported tmux/RMUX semantics without creating a hidden second fleet
      - [~] shared parser and command catalog with `agentermctl`; mux aliases map
        to typed internal operations, not shelling out to `agentermctl.exe`
    - Compatibility surface
      - [x] sessions map to AgenTerm workspaces and windows map to tree tabs;
        one tab remains one pane until split panes are genuinely implemented
      - [~] support tmux/RMUX aliases, `-t` targets, `-F` formats, stable IDs,
        exit codes, stdout/stderr separation, and unsupported-command errors
      - [x] expose shipped native AgenTerm tree, composer, screenshot, wait, and
        agent extensions under an unambiguous namespace
      - [ ] expose future scripting commands through that same native namespace
        without masquerading as tmux features
      - [x] `agentermctl` remains the richer machine API; `agenterm-mux` is the
        compatibility UX and migration path
    - Conformance
      - [x] machine-readable compatibility matrix generated from the command
        registry and exposed through `agenterm-mux compatibility`
      - [~] black-box argv/output/exit-code corpus runs against AgenTerm and,
        where practical, reference tmux and RMUX versions
      - [x] behavioral differences are explicit, especially persistence,
        process ownership, confirmation, single-pane tabs, and server lifetime
      - [ ] function-key, mouse, nested RMUX, and Byobu-style flows remain in
        the public regression suite
  - Human workspace
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
  - Agent control plane
    - Observation
      - [x] stable active tab `id:name`
      - [x] text capture, raw escaped output, styled cell dumps
      - [x] JSON pane, tab, focus, modal, layout, and protocol snapshots
      - [x] whole-window and selected-pane PNG screenshots
      - [ ] non-intrusive bounded transcript capture by stable tab ID, with
        explicit visible-vs-scrollback range, truncation metadata, and no
        viewport mutation; this requires a versioned protocol addition rather
        than automating `scroll-pane`
      - [ ] incremental output sequence and event stream
    - Action
      - [x] create, select, rename, annotate, and close tabs
      - [x] launch Codex agent tabs with stable tab/session/IPC context and
        optional tab-scoped proxy settings
      - [x] send keys and terminal mouse events
      - [x] scroll a selected terminal viewport by rows, pages, top, or bottom
        while keeping screenshots and capture aligned with the human view
      - [x] read, replace, and submit composer content
      - [x] semantic focus and UI actions
      - [x] deterministic waits for output, composer completion, dead state,
        active tab, and focus
      - [ ] direct deterministic wait predicates for modal kind and target
      - [ ] broadcast input and synchronized panes
    - Protocol
      - [x] loopback-only newline-delimited JSON IPC
      - [x] feature discovery through `protocol-info`
      - [x] registered multi-instance discovery with PID, address, version,
        session, workspace, tab count, active tab, and liveness
      - [x] explicit `--address` targeting and opt-in stale-record pruning
      - [x] bounded discovery probes and clean-machine-safe explicit-address
        GUI autostart that returns as soon as IPC becomes ready
      - [x] explicit errors for unsupported operations
      - [ ] named-pipe transport and stable event subscription
  - Observable Fleet event core (v0.1.5 minimum slice)
    - Contract
      - [x] assign a new server `epoch` on every workspace-authority start so a
        consumer can distinguish restart from an in-process event gap
      - [x] assign one strictly increasing `sequence` within an epoch after
        each committed observable state transition in the minimum event schema
      - [x] expose a bounded in-memory event journal whose envelopes contain
        schema version, epoch, sequence, event kind, stable tab ID when
        applicable, and a minimal typed payload
      - [x] cover only tab create/close/select/rename/note/parent/state,
        composer-draft/submit state, terminal output advancement, viewport, and
        workspace save/shutdown events in the first schema
      - [x] snapshot responses include the current epoch and sequence so clients
        can atomically establish a baseline before following the journal
    - Read and wait slice
      - [x] add one public bounded read operation for events after
        `(epoch, sequence)`, with explicit gap/restart errors rather than silent
        loss or replay ambiguity
      - [x] add one deterministic wait operation over the same journal for a
        small allowlisted predicate set and deadline; cancellation or timeout
        cannot block the GUI thread
      - [x] bounded event reads and waits follow a snapshot epoch and sequence
      - [x] journal mutation happens only after the corresponding state change
        commits; wired event kinds are snapshot-verifiable
      - [~] black-box tests prove read/wait ordering, timeout, and
        snapshot-to-follow handoff; restart, gap, and concurrent-reader
        black-box coverage remains
    - Explicitly deferred beyond the minimum slice
      - [ ] durable replay across process restarts, remote/network transport,
        arbitrary user predicates, unbounded terminal byte logging, delivery
        acknowledgements, and exactly-once side effects
      - [ ] Rhai event handlers, MCP subscriptions, brain/flow scheduling,
        status-provider events, and multi-agent orchestration remain consumers
        of this core, not shortcuts around its acceptance gate
  - Self-hosted development loop
    - [x] a running AgenTerm can build and stage the next AgenTerm binaries
      without first terminating the development fleet
    - [ ] surface staged-version availability and offer an explicit restart
      action without destroying persisted tabs
  - Rust host + Rhai scripting
    - [x] pure Rhai run, eval, check, and API discovery execute in a sidecar
    - [x] observe Rhai receives one brokered immutable UI snapshot
    - [x] Rhai denies ambient mutation authority and enforces operation budgets
    - Product boundary
      - [x] design choice: Rust (`.rs`) implements the host, capability checks,
        and stable APIs; Rhai (`.rhai`) is the user scripting language
      - [x] scripting executes in an optional sidecar/worker; the GUI owns no
        Rhai engine and communicates only through versioned typed contracts
      - [ ] scripts automate the public tab/workspace control plane; they do
        not receive direct Win32, PTY, or mutable GUI-state access
      - [ ] execution runs off the window thread and must never delay first
        window display or terminal painting
      - [ ] a script failure is isolated from the GUI, IPC server, terminal
        readers, and workspace persistence
    - Runtime architecture
      - [ ] optional `scripting` feature lives in a sidecar crate/binary behind
        a narrow `ScriptHost` trait; the GUI binary and state machine remain
        independent of Rhai types
      - [ ] one engine factory builds each invocation from an immutable
        capability set, source label, API version, and resource budget
      - [ ] lazy initialization on first use; no engine construction or script
        directory scan on GUI startup
      - [ ] bounded compiled-AST cache keyed by source fingerprint, API version,
        and capability profile
      - [ ] versioned embedded prelude for shared helpers; module loading stays
        disabled until real use cases justify its size and resolver surface
      - [ ] user scripts default to `%LOCALAPPDATA%\AgenTerm\scripts`; one-shot
        execution also accepts an explicit path or stdin
      - [ ] source loading and runtime authority are separate: selecting a file
        never grants filesystem, process, or terminal-write access
    - Capability profiles
      - [ ] `pure`: JSON, bounded computation, arguments, and stdout
      - [ ] `observe`: `pure` plus tab tree/list, active tab, pane capture,
        settings read, workspace info, and status snapshots
      - [ ] `control`: `observe` plus create/select/rename/reparent/close,
        composer operations, send keys/mouse, and deterministic waits
      - [ ] separately scoped `fs.read`, `fs.write`, `env.read`, and
        `proc.exec`; roots, variable names, and executables are allowlists
      - [ ] scripts cannot grant themselves capabilities
      - [ ] destructive actions preserve native live-process confirmation
        unless an explicit, visible automation policy authorizes them
    - AgenTerm host API
      - [ ] typed maps/arrays and stable error codes; scripts never parse
        human-facing CLI output
      - [ ] observation: `tabs()`, `active_tab()`, `capture(tab)`,
        `workspace_info()`, `settings()`, and `ui_snapshot()`
      - [ ] action: `new_tab()`, `select_tab()`, `set_parent()`, `set_name()`,
        `set_note()`, `set_composer()`, `send_composer()`, `send_keys()`,
        `close_tab()`, and bounded `wait_*()`
      - [ ] status providers return structured segments; the status bar owns
        layout, refresh, truncation, and error presentation
      - [ ] API catalog and version are discoverable without script execution
    - Resource and security envelope
      - [ ] cap source bytes, operations, call depth, collection sizes, output,
        wall-clock duration, and concurrent invocations
      - [ ] timeout has a stable error/exit code and never blocks the GUI
      - [ ] process execution accepts executable plus argv, never an implicit
        shell string; timeout and output caps are mandatory
      - [ ] canonical root containment and reparse-point/symlink rejection for
        scoped Windows file access
      - [ ] audit source, requested/granted capabilities, duration, result
        class, and denial reason without recording content or secrets
    - Future safe-scripting sidecar contract
      - Process boundary
        - [ ] one fresh worker process executes one `run`, `eval`, or `check`
          invocation; the first delivery has no persistent daemon, background handler,
          module resolver, or cross-invocation mutable state
        - [ ] the launcher places the worker in a kill-on-close Windows Job
          Object and owns its deadline, cancellation, stdout, stderr, and final
          exit status; a crashed or killed worker cannot affect the GUI server
        - [ ] a versioned invocation envelope and result envelope travel over
          inherited anonymous pipes; source text, arguments, capabilities, and
          secrets are not placed in the process command line
        - [ ] the worker never connects to GUI IPC directly. A host broker
          validates the profile and supplies only immutable typed inputs over
          the invocation channel
      - Initial profiles
        - [x] `pure` exposes bounded Rhai evaluation, JSON-compatible values,
          invocation arguments, and captured stdout only; it has no clock,
          environment, filesystem, process, network, terminal, or fleet access
        - [ ] `observe` adds typed workspace metadata, tab-tree snapshots, pane
          snapshots, settings/status snapshots, and bounded Observable Fleet
          journal reads/waits; every input carries schema version, epoch, and
          sequence where applicable
        - [ ] an observe invocation starts from one snapshot baseline and then
          consumes only validated event envelopes after that sequence; restart
          and journal-gap errors are delivered as typed errors, never hidden by
          an implicit resnapshot
        - [ ] `control`, filesystem, environment, process execution, network,
          package loading, event handlers, and status providers remain outside
          the first-delivery acceptance gate
      - Public commands and discovery
        - [x] `script run FILE.rhai|- [-- ARGS...]` loads a file or stdin and
          returns script stdout separately from diagnostics
        - [x] `script eval EXPRESSION` evaluates one explicit expression under
          the selected profile without loading user modules
        - [x] `script check FILE.rhai` parses and validates API names,
          capability requirements, and static limits without executing code or
          contacting a live AgenTerm server
        - [x] `script api --json` reports host API/schema versions, profiles,
          functions, typed parameters/results/errors, limits, and availability
          without starting a Rhai engine or AgenTerm GUI
        - [ ] every command accepts an explicit profile and bounded overrides;
          unknown API versions, profiles, capabilities, or options fail closed
          with stable documented exit codes
      - Budgets, cancellation, and audit
        - [ ] versioned nonzero defaults and immutable hard ceilings cover
          source bytes, operations, call depth, collection/string size, output
          bytes, wall time, journal events, wait duration, and worker count;
          `script api --json` exposes the effective values
        - [ ] exceeding any budget produces a typed limit result and bounded
          diagnostics; output truncation is explicit and cannot produce a
          successful status
        - [ ] cancellation first signals the worker cooperatively, then
          terminates its Job Object after a bounded grace period; CLI
          interruption, timeout, and parent exit cannot orphan a worker
        - [ ] one append-only audit record captures invocation ID, timestamp,
          source fingerprint/label, API version, profile, requested/effective
          budgets, duration, exit class, cancellation, and denials, but not
          source, arguments, pane contents, environment values, or stdout
      - Public black-box acceptance
        - [ ] tests invoke only released `agentermctl script` commands and
          validate stdout/stderr separation, JSON discovery, stable exit codes,
          file/stdin/eval/check behavior, and clean-machine missing-sidecar
          diagnostics
        - [ ] pure-profile fixtures prove denied filesystem, environment,
          process, network, fleet, and clock access; observe fixtures prove
          typed snapshots, ordered events, epoch restart, bounded-history gap,
          wait timeout, and absence of mutation APIs
        - [ ] adversarial fixtures cover parse/runtime errors, operation and
          output exhaustion, oversized values, cancellation, worker crash,
          parent exit, concurrent worker ceiling, malformed envelopes, and
          unsupported API versions without GUI latency or workspace damage
        - [ ] acceptance records GUI/CLI/worker sizes and first-window timing,
          verifies no Rhai code loads during normal GUI startup, and leaves no
          worker or temporary source behind after every result class
    - Extension surfaces
      - [ ] phase 1 (after the Observable Fleet event core): one-shot
        pure/observe run/eval/check and API
        discovery under the minimum sidecar contract
      - [ ] phase 2: status providers with timeout, last-good value, and visible
        degraded state
      - [ ] phase 3: named commands callable by people, agents, and IPC
      - [ ] phase 4: opt-in tab/process/workspace events with bounded queues and
        no re-entrant GUI mutation
      - [ ] no network client, package manager, arbitrary import, or general
        async runtime without a concrete reviewed product use case
    - Verification and delivery
      - [ ] P0 records current approximately 714 KiB optimized GUI size,
        first-window timing, and no-script behavior
      - [ ] P1 adds the pure engine, budgets, CLI golden tests, and measured
        size/startup A/B results
      - [ ] P2 adds observation APIs before control APIs; each has
        deny/success/scope-boundary black-box tests
      - [ ] P3 adds status-provider timeout, error, reload, truncation, and
        last-good-value tests
      - [ ] every phase keeps `agenterm.exe` below the 4 MiB release gate; a
        large dependency or Rhai feature must earn its measured cost
  - MCP and agentic orchestration (`agenterm-mcp.exe`)
    - Architecture boundary
      - [ ] run MCP server, MCP client connections, Rhai execution, and
        brain/flow scheduling in sidecar/worker processes; `agenterm.exe`
        remains the fleet authority, renderer, and PTY owner
      - [ ] use the public typed control plane plus Observable Fleet epoch,
        sequence, snapshot, journal, and wait contracts; no component reads or
        mutates GUI internals directly
      - [ ] a worker crash, blocked tool, malformed peer, or script budget
        failure cannot stall terminal rendering, corrupt the workspace, or
        terminate tabs
      - [ ] server and client roles have separate capability profiles,
        credentials, connection allowlists, budgets, audit records, and
        lifecycle controls
    - Brain/flow model
      - [ ] `brain` owns bounded decision state and chooses declared tools;
        `flow` owns a persisted, inspectable graph of steps, waits, branches,
        retries, cancellation, and compensation
      - [ ] every run, node, tool call, tab, and child agent has a stable ID;
        transitions are observable and recover from a snapshot plus journal
        without assuming process continuity
      - [ ] MCP tools expose typed AgenTerm operations and return verifiable
        post-state; natural-language output is never the sole success signal
      - [ ] destructive fleet actions retain AgenTerm confirmation and policy
        boundaries, including when initiated by an MCP peer or Rhai flow
    - Delivery dependency
      - [ ] no event-driven MCP, Rhai handler, or brain/flow runtime ships before
        the Observable Fleet minimum slice passes ordering, gap, restart, wait,
        and GUI-isolation tests
      - [ ] begin with read-only inventory/snapshot resources and bounded waits,
        then add explicit control tools, then durable flows; MCP client
        federation and autonomous scheduling remain later gates
    - v0.1.9 first delivery: public read-only surface
      - Executable and discovery
        - [ ] `agenterm-mcp.exe --help`, `--version`, and
          `capabilities --json` work without starting a GUI or model runtime;
          capability output declares protocol/schema versions, transport,
          resources, tools, limits, and unavailable later-stage roles
        - [ ] `agenterm-mcp.exe serve --stdio` is the only first-delivery MCP
          transport; initialization negotiates a supported protocol version and
          publishes stable server identity without opening a network listener
        - [ ] an absent, stale, restarted, or incompatible AgenTerm server
          returns a typed MCP error with address/session diagnostics and never
          causes the sidecar to create a second workspace authority
      - Inventory and snapshots
        - [ ] `resources/list` advertises versioned read-only resources for
          instance inventory, workspace inventory, tab inventory, and one fleet
          snapshot; stable resource URIs do not encode mutable tab indexes or
          titles
        - [ ] `resources/read` returns the same stable IDs and observable state
          as the public AgenTerm control plane, plus schema version, server
          epoch, and snapshot sequence so a client can establish a verifiable
          event baseline
        - [ ] pane text or other content-bearing fields are absent by default;
          any future content snapshot requires an explicit observe capability,
          bounded output, and a resource distinct from metadata inventory
      - Bounded wait
        - [ ] `tools/list` exposes only one read-only `agenterm_wait` tool in
          the first delivery; no create, send, close, script, process, or
          filesystem tool is advertised
        - [ ] `agenterm_wait` accepts a snapshot epoch/sequence, one allowlisted
          predicate, and a bounded `timeout_ms`; success returns the matching
          event and new position, while restart, journal gap, cancellation, and
          deadline expiry remain distinct typed results
        - [ ] client disconnect, cancellation, or timeout releases capacity
          within a bounded grace period and cannot block the GUI IPC loop or
          another MCP client
      - Failure isolation and deferred roles
        - [ ] malformed JSON-RPC, oversized frames, a killed or hung sidecar,
          backend disconnect, and wait exhaustion cannot stall terminal output,
          mutate workspace state, close tabs, or terminate `agenterm.exe`
        - [ ] sidecar restart reconstructs read-only state from a fresh snapshot
          and epoch/sequence; it never claims uninterrupted subscription or
          process continuity
        - [ ] MCP client federation, network transport, subscriptions, control
          tools, Rhai tool execution, brain/flow, durable scheduling, and
          autonomous actions remain outside this first-delivery gate
  - Lightweight specialized intelligence (`agenterm-ai.exe`)
    - Product boundary
      - [ ] run inference in an optional CPU-first sidecar; `agenterm.exe`
        links no model runtime and performs no inference during startup or on
        the paint thread
      - [ ] consume versioned Observable Fleet events and typed feature windows
        by default; raw PTY content requires an explicit scoped capability and
        is never persisted, uploaded, or used for training by default
      - [ ] return confidence, abstention, model ID/version/hash, feature-schema
        version, explanation, source epoch/sequence, TTL, and fallback state
      - [ ] learned output is advisory: it may observe, rank, warn, or escalate
        risk, but cannot authorize or execute a high-risk fleet action
      - [ ] deterministic command-safety rules remain authoritative and a model
        can raise risk but never override a rule denial
    - Runtime and model lifecycle
      - [ ] isolate model execution in a worker with no network or terminal
        control capability and explicit CPU, memory, deadline, and concurrency
        budgets; failure immediately degrades to rules without affecting GUI
      - [ ] keep training, labeling, and evaluation outside the installed
        inference path; begin with shadow mode and human-confirmed labels
      - [ ] signed model packs declare feature schema, preprocessing,
        calibration and rejection thresholds, budgets, provenance/licenses,
        compatibility range, and fixed golden input/output vectors
      - [ ] `agenterm-softmgr.exe` installs, atomically activates, audits, and
        rolls back model and runtime components independently of the GUI
      - [ ] admit a model only after fixed Windows x64 CPU benchmarks cover
        artifact size, RSS, cold start, p95 latency, CPU load, accuracy,
        calibration, false alarms, failure isolation, and simpler baselines
    - Capability route
      - [ ] A0 rules and expert systems cover no-progress/stall detection,
        known error and exit patterns, command risk, event priority, resource
        thresholds, and deterministic degradation
      - [ ] A1 benchmark XGBoost, random forest, constrained SVM, and small MLP
        candidates in shadow mode for anomaly/error classification, resource
        warning, and event prioritization; model and runtime size are measured,
        not assumed from the algorithm name
      - [ ] A2 benchmark small GRU first and LSTM second for typed event rhythm,
        prolonged no-progress, resource trend, and context-exhaustion
        prediction; recurrent state is epoch-bound and resets on restart or gap
      - [ ] A3 keeps sub-million-parameter RWKV-small as research only:
        constant context-state memory does not include weights, vocabulary, or
        runtime, and it must beat simpler models on the Windows CPU baseline
      - [ ] A4 keeps Mamba-small as research only until a reproducible portable
        Windows CPU kernel and export path beat GRU and classic-ML baselines
      - [ ] GPU/NPU-required models, large Transformers, installed endpoint
        training, unsigned model hot-load, default raw-PTY collection, and
        automatic high-risk actions are out of scope
  - Local LLM gateway (`agenterm-llm-gateway.exe`)
    - Dependency and isolation
      - [ ] implementation begins only after Observable Fleet, Rhai capability
        policy, MCP typed tools, credential isolation, and audit contracts pass
        their gates
      - [ ] run as an optional loopback-authenticated sidecar, separate from the
        lightweight specialized-model worker and from GUI startup
      - [ ] keep provider credentials in an OS credential store, outside
        workspaces, tab environments, scripts, and child-process inheritance
      - [ ] prompt and response bodies are not logged by default; PTY content
        requires explicit scoped authorization, redaction, and bounded lifetime
    - Governed forwarding
      - [ ] support provider and local endpoints through destination allowlists,
        policy routing, per-workspace/tab/agent quotas, token and monetary
        budgets, deadlines, retry/idempotency, circuit breaking, health checks,
        streaming cancellation, and policy-controlled fallback
      - [ ] audit provider/model route, latency, actual or estimated token use,
        versioned price basis, cost, policy decision, and denial reason without
        recording credentials or content secrets
      - [ ] prefer provider-reported usage to estimates and reconcile retries so
        cancellation or duplicate attempts cannot silently hide cost
      - [ ] LLM text is never the sole proof of a successful fleet operation;
        MCP tools verify typed post-state through the AgenTerm control plane
  - Research provenance and clean-room boundary
    - [ ] `..\moltbaby` `bin/mux` and mapp `brain`/`flow` are research inputs
      only while their source files lack a definitive reusable license grant
    - [ ] AgenTerm may record public behavior, architectural lessons, test
      vectors created independently, and rejected approaches, but copies no
      source, comments, identifiers, documentation prose, or non-public fixture
      data from that repository
    - [ ] implementation starts from AgenTerm's PRD and public contracts in a
      clean-room pass; provenance is recorded per imported dependency or
      externally derived compatibility fixture
    - [ ] direct reuse requires an explicit compatible license and provenance
      review first; a placeholder or absent license is treated as no permission
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
        - `new-window|neww [-d] [-n name] [--parent target] [-F format]
          [command [args...]]`
        - [x] `new-window -F` can return the new tab's stable ID through
          `#{window_id}` while default numeric-index output remains compatible
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
        - [x] `split-window|splitw` returns an explicit unsupported error
    - AgenTerm extensions
      - Team tree
        - `list-tab-tree [-F format]`
        - `set-tab-parent -t child --parent parent|root`
        - `show-tab-parent [-t target]`
        - `ui-action new-child [-t parent]`
        - parent cycles fail explicitly
        - closing a parent promotes direct children to its parent
      - State and deterministic waits
        - `list-instances [--json] [--prune]`
        - [x] `server-list [--json] [--prune]` is an offline fleet-discovery
          alias over the same registered-instance records; it never autostarts
          a GUI and therefore provides the read-side companion to `kill-server`
        - global `--address HOST:PORT` targets a discovered server explicitly
        - `active-window|active-tab [-F format]`
        - `inspect|pane-snapshot [-t target]`
        - `dump-cells [-t target] [-r row]`
        - `capture-pane --raw-escaped [-t target]`
        - `scroll-pane [-t target]
          up|down|page-up|page-down|top|bottom [rows]`
        - `read-events --epoch EPOCH --after SEQUENCE [--limit COUNT]`
        - `wait-events --epoch EPOCH --after SEQUENCE --kind KIND
          [--tab @ID] [--timeout-ms MS]`
        - `wait-pane|expect-pane [-t target]
          [--contains text|--dead|--submit-complete]
          [--timeout-ms ms]`
        - `ui-snapshot`, `protocol-info`
        - `workspace-info`, `save-workspace`, `shutdown`
        - [ ] explore a coherent `server-*` lifecycle namespace only as aliases
          over typed discovery, health, start, shutdown, and destructive-kill
          operations; do not create a second server registry or weaken the
          current `kill-server` workspace-destruction contract
        - [ ] `shutdown --no-save` escape hatch for instances whose workspace
          destination has become unwritable
        - `wait-ui [--active @id] [--focus surface] [-t target
          --tab-state running|dead] [--timeout-ms ms]`
      - Safe scripting
        - `script api [--json]`
        - `script check FILE|- [--profile pure|observe]`
        - `script eval EXPRESSION [--profile pure|observe]`
        - `script run FILE|- [--profile pure|observe] [-- ARGS...]`
      - Composer and tab metadata
        - `show-composer [-t target]`
        - `set-composer [-t target] text|--stdin|--file path`
        - `send-composer [-t target]`
        - `set-tab-note [-t target] text`, `show-tab-note [-t target]`
      - Semantic UI control
        - `focus terminal|composer|sidebar [-t target]`
        - `ui-action new-tab|new-child|edit-tab|toggle-tree|select-tab|close-tab|confirm|cancel|
          composer-send|copy-selection|open-settings|window-minimize|
          window-maximize|window-restore [-t target]`
        - `ui-action window-resize --width PX --height PX`
        - [x] semantic actions control window state and client size without corrupting the PTY grid
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
      - Planned scripting
        - `script run [OPTIONS] FILE.rhai|- [--] [ARGS...]`
        - `script eval [OPTIONS] EXPRESSION`
        - `script check FILE.rhai`
        - `script api [--json]`
        - `script cache clear|info`
        - options include `--profile pure|observe|control`, repeated `--cap`,
          scoped resources, `--timeout-ms`, and `--max-output`
        - exit codes distinguish script failure, configuration/capability
          error, timeout, and host/API failure
      - AI fleet launch
        - `new-agent [-d] [-n name] [--parent target] [--program executable]
          [-e NAME=VALUE] [--proxy URL] [--no-proxy hosts] [--yolo]
          [-- codex args...]`
        - `new-window` and `new-session` also accept repeated `-e NAME=VALUE`
        - injected values live only for the child process; snapshots expose
          names, and workspace persistence stores neither names nor values
        - every child receives reserved `AGENTERM_IPC_ADDRESS`,
          `AGENTERM_TAB_ID`, `AGENTERM_SESSION`, and
          `AGENTERM_WORKSPACE_PATH`
        - the default launcher uses the system `codex` command through
          `cmd.exe` so standard npm `.cmd` installations work in ConPTY;
          `--program` is the explicit direct-executable override
        - `--yolo` explicitly maps to Codex
          `--dangerously-bypass-approvals-and-sandbox`; the default remains safe
  - tmux/RMUX compatibility
    - [x] common session/window command names, aliases, targets, and formats
    - [x] function-key byte sequences including Byobu F2/F3/F4/F6/F8
    - [x] RMUX status active-marker parsing and clickable window labels
    - [x] Windows native mouse-input bridge for RMUX 0.9.1
    - [x] initial ConPTY grid sizing keeps RMUX status at the bottom
    - [x] minimizing the GUI does not resize PTYs to the iconic rectangle
    - [ ] split panes and layout commands
    - [ ] full behavioral conformance corpus beyond the shipped
      registry-generated command compatibility matrix
  - Delivery and quality
    - [x] fast incremental developer build under ignored local `dist/`
    - [x] release mode and `agenterm.json` build metadata
    - [x] size-optimized release profile and enforced 4 MiB GUI plus 2 MiB
      per-control-CLI budgets
    - [x] GUI `agenterm.exe` has no startup console flash
    - [x] console `agentermctl.exe` preserves CLI output and exit codes
    - [x] startup regression requires a main window within one second locally
    - [x] version-tagged GitHub Release automation for all three EXEs, metadata,
      and ZIP
    - [x] release automation publishes `agenterm-mux.exe` after its acceptance
      gate
    - [ ] `agenterm-bash.exe` remains gated and unpublished
    - [x] release metadata reports version, build time, commit, enabled
      features, and SHA-256 for every executable/runtime component
    - [x] unit tests for command parsing, protocol, settings, and RMUX status
    - [x] PRD alignment lint keeps the public command registry, protocol feature
      flags, mux compatibility output, and declared evidence synchronized
    - [~] stable capability/evidence ID contract covers protocol features and
      critical terminal input behavior; rendering, CJK, performance, and the
      remaining shipped leaves still need registered evidence
    - [x] CLI and semantic UX smoke tests through public interfaces
    - [x] one-command fmt, Clippy, test, build, and smoke regression
    - [x] release CI runs the isolated public CLI and fleet smoke suites before
      packaging, even when the redundant GUI smoke suites are skipped
    - Scripting public-interface evidence gate
      - Rhai black-box evidence
        - [x] `tests/script_smoke.ps1` drives only public `script check`,
          `script eval`, `script run`, and `script api --json` commands; no test
          links the Rhai host or invokes an internal worker API
        - [ ] fixtures prove deterministic `pure` output, an `observe` snapshot
          and journal position matching `agentermctl`, denied mutation and
          ambient authority, stable parse/runtime/limit exit classes, timeout,
          output truncation, worker crash, and subsequent recovery
        - [ ] every Rhai timeout/crash case includes an independent public GUI,
          PTY, and workspace-health assertion; a sidecar error alone is not
          accepted as isolation evidence
      - PRD-command-test alignment
        - [x] evidence IDs `script.rhai-pure`, `script.rhai-observe`, and
          `script.rhai-deny-budget` are registered with post-assertion emissions
        - [x] changing a shipped script command, capability, API entry, or
          evidence ID must atomically update the public command/API catalog,
          PRD `[x]` leaf, black-box assertion, and alignment contract
        - [~] `tests/prd_alignment.ps1` compares the public command/evidence
          catalog with the PRD contract; exact Rhai API-field comparison remains
          planned
        - [x] `check.ps1` runs `tests/script_smoke.ps1` before the
          safe-scripting release tag
    - Autonomous human UX dogfood
      - Latest reproducible findings
        - [ ] P1 target ambiguity: `agentermctl new-window -d -n "Research
          Team"` prints mutable index `1` rather than stable ID `@2`; feeding
          that result to `--parent` or `wait-ui --active` can address a
          different tab. Acceptance: creation JSON or a documented format
          returns the stable ID, and a black-box test uses that exact value for
          create-child, select, wait, rename, and close after indexes shift
        - [ ] P1 settings isolation: distinct `AGENTERM_IPC_ADDRESS` and
          `AGENTERM_WORKSPACE_PATH` still share
          `%LOCALAPPDATA%\AgenTerm\settings.json`, so an isolated font test
          changes every running instance. Acceptance: an explicit settings-path
          override scopes read/write/restart tests and leaves the user's file
          byte-identical
        - [ ] P1 window-control gap: `ui-snapshot` observes minimized state and
          geometry, but the public semantic interface cannot resize, minimize,
          maximize, or restore a window; the 2026-07-27 run required Win32
          automation. Acceptance: public actions drive each state, `wait-ui`
          verifies it, minimize preserves the last PTY grid, and restore/resize
          produce the expected new grid
        - [ ] P2 active-tree readability: the three 24-pixel action targets
          reduce the selected child row's note to `child agent wor...` at the
          default 250-pixel sidebar. Acceptance: screenshot fixtures prove
          name/note and actions remain distinguishable at default width, deep
          nesting, long CJK text, and 125%/150% display scaling
        - [ ] P3 language consistency: the default English surface mixes
          `Settings`, `New`, and `Compose input` with `发送`. Acceptance: one
          locale source selects all visible labels and snapshots contain no
          unintended mixed-language controls
      - [ ] add a public-interface dogfood gate that starts the release artifact
        with isolated IPC, workspace, settings, session, and evidence paths;
        fixed sleeps and private state hooks are forbidden
      - [ ] drive first start, root/child creation, stable-ID targeting,
        rename/note, switching, composer edit/send, keyboard/Backspace, terminal
        mouse, viewport scroll, resize/minimize/restore, exit retention,
        dead/live explicit close, normal shutdown/restart recovery, and font
        settings in one deterministic journey
      - [ ] after every transition save `ui-snapshot`, relevant
        pane/workspace/settings JSON, command/exit result, and whole-window or
        pane PNG under one timestamped evidence directory with build metadata
      - [ ] post-assert state rather than command success alone: composer text
        executes once, scroll offsets and PNG viewport agree, dead exit code and
        final screen remain, live close exposes confirmation, tree/name/note/
        active ID survive restart, and settings restore after the test
      - [ ] always shut down the isolated instance, restore any external state,
        detect orphan workers/windows, and fail release qualification for any
        P0/P1 finding; P2/P3 findings require an owned planned leaf and retained
        reproduction evidence
      - [ ] the 2026-07-27 v0.1.3 baseline evidence is under
        `D:\tmp\agenterm-dogfood-v014\`: `01-first`, `02b-tree-corrected`,
        `03/04-composer`, `05/06/07-scroll`, `08/09/10-window-state`,
        `12-settings`, `13-exit-retained`, `14-live-close-modal`, and
        `16/17-restart` JSON/PNG pairs
    - [ ] automated terminal input/resize/ANSI/CJK/long-output matrix
    - [ ] installer, updater, stable PATH location, and signed releases
  - Focused super-fleet roadmap
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
          word/line/rectangular selection, and terminal paste retain the
          professional-terminal follow-up gates above
    - [ ] v0.1.6 Observable Fleet expansion: complete transition coverage and
      restart/gap/concurrent-reader black-box tests before event-driven
      extensions
    - [ ] M0 boundaries and baselines: extract typed control operations, record
      per-binary size/startup, freeze the compatibility corpus, and define the
      sidecar protocol boundary
    - [x] M1 fleet CLI: ship `agenterm-mux.exe` from the existing supported
      tmux/RMUX command surface and generated compatibility matrix
    - [ ] M2 shell gate: prototype `agenterm-bash.exe`, select and license the
      real Bash runtime strategy, then pass clean-machine terminal tests
    - [ ] M3 optional components: ship signed-manifest inventory/install/update/
      rollback foundations and independently gated SSH, HTTP, and SQLite
      sidecars without adding GUI network authority
    - [ ] M4 / v0.1.7 safe scripting: ship sidecar Rhai pure/observe profiles,
      run/eval/check, API discovery, budgets, and audit records
    - [ ] M5 / v0.1.8 dynamic bridge: script-backed status segments and named
      commands
    - [ ] M6 / v0.1.9 controlled agentic bridge: ship MCP read-only resources,
      then explicit control tools, Rhai control, brain/flow orchestration, and
      agent/token status without weakening close safety
    - [ ] M7 lightweight intelligence: rules first, then benchmark-gated
      classic ML, then small GRU/LSTM candidates in isolated CPU workers
    - [ ] M8 governed LLM gateway: local forwarding, routing, quota, audit,
      cost, credential isolation, and redaction after scripting/MCP/event-core
      gates
    - [ ] M9 experimental sequence frontier: RWKV-small and Mamba-small advance
      only when portable Windows CPU evidence beats simpler models

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
