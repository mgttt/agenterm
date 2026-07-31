//! Windows Control Center native operations.

use std::{ffi::OsStr, fs::OpenOptions, io, os::windows::ffi::OsStrExt, path::Path};

use windows_sys::Win32::{
    Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    UI::WindowsAndMessaging::{
        IsWindow, SW_RESTORE, SW_SHOWNOACTIVATE, SetForegroundWindow, ShowWindowAsync,
    },
};

use crate::platform::control_center::ScreenshotStrategy;

pub(crate) const fn screenshot_strategy() -> ScreenshotStrategy {
    ScreenshotStrategy::DirectNativeWindow
}
pub(crate) fn protect_state_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
pub(crate) fn private_create_new_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
}
pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    let wide = |path: &Path| {
        OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    };
    let source = wide(source);
    let destination = wide(destination);
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub(crate) fn focus_existing_window(raw_handle: i64, no_activate: bool) {
    let window = raw_handle as isize as *mut std::ffi::c_void;
    if window.is_null() || unsafe { IsWindow(window) } == 0 {
        return;
    }
    unsafe {
        ShowWindowAsync(
            window,
            if no_activate {
                SW_SHOWNOACTIVATE
            } else {
                SW_RESTORE
            },
        );
        if !no_activate {
            SetForegroundWindow(window);
        }
    }
}
pub(crate) fn capture_native_window_png(raw_handle: i64, output: &Path) -> io::Result<()> {
    let window = raw_handle as isize as *mut std::ffi::c_void;
    if window.is_null() || unsafe { IsWindow(window) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "control_center_screenshot_window_unavailable",
        ));
    }
    crate::platform::selected::native::screenshot::save_png(
        window,
        output,
        crate::platform::selected::native::screenshot::CaptureArea::Window,
    )
    .map_err(|error| io::Error::other(error.to_string()))
}
