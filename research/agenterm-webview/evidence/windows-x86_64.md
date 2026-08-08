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
.\dist\agenterm-rh.exe run --timeout-ms 60000 --max-operations 10000000 --max-string-bytes 8388608 --max-collection-items 100000 --max-output-bytes 1048576 --project-root . .\research\agenterm-webview\tools\measure.rh -- . --self-check
.\dist\agenterm-rh.exe run --timeout-ms 3600000 --max-operations 100000000 --max-string-bytes 8388608 --max-collection-items 100000 --max-output-bytes 1048576 --project-root . .\research\agenterm-webview\tools\measure.rh -- .
```

The measurement writes a uniquely named append-only JSONL journal and
atomically refreshes its folded JSON receipt after every terminal phase. A
truncated final JSONL line is ignored during salvage, but schema or sequence
corruption in any earlier line fails closed. Per-process deadlines kill only
the process tree created for that phase; an uncontrolled outer termination
cannot erase already flushed build durations or smoke/archive receipts.

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
