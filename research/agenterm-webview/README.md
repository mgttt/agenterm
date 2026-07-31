# AgenTerm system-WebView experiment

This directory is an isolated Rust workspace for the v0.1.12 system-WebView
technical spike. It is not part of the root Cargo package or release build. It
does not replace native `agenterm-cc`, and neither `agenterm.exe` nor
`agenterm-server` acquires a WebView dependency.

The first vertical slice compares direct-WRY with a minimal Tauri v2 reference
host over the same packaged, read-only Cockpit placeholder. Its stable outcome
is still `active_renderer=native`; both hosts are experimental until the three
native platforms have independent runtime, rendering, no-activate,
crash/reload, DPI, PNG, size and resource evidence.

## Architecture and failure boundary

```text
agenterm-cc-web                 fallback-safe launcher; no WRY linkage
  -> agenterm-cc-web-direct-wry isolated direct-WRY process, or
  -> agenterm-cc-web-tauri      isolated Tauri v2 reference process
       -> WebView2 | WKWebView | WebKitGTK 4.1
       -> embedded HTML/CSS/JS only
```

The split is deliberate. In particular, a Linux binary linked to WebKitGTK can
fail in the dynamic loader before Rust `main` runs. The launcher can still
report `status=unavailable`, `active_renderer=native` when the host executable,
loader or runtime is unavailable. It never downloads a runtime and the
workspace contains no fixed browser runtime.

The direct host:

- serves three `include_bytes!` assets from the single
  `agenterm://localhost` custom origin and publishes SHA-256 identities with
  `--asset-manifest`;
- permits only exact packaged routes, denies new windows and downloads, and
  installs a deny-by-default CSP (`connect-src 'none'`, no frames, objects,
  forms, workers or media);
- has no IPC handler or initialization script and exposes no eval, shell,
  process, filesystem, network or Fleet API;
- uses an incognito WebView context, disables clipboard, devtools and zoom
  hotkeys, and creates the native window hidden before the WebView is ready;
- honors `AGENTERM_NO_ACTIVATE=1` and `--no-activate` by keeping the experiment
  hidden and unfocused.

The intended bridge-v1 methods (`host.ready`, `host.facts`, and read-only
`fleet.snapshot`) are intentionally absent in this foundation. Adding them is a
separate security change requiring origin, main-frame, document nonce, request
ID, deadline, 64 KiB and eight-in-flight enforcement. `bridge=absent` is
reported explicitly.

## Build and probe

From this directory:

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release -p agenterm-cc-web-direct-wry
cargo build --release -p agenterm-cc-web
.\target\release\agenterm-cc-web.exe --asset-manifest
.\target\release\agenterm-cc-web.exe --probe
$env:AGENTERM_NO_ACTIVATE = '1'
.\target\release\agenterm-cc-web.exe --smoke
```

The workspace `default-members` contains only the fallback launcher/core, so
ordinary `cargo build` and `cargo test` do not require WebView development
headers. Building the direct host requires the platform's native prerequisites:
WebView2 development support on Windows, Xcode command-line tools on macOS, or
`libwebkit2gtk-4.1-dev` and GTK development packages on Linux.

`--probe` performs WRY's native version query without opening a window.
`--smoke` exits from the page-load event rather than a fixed sleep and reports
`load_complete_ms`. The launcher converts a missing sibling host, pre-main
dynamic-loader failure, runtime query failure or WebView creation failure to a
typed unavailable receipt with exit code 69.

Run `tools/measure.rhai` through `agenterm-script` on a real Windows desktop to
build both layers and write a local JSON receipt containing build times, binary
sizes, runtime probe and event-driven load time. Root process RSS is recorded as
unavailable until the public Script process facts expose it; the measurement
does not substitute an incomplete working-set value.
Every completed phase first appends and flushes an independently parseable JSONL
event, then atomically refreshes the folded partial receipt. A hard outer timeout
can therefore lose only the current phase; prior duration and receipt facts
remain under `evidence/local/`. Run IDs and journal paths fail closed on
collision rather than reusing target caches.

Build, metadata, probe, smoke and archive work have explicit recorded
deadlines. A controlled deadline terminates only the process tree launched for
that phase, writes `status=timed_out`, folds an incomplete receipt and stops the
run. Validate journal folding without building anything with:

```powershell
.\dist\agenterm-script.exe run --timeout-ms 60000 --max-operations 10000000 --max-string-bytes 8388608 --max-collection-items 100000 --max-output-bytes 1048576 --project-root . .\research\agenterm-webview\tools\measure.rhai -- . --self-check
```

Run the full Windows measurement from the repository root with:

```powershell
.\dist\agenterm-script.exe run --timeout-ms 3600000 --max-operations 100000000 --max-string-bytes 8388608 --max-collection-items 100000 --max-output-bytes 1048576 --project-root . .\research\agenterm-webview\tools\measure.rhai -- .
```

The result goes under ignored `evidence/local/` unless an operator explicitly
promotes it as a named, reviewed machine baseline.

## Current evidence and gaps

Windows x86_64 source-level and runtime results are recorded in
`evidence/windows-x86_64.md`. The exact Tauri/direct-WRY comparison and
reproduction inputs are in `evidence/tauri-vs-direct-wry.md`.

No macOS or Linux renderer claim is made from this Windows checkout. Required
follow-up evidence is:

- Windows installed and intentionally missing WebView2 runtime black boxes,
  actual PNG/DPI, crash/reload, process-tree RSS and cold/warm runs;
- macOS WKWebView Retina PNG, no-activate, reload/crash and resource runs;
- Linux WebKitGTK 4.1 X11 and Wayland runs plus a missing-library launcher
  fallback, with no substitute PNG;
- locked third-party licence/SBOM review for each shipped platform artifact;
- a separate bridge-v1 implementation and adversarial message tests.

The Tauri reference is a nested workspace at `tauri-reference/`. It has no
commands, plugins or capabilities and leaves clipboard access disabled by
default. Its frontend points at the same static `assets/`; it does not require
Node/npm and does not enable bundling or runtime download. Build and validate it
separately with:

```powershell
cargo test --manifest-path .\tauri-reference\Cargo.toml
cargo clippy --manifest-path .\tauri-reference\Cargo.toml --all-targets -- -D warnings
```

The reviewed Windows comparison receipt is
`evidence/windows-comparison.json`. Its 2026-07-31 run reached an outer deadline
after sealing both artifacts but before writing in-memory build timings, so the
receipt preserves hashes, sizes and no-activate smoke facts while marking those
timings unavailable.

References: [WRY repository and platform requirements](https://github.com/tauri-apps/wry),
[Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/),
[Tauri process model](https://v2.tauri.app/concept/process-model/), and
[Tauri Windows WebView2 options](https://v2.tauri.app/distribute/windows-installer/).
