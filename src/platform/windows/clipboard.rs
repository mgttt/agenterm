//! Bounded Win32 Unicode clipboard capability.

#![cfg(target_os = "windows")]

use std::{
    fmt, mem, ptr, thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
};

use crate::platform::CapabilityStatus;

const UNICODE_TEXT: u32 = 13;
const OPEN_TIMEOUT: Duration = Duration::from_millis(500);
const RETRY_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipboardError {
    Busy,
    Unavailable,
    TooLarge { limit: usize },
    InvalidUtf16,
    Backend(&'static str),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => write!(
                formatter,
                "could not open the Windows clipboard within 500 ms"
            ),
            Self::Unavailable => write!(formatter, "the clipboard does not contain Unicode text"),
            Self::TooLarge { limit } => {
                write!(formatter, "clipboard text exceeds the {limit}-byte limit")
            }
            Self::InvalidUtf16 => write!(formatter, "clipboard text is not valid UTF-16"),
            Self::Backend(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ClipboardError {}

impl ClipboardError {
    pub(crate) fn to_capability_status(&self) -> CapabilityStatus {
        let code = match self {
            Self::Busy => "clipboard_busy",
            Self::Unavailable => "clipboard_unicode_text_unavailable",
            Self::TooLarge { .. } => "clipboard_too_large",
            Self::InvalidUtf16 => "clipboard_invalid_utf16",
            Self::Backend(_) => "clipboard_backend_error",
        };
        CapabilityStatus::Failed {
            code,
            message: self.to_string(),
        }
    }
}

struct OpenClipboardGuard;

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

fn open(owner: HWND) -> Result<OpenClipboardGuard, ClipboardError> {
    let deadline = Instant::now() + OPEN_TIMEOUT;
    loop {
        if unsafe { OpenClipboard(owner) } != 0 {
            return Ok(OpenClipboardGuard);
        }
        if Instant::now() >= deadline {
            return Err(ClipboardError::Busy);
        }
        thread::sleep(RETRY_INTERVAL);
    }
}

pub(crate) fn has_unicode_text() -> bool {
    unsafe { IsClipboardFormatAvailable(UNICODE_TEXT) != 0 }
}

pub(crate) fn set_text(owner: HWND, text: &str) -> Result<(), ClipboardError> {
    let _guard = open(owner)?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err(ClipboardError::Backend(
            "could not clear the Windows clipboard",
        ));
    }

    let encoded = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let allocation = unsafe { GlobalAlloc(GMEM_MOVEABLE, mem::size_of_val(encoded.as_slice())) };
    if allocation.is_null() {
        return Err(ClipboardError::Backend("could not allocate clipboard text"));
    }
    let destination = unsafe { GlobalLock(allocation) } as *mut u16;
    if destination.is_null() {
        unsafe { GlobalFree(allocation) };
        return Err(ClipboardError::Backend("could not lock clipboard text"));
    }
    unsafe {
        ptr::copy_nonoverlapping(encoded.as_ptr(), destination, encoded.len());
        GlobalUnlock(allocation);
    }
    if unsafe { SetClipboardData(UNICODE_TEXT, allocation) }.is_null() {
        unsafe { GlobalFree(allocation) };
        return Err(ClipboardError::Backend("could not publish clipboard text"));
    }
    Ok(())
}

pub(crate) fn get_text(max_utf8_bytes: usize) -> Result<String, ClipboardError> {
    let _guard = open(ptr::null_mut())?;
    if !has_unicode_text() {
        return Err(ClipboardError::Unavailable);
    }
    let allocation = unsafe { GetClipboardData(UNICODE_TEXT) };
    if allocation.is_null() {
        return Err(ClipboardError::Backend(
            "could not read Unicode clipboard data",
        ));
    }
    let allocation_size = unsafe { GlobalSize(allocation) };
    if allocation_size == 0 {
        return Err(ClipboardError::Backend(
            "Unicode clipboard data has no readable allocation",
        ));
    }
    let maximum_utf16_allocation = max_utf8_bytes
        .saturating_add(1)
        .saturating_mul(mem::size_of::<u16>());
    if allocation_size > maximum_utf16_allocation {
        return Err(ClipboardError::TooLarge {
            limit: max_utf8_bytes,
        });
    }

    let source = unsafe { GlobalLock(allocation) } as *const u16;
    if source.is_null() {
        return Err(ClipboardError::Backend(
            "could not lock Unicode clipboard data",
        ));
    }
    let units =
        unsafe { std::slice::from_raw_parts(source, allocation_size / mem::size_of::<u16>()) };
    let length = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    let decoded = String::from_utf16(&units[..length]).map_err(|_| ClipboardError::InvalidUtf16);
    unsafe { GlobalUnlock(allocation) };
    let decoded = decoded?;
    if decoded.len() > max_utf8_bytes {
        return Err(ClipboardError::TooLarge {
            limit: max_utf8_bytes,
        });
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_errors_are_stable_and_typed() {
        assert_eq!(
            ClipboardError::TooLarge { limit: 128 }.to_string(),
            "clipboard text exceeds the 128-byte limit"
        );
        assert_eq!(
            ClipboardError::InvalidUtf16.to_string(),
            "clipboard text is not valid UTF-16"
        );
        assert_eq!(
            ClipboardError::Busy.to_capability_status(),
            CapabilityStatus::Failed {
                code: "clipboard_busy",
                message: "could not open the Windows clipboard within 500 ms".to_string(),
            }
        );
    }

    #[test]
    fn allocation_bound_covers_the_largest_valid_utf16_input() {
        let limit = 64_usize;
        let maximum = limit
            .saturating_add(1)
            .saturating_mul(mem::size_of::<u16>());
        assert_eq!(maximum, 130);
    }
}
