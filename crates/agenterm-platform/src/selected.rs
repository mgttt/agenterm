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

#[cfg(all(feature = "filesystem", windows))]
#[path = "adapters/windows/filesystem.rs"]
pub(crate) mod filesystem;

#[cfg(all(feature = "filesystem", target_os = "linux"))]
#[path = "adapters/linux/filesystem.rs"]
pub(crate) mod filesystem;

#[cfg(all(feature = "filesystem", target_os = "macos"))]
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

#[cfg(all(feature = "process", windows))]
#[path = "adapters/windows/process.rs"]
pub(crate) mod process;

#[cfg(all(feature = "process", windows))]
#[path = "adapters/windows/runtime.rs"]
pub(crate) mod runtime;

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
