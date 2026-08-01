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
#[path = "adapters/windows/remote_frontend.rs"]
pub(crate) mod remote_frontend;

#[cfg(windows)]
#[path = "adapters/windows/script_http.rs"]
pub(crate) mod script_http;

#[cfg(windows)]
#[path = "adapters/windows/script_host.rs"]
pub(crate) mod script_host;

#[cfg(windows)]
#[path = "adapters/windows/supervisor_audit.rs"]
pub(crate) mod supervisor_audit;

#[cfg(windows)]
#[path = "adapters/windows/control_center.rs"]
pub(crate) mod control_center;

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
#[path = "adapters/linux/script_http.rs"]
pub(crate) mod script_http;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/script_host.rs"]
pub(crate) mod script_host;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/supervisor_audit.rs"]
pub(crate) mod supervisor_audit;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/control_center.rs"]
pub(crate) mod control_center;

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
#[path = "adapters/macos/script_http.rs"]
pub(crate) mod script_http;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/script_host.rs"]
pub(crate) mod script_host;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/supervisor_audit.rs"]
pub(crate) mod supervisor_audit;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/control_center.rs"]
pub(crate) mod control_center;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/ui_screenshot.rs"]
pub(crate) mod ui_screenshot;
