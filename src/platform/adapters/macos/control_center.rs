//! macOS Control Center native operations.

use crate::platform::control_center::ScreenshotStrategy;
use std::{
    fs::{self, OpenOptions},
    io,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};
pub(crate) const fn screenshot_strategy() -> ScreenshotStrategy {
    ScreenshotStrategy::RendererRequest
}
pub(crate) fn protect_state_directory(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
pub(crate) fn private_create_new_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    options
}
pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}
pub(crate) fn focus_existing_window(_raw_handle: i64, _no_activate: bool) {}
pub(crate) fn capture_native_window_png(_raw_handle: i64, _output: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "control_center_direct_native_capture_unavailable",
    ))
}
