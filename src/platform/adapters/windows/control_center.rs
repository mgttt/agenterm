//! Windows Control Center composition over public platform-crate capabilities.

use std::{fs::OpenOptions, io, path::Path};

use crate::platform::control_center::ScreenshotStrategy;

pub(crate) const fn screenshot_strategy() -> ScreenshotStrategy {
    ScreenshotStrategy::DirectNativeWindow
}

pub(crate) fn protect_state_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub(crate) fn private_create_new_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
}

pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    agenterm_platform::filesystem::replace_file(source, destination)
}

pub(crate) fn focus_existing_window(raw_handle: i64, no_activate: bool) {
    // SAFETY: the registry value identifies the live Control Center window;
    // the selected adapter returns a typed failure if the handle is stale.
    let Some(window) = (unsafe {
        agenterm_platform::activation::NativeWindowHandle::from_raw(raw_handle as isize)
    }) else {
        return;
    };
    let request = if no_activate {
        agenterm_platform::activation::ActivationRequest::ShowWithoutActivation
    } else {
        agenterm_platform::activation::ActivationRequest::RestoreAndActivate
    };
    let _ = agenterm_platform::activation::apply(window, request);
}

pub(crate) fn capture_native_window_png(raw_handle: i64, output: &Path) -> io::Result<()> {
    // SAFETY: the registry value identifies the Control Center-owned window and
    // capture is synchronous; stale values fail through the typed crate API.
    let window = unsafe {
        agenterm_platform::screenshot::ScreenshotWindowHandle::from_raw(raw_handle as isize)
    }
    .ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "control_center_screenshot_window_unavailable",
        )
    })?;
    agenterm_platform::screenshot::capture_native_window_png(
        window,
        output,
        agenterm_platform::screenshot::NativeCaptureArea::Window,
    )
    .map(|_| ())
    .map_err(|error| io::Error::other(format!("{}: {}", error.code(), error)))
}
