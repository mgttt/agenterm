//! Linux XRGB screenshot adapter.

use crate::platform::{
    contract::ui_screenshot::{UiScreenshotError, XrgbFrame},
    selected::native::screenshot::{ScreenshotClip, ScreenshotError},
};

pub(crate) fn write_xrgb_png(frame: XrgbFrame<'_>) -> Result<(), UiScreenshotError> {
    let clip = frame.clip.map(|clip| ScreenshotClip {
        x: clip.x,
        y: clip.y,
        width: clip.width,
        height: clip.height,
    });
    crate::platform::selected::native::screenshot::write_xrgb_png(
        frame.path,
        frame.width,
        frame.height,
        frame.pixels,
        clip,
    )
    .map(|_| ())
    .map_err(map_error)
}

fn map_error(error: ScreenshotError) -> UiScreenshotError {
    match error {
        ScreenshotError::Headless => UiScreenshotError::Unsupported {
            reason: "headless-display",
        },
        error => UiScreenshotError::Failed {
            code: error.code(),
            message: error.message(),
        },
    }
}
