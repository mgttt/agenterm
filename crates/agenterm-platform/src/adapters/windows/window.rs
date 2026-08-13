//! Windows display facts and generic native text-window host.

use std::{io, mem, ptr, sync::Mutex};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, EndPaint, GetStockObject, InvalidateRect, PAINTSTRUCT, TextOutW, WHITE_BRUSH,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::Input::KeyboardAndMouse::{VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_RETURN, VK_UP},
    UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
        DispatchMessageW, GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostMessageW,
        PostQuitMessage, RegisterClassW, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE,
        SetForegroundWindow, SetTimer, SetWindowTextW, ShowWindow, TranslateMessage, WM_CLOSE,
        WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_PAINT, WM_RBUTTONDOWN,
        WM_SYSKEYDOWN, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    },
};

#[cfg(all(
    feature = "input",
    feature = "ime",
    any(feature = "native-pixel-window", feature = "portable-pixel-window")
))]
use crate::contract::window_host::{PixelWindowApplication, PixelWindowError, PixelWindowOptions};
use crate::window::{
    DisplayBackendFacts, NativeTextInputEvent, NativeTextKey, NativeTextPointerButton,
    NativeTextWindowError, NativeTextWindowFocus, NativeTextWindowHost,
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

#[cfg(all(feature = "input", feature = "ime", feature = "native-pixel-window"))]
mod native_pixel_window;

#[cfg(all(
    feature = "input",
    feature = "ime",
    feature = "portable-pixel-window",
    not(feature = "native-pixel-window")
))]
#[path = "../unix/window_host.rs"]
mod window_host;

#[cfg(all(
    feature = "input",
    feature = "ime",
    any(feature = "native-pixel-window", feature = "portable-pixel-window")
))]
pub(crate) fn run_pixel_window(
    options: PixelWindowOptions,
    application: Box<dyn PixelWindowApplication>,
) -> Result<(), PixelWindowError> {
    #[cfg(feature = "native-pixel-window")]
    {
        native_pixel_window::run_pixel_window(options, application)
    }
    #[cfg(not(feature = "native-pixel-window"))]
    {
        window_host::run_pixel_window(options, application)
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
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (physical_x, physical_y) = client_point(lparam);
            let button = match message {
                WM_LBUTTONDOWN => NativeTextPointerButton::Primary,
                WM_RBUTTONDOWN => NativeTextPointerButton::Secondary,
                _ => NativeTextPointerButton::Middle,
            };
            dispatch_host_input(
                window,
                NativeTextInputEvent::PointerPressed {
                    button,
                    physical_x,
                    physical_y,
                    line: windows_text_line_at(physical_y),
                },
            );
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let Some(key) = normalize_key(wparam) else {
                return unsafe { DefWindowProcW(window, message, wparam, lparam) };
            };
            dispatch_host_input(
                window,
                NativeTextInputEvent::KeyPressed {
                    key,
                    repeat: lparam & (1_isize << 30) != 0,
                },
            );
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

fn dispatch_host_input(window: HWND, event: NativeTextInputEvent) {
    let Some(state) = SHELL_STATE.get() else {
        return;
    };
    let Ok(mut state) = state.lock() else { return };
    match state.host.handle_input(event) {
        Ok(true) => unsafe {
            InvalidateRect(window, ptr::null(), 1);
        },
        Ok(false) => {}
        Err(error) => {
            state.deferred_error = Some(error);
            unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        }
    }
}

fn client_point(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam as u32;
    (
        i32::from((packed & 0xffff) as u16 as i16),
        i32::from((packed >> 16) as u16 as i16),
    )
}

fn windows_text_line_at(physical_y: i32) -> Option<usize> {
    const FIRST_LINE_TOP: i32 = 16;
    const LINE_HEIGHT: i32 = 28;
    (physical_y >= FIRST_LINE_TOP)
        .then(|| usize::try_from((physical_y - FIRST_LINE_TOP) / LINE_HEIGHT).ok())
        .flatten()
}

fn normalize_key(wparam: WPARAM) -> Option<NativeTextKey> {
    match u16::try_from(wparam).ok()? {
        value if value == VK_UP => Some(NativeTextKey::ArrowUp),
        value if value == VK_DOWN => Some(NativeTextKey::ArrowDown),
        value if value == VK_HOME => Some(NativeTextKey::Home),
        value if value == VK_END => Some(NativeTextKey::End),
        value if value == VK_RETURN => Some(NativeTextKey::Enter),
        value if value == VK_ESCAPE => Some(NativeTextKey::Escape),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_pointer_coordinates_preserve_signed_client_values() {
        let packed = (u32::from((-7_i16) as u16) << 16) | u32::from(24_u16);
        assert_eq!(client_point(packed as isize), (24, -7));
    }

    #[test]
    fn text_hit_normalization_rejects_header_margin() {
        assert_eq!(windows_text_line_at(15), None);
        assert_eq!(windows_text_line_at(16), Some(0));
        assert_eq!(windows_text_line_at(44), Some(1));
    }
}
