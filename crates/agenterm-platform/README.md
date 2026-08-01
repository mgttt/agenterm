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
  rev = "7245b60c4e6f1ee201eb9f5c5a8c156985845bd3",
  default-features = false,
  features = ["process", "filesystem"]
}
```

Use an immutable full commit SHA in production. The revision above names a
validated extraction increment; advance it deliberately when adopting newer
capabilities.

## Features

The default feature set is empty. The contract/status surface therefore adds no
third-party dependency. Native dependency subfeatures are forwarded by the
owning capability; enabling `process` or `filesystem` does not enable UI, GDI,
clipboard, IPC, or screenshot modules.

| Feature | Public capability | Extra dependency |
|---|---|---|
| `serde` | `IpcEndpoint` string serialization | `serde` |
| `hardware` | host processor architecture, pointer width, parallelism and CPU features | none |
| `entropy` | fail-closed host CSPRNG byte filling | target `libc` / minimal `windows-sys` |
| `process-control` | typed graceful/forceful single-process termination | target `libc` / minimal `windows-sys` |
| `process` | observation/tree control, shell defaults, child-pipe probes and parent-console diagnostics | target `libc` / `windows-sys` |
| `filesystem-conventions` | host roots and sibling executable naming | none |
| `filesystem` | conventions plus private state files/directories and durable atomic replacement mechanics | target native APIs |
| `locking` | cross-process path locks and bounded slot permits | target `libc` / `windows-sys` |
| `ipc` | typed endpoints and native listener/byte stream | `locking`, target native APIs |
| `pty` | PTY command/master/child lifecycle | `process`, `rmux-pty` |
| `window` | display facts, geometry, native text/pixel/control hosts and process-window automation | target Win32 APIs / Unix `winit` + `softbuffer` |
| `input` | normalized key classification, UTF-16 text decoding, primary-shortcut policy | `window` |
| `ime` | preedit/commit state machine and the neutral pixel-window runner when `window` + `input` are enabled | `input` |
| `activation` | neutral policy, typed requests, native window operation and application wake | `window`, target `winit` / Win32 |
| `clipboard` | caller-bounded Unicode clipboard with configurable open deadline | `process`, target native APIs |
| `screenshot` | bounded XRGB encoding and typed native-window capture | `filesystem`, `png`, target Win32 APIs |
| `font` | discovery, metrics and RAII native font resource | `filesystem`, target `ab_glyph` / GDI |
| `webview` | passive system-runtime discovery | none |
| `full` | every declared feature | union of the above |

## Platform support

| Capability | Windows | Linux | macOS |
|---|---|---|---|
| hardware | compile-target ISA + runtime CPU facts | compile-target ISA + runtime CPU facts | compile-target ISA + runtime CPU facts |
| entropy | BCrypt system-preferred RNG | `getrandom(2)` | `arc4random_buf` |
| process control | forceful termination; graceful Unsupported | SIGTERM/SIGKILL | SIGTERM/SIGKILL |
| process | ToolHelp/Job Objects | `/proc` + process groups | POSIX process groups |
| filesystem | AppData conventions | XDG conventions | Application Support |
| locking | named mutex | `flock` | `flock` |
| IPC | named pipe | Unix socket | Unix socket |
| PTY | ConPTY | POSIX PTY | POSIX PTY |
| window geometry | available | available | available |
| process-window automation | Win32 | typed Unsupported | typed Unsupported |
| native text window | Win32/GDI | winit + softbuffer | winit + softbuffer |
| neutral pixel-window host | typed Unsupported | winit + softbuffer | winit + softbuffer |
| neutral control-window host | Win32 controls/GDI | typed Unsupported | typed Unsupported |
| normalized input | Control/AltGr policy | Control/Super policy | Command/Control policy |
| IME composition | typed Unsupported | display-aware | display-aware |
| activation | native show/focus | winit active intent | winit application intent |
| clipboard | Win32 Unicode | Wayland/X11 helpers | `pbcopy`/`pbpaste` |
| screenshot | PNG + native window/client GDI capture | PNG; native-window capture Unsupported | PNG; native-window capture Unsupported |
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

Private state publishers can call `filesystem::protect_private_directory`
after creating their directory and open receipts with
`filesystem::private_create_new_options`. Unix requests owner-only `0700` and
`0600` modes; Windows replaces inheritance with a protected,
current-user-only ACL that propagates to child objects. Exclusive creation
fails rather than overwriting an existing receipt.
`filesystem::write_private_atomic` publishes bytes through an exclusive private
temporary in such a protected directory, atomically replaces the destination,
and synchronizes the parent without embedding a product-specific file format.
`filesystem::file_identity` reports a typed filesystem/object identity from an
already-open file or directory; it remains stable across rename and hard-link
aliases. `path_identity` is a convenience that follows the final symbolic link,
not a substitute for the handle-based form when path replacement races matter.

Windows embedders can synchronously capture an owned native window without
exposing `HWND` in their public API:

```rust,no_run
use agenterm_platform::screenshot::{
    capture_native_window_png, NativeCaptureArea, ScreenshotWindowHandle,
};

# let raw_window_handle: isize = 1;
// SAFETY: the embedding application keeps this window alive for the call.
let window = unsafe { ScreenshotWindowHandle::from_raw(raw_window_handle) }
    .ok_or("null native window")?;
capture_native_window_png(window, std::path::Path::new("window.png"), NativeCaptureArea::Window)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Product applications supply names, paths, policy limits and protocol framing.
The crate does not know AgenTerm workspaces, Control Center, Fleet, themes,
commands, or UI snapshots.

`locking::PathLock::acquire` waits for ownership; `try_acquire` returns typed
`LockErrorKind::Contended` without waiting. Windows resolves relative paths,
dot segments, existing aliases and case before deriving its named-mutex
identity, and rejects recursive aliases held by the same process. Integration
tests use a real child process to prove contention, normal release and release
when an owner exits without running Rust destructors.

GUI embedders enabling `window`, `input`, and `ime` can implement
`window_host::PixelWindowApplication` and call
`window_host::run_pixel_window`. The callbacks receive only normalized events,
a cloneable neutral window control and a mutable XRGB frame; `winit`,
`softbuffer`, native display details and event-loop proxies stay private to the
selected adapter. A `WindowWaker` can wake the loop from worker threads and
returns a typed failure after that loop exits.

Embedders enabling `window` and `input` can implement
`control_window::ControlWindowApplication` and call
`control_window::run_control_window`. Windows owns native child controls,
system-menu dispatch, focus/capture/cursor operations, polling, the message
loop, and double-buffered GDI presentation. Callbacks and `ControlCanvas`
contain only stable platform-neutral values. Linux and macOS return typed
`Unsupported` until their native control shells ship. Native text controls keep
their selection and insertion point through `copy_control_selection` and
`paste_control_selection`; a requested redraw is flushed before `capture_png`
samples the window, keeping structured state and captured pixels on the same
frame.
