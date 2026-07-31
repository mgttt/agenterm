//! Windows declaration for the Unix frontend screenshot service.

use crate::platform::contract::ui_screenshot::{UiScreenshotError, XrgbFrame};

pub(crate) fn write_xrgb_png(_: XrgbFrame<'_>) -> Result<(), UiScreenshotError> {
    Err(UiScreenshotError::Unsupported {
        reason: "unix frontend screenshot service is unavailable on Windows",
    })
}
