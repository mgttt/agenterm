# agenterm-platform

`agenterm-platform` is AgenTerm's reusable Rust boundary for typed operating-
system capabilities. It contains platform-neutral contracts, capability
facades, one private target selector, and Windows/Linux/macOS adapters. Product
policy, UI state, Fleet behavior, and AgenTerm executable naming stay in the
embedding application.

The crate is under active development. Pin an exact Git revision when consuming
it from another repository.

## Dependency

```toml
[dependencies]
agenterm-platform = {
  git = "https://github.com/mgttt/agenterm.git",
  rev = "5842a57",
  default-features = false,
  features = ["process", "filesystem"]
}
```

Use an immutable full commit SHA in production. The short revision above is an
illustration and advances as the extraction lands.

## Features

The default feature set is empty. The contract/status surface therefore adds no
third-party dependency.

| Feature | Public capability | Extra dependency |
|---|---|---|
| `serde` | `IpcEndpoint` string serialization | `serde` |
| `process` | observation/tree control, shell defaults and child-pipe probes | target `libc` / `windows-sys` |
| `filesystem` | host roots/naming plus durable atomic replacement mechanics | target native APIs |
| `locking` | cross-process path locks and bounded slot permits | target `libc` / `windows-sys` |
| `ipc` | typed endpoints and native listener/byte stream | `locking`, target native APIs |
| `pty` | PTY command/master/child lifecycle | `process`, `rmux-pty` |
| `window` | display facts, geometry and typed process-window automation | target Win32 APIs |
| `input` | normalized key classification, UTF-16 text decoding, primary-shortcut policy | `window` |
| `ime` | preedit/commit state machine and display-aware capability status | `input` |
| `activation` | neutral policy, typed requests, selected native window operation | `window`, target `winit` / Win32 |
| `clipboard` | caller-bounded Unicode clipboard with configurable open deadline | `process`, target native APIs |
| `screenshot` | bounded XRGB framebuffer PNG encoding | `filesystem`, `png` |
| `font` | discovery, metrics and RAII native font resource | `filesystem`, target `ab_glyph` / GDI |
| `webview` | passive system-runtime discovery | none |
| `full` | every declared feature | union of the above |

## Platform support

| Capability | Windows | Linux | macOS |
|---|---|---|---|
| process | ToolHelp/Job Objects | `/proc` + process groups | POSIX process groups |
| filesystem | AppData conventions | XDG conventions | Application Support |
| locking | named mutex | `flock` | `flock` |
| IPC | named pipe | Unix socket | Unix socket |
| PTY | ConPTY | POSIX PTY | POSIX PTY |
| window geometry | available | available | available |
| process-window automation | Win32 | typed Unsupported | typed Unsupported |
| normalized input | Control/AltGr policy | Control/Super policy | Command/Control policy |
| IME composition | typed Unsupported | display-aware | display-aware |
| activation | native show/focus | winit active intent | winit application intent |
| clipboard | Win32 Unicode | Wayland/X11 helpers | `pbcopy`/`pbpaste` |
| screenshot encoding | PNG | PNG | PNG |
| font candidates | product GDI path | system candidates | system candidates |
| system WebView probe | WebView2 | WebKitGTK | WKWebView |

Unsupported endpoint variants and native failures remain typed; adapters never
silently substitute a different transport or capability.

## Public API

```rust
use std::time::Duration;
use agenterm_platform::{Capability, CapabilityStatus, capability_status};
use agenterm_platform::ipc::{IpcEndpoint, NativeStream};

assert_eq!(capability_status(Capability::Ipc), CapabilityStatus::Available);

let endpoint: IpcEndpoint = "pipe:example".parse()?;
endpoint.validate_local()?;
let mut stream = NativeStream::connect(&endpoint, Duration::from_secs(1))?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

For PTYs, construct `pty::ChildCommand`, set a `pty::TerminalSize`, then retain
independent reader and wait handles using the clone methods before coordinating
termination. Dropping public lock and PTY guard values releases only resources
owned by that value.

Product applications supply names, paths, policy limits and protocol framing.
The crate does not know AgenTerm workspaces, Control Center, Fleet, themes,
commands, or UI snapshots.
