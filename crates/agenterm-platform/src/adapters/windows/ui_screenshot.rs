//! Windows screenshot encoding and bounded native GDI capture.

use std::{fs::File, mem, path::Path};

use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetWindowDC, HBITMAP, HDC,
        HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
    },
    UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
};

use crate::contract::ui_screenshot::{
    NativeCaptureArea, ScreenshotWindowHandle, ScreenshotWriteResult, UiScreenshotError, XrgbClip,
    XrgbFrame,
};

const MAX_CAPTURE_PIXELS: usize = 33_554_432;

pub(crate) fn write_xrgb_png(
    frame: XrgbFrame<'_>,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    crate::screenshot::write_xrgb_png_impl(frame)
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

fn rgba_buffer_len(width: i32, height: i32) -> Result<usize, UiScreenshotError> {
    let width = usize::try_from(width).map_err(|_| invalid_bounds())?;
    let height = usize::try_from(height).map_err(|_| invalid_bounds())?;
    if width == 0 || height == 0 {
        return Err(invalid_bounds());
    }
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| too_large(i32::MAX, i32::MAX))?;
    if pixels > MAX_CAPTURE_PIXELS {
        return Err(too_large(
            i32::try_from(width).unwrap_or(i32::MAX),
            i32::try_from(height).unwrap_or(i32::MAX),
        ));
    }
    pixels
        .checked_mul(4)
        .ok_or_else(|| too_large(i32::MAX, i32::MAX))
}

fn invalid_bounds() -> UiScreenshotError {
    UiScreenshotError::failed("screenshot_invalid_bounds", "screenshot bounds are invalid")
}

fn too_large(width: i32, height: i32) -> UiScreenshotError {
    UiScreenshotError::failed(
        "screenshot_too_large",
        format!("screenshot {width}x{height} exceeds the {MAX_CAPTURE_PIXELS}-pixel limit"),
    )
}

fn invalid_clip() -> UiScreenshotError {
    UiScreenshotError::failed(
        "screenshot_invalid_clip",
        "screenshot clip is outside the client framebuffer",
    )
}

fn bgra_to_rgba(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
}

fn validate_client_area(
    frame_width: i32,
    frame_height: i32,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
) -> Result<(), UiScreenshotError> {
    let frame_width = u32::try_from(frame_width).map_err(|_| invalid_clip())?;
    let frame_height = u32::try_from(frame_height).map_err(|_| invalid_clip())?;
    let clip = XrgbClip {
        x: u32::try_from(left).map_err(|_| invalid_clip())?,
        y: u32::try_from(top).map_err(|_| invalid_clip())?,
        width: u32::try_from(width).map_err(|_| invalid_clip())?,
        height: u32::try_from(height).map_err(|_| invalid_clip())?,
    };
    crate::screenshot::checked_clip(frame_width, frame_height, Some(clip))
        .map(|_| ())
        .map_err(|_| invalid_clip())
}

fn source_for_area(
    window: HWND,
    area: NativeCaptureArea,
) -> Result<(SourceDc, i32, i32, i32, i32), UiScreenshotError> {
    let (device, source_x, source_y, width, height) = match area {
        NativeCaptureArea::Window => {
            let mut outer: RECT = unsafe { mem::zeroed() };
            if unsafe { GetWindowRect(window, &mut outer) } == 0 {
                return Err(invalid_bounds());
            }
            (
                unsafe { GetWindowDC(window) },
                0,
                0,
                outer.right - outer.left,
                outer.bottom - outer.top,
            )
        }
        NativeCaptureArea::Client {
            left,
            top,
            width,
            height,
        } => {
            let mut client: RECT = unsafe { mem::zeroed() };
            if unsafe { GetClientRect(window, &mut client) } == 0 {
                return Err(invalid_bounds());
            }
            validate_client_area(
                client.right - client.left,
                client.bottom - client.top,
                left,
                top,
                width,
                height,
            )?;
            (unsafe { GetDC(window) }, left, top, width, height)
        }
    };
    rgba_buffer_len(width, height)?;
    if device.is_null() {
        return Err(UiScreenshotError::failed(
            "screenshot_dc_unavailable",
            "failed to acquire the window device context",
        ));
    }
    Ok((
        SourceDc { window, device },
        source_x,
        source_y,
        width,
        height,
    ))
}

pub(crate) fn capture_native_window_png(
    window: ScreenshotWindowHandle,
    path: &Path,
    area: NativeCaptureArea,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    let window = window.raw() as HWND;
    let (source, source_x, source_y, width, height) = source_for_area(window, area)?;
    let memory_dc = MemoryDc(unsafe { CreateCompatibleDC(source.device) });
    if memory_dc.0.is_null() {
        return Err(allocation_failed());
    }
    let bitmap = Bitmap(unsafe { CreateCompatibleBitmap(source.device, width, height) });
    if bitmap.0.is_null() {
        return Err(allocation_failed());
    }
    let previous = unsafe { SelectObject(memory_dc.0, bitmap.0 as HGDIOBJ) };
    if previous.is_null() {
        return Err(allocation_failed());
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
        return Err(UiScreenshotError::failed(
            "screenshot_capture_failed",
            "BitBlt/GetDIBits screenshot capture failed",
        ));
    }
    bgra_to_rgba(&mut rgba);

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            UiScreenshotError::failed("screenshot_file_error", error.to_string())
        })?;
    }
    let file = File::create(path)
        .map_err(|error| UiScreenshotError::failed("screenshot_file_error", error.to_string()))?;
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| UiScreenshotError::failed("screenshot_encode_error", error.to_string()))?;
    writer
        .write_image_data(&rgba)
        .map_err(|error| UiScreenshotError::failed("screenshot_encode_error", error.to_string()))?;
    Ok(ScreenshotWriteResult {
        frame_width: width as u32,
        frame_height: height as u32,
        output_width: width as u32,
        output_height: height as u32,
        output_pixels: (width as usize) * (height as usize),
    })
}

fn allocation_failed() -> UiScreenshotError {
    UiScreenshotError::failed(
        "screenshot_allocation_failed",
        "failed to allocate GDI screenshot resources",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_allocation_is_bounded_and_overflow_safe() {
        assert_eq!(rgba_buffer_len(8_192, 4_096), Ok(134_217_728));
        assert_eq!(
            rgba_buffer_len(8_193, 4_096).expect_err("too large").code(),
            "screenshot_too_large"
        );
        assert_eq!(
            rgba_buffer_len(0, 20).expect_err("zero width").code(),
            "screenshot_invalid_bounds"
        );
    }

    #[test]
    fn color_conversion_is_in_place_and_forces_opaque_alpha() {
        let mut pixels = [3, 2, 1, 0, 30, 20, 10, 7];
        bgra_to_rgba(&mut pixels);
        assert_eq!(pixels, [1, 2, 3, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn client_clip_uses_strict_framebuffer_bounds() {
        assert_eq!(validate_client_area(950, 594, 10, 20, 900, 500), Ok(()));
        assert_eq!(
            validate_client_area(950, 594, 200, 100, 800, 500)
                .expect_err("outside")
                .code(),
            "screenshot_invalid_clip"
        );
    }
}
