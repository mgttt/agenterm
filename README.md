# AgenTerm

AgenTerm is a native Windows terminal and AI fleet controller written in Rust.
It combines hierarchical ConPTY tabs, per-tab composers and environments, a
native automation client, and a deliberately bounded tmux/RMUX frontend.

## Current highlights

- Native Win32/GDI UI with hierarchical team tabs on the left.
- Compact tree-first sidebar with adjacent `Settings` and `New` actions.
- Full-width bottom status surface ready for metrics and agent context providers.
- Branded Windows icon and a persistent terminal font/size settings panel.
- `cmd.exe` is the default shell.
- Two-line tabs separate program/terminal TITLE from a user-maintained note.
- Tabs can be nested as agent/program teams without coupling process lifetimes.
- Normal app restarts restore the tab tree, names, notes, drafts, commands, and
  active tab; PTY commands restart as new processes.
- Exited processes leave a `[dead]` tab until the user explicitly closes it.
- Every tab owns a composer text box and Send button.
- Local CLI can create, select, rename, inspect, capture, and drive tabs.
- Mouse-wheel history, a draggable scrollbar, and highlighted terminal text
  selection share the same viewport; selected text copies to the Windows
  clipboard.
- Snapshot-positioned bounded event reads and waits expose explicit restart,
  gap, and timeout results.
- `agenterm-script.exe` runs bounded one-invocation Rhai `pure` and immutable
  `observe` profiles without linking the scripting engine into the GUI.
- `new-agent` launches Codex in a named fleet tab with stable AgenTerm context.
- Tab-scoped environment and proxy values apply only to the child process and
  are not written to the persistent workspace.
- `agenterm-mux.exe` provides the supported tmux/RMUX session/window surface;
  unsupported operations fail explicitly.
- Whole-window and per-pane PNG screenshots support visual feedback testing.
- PTY process management uses `rmux-pty`.

## Build and run

```powershell
cd D:\dev\agenterm
.\build.bat
.\dist\agenterm.exe
```

The default build is an incremental development build. Use
`.\build.bat release` only for a distributable build. Both modes produce four
ignored local artifacts under `dist/`:

- `dist/agenterm.exe` — GUI application; double-clicking does not create a
  temporary console window.
- `dist/agentermctl.exe` — full native observation and automation client.
- `dist/agenterm-mux.exe` — tmux/RMUX compatibility frontend over the same IPC
  server.
- `dist/agenterm-script.exe` — optional bounded Rhai scripting worker.
- `dist/agenterm.json` — version, UTC build time, Git state, Rust target, size, and
  SHA-256 metadata.

Run the complete quality gate:

```powershell
.\check.ps1
```

## Examples

```powershell
$r = ".\dist\agentermctl.exe"

& $r new-window -d -n build
& $r set-composer -t build "cargo check"
& $r send-composer -t build
& $r wait-pane -t build --contains "Finished" --timeout-ms 30000
& $r capture-pane -p -t build
& $r scroll-pane -t build page-up
& $r screenshot-pane -t build -o build.png

# Discover and run the bounded scripting surface.
& $r script api --json
& $r script eval "40 + 2"
& $r script eval "observe.event_position.sequence" --profile observe

# Discover every registered server, then target one explicitly.
& $r server-list
& $r --address 127.0.0.1:48915 ui-snapshot

# Launch Codex with proxy settings scoped to this tab.
& $r new-agent -n reviewer --proxy http://127.0.0.1:7890 -- --full-auto

# Explicit opt-in convenience for Codex's unsafe bypass mode; omitted by default.
& $r new-agent -n scratch --yolo

# Inspect the honest mux compatibility matrix.
.\dist\agenterm-mux.exe compatibility --json
```

IPC listens and connects only on numeric loopback addresses (`127.0.0.0/8` or
`::1`), including explicit `agenterm-mux --address` overrides.

## Release

Keep `Cargo.toml`'s version current, commit the release state on `main`, then
run:

```powershell
.\release.ps1
```

The script runs the full local quality gate and atomically pushes `main` plus
the `v<version>` tag. GitHub Actions then builds on a clean Windows runner and
publishes all four EXEs, metadata, ZIP, and generated notes to GitHub Releases.

## Documentation

- [Product tree and requirements](docs/PRD.md)
- [Coding-agent guide](AGENTS.md)
