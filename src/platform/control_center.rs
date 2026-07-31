//! Compatibility projection for Control Center native operations.
//!
//! The selected platform adapter owns host filesystem durability, window focus,
//! and direct capture mechanics. Product code uses this stable facade.

use std::{fs::OpenOptions, io, path::Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScreenshotStrategy {
    DirectNativeWindow,
    RendererRequest,
    Unsupported,
}

pub(crate) const fn screenshot_strategy() -> ScreenshotStrategy {
    crate::platform::services::control_center::screenshot_strategy()
}

pub(crate) const fn screenshot_capability() -> &'static str {
    match screenshot_strategy() {
        ScreenshotStrategy::DirectNativeWindow | ScreenshotStrategy::RendererRequest => "available",
        ScreenshotStrategy::Unsupported => "unavailable",
    }
}

pub(crate) fn protect_state_directory(path: &Path) -> io::Result<()> {
    crate::platform::services::control_center::protect_state_directory(path)
}

pub(crate) fn private_create_new_options() -> OpenOptions {
    crate::platform::services::control_center::private_create_new_options()
}

pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    crate::platform::services::control_center::replace_file(source, destination)
}

pub(crate) fn focus_existing_window(raw_handle: i64, no_activate: bool) {
    crate::platform::services::control_center::focus_existing_window(raw_handle, no_activate);
}

pub(crate) fn capture_native_window_png(raw_handle: i64, output: &Path) -> io::Result<()> {
    crate::platform::services::control_center::capture_native_window_png(raw_handle, output)
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
