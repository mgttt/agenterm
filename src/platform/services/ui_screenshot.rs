//! AgenTerm compatibility facade over `agenterm-platform` screenshot encoding.

use crate::platform::{
    contract::ui_screenshot::{UiScreenshotError, XrgbFrame},
    selected,
};

pub(crate) fn write_xrgb_png(frame: XrgbFrame<'_>) -> Result<(), UiScreenshotError> {
    selected::ui_screenshot::write_xrgb_png(frame).map(|_| ())
}
