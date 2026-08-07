# Executable family

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] `agenterm.exe`: Windows-subsystem main program. Default launch is the
  replaceable GUI client (HWND, renderer, layout, focus, clipboard, menus —
  never PTY/workspace truth). Headless Fleet authority is the same PE via the
  **`server` subcommand** (`agenterm server …`); there is no separate
  `agenterm-server` product binary.
- [~] `agenterm` on Linux/macOS: GUI + POSIX PTY + software-raster window;
  shared `control_dispatch` covers observe/input/tab lifecycle/kill
  (`protocol-info`, `list-*`, `new/select/kill-window`, `send-keys`,
  `capture-pane`, `inspect`, `rename-session`, `kill-server`); Win32
  `execute_command` routes the same arms through `ControlHost`; remaining
  UI-only commands (`ui-snapshot`, screenshots, composer HWND, settings)
  stay host-specific. Headless authority is likewise `agenterm server`
  (same binary, separate process), not a second executable.
- [x] the shipped architecture separates the replaceable Win32 GUI client from the
  workspace/PTY/server authority so a GUI-only restart can preserve live tabs;
  this is now an accepted v0.1.9 requirement rather than an exploratory
  ownership question:
  - [x] authority is **merged into the main `agenterm` program** as the
    **`server` subcommand** (same process role as the old headless server,
    still a separate OS process from the GUI — not in-process with the window).
    The former `agenterm-server.exe` PE is **removed**; GUI autostart spawns
    `agenterm.exe server …` (see
    [`plan/archive/plan-agenterm-server-mode.md`](../plan/archive/plan-agenterm-server-mode.md)).
    Sharing one PE means Windows may lock `agenterm.exe` while authority
    lives — stop the server before replacing that image. The short-lived
    `agenterm --server` flag remains a transitional alias.
  - [x] the headless authority process is the stable owner of workspace/tree
    selection, PTYs and child PIDs, terminal parser/scrollback, composer
    drafts, working-context facts, operation receipts and the event journal;
    it has no user-facing HWND and does not own layout, theme, focus,
    clipboard, menus or rendering
    - [x] the first internal headless `agenterm server` process is a real headless process that owns workspace persistence, tab/tree selection, ConPTY children, parser/scrollback, the event journal, shared replay/receipt authority outside Win32 `AppState`, and a single live interactive UI lease; public server smoke proves hello/bootstrap/delta, lease attach/idempotent renewal/live-owner conflict/heartbeat/detach, lease-gated stable-ID selection/bounded binary input/PTY resize, terminal output, committed replay, conflict rejection, asynchronous receipt completion, persistence, graceful shutdown and zero user-facing HWND
    - [x] the shared command surface and ordinary-launch black boxes prove this
      process is the default authority
  - [x] `agenterm.exe` without the `server` subcommand always runs the current
    on-disk GUI client; if no compatible server exists it bootstraps
    `agenterm server` as a separate process of the same PE, then connects
    through the same typed loopback control boundary instead of becoming the
    server itself
    - [x] opt-in `agenterm.exe --ui-client` starts or connects to the independent headless authority, acquires an interactive lease (concurrent with other GUIs) with an observable additive client-build identity, renders renderer-neutral tab/screen/composer DTOs, routes stable-ID selection/input/resize through the lease, acknowledges applied event positions, detaches without ending the server or PTY, and a replacement GUI recovers the same server PID, active tab and live terminal marker with PNG and orphan-free public evidence
      - [x] Windows GUI server autostart reuses the platform process facade so
        the independent authority has null stdio, no console window, and breaks
        away from a caller-owned kill-on-close Job. A v0.1.12 live regression
        where the default `Keep Server Running` lost the old session is covered
        by the full replaceable-UI journey: the detached lease, server PID/epoch,
        stable tab, PTY marker and draft survive GUI exit and a replacement GUI
        reconnects before explicit Stop Server cleanup. A restrictive parent Job
        can explicitly deny `CREATE_BREAKAWAY_FROM_JOB`; in that case startup
        retries inside the caller Job, returns `CallerJobFallback`, and writes a
        parent-console diagnostic instead of failing with error 5 or silently
        claiming independent lifetime. The server may then end with that owning
        Job, which remains an observable host limitation rather than a false
        Keep-Running guarantee.
    - [x] the live lease owner publishes a bounded, versioned and redacted
      `replaceable_ui_client` projection back to the stable server; public
      `ui-snapshot` therefore observes client-owned window/layout/focus/modal/
      editing/selection facts without moving those facts into server
      ownership. Publication rejects mismatched PID, lease, server epoch/PID,
      future sequence, malformed shape and payloads above 1 MiB. Detach,
      replacement or stale-owner reaping clears the projection immediately
      and `ui-snapshot` falls back to truthful `headless_server` state.
      Public replaceable-UI smoke proves both projection and fallback with
      the same server PID and retained PTY.
    - [x] a bounded lease-owned command relay preserves synchronous public
      CLI results without making the server call back into the GUI while its
      state loop is blocked: the server queues at most 64 commands, the exact
      GUI lease polls and completes them, and the CLI waits on a typed command
      ID for the final `IpcResponse`. Public black-box evidence now covers
      client-owned Tabs, Settings, focus and PNG screenshot actions plus
      exact-lease server apply/invoke paths; queue arguments are capped at
      64/256 KiB and responses at 1 MiB. The local transport retains a
      separate 4 MiB frame ceiling because completion embeds and JSON-escapes
      that bounded response inside its request; this is not an increase to
      the public operation-argument budget. GUI-destroying
      `keep-server-running` completes its detached response before releasing
      the lease, while `stop-server-and-exit` additionally delays server
      shutdown until the CLI has collected that result; both paths are
      orphan-free in the public black box.
    - [x] ordinary `agenterm.exe` launches use the replaceable client after
      passing workbench, settings, editing, selection, clipboard, scrollback,
      close-dialog, observation and orphan-free parity gates
  - [x] UI bootstrap uses a versioned hello, complete bounded workspace and
    terminal-screen snapshot, event baseline, then ordered deltas; reconnect
    detects restart, journal gap and incompatible protocol without silently
    discarding live server state
    - [x] renderer-neutral UI bootstrap and terminal-screen DTOs publish independent schema versions, causal server epoch/sequence identity, stable tab/tree identity, completeness facts and hard byte/item/dimension limits
    - [x] `ui-bootstrap` projects authoritative server tab/tree/process/composer/working-context/screen truth through those DTOs and public black-box evidence compares its causal position and tab metadata with `ui-snapshot` and `inspect`
    - [x] `ui-hello` negotiates a bounded protocol range, echoes an additive bounded client-build identity, and returns the server build, PID, epoch, sequence and contract schemas while compatible prior peers may omit the new identities; `ui-deltas` follows that baseline with ordered journal events, affected-tab terminal post-state, active-tab identity, explicit completeness and typed restart, gap and future-sequence recovery
    - [x] the independent server serves hello, bootstrap and bounded delta-poll
      contracts through public loopback IPC; the replaceable GUI consumes them
      and reconnects after epoch restart. A dedicated subscription channel is
      a future transport optimization, not a correctness dependency.
  - [x] concurrent interactive UI leases may share terminal resize/focus/input
    on one server (bounded capacity); replacing or crashing one GUI releases
    only that client's lease and never ends PTYs
    - [x] `ui-lease attach|heartbeat|detach|status` provides multi-client
      interactive GUI leases: matching attach renews that client idempotently,
      additional live PIDs attach concurrently (not exclusive single-owner),
      capacity overflow is the only conflict, and exited/expired leases reaped
      without ending PTYs
    - [x] the dedicated `ui-interact` path requires a live lease for
      stable-ID active-tab selection, bounded binary terminal input and bounded
      PTY resize; independent typed automation remains a separate control plane
    - [x] the ordinary replaceable GUI consumer acquires and uses that path; it
      reconnects in place with the same GUI PID/HWND after a server epoch
      restart and adopts the new causal bootstrap/lease identity. Tabs
      visibility and drag width remain client-owned and persist through the
      shared settings file; hiding Tabs collapses only the full-height tree
      while the terminal-owned top toolbar remains available for direct
      recovery, and an always-available checked
      `Toggle Tabs` system-menu item and a host-owned bottom status recovery
      segment prevent a persisted hidden state from trapping the user.
      Mouse-wheel history navigation mutates the server-owned terminal
      viewport and PTY resize follows the effective layout. Tab rows use the
      shared responsive row geometry for painting and
      hit-testing and expose `+`, `Edit`, and `Close`: add creates and selects
      a direct child through typed server control and immediately opens that
      child's inline editor; closing a live child requires a non-blocking
      client-owned `Terminate & Close`/`Cancel` decision, while the server
      remains the authority for termination and child-promotion semantics.
      Shared disclosure geometry now collapses/expands the server-owned
      parent-first tree without removing hidden descendants or changing the
      active stable ID; the additive bootstrap `collapsed` fact defaults to
      expanded when read from a prior compatible server, and every toggle has
      a causal `layout.tree.collapse` event plus tab post-state.
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
      The bottom workbench now reserves a bounded CWD segment sourced from the
      active server tab; clicking it enters a client-owned inline editor,
      `Prepare`/Ctrl+Enter asks the server to generate a shell-safe replacement
      Composer command and publish pending working-context plus causal events,
      while Esc or a second segment click restores the prior draft unchanged.
      Same-server GUI upgrade/rollback, ordinary-launch and observation-shape
      parity qualification are shipped.
    - [x] Windows layout and font changes never wait for PTY resize IPC on the
      Win32 event thread. One owned background worker serializes resize calls,
      retains only the latest queued grid while a request is in flight, and
      binds every result to its lease, client PID, server epoch, stable tab ID,
      rows and columns. Superseded or pre-reconnect results cannot mutate the
      current GUI state; a failed current request remains a typed visible
      failure without poisoning later terminal input. The published UI
      projection exposes current/desired grid and pending convergence, and the
      owning Windows journey alternates native z/Z controls 18 times, waits for
      exact grid convergence, then proves immediate PTY input before completing
      detach, same-session reconnect and server-fault recovery.
  - [x] compatibility is fail-closed and asymmetric: a new GUI may connect to
    its declared server protocol range; an incompatible server remains alive
    and reports a precise upgrade/restart choice instead of being killed
  - [x] S0 protocol discovery publishes a typed UI bridge schema, compatible
     version range, ownership mode, target executable
     and independently truthful capability flags. Bootstrap, ordered delta
     polling, an opt-in replaceable consumer, and in-place reconnect are now
     shipped. Split-server facts advertise the proven replaceable/reconnect/
     rollback/default-launch set.
  - [x] black-box upgrade proof uses two genuinely different GUI binaries and keeps server PID, epoch, stable tab ID, PTY child PID, Composer/CWD facts, scrollback markers and continuing output stable while HWND and GUI build identity change; competing startup exits nonzero without a blocking dialog, incompatible hello fails closed without disturbing the server, and rollback restores the prior compatible GUI identity
  - [x] migration completed through extracted server state and renderer-neutral
    screen contracts. The unreachable combined Win32 runtime was removed after
    parity gates; ordinary launches never become the server process.
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
- [x] `agenterm-cli mux ...` and `agenterm-cli mcp ...` are the **only** public
  fleet-mux and MCP entry points. Each subcommand strips its own name and
  runs the shared library frontend (same implementation as the former
  standalone PEs). Routing is decided before the CLI parses its own options,
  because both frontends own their flag grammar (`mux --address` must reach
  the mux parser). The dedicated `agenterm-mux` / `agenterm-mcp` executables
  are **removed** (not compatibility shims). Control Center remains a separate
  PE (`agenterm-cc`); it is deliberately **not** folded into the CLI.
- [ ] `agenterm-cc.exe` / `agenterm-cc`: independent AgenTerm Control Center
  client. It owns only its window, navigation and uncommitted display state;
  it observes and controls existing authorities through public typed
  contracts, and its absence, crash, renderer failure or upgrade never ends a
  server, PTY or terminal GUI. Detailed product scope belongs to
  [Control Center](PRD_02_21_control_center.md).
- [~] `agenterm-rhai.exe`: optional general-purpose local Rhai runtime with
  one-shot run/eval/check/task entry points and an explicit persistent REPL
  session. Each one-shot invocation owns a fresh supervised worker; the
  foreground REPL instead owns bounded supervised worker generations and
  process-local variables/functions until exit. Generation replacement starts
  a fresh language/history state, reports the old/new generation and PID, and
  never replays already-observed external side effects. On Windows,
  `agenterm-cli script repl ...` remains a thin stdio/exit-code forwarding
  adapter to this adjacent optional executable and does not embed the Rhai
  engine; Linux/macOS callers invoke `agenterm-rhai` directly because CLI
  Script hosting is not available there. Existing one-shot commands retain
  their single-worker supervisor. Both paths reuse one runtime library, API
  graph, local scheduler, standard library, modules, named tasks, and typed
  Fleet APIs without becoming a persistent daemon or an Agent permission layer.
  The worker, framed-protocol, REPL and execute implementation is owned by the
  shared `agenterm` library; `agenterm-rhai` retains a thin dispatch surface
  plus its incremental-build wrapper rather than a second worker implementation.
- [x] v0.1.12 executable-name decision: retain `agenterm-rhai.exe` /
  `agenterm-rhai` as the canonical name for the same unrestricted Rhai
  runtime. Renaming is deferred until measured external usage and a complete
  bootstrap/package/test/documentation caller inventory justify a migration.
  Any future compatibility entry must forward to one implementation and one
  public CLI/task contract; it must not create two runtimes, two task catalogs,
  or a permission-reduced variant.
  Future consolidation should prefer a shared Rust runtime library plus thin
  role-specific entry points; merging executables is not accepted merely to
  reduce file count when GUI/Console subsystem behavior, pipeline exit codes,
  process supervision, upgrade isolation, or independent failure boundaries
  would regress.
- [ ] `agenterm-net` is an independent research executable and future optional
  service for libp2p/IPFS transport and content primitives. It is not linked
  into `agenterm.exe` (GUI or `server`), is not a stable release asset until
  its own gates pass, and is owned by
  [Decentralized network foundation](PRD_02_22_decentralized_network.md).
- [ ] `agenterm-bash.exe`: AgenTerm-owned default Bash entry point
- Future executable hypotheses, not release commitments:
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
