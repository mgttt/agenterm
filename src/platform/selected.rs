//! The sole compile-time operating-system adapter selector.
//!
//! Facade services call these private adapter modules through typed contracts;
//! product modules never import an OS-specific implementation.

#[cfg(windows)]
#[path = "adapters/windows/frontend.rs"]
pub(crate) mod frontend;
#[cfg(windows)]
#[path = "adapters/windows/ipc.rs"]
pub(crate) mod ipc;
#[cfg(windows)]
#[path = "adapters/windows/native/mod.rs"]
pub(crate) mod native;
#[cfg(windows)]
#[path = "adapters/windows/pty.rs"]
pub(crate) mod pty;
#[cfg(windows)]
#[path = "adapters/windows/remote_frontend.rs"]
pub(crate) mod remote_frontend;

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

#[cfg(windows)]
#[path = "adapters/windows/script_host.rs"]
pub(crate) mod script_host;

#[cfg(windows)]
#[path = "adapters/windows/supervisor_audit.rs"]
pub(crate) mod supervisor_audit;

#[cfg(windows)]
#[path = "adapters/windows/paths.rs"]
pub(crate) mod paths;

#[cfg(windows)]
#[path = "adapters/windows/control_center.rs"]
pub(crate) mod control_center;
#[cfg(windows)]
#[path = "adapters/windows/control_center_shell.rs"]
pub(crate) mod control_center_shell;

#[cfg(windows)]
#[path = "adapters/windows/runtime.rs"]
pub(crate) mod runtime;

#[cfg(windows)]
#[path = "adapters/windows/webview.rs"]
pub(crate) mod webview;

#[cfg(windows)]
#[path = "adapters/windows/ui_clipboard.rs"]
pub(crate) mod ui_clipboard;
#[cfg(windows)]
#[path = "adapters/windows/ui_font.rs"]
pub(crate) mod ui_font;
#[cfg(windows)]
#[path = "adapters/windows/ui_screenshot.rs"]
pub(crate) mod ui_screenshot;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/frontend.rs"]
pub(crate) mod frontend;
#[cfg(target_os = "linux")]
#[path = "adapters/linux/ipc.rs"]
pub(crate) mod ipc;
#[cfg(target_os = "linux")]
#[path = "adapters/linux/native/mod.rs"]
pub(crate) mod native;
#[cfg(target_os = "linux")]
#[path = "adapters/linux/pty.rs"]
pub(crate) mod pty;
#[cfg(target_os = "linux")]
#[path = "scale.rs"]
pub(crate) mod scale_contract;

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

#[cfg(target_os = "linux")]
#[path = "adapters/linux/script_host.rs"]
pub(crate) mod script_host;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/supervisor_audit.rs"]
pub(crate) mod supervisor_audit;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/paths.rs"]
pub(crate) mod paths;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/control_center.rs"]
pub(crate) mod control_center;
#[cfg(target_os = "linux")]
#[path = "adapters/linux/control_center_shell.rs"]
pub(crate) mod control_center_shell;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/runtime.rs"]
pub(crate) mod runtime;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/scale.rs"]
pub(crate) mod scale;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/webview.rs"]
pub(crate) mod webview;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/ui_clipboard.rs"]
pub(crate) mod ui_clipboard;
#[cfg(target_os = "linux")]
#[path = "adapters/linux/ui_font.rs"]
pub(crate) mod ui_font;
#[cfg(target_os = "linux")]
#[path = "adapters/linux/ui_screenshot.rs"]
pub(crate) mod ui_screenshot;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/frontend.rs"]
pub(crate) mod frontend;
#[cfg(target_os = "macos")]
#[path = "adapters/macos/ipc.rs"]
pub(crate) mod ipc;
#[cfg(target_os = "macos")]
#[path = "adapters/macos/native/mod.rs"]
pub(crate) mod native;
#[cfg(target_os = "macos")]
#[path = "adapters/macos/pty.rs"]
pub(crate) mod pty;
#[cfg(target_os = "macos")]
#[path = "scale.rs"]
pub(crate) mod scale_contract;

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

#[cfg(target_os = "macos")]
#[path = "adapters/macos/script_host.rs"]
pub(crate) mod script_host;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/supervisor_audit.rs"]
pub(crate) mod supervisor_audit;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/paths.rs"]
pub(crate) mod paths;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/control_center.rs"]
pub(crate) mod control_center;
#[cfg(target_os = "macos")]
#[path = "adapters/macos/control_center_shell.rs"]
pub(crate) mod control_center_shell;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/runtime.rs"]
pub(crate) mod runtime;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/webview.rs"]
pub(crate) mod webview;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/ui_clipboard.rs"]
pub(crate) mod ui_clipboard;
#[cfg(target_os = "macos")]
#[path = "adapters/macos/ui_font.rs"]
pub(crate) mod ui_font;
#[cfg(target_os = "macos")]
#[path = "adapters/macos/ui_screenshot.rs"]
pub(crate) mod ui_screenshot;
