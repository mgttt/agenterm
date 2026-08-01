//! macOS selection for caller-owned XRGB screenshot encoding.

use crate::contract::ui_screenshot::{ScreenshotWriteResult, UiScreenshotError, XrgbFrame};

pub(crate) fn write_xrgb_png(
    frame: XrgbFrame<'_>,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    crate::screenshot::write_xrgb_png_impl(frame)
}
