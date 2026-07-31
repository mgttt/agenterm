# Tauri v2 versus direct-WRY comparison input

Recorded 2026-07-31. Versions are exact crates.io releases observed for this
spike, not floating recommendations:

| Dimension | direct-WRY baseline | minimal Tauri v2 reference |
|---|---|---|
| Runtime crate | `wry 0.56.0` | `tauri 2.11.5` (uses `tauri-runtime-wry`) |
| Window/event loop | `tao 0.36.0` | selected and integrated by Tauri runtime |
| Build/CLI | Cargo only | `tauri-build 2.6.3`, optional `tauri-cli 2.11.4` |
| Rust | repository-pinned 1.97.0 | Rust plus the same native platform prerequisites |
| JavaScript toolchain | none; three static files are embedded by Rust | none required when `frontendDist` points at static files; Node/npm is required only if a chosen frontend build needs it |
| Web engine | WebView2 / WKWebView / WebKitGTK 4.1 | WebView2 / WKWebView / WebKitGTK 4.1 |
| Host API surface | explicit WRY builder callbacks only | Tauri app/runtime/config/command surface in addition to WRY |
| Fallback architecture | small launcher does not link WRY; isolated host may fail | would need the same external launcher/process boundary to survive a pre-main Linux loader failure |
| Current implementation evidence | builds from this workspace and has local-origin policy tests | dependency/toolchain reference only; no Tauri binary result is claimed yet |

## Dependency and licence method

For direct-WRY, `Cargo.lock` is the exact dependency inventory. Produce the
normal build graph with:

```powershell
cargo tree -p agenterm-cc-web-direct-wry -e normal
cargo metadata --locked --format-version 1
```

Before any adoption decision, run the repository-approved licence/SBOM tooling
against that lock and manually review native runtime licences. The first-party
direct dependencies use these published licences:

- WRY: `Apache-2.0 OR MIT`;
- Tao: `Apache-2.0 OR MIT`;
- Serde/serde_json: `MIT OR Apache-2.0`;
- SHA-2: `MIT OR Apache-2.0`.

Do not infer the transitive result from those four rows. The lockfile graph and
platform-native packages are the evidence owners.

For a fair Tauri reference, create a second isolated package pinned to the
versions above, point `frontendDist` at the exact same `assets/`, define no
commands/plugins/updater/shell/process APIs, set the Windows installer
`webviewInstallMode` to `skip`, and generate its own lockfile. Record `cargo
tree` and metadata with the same commands. The reference must not reuse this
workspace lock because Tauri changes the graph.

## Size and performance method

Build each implementation from a clean implementation-specific target once,
then a warm unchanged build, on the same machine and power mode. Measure:

1. stripped release executable bytes;
2. a ZIP containing only identical app assets and required host files;
3. installer bytes, with the system runtime excluded and separately reported;
4. process-tree private working set/RSS at native-window creation and packaged
   page-load completion;
5. process start to page-load completion, cold after reboot and warm for at
   least five samples;
6. first visually presented frame separately from page load once a renderer
   instrumentation path exists.

`load_complete_ms` in this baseline is not called first paint. The current
PowerShell measurement captures the Rust host's peak working set, not the full
WebView2 process tree; both limitations remain visible rather than being
silently promoted into stronger claims.

## Decision status

`defer`. Direct-WRY provides the narrower dependency and API surface for the
foundation, while Tauri offers packaging/configuration conventions that may be
valuable for a larger independent application. Neither choice has the required
three-platform evidence, and the stable renderer remains native.

Primary references:

- [WRY platform requirements and licence](https://github.com/tauri-apps/wry)
- [Tauri v2 process model](https://v2.tauri.app/concept/process-model/)
- [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Tauri Windows runtime modes](https://v2.tauri.app/distribute/windows-installer/)
