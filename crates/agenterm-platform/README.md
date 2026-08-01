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
| `cache-hierarchy` | CPU cache levels, kinds, per-instance capacity, coherency line and sharing geometry | target `libc` / minimal `windows-sys` |
| `virtualization-probe` | current-host WHPX/KVM/Hypervisor.framework availability facts without VM lifecycle | target `libc` / minimal dynamically loaded Win32 APIs |
| `processor-topology` | online logical CPUs, physical cores, packages, NUMA nodes and processor groups | target `libc` / minimal `windows-sys` |
| `processor-affinity` | current-process logical processor set with explicit scheduler/mask semantics | target `libc` / minimal `windows-sys` |
| `host-memory` | host page size, mapping granularity, total physical memory and a typed dynamic availability estimate | target `libc` / minimal `windows-sys` |
| `storage` | path-scoped volume capacity, caller-available bytes and allocation unit | target `libc` / minimal `windows-sys` |
| `entropy` | fail-closed host CSPRNG byte filling | target `libc` / minimal `windows-sys` |
| `console-interrupt` | RAII Ctrl-C/SIGINT observation or temporary ignore with typed failures | target `libc` / minimal `windows-sys` |
| `user-identity` | current Windows SID or POSIX real/effective uid/gid facts | target `libc` / minimal `windows-sys` |
| `process-control` | typed single-process termination and Unix suspend/resume | target `libc` / minimal `windows-sys` |
| `process-observation` | fail-closed single-process liveness and stable start identity | target `libc` / minimal `windows-sys` |
| `process-reference` | owned stable process reference plus bounded or indefinite exact-object exit waits via HANDLE, pidfd, or kqueue | target `libc` / minimal `windows-sys` |
| `process-security` | effective process principal plus typed sandbox identity, with handle-bound Windows queries | target `libc` / minimal `windows-sys` |
| `process-image` | executable path for one selected host process | target `libc` / minimal `windows-sys` |
| `process-metrics` | cumulative CPU time, resident bytes and partially classified page faults for one selected process | target `libc` / minimal `windows-sys` |
| `process-spawn` | detached child launch with retained `Child`, explicit Windows job fallback, ambient-stdio protection and transactional explicit-handle inheritance | target `libc` / minimal `windows-sys` |
| `shared-memory` | exclusive named read/write mappings for cross-process zero-copy data | target `libc` / minimal `windows-sys` |
| `process` | observation/tree control, shell defaults, child-pipe probes and parent-console diagnostics | target `libc` / `windows-sys` |
| `filesystem-conventions` | user home, host roots and sibling executable naming | none |
| `filesystem-entry` | classify path metadata or already-open objects, treating Unix symbolic links and every Windows reparse point as link-like | none |
| `filesystem-open` | open an existing path or one child component without following the final link, then verify the opened object type | target `libc` / minimal `windows-sys` |
| `filesystem-cleanup` | remove caller-owned quiescent trees after restoring deletable permissions without following links | none |
| `filesystem-publish` | recoverable same-parent directory publication with typed rollback outcomes | `filesystem-cleanup` |
| `filesystem-usage` | checked logical-byte accounting without traversing symbolic links or reparse points | none |
| `file-identity` | opened/path host object identity across rename and hard-link aliases | target minimal `windows-sys`; none on Unix |
| `filesystem` | conventions plus private state files/directories and durable atomic replacement mechanics | target native APIs |
| `locking` | cross-process path locks and bounded slot permits | target `libc` / `windows-sys` |
| `ipc` | typed endpoints and product-neutral local byte streams; target-selected extension traits own borrowed/owned handle or fd transfer | `locking`, target native APIs |
| `pty` | PTY command/master/child lifecycle | `process`, `rmux-pty` |
| `window` | display facts, geometry, native text/pixel/control hosts and process-window automation | target Win32 APIs / Linux `x11rb` / Unix `winit` + `softbuffer` / macOS system frameworks |
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
| cache hierarchy | RelationCache geometry | cache sysfs | cache-size sysctl geometry |
| native virtualization probe | dynamically discovered WHPX capability | `/dev/kvm` + API version | `kern.hv_support` |
| processor topology | cores/packages/NUMA/groups | sysconf + sysfs | logical/physical/package sysctl |
| processor affinity | single-group process affinity mask | `sched_getaffinity` effective mask | typed Unsupported; affinity tags are advisory |
| host memory | page/allocation geometry + physical total | page geometry + physical pages | page geometry + `hw.memsize` |
| storage | volume capacity + cluster geometry | `statvfs` | `statvfs` |
| entropy | BCrypt system-preferred RNG | `getrandom(2)` | `arc4random_buf` |
| console interrupt | Ctrl-C-only console handler + atomic notification | SIGINT `sigaction` + self-pipe | SIGINT `sigaction` + self-pipe |
| process control | forceful termination; graceful Unsupported | SIGTERM/SIGKILL | SIGTERM/SIGKILL |
| process image | queried full image path | `/proc/<pid>/exe` | `proc_pidpath` |
| process metrics | process times + working set + total faults | `/proc` stat/statm + minor/major faults | `PROC_PIDTASKINFO` total faults + page-ins |
| process spawn | job breakaway or explicit caller-job fallback | new session via `setsid` | new session via `setsid` |
| shared memory | page-file mapping | POSIX shared memory | POSIX shared memory |
| process | ToolHelp/Job Objects | `/proc` + process groups | POSIX process groups |
| filesystem | AppData conventions | XDG conventions | Application Support |
| filesystem cleanup | clear readonly attributes; do not traverse reparse points | restore owner access; do not follow symlinks | restore owner access; do not follow symlinks |
| filesystem usage | logical bytes; reparse points are leaves | logical bytes; symlinks are leaves | logical bytes; symlinks are leaves |
| locking | named mutex | `flock` | `flock` |
| IPC | named pipe | Unix socket | Unix socket |
| PTY | ConPTY | POSIX PTY | POSIX PTY |
| window geometry | available | available | available |
| process-window automation | Win32 | exact-PID X11; Wayland Unsupported | exact-PID Quartz; TCC-gated input |
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

`filesystem_usage::logical_tree_size` is path-based accounting, not an
adversarial traversal primitive. Callers choose the roots and must not infer
allocated or physically reclaimable bytes from its logical-byte result.

`processor_affinity::current_process` reports logical processor identities only
when the native API provides a complete result under the declared semantics.
Linux returns the scheduler's effective allowed mask. Windows returns the
process affinity mask only when the process belongs to one processor group;
CPU Sets and thread-specific policies may narrow scheduling further. A
multi-group process is typed Unsupported instead of returning only its primary
group. macOS affinity tags are advisory, so the adapter does not invent an
exact allowed-CPU set. Thread placement and NUMA policy remain product concerns.

`host_memory::facts` reports stable page geometry and installed physical
capacity. `host_memory::availability` is a separate point-in-time observation:
Windows uses `ullAvailPhys`, Linux uses the kernel's `MemAvailable` estimate,
and macOS reports free plus inactive Mach pages. Its typed semantics must be
retained because reclaimability differs between hosts. None of these values is
a cgroup, Job Object, process, container, or guest allocation budget.

`process_spawn::spawn_detached_child` configures a new Unix session or Windows
job breakaway and returns both the live `Child` and a typed launch mode. Windows
retries without breakaway only for access denied and reports
`caller-job-fallback`; it also serializes the process-wide interval in which
ambient standard-handle inherit flags are cleared and restored. Explicit child
stdio, executable discovery, arguments, environment, readiness, restart and
reaping remain caller policy. The older fire-and-forget entry point remains for
compatibility, while supervised callers should retain the child handle.

`process_observation::observe` reports `Live`, `Dead`, or fail-closed `Unknown`
for one PID and includes a native start identity when live. Windows uses process
creation FILETIME, Linux uses `/proc/<pid>/stat` start ticks and treats zombie
states as dead, and macOS uses `proc_bsdinfo` start time. Permission, parsing,
and incomplete-query failures remain `Unknown`; callers must not clean up
another process's state unless the observation is explicitly `Dead`.

`filesystem_cleanup::remove_tree` is for caller-owned trees that are no longer
being mutated. It restores only the access needed for removal, treats a missing
path as success, and never intentionally traverses Unix symlinks or Windows
reparse points. Choosing a deletion root, handling an actively hostile writer,
and deciding whether cleanup failure is fatal remain caller policy.

`filesystem_entry::opened_file_entry_facts` classifies an already-open object
without reopening its path. Callers doing component-wise traversal must use a
native no-follow open first; the returned directory/link-like facts then avoid
a second name-resolution race. The feature remains free of native crate
dependencies and does not choose traversal roots or authorization policy.

`filesystem_open::open_existing_child` accepts exactly one ordinary component
and resolves it relative to a retained directory object. Windows opens the
reparse object itself through a root HANDLE; Linux and macOS use `openat` with
`O_NOFOLLOW`. The facade then verifies the type through that same opened
object, so a renamed or replaced parent path cannot redirect traversal.
Selecting the root, parsing guest paths, granting access, and deciding which
children are authorized remain caller policy.

`filesystem_publish::publish_directory` installs a prepared directory beside
its destination. If a destination already exists, it is first renamed to a
unique sibling backup; an install failure attempts to restore that backup, and
a successful install reports any backup that could not be cleaned. This is a
recoverable two-rename protocol, not a claim that replacement is one atomic
operation or crash-durable. Callers still serialize writers, choose trusted
paths, recover abandoned backups after crashes, and decide how to surface a
non-fatal cleanup warning.

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

Console applications can enable `console-interrupt` and install either one
`ConsoleInterruptObserver` or one `ConsoleInterruptIgnoreGuard`. Observation
coalesces one or more Ctrl-C/SIGINT deliveries until `take_pending` consumes
them. Dropping either value restores the prior native disposition. Windows
claims only `CTRL_C_EVENT`; Ctrl-Break, close, logoff and shutdown continue
through the existing handler chain. Unix signal handlers perform only an
async-signal-safe self-pipe write. These process-wide guards are intentionally
independent from PTY child-console setup and never install the PTY adapter's
ignore-all handler.
On Unix, callers must not independently replace the SIGINT disposition while
either guard is alive; doing so would violate the guard's restoration ownership.
The Unix observer initializes two non-inheritable self-pipe descriptors once
and keeps them for the process lifetime. This prevents an already-entered signal
handler from writing through a closed descriptor after the OS reuses its number;
each new observer drains stale notifications before installing its handler.

```rust,no_run
use agenterm_platform::console_interrupt::ConsoleInterruptObserver;

let interrupts = ConsoleInterruptObserver::install()?;
if interrupts.take_pending()? {
    // Perform shutdown or cancellation in ordinary Rust code.
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`native_virtualization::probe` is passive: it does not create a VM or choose a
guest/provider. Its result distinguishes available, unavailable, access denied,
ABI incompatible and failed states while retaining an API version or native
error code when one exists. Windows resolves `WHvGetCapability` dynamically so
systems without Windows Hypervisor Platform still start normally; Linux accepts
only KVM API version 12. The embedding product owns fallback and routing policy.

`shared_memory::SharedMemory` converts one portable ASCII name into a
session-local page-file mapping on Windows or an owner-readable POSIX shared
memory object on Linux/macOS. Creation is exclusive. Keep the creator alive
through peer discovery: its drop unlinks the POSIX name, while Windows removes
the name after the last handle closes; already-open views remain valid. Byte
layout, synchronization, worker ownership and crash-recovery naming remain
product protocol concerns. An opener requesting more bytes than the native
object contains fails before pointer access; POSIX checks the object with
`fstat` before `mmap`, avoiding a delayed `SIGBUS` on the oversized tail.

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
