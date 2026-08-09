# Command line (`agenterm cli`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

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
    - [x] named buffer commands (tmux/RMUX B′ minimal set; in-process
      only, bounded byte limits, no cross-server sharing)
      - `set-buffer|setb [-b name] [--] data...`
      - `load-buffer|loadb [-b name] path`
      - `show-buffer|showb [-b name]`
      - `list-buffers|lsb`
      - `delete-buffer|deleteb [-b name]`
      - `paste-buffer|pasteb [-b name] [-t target]` (empty buffer fails;
        UTF-8 text is normalized and honors application bracketed-paste)
      - [x] `save-buffer|saveb` is **explicitly unsupported** (typed fail /
        mux unsupported); use `show-buffer` redirect or `load-buffer`
      - **B′ vs agent messaging (B5):** PTY inject is not a mailbox.
        Short collab/status → note/handoff (M1/M3). Drive shell / type
        keys → `send-keys` / `paste-buffer`. Intent then optional PTY
        inject → handoff on disk, then human/script `paste-buffer`.
        Optional copy-mode → buffer is not in this minimal set.
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
      a GUI, automatically removes definitively dead PID records, retains
      live-but-unreachable records for diagnosis, and therefore provides
      the read-side companion to `kill-server`
    - [x] `server-kill` is a client-side alias that canonicalizes to the
      existing `kill-server` operation before IPC dispatch; it preserves
      the same destructive workspace and process-lifecycle semantics
    - [x] global `--address HOST:PORT` targets a discovered loopback TCP
      server explicitly
    - [x] v0.1.11 adds global selectors `--instance main|dev` and
      `--endpoint unix:<path>|pipe:<name>|tcp:<host>:<port>`; future explicit
      `ephemeral/custom` instance forms must reuse the same resolver and must
      not be invented independently by individual commands
    - [x] selector precedence is an explicit CLI selector, then
      `AGENTERM_IPC_ENDPOINT`, then legacy `AGENTERM_IPC_ADDRESS`, then
      `AGENTERM_INSTANCE`, then the platform-derived `main` default. A CLI
      selector overrides inherited environment selectors; conflicting typed
      environment selectors fail unless a CLI selector intentionally replaces
      them. `--address` remains the compatibility spelling for an explicit TCP
      endpoint; any two CLI selectors (`--instance`, `--endpoint`, or
      `--address`) conflict and fail with a typed error rather than silently
      overriding or falling back
    - [x] selector parsing, platform endpoint derivation, user scoping,
      singleton ownership, peer validation, stale recovery, and migration are
      owned by
      [Agent control plane](PRD_02_07_agent_control_plane.md);
      every CLI command consumes that shared resolver
    - [ ] `server-list` human output replaces the TCP-specific `ADDRESS`
      assumption with typed `INSTANCE`, `TRANSPORT`, and `ENDPOINT` facts while
      retaining PID/version/status/window/tab/session/workspace diagnostics;
      endpoint rendering is bounded and safe for terminals
    - [ ] `server-list --json` emits registration schema-v2 logical-instance,
      server-scope, and typed-endpoint fields while preserving legacy JSON
      fields during the compatibility window. Legacy TCP rows remain visible,
      mixed v1/v2 discovery is deterministic, and an unavailable native
      endpoint is reported as unreachable/incompatible/owner-unknown rather
      than omitted or auto-started
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
    - `ui-snapshot`, `ui-bootstrap`, `protocol-info`
    - [x] `agent-tools [--format agenterm|mcp] [--include-unavailable]`
      emits the LLM tool table an autonomous agent binds to. It is a pure
      projection of the typed operation catalog
      ([Agent control plane](PRD_02_07_agent_control_plane.md)), never a
      hand-written list: every parameter becomes JSON Schema with its declared
      bounds, every operation carries its class, its declared typed failures,
      and — for destructive operations — an explicit approval gate the caller
      must open before invoking. Unavailable operations are excluded unless
      asked for, so an agent can never select a capability that does not ship.
      The command answers from the binary alone and never contacts a server
    - [x] `ui-input pointer --x PX --y PX [--button left|right|middle]
      [--action press|release|move] [--count 1|2|3]
      [--mods shift,ctrl,alt,meta]` and
      `ui-input wheel --x PX --y PX --delta-y N [--units lines|pixels]`
      give an agent the pointer affordances a human has. Coordinates are
      pixels in the same space `ui-snapshot` reports element bounds in, so a
      caller can read a snapshot, find an element, and press its centre
      without converting. Requests are turned into the same window events a
      window manager delivers and enter the frontend through its ordinary
      event path; no second hit-test exists, and multi-click is delivered as
      repeated press/release pairs rather than by seeding promotion state, so
      a synthetic gesture cannot diverge from a human one. Wheel deltas
      default to notches (`lines`); trackpad callers ask for `pixels`.
    - [x] `ui-hello --minimum VERSION --maximum VERSION [--client-id ID]`
      negotiates the renderer protocol and returns server identity plus a causal
      baseline without mutating state
    - [x] `ui-deltas --epoch EPOCH --after SEQUENCE [--limit 1..64]`
      returns ordered events and affected-tab post-state under an 8 MiB response
      budget; restart, journal gap and future sequence fail with typed recovery
      facts
    - [x] `ui-client-state publish --lease-id ID --client-pid PID
      --snapshot-json JSON` is the bounded internal publication arm used by
      a live interactive lease holder; it preserves public `ui-snapshot`
      observation while keeping client-local UI facts out of server authority
    - [x] `ui-client-command poll|apply|invoke|complete|result` is the internal
      exact-lease command/result relay behind synchronous public GUI commands;
      its bounded queue, final-response path, detach-before-destroy ordering,
      and result-before-server-shutdown ordering are black-box proven
    - `workspace-info`, `save-workspace`, `shutdown`
    - [~] `server-list` and `server-kill` establish a `server-*` lifecycle
      namespace; explore health, start, and graceful-shutdown helpers only
      as aliases over typed operations, without creating a second server
      registry or weakening the `kill-server` destruction contract
    - [ ] `shutdown --no-save` escape hatch for instances whose workspace
      destination has become unwritable
    - `wait-ui [--active @id] [--focus surface] [-t target
      --tab-state running|dead|--tab-editor-state open|closed]
      [--modal-kind KIND|none|closed]
      [--modal-target target] [--timeout-ms ms]`
  - Shipped scripting baseline
    - `script api [--json]`
    - `script check FILE|-`
    - `script eval EXPRESSION`
    - `script run FILE|- [-- ARGS...]`
    - [x] `rh-pack --path PATH [--json]` inspects a built rh AOT pack
      directory (native library + manifest; offline CLI surface)
  - Composer and tab metadata
    - `show-composer [-t target]`
    - `set-composer [-t target] text|--stdin|--file path`
    - `send-composer [-t target]`
    - `set-tab-note [-t target] text`, `show-tab-note [-t target]`
  - Semantic UI control
    - [~] `control-center open|status|snapshot|close [--no-activate]`
      owns the typed lifecycle surface for the independent Control Center
      process; its read-only live evidence remains a v0.1.11 qualification
      gate.
    - `focus terminal|composer|tabs [-t target]` (`sidebar` remains an alias)
    - `ui-action new-tab|new-child|edit-tab|toggle-tree|tabs-show|tabs-hide|tabs-toggle|toggle-tabs|tabs-set-width|select-tab|close-tab|confirm|cancel|
      composer-send|copy-selection|open-settings|window-minimize|
      window-maximize|window-restore [-t target]`
    - `ui-action window-resize --width PX --height PX`
    - [x] semantic actions control window state and client size without corrupting the PTY grid
    - [x] `wait-ui` directly waits for modal kind, target, or closed state with a stable timeout code
      - Settings, confirmation, CWD, and proxy surfaces are addressable by
        kind/target; `none` and `closed` both mean no modal, and timeout
        failures expose the stable `ui_wait_timeout` code
    - [x] `wait-ui -t target --tab-editor-state open|closed` exposes a bounded
      condition wait for the inline editor without repeated process spawning
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
  - v0.1.9 local runtime and named tasks
    - [ ] `script run [OPTIONS] FILE.rhai|- [--] [ARGS...]`
    - [ ] `script eval [OPTIONS] EXPRESSION [--] [ARGS...]`
    - [ ] `script check [OPTIONS] FILE.rhai|-`
    - [x] `script api [MODULE] [--status shipped|planned|all]` renders the
      deterministic hierarchical human catalog, while `--json` emits the same
      filtered versioned source with explicit view metadata
    - [ ] `script api --compare rust|node|bun|all` renders reviewed analogues
      and semantic differences from catalog-owned comparison metadata
    - [ ] `script task list [--manifest PATH] [--json]`
    - [ ] `script task show TASK [--manifest PATH] [--json]`
    - [ ] `script task run TASK [--manifest PATH] [--] [ARGS...]`
    - [ ] ordinary `run`, `eval`, `check`, and named tasks use one unrestricted
      local runtime surface; legacy profile spellings are inert compatibility
      data and never change API registration, visibility, arguments, targets,
      or execution behavior
    - [ ] runtime options include explicit `--cwd`, bounded timeout/output/
      task/stream overrides, and machine-readable result selection; Script
      Runtime does not require per-file, per-process, per-tool, or per-network
      permission flags
    - [ ] task commands discover one versioned declarative project manifest,
      retain invalid entries in `list` with a typed degraded reason, and use
      stable task IDs rather than display names as authority
    - [ ] task listing, inspection, and invocation are P0; a future GUI command
      palette is a P1 consumer of the same catalog and cannot define a second
      registry
    - [ ] exit codes and JSON envelopes distinguish script/runtime failure,
      invalid manifest or arguments, unavailable/degraded API, resource limit,
      cancellation/timeout, child-process result, and host/protocol failure
  - AI fleet launch
    - `new-agent [-d] [-n name] [--parent target] [--program executable]
      [-e NAME=VALUE] [--proxy URL] [--no-proxy hosts] [--yolo]
      [-- codex args...]`
    - `new-window` and `new-session` also accept repeated `-e NAME=VALUE`
    - injected values live only for the child process; snapshots expose
      names, and workspace persistence stores neither names nor values
    - every child currently receives reserved `AGENTERM_IPC_ADDRESS`,
      `AGENTERM_TAB_ID`, `AGENTERM_SESSION`, and
      `AGENTERM_WORKSPACE_PATH`
    - [ ] during native-local IPC migration, children also receive the shared
      resolver's typed endpoint and logical-instance representation;
      `AGENTERM_IPC_ADDRESS` remains populated only as the legacy TCP
      compatibility form and is not fabricated for Unix sockets or named pipes
    - the default launcher uses the system `codex` command through
      `cmd.exe` so standard npm `.cmd` installations work in ConPTY;
      `--program` is the explicit direct-executable override
    - `--yolo` explicitly maps to Codex
      `--dangerously-bypass-approvals-and-sandbox`; the default remains safe
