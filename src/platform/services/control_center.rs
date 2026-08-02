//! OS-neutral Control Center native-operation facade.

use std::{borrow::Cow, fs::OpenOptions, io, path::Path};

use agenterm_platform::{
    contract::ui_screenshot::UiScreenshotError,
    screenshot::{NativeCaptureArea, ScreenshotWindowHandle},
};

use crate::platform::control_center::ScreenshotStrategy;

pub(crate) fn screenshot_strategy() -> ScreenshotStrategy {
    crate::platform::control_center_screenshot_strategy()
}

pub(crate) fn protect_state_directory(path: &Path) -> io::Result<()> {
    agenterm_platform::filesystem::protect_private_directory(path)
}

pub(crate) fn private_create_new_options() -> OpenOptions {
    agenterm_platform::filesystem::private_create_new_options()
}

pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    agenterm_platform::filesystem::replace_file(source, destination)
}

pub(crate) fn capture_native_window_png(
    raw_handle: i64,
    output: &Path,
) -> Result<(), UiScreenshotError> {
    // SAFETY: the registry value identifies the live Control Center window and
    // capture is synchronous, so the owner keeps it alive for the whole call.
    let Some(window) = (unsafe { ScreenshotWindowHandle::from_raw(raw_handle as isize) }) else {
        return Err(UiScreenshotError::Failed {
            code: Cow::Borrowed("control_center_screenshot_window_unavailable"),
            message: "Control Center did not publish a screenshot window handle".to_owned(),
        });
    };
    agenterm_platform::screenshot::capture_native_window_png(
        window,
        output,
        NativeCaptureArea::Window,
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_control_center_renderers_own_linux_and_macos_capture_requests() {
        let strategy = screenshot_strategy();
        let expected = crate::platform::control_center_screenshot_strategy();
        assert_eq!(strategy, expected);
    }

    #[test]
    fn invalid_native_capture_handles_remain_typed_failures() {
        assert!(matches!(
            capture_native_window_png(0, Path::new("unused.png")),
            Err(UiScreenshotError::Failed { code, .. })
                if code == "control_center_screenshot_window_unavailable"
        ));
    }
}
