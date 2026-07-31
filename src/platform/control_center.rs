//! Native Control Center platform services.
//!
//! The Control Center owns projection semantics.  This adapter owns native
//! window operations, filesystem durability details, and renderer strategy
//! selection so product code does not branch on an operating-system target.

use std::{fs::OpenOptions, io, path::Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScreenshotStrategy {
    DirectNativeWindow,
    RendererRequest,
    Unsupported,
}

pub(crate) const fn screenshot_strategy() -> ScreenshotStrategy {
    selected::SCREENSHOT_STRATEGY
}

pub(crate) const fn screenshot_capability() -> &'static str {
    match screenshot_strategy() {
        ScreenshotStrategy::DirectNativeWindow | ScreenshotStrategy::RendererRequest => "available",
        ScreenshotStrategy::Unsupported => "unavailable",
    }
}

/// Configure a newly created state directory with private host permissions.
pub(crate) fn protect_state_directory(path: &Path) -> io::Result<()> {
    selected::protect_state_directory(path)
}

/// Construct exclusive-create options with private host permissions.
pub(crate) fn private_create_new_options() -> OpenOptions {
    selected::private_create_new_options()
}

/// Atomically replace one state file with host-appropriate durability.
pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    selected::replace_file(source, destination)
}

/// Restore/focus a pre-existing native Control Center window when supported.
/// A missing or stale native handle is a benign no-op because the registry
/// request remains the causal projection-refresh path.
pub(crate) fn focus_existing_window(raw_handle: i64, no_activate: bool) {
    selected::focus_existing_window(raw_handle, no_activate);
}

/// Capture a native window for the direct-native strategy.
pub(crate) fn capture_native_window_png(raw_handle: i64, output: &Path) -> io::Result<()> {
    selected::capture_native_window_png(raw_handle, output)
}

#[cfg(target_os = "windows")]
mod selected {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
        UI::WindowsAndMessaging::{
            IsWindow, SW_RESTORE, SW_SHOWNOACTIVATE, SetForegroundWindow, ShowWindowAsync,
        },
    };

    pub(super) const SCREENSHOT_STRATEGY: ScreenshotStrategy =
        ScreenshotStrategy::DirectNativeWindow;

    pub(super) fn protect_state_directory(_path: &Path) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn private_create_new_options() -> OpenOptions {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        options
    }

    pub(super) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
        let source = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn focus_existing_window(raw_handle: i64, no_activate: bool) {
        let window = raw_handle as isize as *mut std::ffi::c_void;
        if window.is_null() || unsafe { IsWindow(window) } == 0 {
            return;
        }
        unsafe {
            ShowWindowAsync(
                window,
                if no_activate {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_RESTORE
                },
            );
            if !no_activate {
                SetForegroundWindow(window);
            }
        }
    }

    pub(super) fn capture_native_window_png(raw_handle: i64, output: &Path) -> io::Result<()> {
        let window = raw_handle as isize as *mut std::ffi::c_void;
        if window.is_null() || unsafe { IsWindow(window) } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "control_center_screenshot_window_unavailable",
            ));
        }
        crate::platform::windows::screenshot::save_png(
            window,
            output,
            crate::platform::windows::screenshot::CaptureArea::Window,
        )
        .map_err(|error| io::Error::other(error.to_string()))
    }
}

#[cfg(unix)]
mod selected {
    use super::*;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    #[cfg(target_os = "macos")]
    pub(super) const SCREENSHOT_STRATEGY: ScreenshotStrategy = ScreenshotStrategy::RendererRequest;
    #[cfg(not(target_os = "macos"))]
    pub(super) const SCREENSHOT_STRATEGY: ScreenshotStrategy = ScreenshotStrategy::Unsupported;

    pub(super) fn protect_state_directory(path: &Path) -> io::Result<()> {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }

    pub(super) fn private_create_new_options() -> OpenOptions {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        options
    }

    pub(super) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
        fs::rename(source, destination)
    }

    pub(super) fn focus_existing_window(_raw_handle: i64, _no_activate: bool) {}

    pub(super) fn capture_native_window_png(_raw_handle: i64, _output: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "control_center_direct_native_capture_unavailable",
        ))
    }
}

#[cfg(not(any(target_os = "windows", unix)))]
mod selected {
    use super::*;

    pub(super) const SCREENSHOT_STRATEGY: ScreenshotStrategy = ScreenshotStrategy::Unsupported;

    pub(super) fn protect_state_directory(_path: &Path) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn private_create_new_options() -> OpenOptions {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        options
    }

    pub(super) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
        fs::rename(source, destination)
    }

    pub(super) fn focus_existing_window(_raw_handle: i64, _no_activate: bool) {}

    pub(super) fn capture_native_window_png(_raw_handle: i64, _output: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "control_center_direct_native_capture_unavailable",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_agrees_with_strategy() {
        let expected = match screenshot_strategy() {
            ScreenshotStrategy::DirectNativeWindow | ScreenshotStrategy::RendererRequest => {
                "available"
            }
            ScreenshotStrategy::Unsupported => "unavailable",
        };
        assert_eq!(screenshot_capability(), expected);
    }

    #[test]
    fn private_create_is_exclusive() {
        let root = std::env::temp_dir().join(format!(
            "agenterm-platform-cc-private-create-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&root);
        private_create_new_options()
            .open(&root)
            .expect("first exclusive create");
        assert_eq!(
            private_create_new_options()
                .open(&root)
                .expect_err("second create must fail")
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        std::fs::remove_file(root).expect("remove test file");
    }
}
