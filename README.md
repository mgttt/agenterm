# AgenTerm

AgenTerm is a native Windows terminal and AI fleet controller written in Rust.
It combines hierarchical ConPTY tabs, per-tab composers and environments, a
native automation client, and a deliberately bounded tmux/RMUX frontend.

![AgenTerm showing a hierarchical terminal workspace, composer, and working-context status bar](assets/screendump0.png)

## Current highlights

- Native Win32/GDI UI with hierarchical team tabs on the left.
- Compact, scrollable tree-first sidebar with two-line names/notes and a
  draggable width boundary.
- Terminal toolbar keeps `<Tabs`/`>Tabs` and `New` at the left while anchoring
  `Settings` at the right.
- Terminal-scoped bottom status surface is ready for metrics and agent context
  providers without consuming the full-height Tabs column.
- Branded Windows icon and a persistent terminal font/size settings panel.
- `cmd.exe` is the default shell.
- Two-line tabs separate program/terminal TITLE from a user-maintained note.
- Tabs can be nested as agent/program teams without coupling process lifetimes.
- Normal app restarts restore the tab tree, names, notes, drafts, commands, and
  active tab; PTY commands restart as new processes.
- Exited processes leave a `[dead]` tab until the user explicitly closes it.
- Every tab owns a composer text box and Send button.
- `New` opens a configuration surface for shell profile, initial command, and
  optional ephemeral HTTP(S) proxy environment before creating a terminal.
- Local CLI can create, select, rename, inspect, capture, and drive tabs.
- Mouse-wheel history, a draggable scrollbar, and highlighted terminal text
  selection share the same viewport; selected text copies to the Windows
  clipboard.
- Snapshot-positioned bounded event reads and waits expose explicit restart,
  gap, and timeout results.
- `agenterm-script.exe` is the public Rhai CLI for local automation, pure
  computation, observable Fleet tools, and versioned named tasks without
  linking the scripting engine into the GUI.
- `agenterm-mcp.exe` is the on-demand read-only MCP sidecar. Its first
  v0.1.10 slice serves four metadata-only Fleet resources and one bounded
  `agenterm_wait` tool over stdio; it exposes no mutation tool or network
  listener.
- An internal, non-default `agenterm-server.exe` now proves the headless
  workspace/PTY/parser/event authority required for replaceable GUI work.
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
`.\build.bat release-fast` for repeated optimized local testing: it skips LTO,
uses parallel code generation, and retains incremental state. Use
`.\build.bat release` only for a distributable build; it applies the
size-focused profile in an isolated `target-release/` scratch directory,
stages the finished artifacts in `dist/`, and then clears only that scratch
cache while preserving the incremental development `target/`. All modes
produce six ignored executables plus
build metadata under `dist/`:

- `dist/agenterm.exe` — GUI application; double-clicking does not create a
  temporary console window.
- `dist/agenterm-server.exe` — internal headless workspace, PTY, parser, and
  event authority; not yet the default GUI backend.
- `dist/agenterm-cli.exe` — full native observation and automation client.
- `dist/agenterm-mux.exe` — tmux/RMUX compatibility frontend over the same IPC
  server.
- `dist/agenterm-script.exe` — public Rhai scripting CLI and worker.
- `dist/agenterm-mcp.exe` — on-demand read-only MCP stdio sidecar.
- `dist/agenterm.json` — version, UTC build time, Git state, Rust target, size, and
  SHA-256 metadata.

Run the complete quality gate:

```powershell
.\check.cmd
```

Smoke tests inherit `AGENTERM_NO_ACTIVATE=1`, so their isolated GUI windows do
not interrupt the foreground application. `.\check.cmd --release` omits the
4,128-write event-journal load test; the clean GitHub release runner adds
`--include-stress`.

### Linux GUI (preview)

Native Linux `agenterm` uses winit + softbuffer. Control clients (`agenterm-cli`,
`agenterm-mux`, `agenterm-script`, `agenterm-mcp`) do not need display libraries.

**Release tarballs** ship a small `lib/` directory plus an `agenterm` launcher that
sets `LD_LIBRARY_PATH` before starting `.agenterm.bin`, so end users do not need
`sudo apt install` for X11/Wayland keyboard libraries.

**Building from source** on a minimal host still needs the same libraries available
to the linker/runtime (CI installs them automatically):

```bash
sudo apt-get install -y \
  libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 \
  libx11-6 libxcb1 libxcb-xkb1
./scripts/build-linux-clients.sh
DISPLAY=:1 ./target/x86_64-unknown-linux-gnu/debug/agenterm
```

The Unix GUI currently rasterizes with the built-in `bitmap-8x8` font; settings
`terminal.font-size` adjusts row pitch (grid density), while `terminal.font-family`
is stored for future TTF support and Windows parity.

## Examples

```powershell
$r = ".\dist\agenterm-cli.exe"

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
& $r script eval "fleet.ui.snapshot().event_position.sequence" --profile observe

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
.\lint.cmd
.\release.cmd
```

The coordinator runs the complete stress-inclusive qualification, creates and
records a byte-qualified package rehearsal, then atomically pushes `main` plus
the `v<version>` tag. Use `.\release.cmd --rehearse` to stop before tag
creation or network mutation. GitHub Actions independently validates the clean
tag before publishing the platform assets and generated notes.

## Documentation

- [Product tree and requirements](PRD.md)
- [Coding-agent guide](AGENTS.md)
