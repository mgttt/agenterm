//! AgenTerm compatibility facade over `agenterm-platform` screenshot encoding.

use crate::platform::contract::ui_screenshot::{UiScreenshotError, XrgbFrame};

pub(crate) fn write_xrgb_png(frame: XrgbFrame<'_>) -> Result<(), UiScreenshotError> {
    agenterm_platform::screenshot::write_xrgb_png(frame).map(|_| ())
}
