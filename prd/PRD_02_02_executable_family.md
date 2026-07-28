# Executable family

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] `agenterm.exe`: Windows-subsystem GUI, PTY owner, workspace authority,
  renderer, and IPC server
- [ ] target architecture separates the replaceable Win32 GUI client from the
  workspace/PTY/server authority so a GUI-only restart can preserve live tabs;
  v0.1.7 freezes an ownership, version-handshake, bootstrap, reconnect, and
  rollback decision plus compatibility test plan; an isolated prototype is
  P1 and the default process model does not change in that internal milestone
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
