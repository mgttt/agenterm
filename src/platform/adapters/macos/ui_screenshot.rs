//! macOS XRGB screenshot adapter.

use crate::platform::contract::ui_screenshot::{UiScreenshotError, XrgbFrame};

pub(crate) fn write_xrgb_png(frame: XrgbFrame<'_>) -> Result<(), UiScreenshotError> {
    let clip = frame
        .clip
        .map(|clip| (clip.x, clip.y, clip.width, clip.height));
    crate::platform::macos::screenshot::write_xrgb_png(
        frame.path,
        frame.width,
        frame.height,
        frame.pixels,
        clip,
    )
    .map_err(|error| UiScreenshotError::Failed {
        code: error_code(&error),
        message: error.message(),
    })
}

fn error_code(error: &crate::platform::macos::screenshot::ScreenshotError) -> &'static str {
    use crate::platform::macos::screenshot::ScreenshotError;
    match error {
        ScreenshotError::InvalidDimensions => "screenshot_invalid_dimensions",
        ScreenshotError::TooLarge { .. } => "screenshot_too_large",
        ScreenshotError::BufferTooSmall { .. } => "screenshot_buffer_too_small",
        ScreenshotError::InvalidClip => "screenshot_invalid_clip",
        ScreenshotError::Io { .. } => "screenshot_io_error",
        ScreenshotError::Encode { .. } => "screenshot_encode_error",
    }
}
