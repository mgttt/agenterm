use std::mem;

use windows_sys::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject, FF_MODERN,
    FIXED_PITCH, FW_NORMAL, GetDC, GetDeviceCaps, GetTextMetricsW, HGDIOBJ, LOGPIXELSY,
    OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, TEXTMETRICW,
};

use crate::contract::font::{
    FontDiscovery, FontError, FontFileCandidate, FontMetrics, FontRequest, OpaqueWindowHandle,
};

pub(crate) fn candidates() -> Vec<FontFileCandidate> {
    Vec::new()
}

pub(crate) fn probe() -> FontDiscovery {
    FontDiscovery {
        available_families: Vec::new(),
        primary_family: None,
    }
}

pub(crate) fn primary_family_name() -> Result<&'static str, FontError> {
    Err(FontError::Unsupported)
}

pub(crate) fn primary_metrics(_size_px: u16) -> Result<FontMetrics, FontError> {
    Err(FontError::Unsupported)
}

pub(crate) fn probe_capability() -> Result<(), FontError> {
    Ok(())
}

pub(crate) fn create_terminal_font(
    window: OpaqueWindowHandle,
    request: FontRequest<'_>,
) -> Result<(isize, FontMetrics), FontError> {
    if request.family.is_empty() || request.point_size == 0 {
        return Err(FontError::InvalidRequest);
    }
    let window = window.get() as *mut core::ffi::c_void;
    let device = unsafe { GetDC(window) };
    if device.is_null() {
        return Err(FontError::DeviceContextUnavailable);
    }
    let dpi = unsafe { GetDeviceCaps(device, i32::try_from(LOGPIXELSY).unwrap_or(90)) };
    let requested_height = -((i32::from(request.point_size) * dpi) / 72).max(1);
    let family: Vec<u16> = request
        .family
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let font = unsafe {
        CreateFontW(
            requested_height,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            u32::from(OUT_DEFAULT_PRECIS),
            u32::from(CLIP_DEFAULT_PRECIS),
            u32::from(CLEARTYPE_QUALITY),
            u32::from(FIXED_PITCH | FF_MODERN),
            family.as_ptr(),
        )
    };
    if font.is_null() {
        unsafe { ReleaseDC(window, device) };
        return Err(FontError::CreateFailed);
    }

    let previous = unsafe { SelectObject(device, font as HGDIOBJ) };
    let mut metrics: TEXTMETRICW = unsafe { mem::zeroed() };
    let measured = unsafe { GetTextMetricsW(device, &mut metrics) };
    unsafe {
        SelectObject(device, previous);
        ReleaseDC(window, device);
    }
    if measured == 0 {
        unsafe { DeleteObject(font as HGDIOBJ) };
        return Err(FontError::MetricsFailed);
    }

    Ok((
        font as isize,
        FontMetrics {
            family: None,
            size_px: u16::try_from(requested_height.unsigned_abs()).unwrap_or(u16::MAX),
            cell_width: metrics.tmAveCharWidth.max(1) as f32,
            cell_height: metrics.tmHeight.max(1) as f32,
            ascent: metrics.tmAscent.max(1) as f32,
        },
    ))
}

pub(crate) fn destroy_terminal_font(raw: isize) {
    if raw != 0 {
        unsafe { DeleteObject(raw as *mut core::ffi::c_void) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_font_request_fails_before_native_access() {
        let window = unsafe { OpaqueWindowHandle::from_raw(0) };
        assert_eq!(
            create_terminal_font(
                window,
                FontRequest {
                    family: "",
                    point_size: 0,
                },
            ),
            Err(FontError::InvalidRequest)
        );
    }
}
