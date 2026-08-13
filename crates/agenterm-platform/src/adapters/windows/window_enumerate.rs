//! Windows top-level window enumeration (user32 FFI).

use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
        IsIconic, IsWindowVisible,
    },
};

use crate::CapabilityStatus;
use crate::contract::window_enumerate::{WindowBounds, WindowEnumerateError, WindowInfo};

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

pub(crate) fn enumerate_top_level() -> Result<Vec<WindowInfo>, WindowEnumerateError> {
    let mut out: Vec<WindowInfo> = Vec::new();
    unsafe {
        let ok = EnumWindows(Some(enum_proc), &mut out as *mut Vec<WindowInfo> as LPARAM);
        if ok == 0 {
            return Err(WindowEnumerateError::failed(
                "enum_windows_failed",
                "EnumWindows returned 0",
            ));
        }
    }
    Ok(out)
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
    // SAFETY: the callback body performs only user32 reads; the output
    // vector was passed as an owned pointer by `enumerate_top_level`.
    unsafe {
        let out = &mut *(lparam as *mut Vec<WindowInfo>);
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let mut title = [0u16; 512];
        let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        if len <= 0 {
            return 1;
        }
        let mut rect = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetWindowRect(hwnd, &mut rect);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        out.push(WindowInfo {
            handle: hwnd as isize,
            title: String::from_utf16_lossy(&title[..len as usize]),
            process_id: pid,
            app_name: process_name(pid),
            bounds: WindowBounds {
                x: rect.left,
                y: rect.top,
                width: (rect.right - rect.left).max(0) as u32,
                height: (rect.bottom - rect.top).max(0) as u32,
            },
            focused: GetForegroundWindow() == hwnd,
            minimized: IsIconic(hwnd) != 0,
        });
        1
    }
}

fn process_name(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return String::new();
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut name = String::new();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32ProcessID == pid {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        name
    }
}

pub(crate) fn list_screens()
-> Result<Vec<crate::contract::window_enumerate::ScreenInfo>, WindowEnumerateError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1) as u32;
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1) as u32;
    let bounds = WindowBounds {
        x: 0,
        y: 0,
        width,
        height,
    };
    Ok(vec![crate::contract::window_enumerate::ScreenInfo {
        frame: bounds,
        visible: bounds,
        primary: true,
    }])
}
