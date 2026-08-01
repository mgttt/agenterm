//! Windows display facts and generic native text-window host.

use std::{io, mem, ptr, sync::Mutex};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, EndPaint, GetStockObject, InvalidateRect, PAINTSTRUCT, TextOutW, WHITE_BRUSH,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
        DispatchMessageW, GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostMessageW,
        PostQuitMessage, RegisterClassW, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE,
        SetForegroundWindow, SetTimer, SetWindowTextW, ShowWindow, TranslateMessage, WM_CLOSE,
        WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    },
};

use crate::window::{
    DisplayBackendFacts, NativeTextWindowError, NativeTextWindowFocus, NativeTextWindowHost,
};

const WINDOW_TIMER_ID: usize = 1;
const WINDOW_TIMER_INTERVAL_MS: u32 = 200;

struct ShellState {
    host: Box<dyn NativeTextWindowHost>,
    deferred_error: Option<NativeTextWindowError>,
}

static SHELL_STATE: std::sync::OnceLock<Mutex<ShellState>> = std::sync::OnceLock::new();

pub(crate) fn display_backend_facts() -> DisplayBackendFacts {
    DisplayBackendFacts {
        x11: false,
        wayland: false,
        headless: false,
    }
}

pub(crate) fn run_native_text_window(
    host: Box<dyn NativeTextWindowHost>,
    no_activate: bool,
) -> Result<(), NativeTextWindowError> {
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        return Err(last_os_error("native_text_window_module_handle_failed"));
    }

    let title = wide_null(&host.title());
    SHELL_STATE
        .set(Mutex::new(ShellState {
            host,
            deferred_error: None,
        }))
        .map_err(|_| {
            NativeTextWindowError::failed(
                "native_text_window_already_initialized",
                "the native text window may only be initialized once",
            )
        })?;

    let class = wide_null("AgentermPlatformNativeTextWindow");
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        hbrBackground: unsafe { GetStockObject(WHITE_BRUSH) } as _,
        lpszClassName: class.as_ptr(),
        ..unsafe { mem::zeroed() }
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(last_os_error("native_text_window_class_register_failed"));
    }

    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            480,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        )
    };
    if window.is_null() {
        return Err(last_os_error("native_text_window_create_failed"));
    }

    if let Err(error) = with_state(|state| state.host.publish_native_window(window as isize as i64))
    {
        unsafe { DestroyWindow(window) };
        return Err(error);
    }
    if unsafe { SetTimer(window, WINDOW_TIMER_ID, WINDOW_TIMER_INTERVAL_MS, None) } == 0 {
        let error = last_os_error("native_text_window_timer_failed");
        unsafe { DestroyWindow(window) };
        return Err(error);
    }
    unsafe {
        ShowWindow(
            window,
            if no_activate {
                SW_SHOWNOACTIVATE
            } else {
                SW_SHOW
            },
        )
    };

    let mut message: MSG = unsafe { mem::zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if result == -1 {
            return Err(last_os_error("native_text_window_message_loop_failed"));
        }
        if result == 0 {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    with_state(|state| match state.deferred_error.take() {
        Some(error) => Err(error),
        None => Ok(()),
    })
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { mem::zeroed() };
            let device = unsafe { BeginPaint(window, &mut paint) };
            let lines = SHELL_STATE
                .get()
                .and_then(|state| state.lock().ok())
                .map(|state| state.host.lines())
                .unwrap_or_else(|| vec!["native text window state unavailable".to_owned()]);
            for (index, line) in lines.iter().enumerate() {
                let wide = line.encode_utf16().collect::<Vec<_>>();
                unsafe {
                    TextOutW(
                        device,
                        24,
                        24 + i32::try_from(index).unwrap_or(0) * 28,
                        wide.as_ptr(),
                        i32::try_from(wide.len()).unwrap_or(0),
                    );
                }
            }
            unsafe { EndPaint(window, &paint) };
            0
        }
        WM_TIMER => {
            poll_host(window);
            0
        }
        WM_DESTROY => {
            unsafe {
                KillTimer(window, WINDOW_TIMER_ID);
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn poll_host(window: HWND) {
    let Some(state) = SHELL_STATE.get() else {
        return;
    };
    let Ok(mut state) = state.lock() else { return };
    if state.host.close_requested() {
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        return;
    }
    if let Some(request) = state.host.take_focus_request() {
        unsafe {
            match request {
                NativeTextWindowFocus::Activate => {
                    ShowWindow(window, SW_RESTORE);
                    SetForegroundWindow(window);
                }
                NativeTextWindowFocus::NoActivate => {
                    ShowWindow(window, SW_SHOWNOACTIVATE);
                }
            }
        }
    }
    if let Err(error) = state.host.capture_requested_screenshot(None) {
        state.deferred_error = Some(error);
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        return;
    }
    if state.host.poll() {
        let title = wide_null(&state.host.title());
        unsafe {
            SetWindowTextW(window, title.as_ptr());
            InvalidateRect(window, ptr::null(), 1);
        }
    }
}

fn with_state<T>(operation: impl FnOnce(&mut ShellState) -> T) -> T {
    let state = SHELL_STATE
        .get()
        .expect("native text window state initialized before creation");
    let mut state = state
        .lock()
        .expect("native text window state is not poisoned");
    operation(&mut state)
}

fn last_os_error(code: &'static str) -> NativeTextWindowError {
    NativeTextWindowError::failed(code, io::Error::last_os_error())
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
