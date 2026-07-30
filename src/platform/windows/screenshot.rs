//! Bounded Win32 GDI screenshot capability.

#![cfg(target_os = "windows")]

use std::{fmt, fs::File, mem, path::Path};

use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetWindowDC, HBITMAP, HDC,
        HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
    },
    UI::WindowsAndMessaging::GetWindowRect,
};

use crate::platform::CapabilityStatus;

/// Supports an 8K frame while preventing unbounded GDI/heap allocation.
const MAX_CAPTURE_PIXELS: usize = 33_554_432;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureArea {
    Window,
    Client {
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScreenshotError {
    InvalidBounds,
    TooLarge {
        width: i32,
        height: i32,
        max_pixels: usize,
    },
    DeviceContextUnavailable,
    ResourceAllocationFailed,
    CaptureFailed,
    File(String),
    Encode(String),
}

impl fmt::Display for ScreenshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBounds => formatter.write_str("screenshot bounds are invalid"),
            Self::TooLarge {
                width,
                height,
                max_pixels,
            } => write!(
                formatter,
                "screenshot {width}x{height} exceeds the {max_pixels}-pixel limit"
            ),
            Self::DeviceContextUnavailable => {
                formatter.write_str("failed to acquire the window device context")
            }
            Self::ResourceAllocationFailed => {
                formatter.write_str("failed to allocate GDI screenshot resources")
            }
            Self::CaptureFailed => {
                formatter.write_str("BitBlt/GetDIBits screenshot capture failed")
            }
            Self::File(message) => write!(formatter, "screenshot file error: {message}"),
            Self::Encode(message) => write!(formatter, "screenshot PNG error: {message}"),
        }
    }
}

impl std::error::Error for ScreenshotError {}

impl ScreenshotError {
    pub(crate) fn to_capability_status(&self) -> CapabilityStatus {
        let code = match self {
            Self::InvalidBounds => "screenshot_invalid_bounds",
            Self::TooLarge { .. } => "screenshot_too_large",
            Self::DeviceContextUnavailable => "screenshot_dc_unavailable",
            Self::ResourceAllocationFailed => "screenshot_allocation_failed",
            Self::CaptureFailed => "screenshot_capture_failed",
            Self::File(_) => "screenshot_file_error",
            Self::Encode(_) => "screenshot_encode_error",
        };
        CapabilityStatus::Failed {
            code,
            message: self.to_string(),
        }
    }
}

struct SourceDc {
    window: HWND,
    device: HDC,
}

impl Drop for SourceDc {
    fn drop(&mut self) {
        unsafe { ReleaseDC(self.window, self.device) };
    }
}

struct MemoryDc(HDC);

impl Drop for MemoryDc {
    fn drop(&mut self) {
        unsafe { DeleteDC(self.0) };
    }
}

struct Bitmap(HBITMAP);

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe { DeleteObject(self.0 as HGDIOBJ) };
    }
}

fn rgba_buffer_len(width: i32, height: i32) -> Result<usize, ScreenshotError> {
    let width = usize::try_from(width).map_err(|_| ScreenshotError::InvalidBounds)?;
    let height = usize::try_from(height).map_err(|_| ScreenshotError::InvalidBounds)?;
    if width == 0 || height == 0 {
        return Err(ScreenshotError::InvalidBounds);
    }
    let pixels = width.checked_mul(height).ok_or(ScreenshotError::TooLarge {
        width: i32::MAX,
        height: i32::MAX,
        max_pixels: MAX_CAPTURE_PIXELS,
    })?;
    if pixels > MAX_CAPTURE_PIXELS {
        return Err(ScreenshotError::TooLarge {
            width: i32::try_from(width).unwrap_or(i32::MAX),
            height: i32::try_from(height).unwrap_or(i32::MAX),
            max_pixels: MAX_CAPTURE_PIXELS,
        });
    }
    pixels.checked_mul(4).ok_or(ScreenshotError::TooLarge {
        width: i32::try_from(width).unwrap_or(i32::MAX),
        height: i32::try_from(height).unwrap_or(i32::MAX),
        max_pixels: MAX_CAPTURE_PIXELS,
    })
}

fn bgra_to_rgba(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
}

fn source_for_area(
    window: HWND,
    area: CaptureArea,
) -> Result<(SourceDc, i32, i32, i32, i32), ScreenshotError> {
    let (device, source_x, source_y, width, height) = match area {
        CaptureArea::Window => {
            let mut outer: RECT = unsafe { mem::zeroed() };
            if unsafe { GetWindowRect(window, &mut outer) } == 0 {
                return Err(ScreenshotError::InvalidBounds);
            }
            (
                unsafe { GetWindowDC(window) },
                0,
                0,
                outer.right - outer.left,
                outer.bottom - outer.top,
            )
        }
        CaptureArea::Client {
            left,
            top,
            width,
            height,
        } => (unsafe { GetDC(window) }, left, top, width, height),
    };
    rgba_buffer_len(width, height)?;
    if device.is_null() {
        return Err(ScreenshotError::DeviceContextUnavailable);
    }
    Ok((
        SourceDc { window, device },
        source_x,
        source_y,
        width,
        height,
    ))
}

pub(crate) fn save_png(
    window: HWND,
    path: &Path,
    area: CaptureArea,
) -> Result<(), ScreenshotError> {
    let (source, source_x, source_y, width, height) = source_for_area(window, area)?;
    let memory_dc = MemoryDc(unsafe { CreateCompatibleDC(source.device) });
    if memory_dc.0.is_null() {
        return Err(ScreenshotError::ResourceAllocationFailed);
    }
    let bitmap = Bitmap(unsafe { CreateCompatibleBitmap(source.device, width, height) });
    if bitmap.0.is_null() {
        return Err(ScreenshotError::ResourceAllocationFailed);
    }
    let previous = unsafe { SelectObject(memory_dc.0, bitmap.0 as HGDIOBJ) };
    if previous.is_null() {
        return Err(ScreenshotError::ResourceAllocationFailed);
    }

    let copied = unsafe {
        BitBlt(
            memory_dc.0,
            0,
            0,
            width,
            height,
            source.device,
            source_x,
            source_y,
            SRCCOPY,
        )
    };
    let mut info: BITMAPINFO = unsafe { mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..unsafe { mem::zeroed() }
    };
    let mut rgba = vec![0_u8; rgba_buffer_len(width, height)?];
    let scanlines = if copied != 0 {
        unsafe {
            GetDIBits(
                memory_dc.0,
                bitmap.0,
                0,
                height as u32,
                rgba.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            )
        }
    } else {
        0
    };
    unsafe { SelectObject(memory_dc.0, previous) };
    if copied == 0 || scanlines != height {
        return Err(ScreenshotError::CaptureFailed);
    }
    bgra_to_rgba(&mut rgba);

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| ScreenshotError::File(error.to_string()))?;
    }
    let file = File::create(path).map_err(|error| ScreenshotError::File(error.to_string()))?;
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| ScreenshotError::Encode(error.to_string()))?;
    writer
        .write_image_data(&rgba)
        .map_err(|error| ScreenshotError::Encode(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_allocation_is_bounded_and_overflow_safe() {
        assert_eq!(rgba_buffer_len(8_192, 4_096), Ok(134_217_728));
        assert!(matches!(
            rgba_buffer_len(8_193, 4_096),
            Err(ScreenshotError::TooLarge { .. })
        ));
        assert_eq!(rgba_buffer_len(0, 20), Err(ScreenshotError::InvalidBounds));
        assert_eq!(rgba_buffer_len(-1, 20), Err(ScreenshotError::InvalidBounds));
    }

    #[test]
    fn color_conversion_is_in_place_and_forces_opaque_alpha() {
        let mut pixels = [3, 2, 1, 0, 30, 20, 10, 7];
        bgra_to_rgba(&mut pixels);
        assert_eq!(pixels, [1, 2, 3, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn screenshot_failures_have_stable_typed_codes() {
        assert_eq!(
            ScreenshotError::CaptureFailed.to_capability_status(),
            CapabilityStatus::Failed {
                code: "screenshot_capture_failed",
                message: "BitBlt/GetDIBits screenshot capture failed".to_string(),
            }
        );
    }
}
