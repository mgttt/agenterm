//! XRGB screenshot facade.

use std::{fs::File, io::BufWriter};

use png::{BitDepth, ColorType, Encoder};

use crate::{
    contract::ui_screenshot::{ScreenshotWriteResult, UiScreenshotError, XrgbClip, XrgbFrame},
    selected,
};

pub use crate::contract::ui_screenshot::{NativeCaptureArea, ScreenshotWindowHandle};

/// Maximum accepted framebuffer side length in pixels.
pub const MAX_FRAME_SIDE: u32 = 16_384;

/// Maximum accepted framebuffer pixel count.
pub const MAX_FRAME_PIXELS: usize = 64 * 1024 * 1024;

/// Encode a caller-owned little-endian `0x00RRGGBB` framebuffer as an RGBA PNG.
pub fn write_xrgb_png(frame: XrgbFrame<'_>) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    selected::ui_screenshot::write_xrgb_png(frame)
}

/// Capture a native window or a strict client-area rectangle into a PNG.
pub fn capture_native_window_png(
    window: ScreenshotWindowHandle,
    path: &std::path::Path,
    area: NativeCaptureArea,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    selected::ui_screenshot::capture_native_window_png(window, path, area)
}

pub(crate) fn write_xrgb_png_impl(
    frame: XrgbFrame<'_>,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    if frame.path().as_os_str().is_empty() {
        return Err(UiScreenshotError::failed(
            "screenshot_empty_path",
            "screenshot output path is empty",
        ));
    }
    let frame_pixels = checked_pixel_count(frame.width(), frame.height())?;
    if frame.pixels().len() < frame_pixels {
        return Err(UiScreenshotError::failed(
            "screenshot_buffer_too_small",
            format!(
                "screenshot requires {frame_pixels} pixels but received {}",
                frame.pixels().len()
            ),
        ));
    }
    let (x, y, output_width, output_height) =
        checked_clip(frame.width(), frame.height(), frame.clip())?;
    let output_pixels = checked_pixel_count(output_width, output_height)?;
    let rgba_capacity = output_pixels.checked_mul(4).ok_or_else(|| {
        UiScreenshotError::failed(
            "screenshot_too_large",
            format!("screenshot exceeds the {MAX_FRAME_PIXELS}-pixel limit"),
        )
    })?;
    let mut rgba = Vec::with_capacity(rgba_capacity);
    for row in y..y + output_height {
        let row_start = row as usize * frame.width() as usize;
        for column in x..x + output_width {
            let pixel = frame.pixels()[row_start + column as usize] & 0x00FF_FFFF;
            rgba.extend_from_slice(&[
                ((pixel >> 16) & 0xFF) as u8,
                ((pixel >> 8) & 0xFF) as u8,
                (pixel & 0xFF) as u8,
                255,
            ]);
        }
    }

    let file = File::create(frame.path())
        .map_err(|error| UiScreenshotError::failed("screenshot_io_error", error.to_string()))?;
    let mut encoder = Encoder::new(BufWriter::new(file), output_width, output_height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(encode_error)?;
    writer.write_image_data(&rgba).map_err(encode_error)?;

    Ok(ScreenshotWriteResult {
        frame_width: frame.width(),
        frame_height: frame.height(),
        output_width,
        output_height,
        output_pixels,
    })
}

fn checked_pixel_count(width: u32, height: u32) -> Result<usize, UiScreenshotError> {
    if width == 0 || height == 0 {
        return Err(UiScreenshotError::failed(
            "screenshot_invalid_dimensions",
            "screenshot dimensions must be non-zero",
        ));
    }
    if width > MAX_FRAME_SIDE || height > MAX_FRAME_SIDE {
        return Err(frame_too_large(width, height));
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| frame_too_large(width, height))?;
    if pixels > MAX_FRAME_PIXELS {
        return Err(frame_too_large(width, height));
    }
    Ok(pixels)
}

fn frame_too_large(width: u32, height: u32) -> UiScreenshotError {
    UiScreenshotError::failed(
        "screenshot_too_large",
        format!(
            "screenshot {width}x{height} exceeds side limit {MAX_FRAME_SIDE} or pixel limit {MAX_FRAME_PIXELS}"
        ),
    )
}

pub(crate) fn checked_clip(
    width: u32,
    height: u32,
    clip: Option<XrgbClip>,
) -> Result<(u32, u32, u32, u32), UiScreenshotError> {
    let Some(clip) = clip else {
        return Ok((0, 0, width, height));
    };
    let right = clip.x.checked_add(clip.width);
    let bottom = clip.y.checked_add(clip.height);
    if clip.width == 0
        || clip.height == 0
        || clip.x >= width
        || clip.y >= height
        || right.is_none_or(|right| right > width)
        || bottom.is_none_or(|bottom| bottom > height)
    {
        return Err(UiScreenshotError::failed(
            "screenshot_invalid_clip",
            format!(
                "screenshot clip {}x{} at ({},{}) is outside {}x{} frame",
                clip.width, clip.height, clip.x, clip.y, width, height
            ),
        ));
    }
    Ok((clip.x, clip.y, clip.width, clip.height))
}

fn encode_error(error: impl std::fmt::Display) -> UiScreenshotError {
    UiScreenshotError::failed("screenshot_encode_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn dimensions_buffers_and_clip_fail_before_encoding() {
        assert_eq!(
            write_xrgb_png_impl(XrgbFrame::new(Path::new("unused.png"), 0, 2, &[]))
                .expect_err("zero width")
                .code(),
            "screenshot_invalid_dimensions"
        );
        assert_eq!(
            write_xrgb_png_impl(XrgbFrame::new(Path::new("unused.png"), 2, 2, &[0; 3]))
                .expect_err("short buffer")
                .code(),
            "screenshot_buffer_too_small"
        );
        assert_eq!(
            write_xrgb_png_impl(
                XrgbFrame::new(Path::new("unused.png"), 2, 2, &[0; 4])
                    .with_clip(XrgbClip::new(1, 1, 2, 2))
            )
            .expect_err("outside clip")
            .code(),
            "screenshot_invalid_clip"
        );
    }

    #[test]
    fn writes_clipped_rgba_png() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-platform-screenshot-{}.png",
            std::process::id()
        ));
        let pixels = [0x00FF00u32, 0x0000FFu32, 0xFF0000u32, 0xFFFFFFu32];
        let result = write_xrgb_png_impl(
            XrgbFrame::new(&path, 2, 2, &pixels).with_clip(XrgbClip::new(0, 0, 2, 1)),
        )
        .expect("PNG");
        assert_eq!(result.output_width, 2);
        assert_eq!(result.output_height, 1);
        assert_eq!(result.output_pixels, 2);
        assert!(std::fs::metadata(&path).expect("metadata").len() > 32);
        let _ = std::fs::remove_file(path);
    }
}
