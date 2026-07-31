use crate::platform::contract::script_clipboard::ScriptClipboardError;

fn error(
    code: &'static str,
    message: &'static str,
    cause: Option<&'static str>,
) -> ScriptClipboardError {
    ScriptClipboardError::new(code, message, cause)
}

fn open_clipboard() -> Result<(), ScriptClipboardError> {
    use std::{ptr, thread, time::Duration};
    use windows_sys::Win32::System::DataExchange::OpenClipboard;

    for _ in 0..200 {
        if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(error(
        "clipboard_open",
        "Windows clipboard could not be opened within two seconds",
        Some("busy"),
    ))
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::DataExchange::CloseClipboard;
        unsafe { CloseClipboard() };
    }
}

pub(crate) fn get_text() -> Result<String, ScriptClipboardError> {
    use windows_sys::Win32::System::{
        DataExchange::{GetClipboardData, IsClipboardFormatAvailable},
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    };
    const CF_UNICODETEXT: u32 = 13;

    open_clipboard()?;
    let _guard = ClipboardGuard;
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
        return Err(error(
            "clipboard_text_unavailable",
            "clipboard does not contain Unicode text",
            Some("not_found"),
        ));
    }
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if handle.is_null() {
        return Err(error(
            "clipboard_read",
            "clipboard Unicode-text handle is unavailable",
            Some("platform_error"),
        ));
    }
    let bytes = unsafe { GlobalSize(handle) };
    if bytes < 2 {
        return Ok(String::new());
    }
    let pointer = unsafe { GlobalLock(handle) }.cast::<u16>();
    if pointer.is_null() {
        return Err(error(
            "clipboard_read",
            "clipboard Unicode text could not be locked",
            Some("platform_error"),
        ));
    }
    let maximum = bytes / std::mem::size_of::<u16>();
    let units = unsafe { std::slice::from_raw_parts(pointer, maximum) };
    let length = units.iter().position(|unit| *unit == 0).unwrap_or(maximum);
    let text = String::from_utf16(&units[..length]).map_err(|_| {
        error(
            "clipboard_text_invalid",
            "clipboard text is not valid UTF-16",
            Some("invalid_data"),
        )
    });
    unsafe { GlobalUnlock(handle) };
    text
}

pub(crate) fn set_text(text: &str) -> Result<(), ScriptClipboardError> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{
            DataExchange::{EmptyClipboard, SetClipboardData},
            Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    };
    const CF_UNICODETEXT: u32 = 13;

    let wide = OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let byte_length = wide
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| {
            error(
                "clipboard_text_too_large",
                "clipboard text length overflowed native allocation",
                Some("limit"),
            )
        })?;
    open_clipboard()?;
    let _guard = ClipboardGuard;
    if unsafe { EmptyClipboard() } == 0 {
        return Err(error(
            "clipboard_clear",
            "Windows clipboard could not be cleared",
            Some("platform_error"),
        ));
    }
    let handle = unsafe { GlobalAlloc(GHND, byte_length) };
    if handle.is_null() {
        return Err(error(
            "clipboard_allocate",
            "clipboard Unicode-text allocation failed",
            Some("resource"),
        ));
    }
    let pointer = unsafe { GlobalLock(handle) }.cast::<u16>();
    if pointer.is_null() {
        unsafe { GlobalFree(handle) };
        return Err(error(
            "clipboard_write",
            "clipboard Unicode-text allocation could not be locked",
            Some("platform_error"),
        ));
    }
    unsafe {
        ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
        GlobalUnlock(handle);
    }
    if unsafe { SetClipboardData(CF_UNICODETEXT, handle) }.is_null() {
        unsafe { GlobalFree(handle) };
        return Err(error(
            "clipboard_write",
            "Windows clipboard rejected Unicode text",
            Some("platform_error"),
        ));
    }
    Ok(())
}
