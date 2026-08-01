//! Linux selection for caller-owned XRGB screenshot encoding.

use std::{borrow::Cow, path::Path};

use crate::contract::ui_screenshot::{
    NativeCaptureArea, ScreenshotWindowHandle, ScreenshotWriteResult, UiScreenshotError, XrgbFrame,
};

pub(crate) fn write_xrgb_png(
    frame: XrgbFrame<'_>,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    crate::screenshot::write_xrgb_png_impl(frame)
}

pub(crate) fn capture_native_window_png(
    _window: ScreenshotWindowHandle,
    _path: &Path,
    _area: NativeCaptureArea,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    Err(UiScreenshotError::Unsupported {
        reason: Cow::Borrowed("native-window-capture-is-unavailable"),
    })
}
