# Executable family

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] `agenterm.exe`: Windows-subsystem GUI, PTY owner, workspace authority,
  renderer, and IPC server
- [~] `agenterm` on Linux/macOS: GUI + POSIX PTY + software-raster window;
  shared `control_dispatch` covers observe/input/tab lifecycle/kill
  (`protocol-info`, `list-*`, `new/select/kill-window`, `send-keys`,
  `capture-pane`, `inspect`, `rename-session`, `kill-server`); Win32
  `execute_command` routes the same arms through `ControlHost`; remaining
  UI-only commands (`ui-snapshot`, screenshots, composer HWND, settings)
  stay host-specific
- [~] target architecture separates the replaceable Win32 GUI client from the
  workspace/PTY/server authority so a GUI-only restart can preserve live tabs;
  this is now an accepted v0.1.9 requirement rather than an exploratory
  ownership question:
  - [~] `agenterm-server.exe` is an internal Windows-subsystem process and the
    stable owner of workspace/tree selection, PTYs and child PIDs, terminal
    parser/scrollback, composer drafts, working-context facts, operation
    receipts and the event journal; it has no user-facing HWND and does not own
    layout, theme, focus, clipboard, menus or rendering
    - [x] the first internal `agenterm-server.exe` is a real headless process that owns workspace persistence, tab/tree selection, ConPTY children, parser/scrollback, the event journal, shared replay/receipt authority outside Win32 `AppState`, and a single live interactive UI lease; public server smoke proves hello/bootstrap/delta, lease attach/idempotent renewal/live-owner conflict/heartbeat/detach, lease-gated stable-ID selection/bounded binary input/PTY resize, terminal output, committed replay, conflict rejection, asynchronous receipt completion, persistence, graceful shutdown and zero user-facing HWND
    - [ ] complete every server-owned command and make this executable the
      default authority before declaring the server role complete
  - [ ] `agenterm.exe` always runs the current on-disk replaceable GUI client;
    if no compatible server exists it bootstraps `agenterm-server.exe`, then
    connects through the same typed loopback control boundary instead of
    becoming the server itself
    - [x] opt-in `agenterm.exe --ui-client` starts or connects to the independent headless authority, acquires the exact interactive lease, renders renderer-neutral tab/screen/composer DTOs, routes stable-ID selection/input/resize through the lease, acknowledges applied event positions, detaches without ending the server or PTY, and a replacement GUI recovers the same server PID, active tab and live terminal marker with PNG and orphan-free public evidence
    - [ ] switch ordinary `agenterm.exe` launches only after the replaceable
      client reaches the accepted workbench, settings, editing, selection,
      clipboard, scrollback, close-dialog and observation parity gates
  - [~] UI bootstrap uses a versioned hello, complete bounded workspace and
    terminal-screen snapshot, event baseline, then ordered deltas; reconnect
    detects restart, journal gap and incompatible protocol without silently
    discarding live server state
    - [x] renderer-neutral UI bootstrap and terminal-screen DTOs publish independent schema versions, causal server epoch/sequence identity, stable tab/tree identity, completeness facts and hard byte/item/dimension limits
    - [x] `ui-bootstrap` projects current combined-server tab/tree/process/composer/working-context/screen truth through those DTOs and public black-box evidence compares its causal position and tab metadata with `ui-snapshot` and `inspect`
    - [x] `ui-hello` negotiates a bounded protocol range and returns the current server PID, epoch, sequence and contract schemas; `ui-deltas` follows that baseline with ordered journal events, affected-tab terminal post-state, active-tab identity, explicit completeness and typed restart, gap and future-sequence recovery
    - [~] the current combined and split servers populate and serve hello,
      bootstrap and bounded delta-poll contracts through public loopback IPC;
      the replaceable GUI consumes them and reconnects after epoch restart.
      A dedicated subscription channel remains pending; polling is the shipped
      bounded transport.
  - [~] one interactive UI lease owns terminal resize/focus/input while future
    read-only observers remain possible; replacing or crashing the GUI releases
    only that lease and never ends PTYs
    - [x] `ui-lease attach|heartbeat|detach|status` provides single-live-owner
      identity: matching attach renews idempotently, another live PID conflicts,
      and an exited or expired owner can be recovered without ending PTYs
    - [x] the dedicated `ui-interact` path requires the exact live lease for
      stable-ID active-tab selection, bounded binary terminal input and bounded
      PTY resize; independent typed automation remains a separate control plane
    - [~] the opt-in replaceable GUI consumer acquires and uses that path; it
      reconnects in place with the same GUI PID/HWND after a server epoch
      restart and adopts the new causal bootstrap/lease identity. Tabs
      visibility and drag width remain client-owned and persist through the
      shared settings file, while mouse-wheel history navigation mutates the
      server-owned terminal viewport and PTY resize follows the effective
      layout. Tab rows use the shared responsive row geometry for painting and
      hit-testing and expose `+`, `Edit`, and `Close`: add creates and selects
      a direct child through typed server control and immediately opens that
      child's inline editor; closing a live child requires a non-blocking
      client-owned `Terminate & Close`/`Cancel` decision, while the server
      remains the authority for termination and child-promotion semantics.
      Each row owns its title/note editor in place: `Edit` is replaced by
      bounded native title/note inputs plus `Save` and `Cancel`; Save updates
      the same stable server tab and Cancel performs no mutation.
      Window close now uses non-blocking native `Keep Server Running` (default),
      `Stop Server & Exit`, and `Cancel` choices: the current Composer draft is
      synchronized first, keep releases only the UI lease, stop performs the
      typed workspace-preserving server shutdown, and cancel returns without a
      server mutation. Client-owned Settings now keeps `Tabs` immediately to
      its left and provides native font family/size plus Dark/Light controls:
      theme preview is immediate, Apply validates and persists the shared
      settings atomically before rebuilding the UI font, and Cancel restores
      the last applied palette without changing server state. Terminal drag
      selection is client-owned and generation-bound, paints through the
      selected palette, reconstructs Unicode/wide-cell text from the causal
      screen DTO, and serves both Ctrl+C/Ctrl+V and native system-menu
      Copy/Paste; paste remains bounded and enters the PTY only through the
      interactive lease. Screen schema v2 adds the bounded maximum history
      offset; the client reserves a visible terminal scrollbar, paints its
      proportional thumb, supports track paging and exact top/bottom dragging,
      and routes every viewport change back through typed server control.
      Exact-modifier directional focus navigation is also client-owned:
      Ctrl+Down/Up moves Terminal↔Composer and Ctrl+Left/Right moves
      Terminal↔Tabs; native arrows and Ctrl+Shift/Ctrl+Alt combinations are not
      intercepted, and the focused surface has a palette focus ring.
      Ordinary launches, remaining workbench parity and same-server rollback
      qualification are still pending.
  - [ ] compatibility is fail-closed and asymmetric: a new GUI may connect to
    its declared server protocol range; an incompatible server remains alive
    and reports a precise upgrade/restart choice instead of being killed
  - [x] S0 protocol discovery publishes a typed UI bridge schema, compatible
     version range, current `combined_gui_server` ownership, target executable
     and independently truthful capability flags. Bootstrap, ordered delta
     polling, an opt-in replaceable consumer, and in-place reconnect are now
     shipped. Combined-host facts remain conservative; split-server facts
     advertise only the proven replaceable/reconnect pair, while default
     replacement and rollback remain false until their own black-box gates pass.
  - [ ] black-box upgrade proof keeps server PID, epoch, tab IDs, PTY child
    PIDs, scrollback and continuing output stable while HWND and GUI build
    identity change; rollback to the previous compatible GUI is also proven
  - [~] migration is phased through extracted server state and renderer-neutral
    screen contracts. The current combined `agenterm.exe` remains truthful
    until those gates pass; merely hiding its old HWND is not GUI replacement
- [x] `agenterm.exe` rejects CLI-style or invalid GUI arguments without
  creating a window or information dialog: it writes best-effort
  inherited-stderr guidance and exits nonzero; normal and focus-existing
  launches use the same compact four-line summary for launcher PID,
  configured server address, and pointers to
  `agenterm-cli.exe server-list` for the authoritative PID/port map and
  `agenterm-cli.exe -h` for further commands; it prints before GUI
  initialization so an interactive shell prompt is not overwritten by
  delayed output, prefers inherited stderr, and otherwise briefly attaches
  to the parent console without allocating a console or rebinding stdio;
  startup smoke verifies new-GUI and focus-existing inherited-stderr paths
- [x] `agenterm.exe --no-activate` is a per-launch, non-persistent
  no-activate request accepted before or after `--address HOST:PORT`; the
  original `--not-foreground` spelling remains a compatibility alias: a new
  workspace becomes visible without activation, while an existing visible
  or minimized window is left untouched and a detached window is shown in
  the background without changing its server, tabs, or PTYs; duplicate,
  unknown, and missing-value options fail before startup, and a running
  older server that rejects the internal handoff produces nonzero stderr
  guidance rather than a false-success launcher exit
- [x] `agenterm-cli.exe`: native AgenTerm observation and automation client;
  the pre-release `agentermctl.exe` name is removed rather than retained as
  a parallel compatibility shim
- [ ] optional `agenterm.exe` command forwarding remains exploratory:
  `agenterm-cli.exe` stays the authoritative Console-subsystem entry point,
  no forwarding path may call `AllocConsole` or regress the no-console-flash
  GUI launch, and acceptance requires correct inherited/redirection handles,
  synchronous pipeline behavior, and child exit-code propagation in both
  `cmd.exe` and PowerShell
- [x] `agenterm-mux.exe`: tmux/RMUX-compatible fleet control entry point
- [~] `agenterm-script.exe`: optional general-purpose local Rhai runtime
  sidecar; the shipped v0.1.5 baseline executes one bounded pure/observe
  invocation per fresh worker, while v0.1.9 keeps the one-invocation process
  boundary and adds a task-lived local scheduler, standard library, modules,
  named tasks, and typed Fleet APIs without becoming a persistent daemon
- [ ] `agenterm-bash.exe`: AgenTerm-owned default Bash entry point
- Future executable hypotheses, not release commitments:
  - `agenterm-mcp.exe`: MCP server/client and agentic orchestration sidecar
  - `agenterm-desktop.exe`: optional companion desktop/workspace application;
    it must coexist with Explorer before any separately approved shell role
  - `agenterm-shell-host.exe`: possible minimal recovery watchdog for a much
    later opt-in shell-replacement mode, never the desktop feature process
  - `agenterm-ai.exe`: CPU-first specialized-intelligence sidecar
  - `agenterm-llm-gateway.exe`: governed local LLM forwarding and routing
    sidecar
  - `agenterm-ssh.exe`, `agenterm-curl.exe`, and `agenterm-sqlite.exe`:
    possible stable fleet-integrated entry points over mature runtimes
  - `agenterm-softmgr.exe`: possible signed optional-component lifecycle
    manager
  - an AgenTerm-owned executable would mean a stable product contract,
    discovery, diagnostics, policy, and fleet integration; it would not imply
    rewriting mature Bash, SSH, HTTP, or SQLite protocol engines
  - the `agenterm-{role}.exe` family is also the future package/distribution
    boundary: each optional role remains independently discoverable,
    versioned, installable, repairable, and removable without turning
    `agenterm.exe` into a monolith
- [x] all control frontends reuse shared request/target/format libraries;
  they do not duplicate GUI state or start a second workspace authority
- [~] each binary has independent release size reporting and an enforced
  budget (4 MiB GUI, 2 MiB per CLI); per-binary startup reporting remains
  planned, and adding a frontend must not inflate `agenterm.exe`
