//! Windows GDI terminal-font creation and metric measurement.
//! Adapter-private native mechanism selected only by platform::selected.

#![cfg(target_os = "windows")]

use std::mem;

use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject,
        FF_MODERN, FIXED_PITCH, FW_NORMAL, GetDC, GetDeviceCaps, GetTextMetricsW, HFONT, HGDIOBJ,
        LOGPIXELSY, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, TEXTMETRICW,
    },
};

use crate::platform::CapabilityStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FontError {
    DeviceContextUnavailable,
    CreateFailed,
    MetricsFailed,
}

impl FontError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::DeviceContextUnavailable => "font_device_context_unavailable",
            Self::CreateFailed => "font_create_failed",
            Self::MetricsFailed => "font_metrics_failed",
        }
    }

    pub(crate) fn to_capability_status(self) -> CapabilityStatus {
        CapabilityStatus::Failed {
            code: self.code(),
            message: match self {
                Self::DeviceContextUnavailable => "GetDC failed",
                Self::CreateFailed => "CreateFontW failed",
                Self::MetricsFailed => "GetTextMetricsW failed",
            }
            .to_owned(),
        }
    }
}

pub(crate) const fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

pub(crate) fn create_terminal_font(
    window: HWND,
    family: &str,
    point_size: u16,
) -> Result<(HFONT, i32, i32), FontError> {
    let device = unsafe { GetDC(window) };
    if device.is_null() {
        return Err(FontError::DeviceContextUnavailable);
    }
    let dpi = unsafe { GetDeviceCaps(device, i32::try_from(LOGPIXELSY).unwrap_or(90)) };
    let height = -((i32::from(point_size) * dpi) / 72).max(1);
    let family = wide(family);
    let font = unsafe {
        CreateFontW(
            height,
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

    Ok((font, metrics.tmAveCharWidth.max(1), metrics.tmHeight.max(1)))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_failures_have_stable_typed_codes() {
        let cases = [
            (
                FontError::DeviceContextUnavailable,
                "font_device_context_unavailable",
                "GetDC failed",
            ),
            (
                FontError::CreateFailed,
                "font_create_failed",
                "CreateFontW failed",
            ),
            (
                FontError::MetricsFailed,
                "font_metrics_failed",
                "GetTextMetricsW failed",
            ),
        ];
        for (error, code, message) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(
                error.to_capability_status(),
                CapabilityStatus::Failed {
                    code,
                    message: message.to_owned(),
                }
            );
        }
    }
}
