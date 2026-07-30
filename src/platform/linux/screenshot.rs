//! Linux screenshot capability bridge for platform migration slice-2
//! (contract revision 1).
//!
//! Captures the softbuffer framebuffer already rendered by `unix_app` (not a
//! compositor screencopy API). Dimension, pixel-budget, clip, and I/O failures
//! are typed — never a silent Available with a truncated or empty PNG.
//!
//! X11 and Wayland share the same rendered-frame path; headless is Unsupported.

#![cfg(target_os = "linux")]

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use png::{BitDepth, ColorType, Encoder};

use crate::platform::{CapabilityStatus, DisplayBackendFacts};

use super::display_facts_from_env;

/// Maximum accepted framebuffer side length (pixels).
pub(crate) const MAX_FRAME_SIDE: u32 = 16_384;

/// Maximum accepted framebuffer pixel count (width × height).
pub(crate) const MAX_FRAME_PIXELS: usize = 64 * 1024 * 1024;

/// Display backend that owns the rendered frame being encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScreenshotBackend {
    X11,
    Wayland,
    X11AndWayland,
}

impl ScreenshotBackend {
    pub(crate) fn from_display(facts: DisplayBackendFacts) -> Result<Self, ScreenshotError> {
        match (facts.x11, facts.wayland, facts.headless) {
            (_, _, true) => Err(ScreenshotError::Headless),
            (true, true, false) => Ok(Self::X11AndWayland),
            (true, false, false) => Ok(Self::X11),
            (false, true, false) => Ok(Self::Wayland),
            (false, false, false) => Err(ScreenshotError::Headless),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::X11 => "x11",
            Self::Wayland => "wayland",
            Self::X11AndWayland => "x11+wayland",
        }
    }
}

/// Inclusive clip rectangle in framebuffer pixels (`x`, `y`, `width`, `height`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScreenshotClip {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Facts returned after a successful PNG encode (evidence / diagnostics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScreenshotWriteResult {
    pub backend: ScreenshotBackend,
    pub frame_width: u32,
    pub frame_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub output_pixels: usize,
}

/// Typed Linux screenshot failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScreenshotError {
    Headless,
    EmptyFrame,
    DimensionMismatch {
        width: u32,
        height: u32,
        pixels: usize,
    },
    FrameTooLarge {
        width: u32,
        height: u32,
        limit_side: u32,
        limit_pixels: usize,
    },
    InvalidClip {
        message: String,
    },
    EmptyOutputPath,
    Encode {
        message: String,
    },
}

impl ScreenshotError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Headless => "screenshot_headless",
            Self::EmptyFrame => "screenshot_empty_frame",
            Self::DimensionMismatch { .. } => "screenshot_dimension_mismatch",
            Self::FrameTooLarge { .. } => "screenshot_frame_too_large",
            Self::InvalidClip { .. } => "screenshot_invalid_clip",
            Self::EmptyOutputPath => "screenshot_empty_path",
            Self::Encode { .. } => "screenshot_encode_failed",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Headless => "screenshot unavailable without a graphical display".to_string(),
            Self::EmptyFrame => "no rendered frame is available".to_string(),
            Self::DimensionMismatch {
                width,
                height,
                pixels,
            } => format!("pixel buffer length {pixels} is smaller than declared {width}x{height}"),
            Self::FrameTooLarge {
                width,
                height,
                limit_side,
                limit_pixels,
            } => format!(
                "framebuffer {width}x{height} exceeds side limit {limit_side} or pixel budget {limit_pixels}"
            ),
            Self::InvalidClip { message } => message.clone(),
            Self::EmptyOutputPath => "screenshot output path is empty".to_string(),
            Self::Encode { message } => message.clone(),
        }
    }

    pub(crate) fn to_capability_status(&self) -> CapabilityStatus {
        match self {
            Self::Headless => CapabilityStatus::Unsupported {
                reason: "headless-display",
            },
            other => CapabilityStatus::Failed {
                code: other.code(),
                message: other.message(),
            },
        }
    }
}

/// Screenshot capability: Available on X11 and/or Wayland softbuffer paths.
pub(crate) fn screenshot_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    match ScreenshotBackend::from_display(facts) {
        Ok(_) => CapabilityStatus::Available,
        Err(error) => error.to_capability_status(),
    }
}

pub(crate) fn screenshot_capability_status_from_env() -> CapabilityStatus {
    screenshot_capability_status(display_facts_from_env())
}

/// Validate framebuffer dimensions against declared length and hard budgets.
pub(crate) fn validate_frame(
    width: u32,
    height: u32,
    pixels: &[u32],
) -> Result<(), ScreenshotError> {
    if width == 0 || height == 0 {
        return Err(ScreenshotError::EmptyFrame);
    }
    if width > MAX_FRAME_SIDE
        || height > MAX_FRAME_SIDE
        || (width as usize).saturating_mul(height as usize) > MAX_FRAME_PIXELS
    {
        return Err(ScreenshotError::FrameTooLarge {
            width,
            height,
            limit_side: MAX_FRAME_SIDE,
            limit_pixels: MAX_FRAME_PIXELS,
        });
    }
    let needed = (width as usize).saturating_mul(height as usize);
    if pixels.len() < needed {
        return Err(ScreenshotError::DimensionMismatch {
            width,
            height,
            pixels: pixels.len(),
        });
    }
    Ok(())
}

/// Resolve an optional clip into strict framebuffer bounds.
pub(crate) fn resolve_clip(
    frame_width: u32,
    frame_height: u32,
    clip: Option<ScreenshotClip>,
) -> Result<(u32, u32, u32, u32), ScreenshotError> {
    if frame_width == 0 || frame_height == 0 {
        return Err(ScreenshotError::EmptyFrame);
    }
    let Some(clip) = clip else {
        return Ok((0, 0, frame_width, frame_height));
    };
    if clip.width == 0 || clip.height == 0 {
        return Err(ScreenshotError::InvalidClip {
            message: "screenshot clip width/height must be non-zero".to_string(),
        });
    }
    if clip.x >= frame_width || clip.y >= frame_height {
        return Err(ScreenshotError::InvalidClip {
            message: format!(
                "screenshot clip origin ({},{}) is outside {}x{} frame",
                clip.x, clip.y, frame_width, frame_height
            ),
        });
    }
    let right = clip
        .x
        .checked_add(clip.width)
        .ok_or_else(|| ScreenshotError::InvalidClip {
            message: "screenshot clip horizontal bounds overflow".to_string(),
        })?;
    let bottom = clip
        .y
        .checked_add(clip.height)
        .ok_or_else(|| ScreenshotError::InvalidClip {
            message: "screenshot clip vertical bounds overflow".to_string(),
        })?;
    if right > frame_width || bottom > frame_height {
        return Err(ScreenshotError::InvalidClip {
            message: format!(
                "screenshot clip {}x{} at ({},{}) exceeds {}x{} frame",
                clip.width, clip.height, clip.x, clip.y, frame_width, frame_height
            ),
        });
    }
    Ok((clip.x, clip.y, clip.width, clip.height))
}

/// Encode softbuffer little-endian `0x00RRGGBB` pixels as an RGBA PNG.
pub(crate) fn write_xrgb_png(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[u32],
    clip: Option<ScreenshotClip>,
) -> Result<ScreenshotWriteResult, ScreenshotError> {
    let backend = ScreenshotBackend::from_display(display_facts_from_env())?;
    if path.as_os_str().is_empty() {
        return Err(ScreenshotError::EmptyOutputPath);
    }
    validate_frame(width, height, pixels)?;
    let (x0, y0, out_w, out_h) = resolve_clip(width, height, clip)?;

    let mut rgba = Vec::with_capacity((out_w as usize).saturating_mul(out_h as usize) * 4);
    for row in y0..y0 + out_h {
        let row_start = (row as usize) * (width as usize);
        for col in x0..x0 + out_w {
            let pixel = pixels[row_start + col as usize] & 0x00FF_FFFF;
            let r = ((pixel >> 16) & 0xFF) as u8;
            let g = ((pixel >> 8) & 0xFF) as u8;
            let b = (pixel & 0xFF) as u8;
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }

    let file = File::create(path).map_err(|error| ScreenshotError::Encode {
        message: error.to_string(),
    })?;
    let mut encoder = Encoder::new(BufWriter::new(file), out_w, out_h);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| ScreenshotError::Encode {
            message: error.to_string(),
        })?
        .into_stream_writer()
        .map_err(|error| ScreenshotError::Encode {
            message: error.to_string(),
        })?;
    writer
        .write_all(&rgba)
        .map_err(|error| ScreenshotError::Encode {
            message: error.to_string(),
        })?;
    writer.finish().map_err(|error| ScreenshotError::Encode {
        message: error.to_string(),
    })?;

    Ok(ScreenshotWriteResult {
        backend,
        frame_width: width,
        frame_height: height,
        output_width: out_w,
        output_height: out_h,
        output_pixels: (out_w as usize).saturating_mul(out_h as usize),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_screenshot_is_unsupported() {
        let status = screenshot_capability_status(DisplayBackendFacts {
            x11: false,
            wayland: false,
            headless: true,
        });
        assert!(matches!(
            status,
            CapabilityStatus::Unsupported {
                reason: "headless-display"
            }
        ));
    }

    #[test]
    fn x11_and_wayland_screenshot_are_available() {
        assert_eq!(
            screenshot_capability_status(DisplayBackendFacts {
                x11: true,
                wayland: false,
                headless: false,
            }),
            CapabilityStatus::Available
        );
        assert_eq!(
            screenshot_capability_status(DisplayBackendFacts {
                x11: false,
                wayland: true,
                headless: false,
            }),
            CapabilityStatus::Available
        );
    }

    #[test]
    fn validate_frame_rejects_mismatch_and_oversize() {
        assert!(matches!(
            validate_frame(2, 2, &[1]),
            Err(ScreenshotError::DimensionMismatch { .. })
        ));
        assert!(matches!(
            validate_frame(
                MAX_FRAME_SIDE + 1,
                1,
                &vec![0; (MAX_FRAME_SIDE as usize) + 1]
            ),
            Err(ScreenshotError::FrameTooLarge { .. })
        ));
        assert!(validate_frame(2, 2, &[1, 2, 3, 4]).is_ok());
    }

    #[test]
    fn resolve_clip_rejects_empty_and_out_of_bounds() {
        assert!(matches!(
            resolve_clip(
                10,
                10,
                Some(ScreenshotClip {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 5
                })
            ),
            Err(ScreenshotError::InvalidClip { .. })
        ));
        assert!(matches!(
            resolve_clip(
                10,
                10,
                Some(ScreenshotClip {
                    x: 10,
                    y: 0,
                    width: 1,
                    height: 1
                })
            ),
            Err(ScreenshotError::InvalidClip { .. })
        ));
        assert!(matches!(
            resolve_clip(
                10,
                10,
                Some(ScreenshotClip {
                    x: 8,
                    y: 8,
                    width: 4,
                    height: 4
                })
            ),
            Err(ScreenshotError::InvalidClip { .. })
        ));
    }

    #[test]
    fn write_xrgb_png_emits_readable_file_when_display_present() {
        if display_facts_from_env().headless {
            assert!(matches!(
                write_xrgb_png(
                    Path::new("/tmp/agenterm-linux-screenshot-skip.png"),
                    2,
                    2,
                    &[0x00FF00, 0x0000FF, 0xFF0000, 0xFFFFFF],
                    None
                ),
                Err(ScreenshotError::Headless)
            ));
            return;
        }
        let path = std::env::temp_dir().join("agenterm-linux-screenshot-test.png");
        let result = write_xrgb_png(
            &path,
            2,
            2,
            &[0x00FF00, 0x0000FF, 0xFF0000, 0xFFFFFF],
            Some(ScreenshotClip {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            }),
        )
        .expect("png write");
        assert_eq!(result.output_width, 2);
        assert_eq!(result.output_height, 1);
        assert_eq!(result.output_pixels, 2);
        let meta = std::fs::metadata(&path).expect("meta");
        assert!(meta.len() > 32);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_output_path_is_typed_failure() {
        if display_facts_from_env().headless {
            return;
        }
        assert!(matches!(
            write_xrgb_png(Path::new(""), 1, 1, &[0], None),
            Err(ScreenshotError::EmptyOutputPath)
        ));
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(ScreenshotError::EmptyFrame.code(), "screenshot_empty_frame");
        assert!(matches!(
            ScreenshotError::EmptyFrame.to_capability_status(),
            CapabilityStatus::Failed {
                code: "screenshot_empty_frame",
                ..
            }
        ));
    }
}
