//! The sole compile-time operating-system adapter selector.
//!
//! Facade services call these private adapter modules through typed contracts;
//! product modules never import an OS-specific implementation.

#[cfg(windows)]
#[path = "adapters/windows/ipc.rs"]
pub(crate) mod ipc;

#[cfg(windows)]
#[path = "adapters/windows/script_http.rs"]
pub(crate) mod script_http;

#[cfg(windows)]
#[path = "adapters/windows/script_clipboard.rs"]
pub(crate) mod script_clipboard;

#[cfg(windows)]
#[path = "adapters/windows/script_files.rs"]
pub(crate) mod script_files;

#[cfg(windows)]
#[path = "adapters/windows/process.rs"]
pub(crate) mod process;

#[cfg(windows)]
#[path = "adapters/windows/script_window.rs"]
pub(crate) mod script_window;

#[cfg(windows)]
#[path = "adapters/windows/script_stream.rs"]
pub(crate) mod script_stream;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/ipc.rs"]
pub(crate) mod ipc;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/script_http.rs"]
pub(crate) mod script_http;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/script_clipboard.rs"]
pub(crate) mod script_clipboard;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/script_files.rs"]
pub(crate) mod script_files;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/process.rs"]
pub(crate) mod process;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/script_window.rs"]
pub(crate) mod script_window;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/script_stream.rs"]
pub(crate) mod script_stream;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/ipc.rs"]
pub(crate) mod ipc;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/script_http.rs"]
pub(crate) mod script_http;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/script_clipboard.rs"]
pub(crate) mod script_clipboard;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/script_files.rs"]
pub(crate) mod script_files;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/process.rs"]
pub(crate) mod process;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/script_window.rs"]
pub(crate) mod script_window;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/script_stream.rs"]
pub(crate) mod script_stream;
