//! The only compile-time native adapter selection for enabled capabilities.

pub(crate) const fn platform_kind() -> crate::PlatformKind {
    #[cfg(windows)]
    {
        crate::PlatformKind::Windows
    }
    #[cfg(target_os = "linux")]
    {
        crate::PlatformKind::Linux
    }
    #[cfg(target_os = "macos")]
    {
        crate::PlatformKind::Macos
    }
}

pub(crate) const fn app_container_process_supported() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(crate) const fn app_container_profile_supported() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
#[path = "adapters/windows/threading.rs"]
pub(crate) mod threading;

#[cfg(not(windows))]
#[path = "adapters/unix/threading.rs"]
pub(crate) mod threading;

#[cfg(all(windows, any(feature = "pty", feature = "runtime")))]
#[path = "adapters/windows/environment.rs"]
pub(crate) mod environment;

#[cfg(all(feature = "console-interrupt", windows))]
#[path = "adapters/windows/console_interrupt.rs"]
pub(crate) mod console_interrupt;

#[cfg(all(feature = "window", windows))]
#[path = "adapters/windows/alert.rs"]
pub(crate) mod alert;

#[cfg(all(feature = "window", not(windows)))]
#[path = "adapters/unix/alert.rs"]
pub(crate) mod alert;

#[cfg(all(feature = "console-interrupt", target_os = "linux"))]
#[path = "adapters/linux/console_interrupt.rs"]
pub(crate) mod console_interrupt;

#[cfg(all(feature = "console-interrupt", target_os = "macos"))]
#[path = "adapters/macos/console_interrupt.rs"]
pub(crate) mod console_interrupt;

#[cfg(all(feature = "console-line-editor", windows))]
#[path = "adapters/windows/console_line_editor.rs"]
pub(crate) mod console_line_editor;

#[cfg(all(feature = "console-line-editor", target_os = "linux"))]
#[path = "adapters/linux/console_line_editor.rs"]
pub(crate) mod console_line_editor;

#[cfg(all(feature = "console-line-editor", target_os = "macos"))]
#[path = "adapters/macos/console_line_editor.rs"]
pub(crate) mod console_line_editor;

#[cfg(all(feature = "filesystem-cleanup", windows))]
#[path = "adapters/windows/filesystem_cleanup.rs"]
pub(crate) mod filesystem_cleanup;

#[cfg(all(feature = "filesystem-entry", windows))]
#[path = "adapters/windows/filesystem_entry.rs"]
pub(crate) mod filesystem_entry;

#[cfg(all(feature = "directory-access", windows))]
#[path = "adapters/windows/directory_access.rs"]
pub(crate) mod directory_access;

#[cfg(all(
    feature = "directory-access",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/directory_access.rs"]
pub(crate) mod directory_access;

#[cfg(all(feature = "filesystem-open", windows))]
#[path = "adapters/windows/filesystem_open.rs"]
pub(crate) mod filesystem_open;

#[cfg(all(
    feature = "filesystem-open",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/filesystem_open.rs"]
pub(crate) mod filesystem_open;

#[cfg(all(
    feature = "filesystem-entry",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/filesystem_entry.rs"]
pub(crate) mod filesystem_entry;

#[cfg(all(feature = "filesystem-cleanup", target_os = "linux"))]
#[path = "adapters/linux/filesystem_cleanup.rs"]
pub(crate) mod filesystem_cleanup;

#[cfg(all(feature = "filesystem-cleanup", target_os = "macos"))]
#[path = "adapters/macos/filesystem_cleanup.rs"]
pub(crate) mod filesystem_cleanup;

#[cfg(all(feature = "filesystem-publish", windows))]
#[path = "adapters/windows/filesystem_publish.rs"]
pub(crate) mod filesystem_publish;

#[cfg(all(feature = "filesystem-read", windows))]
#[path = "adapters/windows/filesystem_read.rs"]
pub(crate) mod filesystem_read;

#[cfg(all(feature = "filesystem-read", unix))]
#[path = "adapters/unix/filesystem_read.rs"]
pub(crate) mod filesystem_read;

#[cfg(all(
    feature = "filesystem-publish",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/filesystem_publish.rs"]
pub(crate) mod filesystem_publish;

#[cfg(all(
    windows,
    any(feature = "cache-hierarchy", feature = "processor-topology")
))]
#[path = "adapters/windows/logical_processor.rs"]
pub(crate) mod logical_processor;

#[cfg(all(feature = "cache-hierarchy", windows))]
#[path = "adapters/windows/cache_hierarchy.rs"]
pub(crate) mod cache_hierarchy;

#[cfg(all(feature = "cache-hierarchy", target_os = "linux"))]
#[path = "adapters/linux/cache_hierarchy.rs"]
pub(crate) mod cache_hierarchy;

#[cfg(all(feature = "cache-hierarchy", target_os = "macos"))]
#[path = "adapters/macos/cache_hierarchy.rs"]
pub(crate) mod cache_hierarchy;

#[cfg(all(feature = "processor-topology", windows))]
#[path = "adapters/windows/processor_topology.rs"]
pub(crate) mod processor_topology;

#[cfg(all(feature = "processor-topology", target_os = "linux"))]
#[path = "adapters/linux/processor_topology.rs"]
pub(crate) mod processor_topology;

#[cfg(all(feature = "processor-topology", target_os = "macos"))]
#[path = "adapters/macos/processor_topology.rs"]
pub(crate) mod processor_topology;

#[cfg(all(feature = "processor-affinity", windows))]
#[path = "adapters/windows/processor_affinity.rs"]
pub(crate) mod processor_affinity;

#[cfg(all(feature = "processor-affinity", target_os = "linux"))]
#[path = "adapters/linux/processor_affinity.rs"]
pub(crate) mod processor_affinity;

#[cfg(all(feature = "processor-affinity", target_os = "macos"))]
#[path = "adapters/macos/processor_affinity.rs"]
pub(crate) mod processor_affinity;

#[cfg(all(feature = "virtualization-probe", windows))]
#[path = "adapters/windows/native_virtualization.rs"]
pub(crate) mod native_virtualization;

#[cfg(all(feature = "virtualization-probe", target_os = "linux"))]
#[path = "adapters/linux/native_virtualization.rs"]
pub(crate) mod native_virtualization;

#[cfg(all(feature = "virtualization-probe", target_os = "macos"))]
#[path = "adapters/macos/native_virtualization.rs"]
pub(crate) mod native_virtualization;

#[cfg(all(feature = "storage", windows))]
#[path = "adapters/windows/storage.rs"]
pub(crate) mod storage;

#[cfg(all(feature = "storage", any(target_os = "linux", target_os = "macos")))]
#[path = "adapters/unix/storage.rs"]
pub(crate) mod storage;

#[cfg(all(feature = "host-memory", windows))]
#[path = "adapters/windows/host_memory.rs"]
pub(crate) mod host_memory;

#[cfg(all(feature = "host-memory", target_os = "linux"))]
#[path = "adapters/linux/host_memory.rs"]
pub(crate) mod host_memory;

#[cfg(all(feature = "host-memory", target_os = "macos"))]
#[path = "adapters/macos/host_memory.rs"]
pub(crate) mod host_memory;

#[cfg(all(feature = "process-image", windows))]
#[path = "adapters/windows/process_image.rs"]
pub(crate) mod process_image;

#[cfg(all(feature = "process-image", target_os = "linux"))]
#[path = "adapters/linux/process_image.rs"]
pub(crate) mod process_image;

#[cfg(all(feature = "process-image", target_os = "macos"))]
#[path = "adapters/macos/process_image.rs"]
pub(crate) mod process_image;

#[cfg(all(feature = "shared-memory", windows))]
#[path = "adapters/windows/shared_memory.rs"]
pub(crate) mod shared_memory;

#[cfg(all(
    feature = "shared-memory",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/shared_memory.rs"]
pub(crate) mod shared_memory;

#[cfg(all(feature = "clipboard", windows))]
#[path = "adapters/windows/clipboard.rs"]
pub(crate) mod clipboard;

#[cfg(all(feature = "entropy", windows))]
#[path = "adapters/windows/entropy.rs"]
pub(crate) mod entropy;

#[cfg(all(feature = "user-identity", windows))]
#[path = "adapters/windows/user_identity.rs"]
pub(crate) mod user_identity;

#[cfg(all(
    feature = "user-identity",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/user_identity.rs"]
pub(crate) mod user_identity;

#[cfg(all(feature = "entropy", target_os = "linux"))]
#[path = "adapters/linux/entropy.rs"]
pub(crate) mod entropy;

#[cfg(all(feature = "entropy", target_os = "macos"))]
#[path = "adapters/macos/entropy.rs"]
pub(crate) mod entropy;

#[cfg(all(feature = "process-metrics", windows))]
#[path = "adapters/windows/process_metrics.rs"]
pub(crate) mod process_metrics;

#[cfg(all(feature = "process-metrics", target_os = "linux"))]
#[path = "adapters/linux/process_metrics.rs"]
pub(crate) mod process_metrics;

#[cfg(all(feature = "process-metrics", target_os = "macos"))]
#[path = "adapters/macos/process_metrics.rs"]
pub(crate) mod process_metrics;

#[cfg(all(feature = "process-observation", windows))]
#[path = "adapters/windows/process_observation.rs"]
pub(crate) mod process_observation;

#[cfg(all(feature = "process-observation", target_os = "linux"))]
#[path = "adapters/linux/process_observation.rs"]
pub(crate) mod process_observation;

#[cfg(all(feature = "process-observation", target_os = "macos"))]
#[path = "adapters/macos/process_observation.rs"]
pub(crate) mod process_observation;

#[cfg(all(feature = "process-security", windows))]
#[path = "adapters/windows/process_security.rs"]
pub(crate) mod process_security;

#[cfg(all(feature = "process-security", target_os = "linux"))]
#[path = "adapters/linux/process_security.rs"]
pub(crate) mod process_security;

#[cfg(all(feature = "process-security", target_os = "macos"))]
#[path = "adapters/macos/process_security.rs"]
pub(crate) mod process_security;

#[cfg(all(feature = "process-reference", windows))]
pub(crate) use crate::adapters::windows::process_reference;

#[cfg(all(feature = "process-containment", windows))]
#[path = "adapters/windows/process_containment.rs"]
pub(crate) mod process_containment;

#[cfg(all(feature = "app-container-process", windows))]
#[path = "adapters/windows/app_container_process.rs"]
pub(crate) mod app_container_process;

#[cfg(all(
    feature = "app-container-process",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/app_container_process.rs"]
pub(crate) mod app_container_process;

#[cfg(all(
    feature = "process-containment",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/process_containment.rs"]
pub(crate) mod process_containment;

#[cfg(all(feature = "process-reference", target_os = "linux"))]
#[path = "adapters/linux/process_reference.rs"]
pub(crate) mod process_reference;

#[cfg(all(feature = "process-reference", target_os = "macos"))]
#[path = "adapters/macos/process_reference.rs"]
pub(crate) mod process_reference;

#[cfg(all(feature = "process-spawn", windows))]
#[path = "adapters/windows/process_spawn.rs"]
pub(crate) mod process_spawn;

#[cfg(all(feature = "process-spawn", target_os = "linux"))]
#[path = "adapters/linux/process_spawn.rs"]
pub(crate) mod process_spawn;

#[cfg(all(feature = "process-spawn", target_os = "macos"))]
#[path = "adapters/macos/process_spawn.rs"]
pub(crate) mod process_spawn;

#[cfg(all(feature = "clipboard", target_os = "linux"))]
#[path = "adapters/linux/clipboard.rs"]
pub(crate) mod clipboard;

#[cfg(all(feature = "clipboard", target_os = "macos"))]
#[path = "adapters/macos/clipboard.rs"]
pub(crate) mod clipboard;

#[cfg(all(
    any(feature = "filesystem-conventions", feature = "filesystem"),
    windows
))]
#[path = "adapters/windows/filesystem.rs"]
pub(crate) mod filesystem;

#[cfg(all(feature = "file-identity", windows))]
#[path = "adapters/windows/file_identity.rs"]
pub(crate) mod file_identity;

#[cfg(all(
    feature = "file-identity",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/file_identity.rs"]
pub(crate) mod file_identity;

#[cfg(windows)]
#[path = "adapters/windows/local_clock.rs"]
pub(crate) mod local_clock;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/local_clock.rs"]
pub(crate) mod local_clock;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/local_clock.rs"]
pub(crate) mod local_clock;

#[cfg(all(feature = "font", windows))]
#[path = "adapters/windows/font.rs"]
pub(crate) mod font;

#[cfg(all(feature = "font", target_os = "linux"))]
#[path = "adapters/linux/font.rs"]
pub(crate) mod font;

#[cfg(all(feature = "font", target_os = "macos"))]
#[path = "adapters/macos/font.rs"]
pub(crate) mod font;

#[cfg(all(feature = "font", any(target_os = "linux", target_os = "macos")))]
#[path = "adapters/unix/font_raster.rs"]
pub(crate) mod portable_font_raster;

#[cfg(all(
    any(feature = "filesystem-conventions", feature = "filesystem"),
    target_os = "linux"
))]
#[path = "adapters/linux/filesystem.rs"]
pub(crate) mod filesystem;

#[cfg(all(
    any(feature = "filesystem-conventions", feature = "filesystem"),
    target_os = "macos"
))]
#[path = "adapters/macos/filesystem.rs"]
pub(crate) mod filesystem;

#[cfg(all(feature = "locking", windows))]
#[path = "adapters/windows/locking.rs"]
pub(crate) mod locking;

#[cfg(all(feature = "locking", target_os = "linux"))]
#[path = "adapters/linux/locking.rs"]
pub(crate) mod locking;

#[cfg(all(feature = "locking", target_os = "macos"))]
#[path = "adapters/macos/locking.rs"]
pub(crate) mod locking;

#[cfg(all(feature = "ipc", windows))]
#[path = "adapters/windows/ipc.rs"]
pub mod ipc;

#[cfg(all(feature = "input", windows))]
#[path = "adapters/windows/input.rs"]
pub(crate) mod input;

#[cfg(all(
    any(feature = "window-enum", feature = "window-op"),
    target_os = "macos"
))]
#[path = "adapters/macos/foreign_windows.rs"]
pub(crate) mod macos_foreign_windows;

#[cfg(all(feature = "window-enum", windows))]
#[path = "adapters/windows/window_enumerate.rs"]
pub(crate) mod window_enumerate;

#[cfg(all(feature = "a11y-tree", target_os = "linux"))]
#[path = "adapters/linux/accessibility_tree.rs"]
pub(crate) mod accessibility_tree;

#[cfg(all(feature = "a11y-tree", not(target_os = "linux")))]
#[path = "adapters/unix/accessibility_tree.rs"]
pub(crate) mod accessibility_tree;

#[cfg(all(feature = "a11y-publish", target_os = "linux"))]
#[path = "adapters/linux/accessibility_publish.rs"]
pub(crate) mod accessibility_publish;

#[cfg(all(feature = "a11y-publish", not(target_os = "linux")))]
#[path = "adapters/unix/accessibility_publish.rs"]
pub(crate) mod accessibility_publish;

#[cfg(all(feature = "window-enum", target_os = "linux"))]
#[path = "adapters/linux/window_enumerate.rs"]
pub(crate) mod window_enumerate;

#[cfg(all(feature = "window-enum", target_os = "macos"))]
#[path = "adapters/macos/window_enumerate.rs"]
pub(crate) mod window_enumerate;

#[cfg(all(
    feature = "window-enum",
    not(any(windows, target_os = "linux", target_os = "macos"))
))]
#[path = "adapters/unix/window_enumerate.rs"]
pub(crate) mod window_enumerate;

#[cfg(all(feature = "window-op", windows))]
#[path = "adapters/windows/window_op.rs"]
pub(crate) mod window_op;

#[cfg(all(feature = "window-op", target_os = "macos"))]
#[path = "adapters/macos/window_op.rs"]
pub(crate) mod window_op;

#[cfg(all(feature = "window-op", target_os = "linux"))]
#[path = "adapters/linux/window_op.rs"]
pub(crate) mod window_op;

#[cfg(all(
    feature = "window-op",
    not(any(windows, target_os = "macos", target_os = "linux"))
))]
#[path = "adapters/unix/window_op.rs"]
pub(crate) mod window_op;

#[cfg(all(feature = "input-inject", windows))]
#[path = "adapters/windows/input_inject.rs"]
pub(crate) mod input_inject;

#[cfg(all(feature = "input-inject", target_os = "linux"))]
#[path = "adapters/linux/input_inject.rs"]
pub(crate) mod input_inject;

#[cfg(all(feature = "input-inject", not(any(windows, target_os = "linux"))))]
#[path = "adapters/unix/input_inject.rs"]
pub(crate) mod input_inject;

#[cfg(all(feature = "activation", windows))]
#[path = "adapters/windows/activation.rs"]
pub(crate) mod activation;

#[cfg(all(feature = "window", windows))]
#[path = "adapters/windows/process_window.rs"]
pub(crate) mod process_window;

#[cfg(all(feature = "window", windows))]
#[path = "adapters/windows/window.rs"]
pub(crate) mod window;

#[cfg(all(feature = "window", feature = "input", windows))]
#[path = "adapters/windows/control_window.rs"]
pub(crate) mod control_window;

#[cfg(all(feature = "window", feature = "input", windows))]
#[path = "adapters/windows/reentrant_dispatch.rs"]
pub(crate) mod reentrant_dispatch;

#[cfg(all(feature = "window", feature = "input", target_os = "linux"))]
#[path = "adapters/linux/control_window.rs"]
pub(crate) mod control_window;

#[cfg(all(feature = "window", feature = "input", target_os = "macos"))]
#[path = "adapters/macos/control_window.rs"]
pub(crate) mod control_window;

#[cfg(all(feature = "window", target_os = "linux"))]
#[path = "adapters/linux/window.rs"]
pub(crate) mod window;

#[cfg(all(feature = "window", target_os = "macos"))]
#[path = "adapters/macos/window.rs"]
pub(crate) mod window;

#[cfg(all(feature = "window", target_os = "linux"))]
#[path = "adapters/linux/process_window.rs"]
pub(crate) mod process_window;

#[cfg(all(feature = "window", target_os = "macos"))]
#[path = "adapters/macos/process_window.rs"]
pub(crate) mod process_window;

#[cfg(all(feature = "activation", target_os = "linux"))]
#[path = "adapters/linux/activation.rs"]
pub(crate) mod activation;

#[cfg(all(feature = "activation", target_os = "macos"))]
#[path = "adapters/macos/activation.rs"]
pub(crate) mod activation;

#[cfg(all(feature = "ime", windows))]
#[path = "adapters/windows/ime.rs"]
pub(crate) mod ime;

#[cfg(all(feature = "ime", target_os = "linux"))]
#[path = "adapters/linux/ime.rs"]
pub(crate) mod ime;

#[cfg(all(feature = "ime", target_os = "macos"))]
#[path = "adapters/macos/ime.rs"]
pub(crate) mod ime;

#[cfg(all(feature = "input", target_os = "linux"))]
#[path = "adapters/linux/input.rs"]
pub(crate) mod input;

#[cfg(all(feature = "input", target_os = "macos"))]
#[path = "adapters/macos/input.rs"]
pub(crate) mod input;

#[cfg(all(feature = "ipc", target_os = "linux"))]
#[path = "adapters/linux/ipc.rs"]
pub mod ipc;

#[cfg(all(feature = "ipc", target_os = "macos"))]
#[path = "adapters/macos/ipc.rs"]
pub mod ipc;

#[cfg(all(feature = "process", windows))]
#[path = "adapters/windows/process.rs"]
pub(crate) mod process;

#[cfg(all(feature = "parent-console", windows))]
#[path = "adapters/windows/parent_console.rs"]
pub(crate) mod parent_console;

#[cfg(all(feature = "parent-console", target_os = "linux"))]
#[path = "adapters/linux/parent_console.rs"]
pub(crate) mod parent_console;

#[cfg(all(feature = "parent-console", target_os = "macos"))]
#[path = "adapters/macos/parent_console.rs"]
pub(crate) mod parent_console;

#[cfg(all(windows, any(feature = "pty", feature = "parent-console")))]
#[path = "adapters/windows/console.rs"]
pub(crate) mod console;

#[cfg(all(feature = "process-control", windows))]
#[path = "adapters/windows/process_control.rs"]
pub(crate) mod process_control;

#[cfg(all(feature = "runtime", windows))]
#[path = "adapters/windows/runtime.rs"]
pub(crate) mod runtime;

#[cfg(all(feature = "screenshot", windows))]
#[path = "adapters/windows/ui_screenshot.rs"]
pub(crate) mod ui_screenshot;

#[cfg(all(feature = "screenshot", unix))]
#[path = "adapters/unix/png.rs"]
pub(crate) mod portable_png;

#[cfg(all(feature = "webview", windows))]
#[path = "adapters/windows/webview.rs"]
pub(crate) mod webview;

#[cfg(all(feature = "webview", target_os = "linux"))]
#[path = "adapters/linux/webview.rs"]
pub(crate) mod webview;

#[cfg(all(feature = "webview", target_os = "macos"))]
#[path = "adapters/macos/webview.rs"]
pub(crate) mod webview;

#[cfg(all(feature = "screenshot", target_os = "linux"))]
#[path = "adapters/linux/ui_screenshot.rs"]
pub(crate) mod ui_screenshot;

#[cfg(all(feature = "screenshot", target_os = "macos"))]
#[path = "adapters/macos/ui_screenshot.rs"]
pub(crate) mod ui_screenshot;

#[cfg(all(feature = "runtime", target_os = "linux"))]
#[path = "adapters/linux/runtime.rs"]
pub(crate) mod runtime;

#[cfg(all(feature = "runtime", target_os = "macos"))]
#[path = "adapters/macos/runtime.rs"]
pub(crate) mod runtime;

#[cfg(all(feature = "pty", windows))]
#[path = "adapters/windows/pty.rs"]
pub(crate) mod pty;

#[cfg(all(feature = "pty", target_os = "linux"))]
#[path = "adapters/linux/pty.rs"]
pub(crate) mod pty;

#[cfg(all(feature = "pty", target_os = "macos"))]
#[path = "adapters/macos/pty.rs"]
pub(crate) mod pty;

#[cfg(all(feature = "process", target_os = "linux"))]
#[path = "adapters/linux/process.rs"]
pub(crate) mod process;

#[cfg(all(feature = "process", target_os = "macos"))]
#[path = "adapters/macos/process.rs"]
pub(crate) mod process;

#[cfg(all(
    feature = "process-control",
    any(target_os = "linux", target_os = "macos")
))]
#[path = "adapters/unix/process_control.rs"]
pub(crate) mod process_control;

/// Cross-platform console-attachment surface.
///
/// The target `cfg`s live HERE by boundary policy (see
/// `src/platform/boundary_tests.rs`): `process.rs` re-exports these items
/// without any target selection of its own.
#[cfg(feature = "process")]
pub mod console_surface {
    /// One duplicated caller-visible std handle. Windows: a real console /
    /// pipe / file handle; Unix: the equivalent owned descriptor (unused —
    /// Unix workers inherit stdio directly).
    #[cfg(windows)]
    pub type StdHandle = std::os::windows::io::OwnedHandle;
    #[cfg(not(windows))]
    pub type StdHandle = std::os::fd::OwnedFd;

    /// Duplicate the caller-visible stdin/stdout/stderr handles for explicit
    /// child stdio wiring (`[stdin, stdout, stderr]`, `None` where the
    /// process holds no valid handle). Attach the parent console first
    /// (`ScopedConsole`) so console-backed slots are populated. Unix returns
    /// all-`None`: workers there inherit stdio without duplication.
    pub fn duplicated_std_handles() -> [Option<StdHandle>; 3] {
        #[cfg(windows)]
        {
            super::console::duplicate_std_handles()
        }
        #[cfg(not(windows))]
        {
            [None, None, None]
        }
    }

    /// Opaque guard that keeps the parent console attached.
    ///
    /// On Windows this wraps the shared `ConsoleGuard`; the console is
    /// released when the guard is dropped. Spawn this before any `println!`
    /// calls in a `windows_subsystem = "windows"` binary.
    ///
    /// On Unix this is a zero-size no-op.
    pub struct ScopedConsole {
        #[cfg(windows)]
        _inner: Option<super::console::ConsoleGuard>,
        #[cfg(not(windows))]
        _unused: (),
    }

    impl ScopedConsole {
        /// Attach to the parent console. Returns `None` when there is no
        /// parent console (double-click launch on Windows).
        pub fn attach_parent() -> Option<Self> {
            #[cfg(windows)]
            {
                super::console::ConsoleGuard::attach_parent()
                    .ok()
                    .map(|inner| Self {
                        _inner: Some(inner),
                    })
            }
            #[cfg(not(windows))]
            {
                Some(Self { _unused: () })
            }
        }

        /// Attach to the parent console without suppressing console control
        /// events, so `Ctrl+C` keeps its default terminate behavior. This is
        /// the variant for CLI worker processes; the GUI host uses
        /// `attach_parent`, which must survive child-console control events.
        pub fn attach_parent_with_default_interrupts() -> Option<Self> {
            #[cfg(windows)]
            {
                super::console::ConsoleGuard::attach_parent_with_default_interrupts()
                    .ok()
                    .map(|inner| Self {
                        _inner: Some(inner),
                    })
            }
            #[cfg(not(windows))]
            {
                Some(Self { _unused: () })
            }
        }
    }
}
