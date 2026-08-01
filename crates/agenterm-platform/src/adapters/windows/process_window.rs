use crate::contract::process_window::*;

fn error(code: &'static str, message: &'static str, cause: &'static str) -> ScriptWindowError {
    ScriptWindowError::new(code, message, Some(cause))
}

fn window_for_process(id: u32) -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};
    struct Search {
        id: u32,
        window: windows_sys::Win32::Foundation::HWND,
    }
    unsafe extern "system" fn visit(
        window: windows_sys::Win32::Foundation::HWND,
        parameter: windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::core::BOOL {
        let search = unsafe { &mut *(parameter as *mut Search) };
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut owner);
        }
        if owner == search.id {
            search.window = window;
            0
        } else {
            1
        }
    }
    let mut search = Search {
        id,
        window: core::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(
            Some(visit),
            (&mut search as *mut Search).cast::<core::ffi::c_void>() as isize,
        );
    }
    search.window
}

fn required_window(id: u32) -> Result<windows_sys::Win32::Foundation::HWND, ScriptWindowError> {
    let window = window_for_process(id);
    if window.is_null() {
        Err(error(
            "process_window_not_found",
            "child has no top-level window",
            "not_found",
        ))
    } else {
        Ok(window)
    }
}

fn control(
    process_id: u32,
    id: i32,
) -> Result<windows_sys::Win32::Foundation::HWND, ScriptWindowError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem;
    let item = unsafe { GetDlgItem(required_window(process_id)?, id) };
    if item.is_null() {
        Err(error(
            "process_window_control_not_found",
            "native child control was not found",
            "not_found",
        ))
    } else {
        Ok(item)
    }
}

pub(crate) fn facts(process_id: u32) -> ScriptWindowFacts {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };
    let window = window_for_process(process_id);
    let foreground = unsafe { GetForegroundWindow() };
    let title = if window.is_null() {
        String::new()
    } else {
        let length = unsafe { GetWindowTextLengthW(window) };
        if length <= 0 {
            String::new()
        } else {
            let mut buffer = vec![0_u16; length as usize + 1];
            let copied =
                unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
            String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
        }
    };
    ScriptWindowFacts {
        supported: true,
        present: !window.is_null(),
        window_id: window as isize as i64,
        title,
        foreground_window_id: foreground as isize as i64,
        is_foreground: !window.is_null() && window == foreground,
    }
}

pub(crate) fn key(process_id: u32, key: ScriptWindowKey) -> Result<(), ScriptWindowError> {
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::{PostMessageW, WM_KEYDOWN, WM_KEYUP},
    };
    let key = match key {
        ScriptWindowKey::Backspace => VK_BACK,
        ScriptWindowKey::Delete => VK_DELETE,
        ScriptWindowKey::Down => VK_DOWN,
        ScriptWindowKey::End => VK_END,
        ScriptWindowKey::Enter => VK_RETURN,
        ScriptWindowKey::Escape => VK_ESCAPE,
        ScriptWindowKey::F2 => VK_F2,
        ScriptWindowKey::Home => VK_HOME,
        ScriptWindowKey::Left => VK_LEFT,
        ScriptWindowKey::Right => VK_RIGHT,
        ScriptWindowKey::Tab => VK_TAB,
        ScriptWindowKey::Up => VK_UP,
    };
    let window = required_window(process_id)?;
    if unsafe { PostMessageW(window, WM_KEYDOWN, usize::from(key), 0) } == 0
        || unsafe { PostMessageW(window, WM_KEYUP, usize::from(key), 0) } == 0
    {
        return Err(error(
            "process_window_input",
            "native window key delivery failed",
            "platform_error",
        ));
    }
    Ok(())
}

pub(crate) fn pointer(
    process_id: u32,
    action: ScriptWindowPointerAction,
    x: i32,
    y: i32,
) -> Result<(), ScriptWindowError> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let window = required_window(process_id)?;
    let point = (((y & 0xffff) << 16) | (x & 0xffff)) as isize;
    if action == ScriptWindowPointerAction::Click {
        let child = unsafe {
            ChildWindowFromPointEx(window, POINT { x, y }, CWP_SKIPINVISIBLE | CWP_SKIPDISABLED)
        };
        unsafe {
            if !child.is_null() && child != window {
                SendMessageW(child, BM_CLICK, 0, 0);
            } else {
                SendMessageW(window, WM_LBUTTONDOWN, 0, point);
                SendMessageW(window, WM_LBUTTONUP, 0, point);
            }
        }
        return Ok(());
    }
    let (message, button, parameter) = match action {
        ScriptWindowPointerAction::Down => (WM_LBUTTONDOWN, 0, point),
        ScriptWindowPointerAction::Move => (WM_MOUSEMOVE, 0, point),
        ScriptWindowPointerAction::MoveHeld => (WM_MOUSEMOVE, 1, point),
        ScriptWindowPointerAction::Up => (WM_LBUTTONUP, 0, point),
        ScriptWindowPointerAction::CaptureChanged => (WM_CAPTURECHANGED, 0, 0),
        ScriptWindowPointerAction::Click => unreachable!(),
    };
    unsafe {
        SendMessageW(window, message, button, parameter);
    }
    Ok(())
}

pub(crate) fn pointer_coordinate_scale(process_id: u32) -> Result<f64, ScriptWindowError> {
    required_window(process_id)?;
    Ok(1.0)
}

pub(crate) fn message(
    process_id: u32,
    message: ScriptWindowMessage,
) -> Result<isize, ScriptWindowError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;
    Ok(unsafe {
        SendMessageW(
            required_window(process_id)?,
            message.message,
            message.wparam,
            message.lparam,
        )
    })
}

pub(crate) fn rect(process_id: u32, client: bool) -> Result<ScriptWindowRect, ScriptWindowError> {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
    };
    let mut rect = RECT::default();
    let window = required_window(process_id)?;
    let ok = unsafe {
        if client {
            GetClientRect(window, &mut rect)
        } else {
            GetWindowRect(window, &mut rect)
        }
    };
    if ok == 0 {
        return Err(error(
            "process_window_rect",
            "native window bounds could not be read",
            "platform_error",
        ));
    }
    Ok(ScriptWindowRect {
        left: i64::from(rect.left),
        top: i64::from(rect.top),
        right: i64::from(rect.right),
        bottom: i64::from(rect.bottom),
    })
}

pub(crate) fn resize(process_id: u32, width: i32, height: i32) -> Result<(), ScriptWindowError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SetWindowPos,
    };
    if unsafe {
        SetWindowPos(
            required_window(process_id)?,
            core::ptr::null_mut(),
            0,
            0,
            width,
            height,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    } == 0
    {
        return Err(error(
            "process_window_resize",
            "native window resize failed",
            "platform_error",
        ));
    }
    Ok(())
}
pub(crate) fn control_exists(process_id: u32, id: i32) -> Result<(), ScriptWindowError> {
    control(process_id, id).map(|_| ())
}
pub(crate) fn control_visible(process_id: u32, id: i32) -> Result<bool, ScriptWindowError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    Ok(unsafe { IsWindowVisible(control(process_id, id)?) != 0 })
}
pub(crate) fn control_text(process_id: u32, id: i32) -> Result<String, ScriptWindowError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
    let item = control(process_id, id)?;
    let length = unsafe { GetWindowTextLengthW(item) };
    let mut buffer = vec![0_u16; usize::try_from(length).unwrap_or(0).saturating_add(1)];
    let copied = unsafe {
        GetWindowTextW(
            item,
            buffer.as_mut_ptr(),
            i32::try_from(buffer.len()).unwrap_or(i32::MAX),
        )
    };
    if copied < 0 {
        return Err(error(
            "process_window_control_text",
            "native child control text could not be read",
            "platform_error",
        ));
    }
    Ok(String::from_utf16_lossy(
        &buffer[..usize::try_from(copied).unwrap_or(0)],
    ))
}
pub(crate) fn control_set_text(
    process_id: u32,
    id: i32,
    text: &str,
) -> Result<(), ScriptWindowError> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SETTEXT};
    let wide = OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe {
        SendMessageW(
            control(process_id, id)?,
            WM_SETTEXT,
            0,
            wide.as_ptr() as isize,
        )
    } == 0
    {
        return Err(error(
            "process_window_control_text",
            "native child control text could not be written",
            "platform_error",
        ));
    }
    Ok(())
}
pub(crate) fn control_click(process_id: u32, id: i32) -> Result<(), ScriptWindowError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{BM_CLICK, SendMessageW};
    unsafe {
        SendMessageW(control(process_id, id)?, BM_CLICK, 0, 0);
    }
    Ok(())
}
