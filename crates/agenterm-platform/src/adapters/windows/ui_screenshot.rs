//! Windows screenshot encoding and bounded native GDI capture.

use std::{mem, os::windows::ffi::OsStrExt, path::Path, ptr};

use windows_sys::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap,
                CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits,
                GetWindowDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
            },
            GdiPlus::{
                GdipCreateBitmapFromScan0, GdipDisposeImage, GdipSaveImageToFile, GdiplusShutdown,
                GdiplusStartup, GdiplusStartupInput, GpBitmap, GpImage, Ok as GDIP_OK,
                PixelFormatGDI,
            },
        },
        UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
    },
    core::GUID,
};

use crate::contract::ui_screenshot::{
    NativeCaptureArea, ScreenshotWindowHandle, ScreenshotWriteResult, UiScreenshotError, XrgbClip,
    XrgbFrame,
};

const MAX_CAPTURE_PIXELS: usize = 33_554_432;
const PIXEL_FORMAT_32BPP_RGB: i32 = (9 | (32 << 8) | PixelFormatGDI) as i32;
const PNG_ENCODER: GUID = GUID::from_u128(0x557cf406_1a04_11d3_9a73_0000f81ef32e);

pub(crate) fn write_xrgb_png(
    frame: XrgbFrame<'_>,
) -> Result<ScreenshotWriteResult, UiScreenshotError> {
    let (x, y, width, height, output_pixels) = crate::screenshot::checked_frame(&frame)?;
    let stride = frame
        .width()
        .checked_mul(4)
        .and_then(|stride| i32::try_from(stride).ok())
        .ok_or_else(allocation_failed)?;
    let start = (y as usize)
        .checked_mul(frame.width() as usize)
        .and_then(|row| row.checked_add(x as usize))
        .ok_or_else(allocation_failed)?;
    save_bgrx_png(
        frame.path(),
        width,
        height,
        stride,
        frame.pixels()[start..].as_ptr().cast(),
    )?;
    Ok(ScreenshotWriteResult {
        frame_width: frame.width(),
        frame_height: frame.height(),
        output_width: width,
        output_height: height,
        output_pixels,
    })
}

struct GdiImage(*mut GpImage);

impl Drop for GdiImage {
    fn drop(&mut self) {
        unsafe { GdipDisposeImage(self.0) };
    }
}

struct GdiPlus(usize);

impl GdiPlus {
    fn start() -> Result<Self, UiScreenshotError> {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..GdiplusStartupInput::default()
        };
        let mut token = 0usize;
        let status = unsafe { GdiplusStartup(&mut token, &input, ptr::null_mut()) };
        if status == GDIP_OK {
            Ok(Self(token))
        } else {
            Err(gdip_error("screenshot_gdiplus_startup", status))
        }
    }
}

impl Drop for GdiPlus {
    fn drop(&mut self) {
        unsafe { GdiplusShutdown(self.0) };
    }
}

fn save_bgrx_png(
    path: &Path,
    width: u32,
    height: u32,
    stride: i32,
    pixels: *const u8,
) -> Result<(), UiScreenshotError> {
    let _gdiplus = GdiPlus::start()?;
    let width = i32::try_from(width).map_err(|_| allocation_failed())?;
    let height = i32::try_from(height).map_err(|_| allocation_failed())?;
    let mut bitmap: *mut GpBitmap = ptr::null_mut();
    let status = unsafe {
        GdipCreateBitmapFromScan0(
            width,
            height,
            stride,
            PIXEL_FORMAT_32BPP_RGB,
            pixels,
            &mut bitmap,
        )
    };
    if status != GDIP_OK || bitmap.is_null() {
        return Err(gdip_error("screenshot_bitmap_create", status));
    }
    let image = GdiImage(bitmap.cast());
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let status = unsafe { GdipSaveImageToFile(image.0, wide.as_ptr(), &PNG_ENCODER, ptr::null()) };
    if status != GDIP_OK {
        return Err(gdip_error("screenshot_encode_error", status));
    }
    Ok(())
}

fn gdip_error(code: &'static str, status: i32) -> UiScreenshotError {
    UiScreenshotError::failed(
        code,
        format!("GDI+ PNG operation failed with status {status}"),
    )
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
    let mut bgrx = vec![0_u8; rgba_buffer_len(width, height)?];
    let scanlines = if copied != 0 {
        unsafe {
            GetDIBits(
                memory_dc.0,
                bitmap.0,
                0,
                height as u32,
                bgrx.as_mut_ptr().cast(),
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
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            UiScreenshotError::failed("screenshot_file_error", error.to_string())
        })?;
    }
    save_bgrx_png(path, width as u32, height as u32, width * 4, bgrx.as_ptr())?;
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
