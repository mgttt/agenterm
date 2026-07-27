# AgenTerm product tree

Status: active development  
Platform: Windows  
Current default shell: the real system `cmd.exe`.
Planned default shell: `agenterm-bash.exe` after its clean-machine gate passes

AgenTerm is a native super-fleet terminal for people and AI agents. Its window
is the bridge, the tab tree is the fleet, shells are crew workspaces, and the
local control and scripting planes let people and agents observe and steer the
same state. Human interaction and local CLI automation operate on the same
tabs, PTYs, drafts, settings, and observable state. A process exiting never
silently destroys its tab.

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
  - Executable family
    - [x] `agenterm.exe`: Windows-subsystem GUI, PTY owner, workspace authority,
      renderer, and IPC server
    - [x] `agentermctl.exe`: native AgenTerm observation and automation client
    - [ ] `agenterm-bash.exe`: AgenTerm-owned default Bash entry point
    - [x] `agenterm-mux.exe`: tmux/RMUX-compatible fleet control entry point
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
      - [x] expose native AgenTerm extensions under an unambiguous namespace so
        tree, composer, screenshot, wait, agent, and scripting commands do not
        masquerade as tmux features
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
      - [x] submit text and Enter as distinct PTY events so interactive TUIs
        such as Codex execute the draft instead of leaving it in their editor
      - [ ] automated interactive-TUI fixture that rejects batched paste+Enter
        without requiring a networked Codex session
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
    - [x] per-tab child environment injection with ephemeral proxy convenience;
      values are never persisted to the workspace
  - Agent control plane
    - Observation
      - [x] stable active tab `id:name`
      - [x] text capture, raw escaped output, styled cell dumps
      - [x] JSON pane, tab, focus, modal, layout, and protocol snapshots
      - [x] whole-window and selected-pane PNG screenshots
      - [ ] incremental output sequence and event stream
    - Action
      - [x] create, select, rename, annotate, and close tabs
      - [x] launch Codex agent tabs with stable tab/session/IPC context and
        optional tab-scoped proxy settings
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
  - Rust host + Rhai scripting
    - Product boundary
      - [x] design choice: Rust (`.rs`) implements the host, capability checks,
        and stable APIs; Rhai (`.rhai`) is the user scripting language
      - [ ] scripts automate the public tab/workspace control plane; they do
        not receive direct Win32, PTY, or mutable GUI-state access
      - [ ] execution runs off the window thread and must never delay first
        window display or terminal painting
      - [ ] a script failure is isolated from the GUI, IPC server, terminal
        readers, and workspace persistence
    - Runtime architecture
      - [ ] optional `scripting` Cargo feature behind a narrow `ScriptHost`
        trait; the GUI state machine remains independent of Rhai types
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
    - Extension surfaces
      - [ ] phase 1: one-shot run/eval/check and API discovery
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
    - [ ] full compatibility matrix; compatibility claims remain semantic
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
    - [~] release automation publishes `agenterm-mux.exe` after its acceptance
      gate; `agenterm-bash.exe` remains gated and unpublished
    - [x] release metadata reports version, build time, commit, enabled
      features, and SHA-256 for every executable/runtime component
    - [x] unit tests for command parsing, protocol, settings, and RMUX status
    - [x] CLI and semantic UX smoke tests through public interfaces
    - [x] one-command fmt, Clippy, test, build, and smoke regression
    - [x] release CI runs the isolated public CLI and fleet smoke suites before
      packaging, even when the redundant GUI smoke suites are skipped
    - [ ] automated terminal input/resize/ANSI/CJK/long-output matrix
    - [ ] installer, updater, stable PATH location, and signed releases
  - Focused super-fleet roadmap
    - [ ] M0 boundaries and baselines: extract typed control operations, record
      per-binary size/startup, and freeze the compatibility corpus
    - [x] M1 fleet CLI: ship `agenterm-mux.exe` from the existing supported
      tmux/RMUX command surface and generated compatibility matrix
    - [ ] M2 shell gate: prototype `agenterm-bash.exe`, select and license the
      real Bash runtime strategy, then pass clean-machine terminal tests
    - [ ] M3 safe scripting: ship Rhai pure/observe profiles, run/eval/check,
      API discovery, budgets, and audit records
    - [ ] M4 dynamic bridge: script-backed status segments and named commands
    - [ ] M5 controlled fleet automation: control profile, bounded event stream,
      team operations, and agent/token status without weakening close safety

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
