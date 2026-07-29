use rhai::{EvalAltResult, Module};

use crate::script_error::runtime_error;

pub(crate) fn register(rhai_module: &mut Module) {
    let mut clipboard = Module::new();
    clipboard.set_native_fn("get_text", get_text);
    clipboard.set_native_fn("set_text", set_text);
    rhai_module.set_sub_module("clipboard", clipboard);
}

#[cfg(windows)]
fn open_clipboard(operation: &'static str) -> Result<(), Box<EvalAltResult>> {
    use std::{ptr, thread, time::Duration};
    use windows_sys::Win32::System::DataExchange::OpenClipboard;

    for _ in 0..200 {
        if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(clipboard_error(
        "clipboard_open",
        operation,
        "Windows clipboard could not be opened within two seconds",
        Some("busy"),
    ))
}

#[cfg(windows)]
struct ClipboardGuard;

#[cfg(windows)]
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::DataExchange::CloseClipboard;
        unsafe {
            CloseClipboard();
        }
    }
}

#[cfg(windows)]
fn get_text() -> Result<String, Box<EvalAltResult>> {
    use windows_sys::Win32::System::{
        DataExchange::{GetClipboardData, IsClipboardFormatAvailable},
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    };
    const CF_UNICODETEXT: u32 = 13;

    open_clipboard("rhai.clipboard.get_text")?;
    let _guard = ClipboardGuard;
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) } == 0 {
        return Err(clipboard_error(
            "clipboard_text_unavailable",
            "rhai.clipboard.get_text",
            "clipboard does not contain Unicode text",
            Some("not_found"),
        ));
    }
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if handle.is_null() {
        return Err(clipboard_error(
            "clipboard_read",
            "rhai.clipboard.get_text",
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
        return Err(clipboard_error(
            "clipboard_read",
            "rhai.clipboard.get_text",
            "clipboard Unicode text could not be locked",
            Some("platform_error"),
        ));
    }
    let maximum = bytes / std::mem::size_of::<u16>();
    let units = unsafe { std::slice::from_raw_parts(pointer, maximum) };
    let length = units.iter().position(|unit| *unit == 0).unwrap_or(maximum);
    let text = String::from_utf16(units.get(..length).unwrap_or_default()).map_err(|_| {
        clipboard_error(
            "clipboard_text_invalid",
            "rhai.clipboard.get_text",
            "clipboard text is not valid UTF-16",
            Some("invalid_data"),
        )
    });
    unsafe {
        GlobalUnlock(handle);
    }
    text
}

#[cfg(not(windows))]
fn get_text() -> Result<String, Box<EvalAltResult>> {
    Err(clipboard_error(
        "clipboard_unsupported",
        "rhai.clipboard.get_text",
        "native clipboard text is not implemented on this platform",
        Some("unsupported"),
    ))
}

#[cfg(windows)]
fn set_text(text: &str) -> Result<(), Box<EvalAltResult>> {
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
            clipboard_error(
                "clipboard_text_too_large",
                "rhai.clipboard.set_text",
                "clipboard text length overflowed native allocation",
                Some("limit"),
            )
        })?;
    open_clipboard("rhai.clipboard.set_text")?;
    let _guard = ClipboardGuard;
    if unsafe { EmptyClipboard() } == 0 {
        return Err(clipboard_error(
            "clipboard_clear",
            "rhai.clipboard.set_text",
            "Windows clipboard could not be cleared",
            Some("platform_error"),
        ));
    }
    let handle = unsafe { GlobalAlloc(GHND, byte_length) };
    if handle.is_null() {
        return Err(clipboard_error(
            "clipboard_allocate",
            "rhai.clipboard.set_text",
            "clipboard Unicode-text allocation failed",
            Some("resource"),
        ));
    }
    let pointer = unsafe { GlobalLock(handle) }.cast::<u16>();
    if pointer.is_null() {
        unsafe {
            GlobalFree(handle);
        }
        return Err(clipboard_error(
            "clipboard_write",
            "rhai.clipboard.set_text",
            "clipboard Unicode-text allocation could not be locked",
            Some("platform_error"),
        ));
    }
    unsafe {
        ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
        GlobalUnlock(handle);
    }
    if unsafe { SetClipboardData(CF_UNICODETEXT, handle) }.is_null() {
        unsafe {
            GlobalFree(handle);
        }
        return Err(clipboard_error(
            "clipboard_write",
            "rhai.clipboard.set_text",
            "Windows clipboard rejected Unicode text",
            Some("platform_error"),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_text(_text: &str) -> Result<(), Box<EvalAltResult>> {
    Err(clipboard_error(
        "clipboard_unsupported",
        "rhai.clipboard.set_text",
        "native clipboard text is not implemented on this platform",
        Some("unsupported"),
    ))
}

fn clipboard_error(
    code: &'static str,
    operation: &'static str,
    message: &'static str,
    cause: Option<&'static str>,
) -> Box<EvalAltResult> {
    runtime_error(
        "clipboard",
        code,
        operation,
        message,
        false,
        "system_clipboard",
        false,
        cause,
    )
}
