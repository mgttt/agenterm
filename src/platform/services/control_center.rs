//! OS-neutral Control Center native-operation facade.

use std::{fs::OpenOptions, io, path::Path};

use crate::platform::{control_center::ScreenshotStrategy, selected};

pub(crate) const fn screenshot_strategy() -> ScreenshotStrategy {
    selected::control_center::screenshot_strategy()
}

pub(crate) fn protect_state_directory(path: &Path) -> io::Result<()> {
    selected::control_center::protect_state_directory(path)
}

pub(crate) fn private_create_new_options() -> OpenOptions {
    selected::control_center::private_create_new_options()
}

pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    selected::control_center::replace_file(source, destination)
}

pub(crate) fn focus_existing_window(raw_handle: i64, no_activate: bool) {
    selected::control_center::focus_existing_window(raw_handle, no_activate);
}

pub(crate) fn capture_native_window_png(raw_handle: i64, output: &Path) -> io::Result<()> {
    selected::control_center::capture_native_window_png(raw_handle, output)
}
