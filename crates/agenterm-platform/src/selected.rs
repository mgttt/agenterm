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

#[cfg(all(feature = "console-interrupt", windows))]
#[path = "adapters/windows/console_interrupt.rs"]
pub(crate) mod console_interrupt;

#[cfg(all(feature = "console-interrupt", target_os = "linux"))]
#[path = "adapters/linux/console_interrupt.rs"]
pub(crate) mod console_interrupt;

#[cfg(all(feature = "console-interrupt", target_os = "macos"))]
#[path = "adapters/macos/console_interrupt.rs"]
pub(crate) mod console_interrupt;

#[cfg(all(feature = "filesystem-cleanup", windows))]
#[path = "adapters/windows/filesystem_cleanup.rs"]
pub(crate) mod filesystem_cleanup;

#[cfg(all(feature = "filesystem-cleanup", target_os = "linux"))]
#[path = "adapters/linux/filesystem_cleanup.rs"]
pub(crate) mod filesystem_cleanup;

#[cfg(all(feature = "filesystem-cleanup", target_os = "macos"))]
#[path = "adapters/macos/filesystem_cleanup.rs"]
pub(crate) mod filesystem_cleanup;

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

#[cfg(all(feature = "font", windows))]
#[path = "adapters/windows/font.rs"]
pub(crate) mod font;

#[cfg(all(feature = "font", target_os = "linux"))]
#[path = "adapters/linux/font.rs"]
pub(crate) mod font;

#[cfg(all(feature = "font", target_os = "macos"))]
#[path = "adapters/macos/font.rs"]
pub(crate) mod font;

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
pub(crate) mod ipc;

#[cfg(all(feature = "input", windows))]
#[path = "adapters/windows/input.rs"]
pub(crate) mod input;

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
pub(crate) mod ipc;

#[cfg(all(feature = "ipc", target_os = "macos"))]
#[path = "adapters/macos/ipc.rs"]
pub(crate) mod ipc;

#[cfg(all(feature = "process", windows))]
#[path = "adapters/windows/process.rs"]
pub(crate) mod process;

#[cfg(all(feature = "process-control", windows))]
#[path = "adapters/windows/process_control.rs"]
pub(crate) mod process_control;

#[cfg(all(feature = "process", windows))]
#[path = "adapters/windows/runtime.rs"]
pub(crate) mod runtime;

#[cfg(all(feature = "screenshot", windows))]
#[path = "adapters/windows/ui_screenshot.rs"]
pub(crate) mod ui_screenshot;

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

#[cfg(all(feature = "process", target_os = "linux"))]
#[path = "adapters/linux/runtime.rs"]
pub(crate) mod runtime;

#[cfg(all(feature = "process", target_os = "macos"))]
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
