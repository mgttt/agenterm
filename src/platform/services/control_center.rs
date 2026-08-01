//! OS-neutral Control Center native-operation facade.

use std::{borrow::Cow, fs::OpenOptions, io, path::Path};

use agenterm_platform::{
    PlatformKind,
    activation::{ActivationError, ActivationRequest, NativeWindowHandle},
    contract::ui_screenshot::UiScreenshotError,
    screenshot::{NativeCaptureArea, ScreenshotWindowHandle},
};

use crate::platform::control_center::ScreenshotStrategy;

pub(crate) const fn screenshot_strategy() -> ScreenshotStrategy {
    screenshot_strategy_for(agenterm_platform::platform_kind())
}

const fn screenshot_strategy_for(platform: PlatformKind) -> ScreenshotStrategy {
    match platform {
        PlatformKind::Windows => ScreenshotStrategy::DirectNativeWindow,
        PlatformKind::Macos => ScreenshotStrategy::RendererRequest,
        PlatformKind::Linux => ScreenshotStrategy::RendererRequest,
        _ => ScreenshotStrategy::Unsupported,
    }
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

pub(crate) fn focus_existing_window(
    raw_handle: i64,
    no_activate: bool,
) -> Result<(), ActivationError> {
    // SAFETY: the registry value is owned by the live Control Center process;
    // the native call is immediate and typed failures cover stale handles.
    let Some(window) = (unsafe { NativeWindowHandle::from_raw(raw_handle as isize) }) else {
        return Err(ActivationError::Failed {
            code: Cow::Borrowed("control_center_window_unavailable"),
            message: "Control Center did not publish a native window handle".to_owned(),
        });
    };
    let request = if no_activate {
        ActivationRequest::ShowWithoutActivation
    } else {
        ActivationRequest::RestoreAndActivate
    };
    agenterm_platform::activation::apply(window, request)
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
        assert_eq!(
            screenshot_strategy_for(PlatformKind::Windows),
            ScreenshotStrategy::DirectNativeWindow
        );
        assert_eq!(
            screenshot_strategy_for(PlatformKind::Linux),
            ScreenshotStrategy::RendererRequest
        );
        assert_eq!(
            screenshot_strategy_for(PlatformKind::Macos),
            ScreenshotStrategy::RendererRequest
        );
    }

    #[test]
    fn invalid_native_handles_remain_typed_failures() {
        assert!(matches!(
            focus_existing_window(0, false),
            Err(ActivationError::Failed { code, .. })
                if code == "control_center_window_unavailable"
        ));
        assert!(matches!(
            capture_native_window_png(0, Path::new("unused.png")),
            Err(UiScreenshotError::Failed { code, .. })
                if code == "control_center_screenshot_window_unavailable"
        ));
    }
}
