//! Portable PNG encoding shared by Linux and macOS adapters.

use std::{fs::File, io::BufWriter};

use png::{BitDepth, ColorType, Encoder};

use crate::{
    contract::ui_screenshot::{ScreenshotWriteResult, UiScreenshotError, XrgbFrame},
    screenshot::{MAX_FRAME_PIXELS, checked_frame},
};

pub(crate) fn write_xrgb_png(
    frame: XrgbFrame<'_>,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    let (x, y, output_width, output_height, output_pixels) = checked_frame(&frame)?;
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

fn encode_error(error: impl std::fmt::Display) -> UiScreenshotError {
    UiScreenshotError::failed("screenshot_encode_error", error.to_string())
}
