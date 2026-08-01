//! The sole compile-time operating-system adapter selector.
//!
//! Facade services call these private adapter modules through typed contracts;
//! product modules never import an OS-specific implementation.

#[cfg(windows)]
#[path = "adapters/windows/frontend.rs"]
pub(crate) mod frontend;
#[cfg(windows)]
#[path = "adapters/windows/remote_frontend.rs"]
pub(crate) mod remote_frontend;

#[cfg(windows)]
#[path = "adapters/windows/script_http.rs"]
pub(crate) mod script_http;

#[cfg(windows)]
#[path = "adapters/windows/control_center.rs"]
pub(crate) mod control_center;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/frontend.rs"]
pub(crate) mod frontend;
#[cfg(target_os = "linux")]
#[path = "adapters/linux/script_http.rs"]
pub(crate) mod script_http;

#[cfg(target_os = "linux")]
#[path = "adapters/linux/control_center.rs"]
pub(crate) mod control_center;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/frontend.rs"]
pub(crate) mod frontend;
#[cfg(target_os = "macos")]
#[path = "adapters/macos/script_http.rs"]
pub(crate) mod script_http;

#[cfg(target_os = "macos")]
#[path = "adapters/macos/control_center.rs"]
pub(crate) mod control_center;
