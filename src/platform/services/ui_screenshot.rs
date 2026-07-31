//! OS-neutral XRGB screenshot encoding service.

use crate::platform::{
    contract::ui_screenshot::{UiScreenshotError, XrgbFrame},
    selected,
};

pub(crate) fn write_xrgb_png(frame: XrgbFrame<'_>) -> Result<(), UiScreenshotError> {
    selected::ui_screenshot::write_xrgb_png(frame)
}
