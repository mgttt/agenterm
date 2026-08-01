//! The only compile-time native adapter selection for enabled capabilities.

#[cfg(all(feature = "process", windows))]
#[path = "adapters/windows/process.rs"]
pub(crate) mod process;

#[cfg(all(feature = "process", target_os = "linux"))]
#[path = "adapters/linux/process.rs"]
pub(crate) mod process;

#[cfg(all(feature = "process", target_os = "macos"))]
#[path = "adapters/macos/process.rs"]
pub(crate) mod process;
