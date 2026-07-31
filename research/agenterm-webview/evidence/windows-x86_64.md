# Windows x86_64 baseline

Observed 2026-07-31 on the local Windows development host with Rust 1.97.0.
This is a development-spike baseline, not a release qualification.

| Evidence | Result |
|---|---:|
| Web engine/version | WebView2 `150.0.4078.105` |
| Direct-WRY release executable | 520,704 bytes |
| Fallback launcher release executable | 282,624 bytes |
| First release host build | 107,233 ms |
| First release launcher build | 16,831 ms |
| Hidden smoke wall time | 888 ms |
| Packaged page load-complete | 856 ms |
| Direct host peak working set | 15,327,232 bytes |
| Normal dependency-tree output | 184 lines |
| Residual experiment processes | none observed after smoke |

The hidden smoke ran through the fallback launcher with `--smoke
--no-activate`. The host receipt reported `status=loaded`,
`no_activate=true`, `bridge=absent`, asset version `cockpit-placeholder/1`, and
the exact custom local URL. Four unit tests cover packaged-asset bounds and
identity, exact route/origin denial, deny-by-default CSP, and absence of bridge,
network, eval, navigation and web-storage tokens in the packaged script.

The release build time is a cold dependency/profile result for this isolated
workspace; it is not paired with a controlled warm sample. Working set covers
the direct Rust host only, not the WebView2 child-process tree. Page load is not
first paint. No screenshot, DPI, crash/reload or intentionally missing-runtime
result has been obtained. Those omissions keep the decision at `defer` and the
stable active renderer at native.

The ignored machine-readable source receipt is reproduced by:

```powershell
.\tools\measure.ps1
```

## Tauri comparison follow-up

The reviewed `windows-comparison.json` records the later direct-WRY/Tauri v2
artifact comparison. Both release hosts loaded the same packaged placeholder
with WebView2 `150.0.4078.105` under `--no-activate`; direct-WRY was 520,704
bytes and the minimal Tauri reference was 8,763,392 bytes. The comparison run
hit its 604-second outer deadline after sealing binaries and ZIPs but before the
script wrote its in-memory timing samples. Those build timings are therefore
explicitly unavailable. The salvaged Tauri smoke exited zero but emitted a
WebView2 window-class unregister warning, so crash/reload and shutdown
stability remain open.
