# AgenTerm

AgenTerm is a native Windows terminal and scriptable terminal controller written
in Rust. It combines a left-side tab UI, one ConPTY-backed shell per tab, a
per-tab external composer, and a tmux/RMUX-style command line.

## Current highlights

- Native Win32/GDI UI with tabs on the left.
- Branded Windows icon and a persistent terminal font/size settings panel.
- `cmd.exe` is the default shell.
- Two-line tabs separate program/terminal TITLE from a user-maintained note.
- Exited processes leave a `[dead]` tab until the user explicitly closes it.
- Every tab owns a composer text box and Send button.
- Local CLI can create, select, rename, inspect, capture, and drive tabs.
- Whole-window and per-pane PNG screenshots support visual feedback testing.
- PTY process management uses `rmux-pty`.

## Build and run

```powershell
cd D:\dev\agenterm
.\build.bat
.\dist\agenterm.exe
```

The default build is an incremental development build. Use
`.\build.bat release` only for a distributable build. Both modes produce two
ignored local artifacts under `dist/`:

- `dist/agenterm.exe` — the selected dev or release executable.
- `dist/agenterm.json` — version, UTC build time, Git state, Rust target, size, and
  SHA-256 metadata.

Run the complete quality gate:

```powershell
.\check.ps1
```

## Examples

```powershell
$r = ".\dist\agenterm.exe"

& $r new-window -d -n build
& $r set-composer -t build "cargo check"
& $r send-composer -t build
& $r wait-pane -t build --contains "Finished" --timeout-ms 30000
& $r capture-pane -p -t build
& $r screenshot-pane -t build -o build.png
```

## Release

Keep `Cargo.toml`'s version current, commit the release state on `main`, then
run:

```powershell
.\release.ps1
```

The script runs the full local quality gate and atomically pushes `main` plus
the `v<version>` tag. GitHub Actions then builds on a clean Windows runner and
publishes the EXE, metadata, ZIP, and generated notes to GitHub Releases.

## Documentation

- [Product tree and requirements](docs/PRD.md)
- [Coding-agent guide](AGENTS.md)
