//! Bounded Retina screenshot encoding for the macOS GUI.
//! Adapter-private native mechanism selected only by platform::selected.

#![cfg(target_os = "macos")]

use std::{fs::File, io::BufWriter, path::Path};

use png::{BitDepth, ColorType, Encoder};

use crate::platform::{CapabilityStatus, ScreenshotClipRect, validate_screenshot_clip};

pub(crate) const MAX_RGBA_BYTES: usize = 64 * 1024 * 1024;
const MAX_PIXELS: usize = MAX_RGBA_BYTES / 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScreenshotError {
    InvalidDimensions,
    TooLarge { max_pixels: usize },
    BufferTooSmall { required: usize, actual: usize },
    InvalidClip,
    Io { message: String },
    Encode { message: String },
}

impl ScreenshotError {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::InvalidDimensions => "screenshot dimensions are invalid".to_owned(),
            Self::TooLarge { max_pixels } => {
                format!("screenshot exceeds the {max_pixels} pixel limit")
            }
            Self::BufferTooSmall { required, actual } => {
                format!("screenshot requires {required} pixels but received {actual}")
            }
            Self::InvalidClip => "screenshot clip is outside the framebuffer".to_owned(),
            Self::Io { message } | Self::Encode { message } => message.clone(),
        }
    }

    pub(crate) fn to_capability_status(&self) -> CapabilityStatus {
        let code = match self {
            Self::InvalidDimensions => "screenshot_invalid_dimensions",
            Self::TooLarge { .. } => "screenshot_too_large",
            Self::BufferTooSmall { .. } => "screenshot_buffer_too_small",
            Self::InvalidClip => "screenshot_invalid_clip",
            Self::Io { .. } => "screenshot_io_error",
            Self::Encode { .. } => "screenshot_encode_error",
        };
        CapabilityStatus::Failed {
            code,
            message: self.message(),
        }
    }
}

pub(crate) fn write_xrgb_png(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[u32],
    clip: Option<(u32, u32, u32, u32)>,
) -> Result<(), ScreenshotError> {
    let frame_pixels = checked_pixel_count(width, height)?;
    if pixels.len() < frame_pixels {
        return Err(ScreenshotError::BufferTooSmall {
            required: frame_pixels,
            actual: pixels.len(),
        });
    }
    let (x, y, output_width, output_height) = checked_clip(width, height, clip)?;
    let output_pixels = checked_pixel_count(output_width, output_height)?;
    let rgba_capacity = output_pixels
        .checked_mul(4)
        .ok_or(ScreenshotError::TooLarge {
            max_pixels: MAX_PIXELS,
        })?;
    let mut rgba = Vec::with_capacity(rgba_capacity);
    for row in y..y + output_height {
        let row_start = row as usize * width as usize;
        for column in x..x + output_width {
            let pixel = pixels[row_start + column as usize] & 0x00FF_FFFF;
            rgba.extend_from_slice(&[
                ((pixel >> 16) & 0xFF) as u8,
                ((pixel >> 8) & 0xFF) as u8,
                (pixel & 0xFF) as u8,
                255,
            ]);
        }
    }

    let file = File::create(path).map_err(|error| ScreenshotError::Io {
        message: error.to_string(),
    })?;
    let mut encoder = Encoder::new(BufWriter::new(file), output_width, output_height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(encode_error)?
        .into_stream_writer()
        .map_err(encode_error)?;
    use std::io::Write;
    writer.write_all(&rgba).map_err(encode_error)?;
    writer.finish().map_err(encode_error)?;
    Ok(())
}

fn checked_pixel_count(width: u32, height: u32) -> Result<usize, ScreenshotError> {
    if width == 0 || height == 0 {
        return Err(ScreenshotError::InvalidDimensions);
    }
    let pixels =
        (width as usize)
            .checked_mul(height as usize)
            .ok_or(ScreenshotError::TooLarge {
                max_pixels: MAX_PIXELS,
            })?;
    if pixels > MAX_PIXELS {
        return Err(ScreenshotError::TooLarge {
            max_pixels: MAX_PIXELS,
        });
    }
    Ok(pixels)
}

fn checked_clip(
    width: u32,
    height: u32,
    clip: Option<(u32, u32, u32, u32)>,
) -> Result<(u32, u32, u32, u32), ScreenshotError> {
    let Some((x, y, clip_width, clip_height)) = clip else {
        return Ok((0, 0, width, height));
    };
    validate_screenshot_clip(
        width,
        height,
        ScreenshotClipRect {
            x,
            y,
            width: clip_width,
            height: clip_height,
        },
    )
    .map_err(|_| ScreenshotError::InvalidClip)?;
    Ok((x, y, clip_width, clip_height))
}

fn encode_error(error: impl std::fmt::Display) -> ScreenshotError {
    ScreenshotError::Encode {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_bounded_retina_png() {
        let path = std::env::temp_dir().join("agenterm-macos-screenshot-test.png");
        let pixels = [0x00FF00u32, 0x0000FFu32, 0xFF0000u32, 0xFFFFFFu32];
        write_xrgb_png(&path, 2, 2, &pixels, None).expect("PNG");
        assert!(std::fs::metadata(&path).expect("metadata").len() > 32);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn dimensions_and_buffers_fail_before_encoding() {
        assert_eq!(
            write_xrgb_png(Path::new("unused.png"), 0, 2, &[], None),
            Err(ScreenshotError::InvalidDimensions)
        );
        assert_eq!(
            write_xrgb_png(Path::new("unused.png"), 2, 2, &[0; 3], None),
            Err(ScreenshotError::BufferTooSmall {
                required: 4,
                actual: 3,
            })
        );
        assert!(matches!(
            write_xrgb_png(Path::new("unused.png"), u32::MAX, 2, &[], None),
            Err(ScreenshotError::TooLarge { .. })
        ));
    }

    #[test]
    fn invalid_clip_is_typed() {
        assert_eq!(
            write_xrgb_png(Path::new("unused.png"), 2, 2, &[0; 4], Some((1, 1, 2, 2))),
            Err(ScreenshotError::InvalidClip)
        );
        assert_eq!(
            ScreenshotError::InvalidClip.to_capability_status(),
            CapabilityStatus::Failed {
                code: "screenshot_invalid_clip",
                message: "screenshot clip is outside the framebuffer".to_owned(),
            }
        );
    }
}
