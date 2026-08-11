//! Minimal Win32 implementation of the platform-neutral pixel-window host.
//!
//! This backend deliberately presents an owned XRGB buffer with
//! `StretchDIBits` instead of linking winit/softbuffer. Product state remains
//! behind `PixelWindowApplication`; HWND and message policy stop here.

use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
    time::Instant,
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, DIB_RGB_COLORS, EndPaint, InvalidateRect,
        PAINTSTRUCT, SRCCOPY, StretchDIBits,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        Input::Ime::{IACE_DEFAULT, ImmAssociateContextEx},
        Input::KeyboardAndMouse::{
            GetCapture, GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE,
            TRACKMOUSEEVENT, TrackMouseEvent, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
            VK_ESCAPE, VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10,
            VK_F11, VK_F12, VK_HOME, VK_INSERT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN,
            VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
        },
        WindowsAndMessaging::{
            CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
            DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW,
            GetWindowLongPtrW, GetWindowRect, IDC_ARROW, IDC_SIZEWE, IsIconic, IsWindowVisible,
            IsZoomed, KillTimer, LoadCursorW, MSG, PM_REMOVE, PeekMessageW, PostMessageW,
            RegisterClassW, SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
            SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetCursor, SetForegroundWindow,
            SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
            TranslateMessage, WM_APP, WM_CANCELMODE, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE,
            WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_IME_CHAR, WM_IME_COMPOSITION,
            WM_IME_ENDCOMPOSITION, WM_IME_SETCONTEXT, WM_IME_STARTCOMPOSITION, WM_KEYDOWN,
            WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
            WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_QUIT,
            WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETFOCUS, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP,
            WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    },
};

use crate::contract::{
    input::{
        KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent, PhysicalKeyCode,
        Utf16TextDecoder,
    },
    window_host::{
        GeometryChange, LogicalPoint, LogicalRect, LogicalSize, PixelPointerCursor, PixelRect,
        PixelWindow, PixelWindowApplication, PixelWindowBackend, PixelWindowDirective,
        PixelWindowError, PixelWindowEvent, PixelWindowMetrics, PixelWindowOptions, PointerButton,
        PointerButtonState, WheelDelta, WindowSemanticFlags, WindowWaker, XrgbPixelFrame,
    },
};

const WAKE_MESSAGE: u32 = WM_APP + 0x41;
const CAPTURE_MESSAGE: u32 = WM_APP + 0x42;
const IME_ALLOWED_MESSAGE: u32 = WM_APP + 0x43;
const MOUSE_LEAVE_MESSAGE: u32 = 0x02a3;
const WAIT_TIMER_ID: usize = 0x41;
const CLASS_NAME: &str = "AgenTermNativePixelWindow";
const WIN32_MAX_COORD: u32 = i32::MAX as u32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Win32Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RedrawInvalidation {
    Empty,
    Full,
    Rect(Win32Rect),
}

fn win32_invalidation_for_damage(damage: PixelRect, width: u32, height: u32) -> RedrawInvalidation {
    if damage.is_empty() {
        return RedrawInvalidation::Empty;
    }
    if width == 0 || height == 0 || width > WIN32_MAX_COORD || height > WIN32_MAX_COORD {
        return RedrawInvalidation::Full;
    }
    if damage.left > WIN32_MAX_COORD
        || damage.top > WIN32_MAX_COORD
        || damage.right > WIN32_MAX_COORD
        || damage.bottom > WIN32_MAX_COORD
    {
        return RedrawInvalidation::Full;
    }
    let clipped = damage.clip(width, height);
    if clipped.is_empty() {
        return RedrawInvalidation::Empty;
    }
    if clipped.left > WIN32_MAX_COORD
        || clipped.top > WIN32_MAX_COORD
        || clipped.right > WIN32_MAX_COORD
        || clipped.bottom > WIN32_MAX_COORD
    {
        return RedrawInvalidation::Full;
    }
    RedrawInvalidation::Rect(Win32Rect {
        left: clipped.left as i32,
        top: clipped.top as i32,
        right: clipped.right as i32,
        bottom: clipped.bottom as i32,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StretchDibGeometry {
    source: Win32Rect,
    destination: Win32Rect,
}

/// `BITMAPINFOHEADER.biHeight` is negative below, so the XRGB buffer is a
/// top-down DIB. The source and destination rectangles therefore share the
/// client-coordinate Y axis without a bottom-up inversion.
fn stretch_geometry_for_paint(
    paint: Win32Rect,
    width: u32,
    height: u32,
) -> Option<StretchDibGeometry> {
    if width == 0 || height == 0 || width > WIN32_MAX_COORD || height > WIN32_MAX_COORD {
        return None;
    }
    let left = i64::from(paint.left).max(0);
    let top = i64::from(paint.top).max(0);
    let right = i64::from(paint.right).min(i64::from(width));
    let bottom = i64::from(paint.bottom).min(i64::from(height));
    if right <= left || bottom <= top {
        return None;
    }
    let rect = Win32Rect {
        left: left as i32,
        top: top as i32,
        right: right as i32,
        bottom: bottom as i32,
    };
    Some(StretchDibGeometry {
        source: rect,
        destination: rect,
    })
}

struct Backend {
    hwnd: Cell<HWND>,
    wake_hwnd: Arc<AtomicIsize>,
    metrics: RefCell<PixelWindowMetrics>,
    alive: Arc<AtomicBool>,
    capture_active: Cell<bool>,
    ime_allowed: Cell<bool>,
}

impl PixelWindowBackend for Backend {
    fn request_redraw(&self) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_null() {
            unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
        }
    }

    fn request_redraw_rect(&self, damage: PixelRect) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        let metrics = *self.metrics.borrow();
        match win32_invalidation_for_damage(damage, metrics.physical_width, metrics.physical_height)
        {
            RedrawInvalidation::Empty => {}
            RedrawInvalidation::Full => self.request_redraw(),
            RedrawInvalidation::Rect(rect) => {
                let native = RECT {
                    left: rect.left,
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                };
                if unsafe { InvalidateRect(hwnd, &native, 0) } == 0 {
                    self.request_redraw();
                }
            }
        }
    }

    fn metrics(&self) -> Result<PixelWindowMetrics, PixelWindowError> {
        Ok(*self.metrics.borrow())
    }

    fn semantic_flags(&self) -> WindowSemanticFlags {
        let hwnd = self.hwnd.get();
        WindowSemanticFlags {
            minimized: !hwnd.is_null() && unsafe { IsIconic(hwnd) } != 0,
            maximized: !hwnd.is_null() && unsafe { IsZoomed(hwnd) } != 0,
            visible: !hwnd.is_null() && unsafe { IsWindowVisible(hwnd) } != 0,
        }
    }

    fn set_minimized(&self, minimized: bool) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_null() {
            unsafe { ShowWindow(hwnd, if minimized { SW_MINIMIZE } else { SW_RESTORE }) };
        }
    }

    fn set_maximized(&self, maximized: bool) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_null() {
            unsafe { ShowWindow(hwnd, if maximized { SW_MAXIMIZE } else { SW_RESTORE }) };
        }
    }

    fn set_visible(&self, visible: bool) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_null() {
            unsafe { ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE }) };
        }
    }

    fn focus(&self) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_null() {
            unsafe {
                SetForegroundWindow(hwnd);
                SetFocus(hwnd);
            }
        }
    }

    fn set_title(&self, title: &str) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_null() {
            let title = wide_null(title);
            unsafe { SetWindowTextW(hwnd, title.as_ptr()) };
        }
    }

    fn set_pointer_cursor(&self, cursor: PixelPointerCursor) -> Result<(), PixelWindowError> {
        let id = match cursor {
            PixelPointerCursor::Arrow => IDC_ARROW,
            PixelPointerCursor::ResizeHorizontal => IDC_SIZEWE,
        };
        let handle = unsafe { LoadCursorW(ptr::null_mut(), id) };
        if handle.is_null() {
            return Err(last_error("pixel_window_cursor_load_failed"));
        }
        unsafe { SetCursor(handle) };
        Ok(())
    }

    fn request_logical_inner_size(&self, size: LogicalSize) -> Result<(), PixelWindowError> {
        if !size.is_valid() {
            return Err(PixelWindowError::failed(
                "pixel_window_invalid_inner_size",
                "logical inner size must be positive and finite",
            ));
        }
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return Err(closed_error());
        }
        let metrics = *self.metrics.borrow();
        let mut client: RECT = unsafe { mem::zeroed() };
        let mut outer: RECT = unsafe { mem::zeroed() };
        if unsafe { GetClientRect(hwnd, &mut client) } == 0
            || unsafe { GetWindowRect(hwnd, &mut outer) } == 0
        {
            return Err(last_error("pixel_window_geometry_query_failed"));
        }
        let chrome_w = (outer.right - outer.left) - (client.right - client.left);
        let chrome_h = (outer.bottom - outer.top) - (client.bottom - client.top);
        let width = (size.width * metrics.scale_factor).round() as i32 + chrome_w;
        let height = (size.height * metrics.scale_factor).round() as i32 + chrome_h;
        if unsafe {
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                0,
                0,
                width.max(1),
                height.max(1),
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        } == 0
        {
            return Err(last_error("pixel_window_resize_failed"));
        }
        Ok(())
    }

    fn set_pointer_capture(&self, captured: bool) -> Result<(), PixelWindowError> {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return Err(closed_error());
        }
        if captured {
            unsafe { SetCapture(hwnd) };
            if unsafe { GetCapture() } != hwnd {
                return Err(PixelWindowError::failed(
                    "pixel_window_pointer_capture_failed",
                    "SetCapture did not transfer pointer capture to the pixel window",
                ));
            }
            self.capture_active.set(true);
        } else {
            self.capture_active.set(false);
            if unsafe { PostMessageW(hwnd, CAPTURE_MESSAGE, 0, 0) } == 0 {
                return Err(last_error("pixel_window_pointer_release_post_failed"));
            }
        }
        Ok(())
    }

    fn set_ime_allowed(&self, allowed: bool) {
        self.ime_allowed.set(allowed);
        let hwnd = self.hwnd.get();
        if !hwnd.is_null() {
            unsafe { PostMessageW(hwnd, IME_ALLOWED_MESSAGE, usize::from(allowed), 0) };
        }
    }

    fn set_ime_cursor_area(&self, area: LogicalRect) -> Result<(), PixelWindowError> {
        let scale = self.metrics.borrow().scale_factor;
        crate::selected::ime::set_anchor_position(
            (area.origin.x * scale).round() as i32,
            (area.origin.y * scale).round() as i32,
        );
        Ok(())
    }
}

struct HostState {
    application: Box<dyn PixelWindowApplication>,
    backend: Rc<Backend>,
    window: PixelWindow,
    pixels: Vec<u32>,
    opened: bool,
    exit: bool,
    deferred_error: Option<PixelWindowError>,
    decoder: Utf16TextDecoder,
    ime_decoder: Utf16TextDecoder,
    ime_composing: bool,
    tracking_mouse: bool,
}

pub(crate) fn run_pixel_window(
    options: PixelWindowOptions,
    application: Box<dyn PixelWindowApplication>,
) -> Result<(), PixelWindowError> {
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        return Err(last_error("pixel_window_module_handle_failed"));
    }
    let class_name = wide_null(CLASS_NAME);
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        lpszClassName: class_name.as_ptr(),
        ..unsafe { mem::zeroed() }
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(last_error("pixel_window_class_register_failed"));
    }

    let scale = 1.0;
    let initial_metrics = PixelWindowMetrics {
        logical_size: options.initial_logical_size,
        physical_width: options.initial_logical_size.width.round().max(1.0) as u32,
        physical_height: options.initial_logical_size.height.round().max(1.0) as u32,
        scale_factor: scale,
    };
    let alive = Arc::new(AtomicBool::new(true));
    let backend = Rc::new(Backend {
        hwnd: Cell::new(ptr::null_mut()),
        wake_hwnd: Arc::new(AtomicIsize::new(0)),
        metrics: RefCell::new(initial_metrics),
        alive: Arc::clone(&alive),
        capture_active: Cell::new(false),
        ime_allowed: Cell::new(options.ime_allowed),
    });
    let wake_alive = Arc::clone(&alive);
    let wake_hwnd = Arc::clone(&backend.wake_hwnd);
    let waker = WindowWaker::new(Arc::new(move || {
        let hwnd = wake_hwnd.load(Ordering::Acquire) as HWND;
        if !wake_alive.load(Ordering::Acquire) || hwnd.is_null() {
            return Err(closed_error());
        }
        if unsafe { PostMessageW(hwnd, WAKE_MESSAGE, 0, 0) } == 0 {
            return Err(last_error("pixel_window_wake_failed"));
        }
        Ok(())
    }));
    let window = PixelWindow::new(backend.clone(), waker);
    let mut state = Box::new(HostState {
        application,
        backend: backend.clone(),
        window,
        pixels: Vec::new(),
        opened: false,
        exit: false,
        deferred_error: None,
        decoder: Utf16TextDecoder::default(),
        ime_decoder: Utf16TextDecoder::default(),
        ime_composing: false,
        tracking_mouse: false,
    });
    let title = wide_null(&options.title);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            initial_metrics.physical_width as i32,
            initial_metrics.physical_height as i32,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            (&mut *state as *mut HostState).cast::<c_void>(),
        )
    };
    if hwnd.is_null() {
        return Err(last_error("pixel_window_create_failed"));
    }
    backend.hwnd.set(hwnd);
    backend.wake_hwnd.store(hwnd as isize, Ordering::Release);
    apply_ime_allowed(hwnd, options.ime_allowed);
    update_metrics(&mut state, GeometryChange::Resized, false);
    let opened = state.application.opened(&state.window);
    apply_directive(&mut state, opened);
    state.opened = true;
    unsafe {
        ShowWindow(
            hwnd,
            if options.no_activate {
                SW_SHOWNOACTIVATE
            } else {
                SW_SHOW
            },
        )
    };
    backend.request_redraw();

    while !state.exit {
        let mut message: MSG = unsafe { mem::zeroed() };
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if result == -1 {
            state.deferred_error = Some(last_error("pixel_window_message_loop_failed"));
            break;
        }
        if result == 0 || message.message == WM_QUIT {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        while !state.exit
            && unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
        {
            if message.message == WM_QUIT {
                state.exit = true;
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if state.exit {
            break;
        }
        let directive = state
            .application
            .about_to_wait(&state.window, Instant::now());
        apply_directive(&mut state, directive);
    }

    alive.store(false, Ordering::Release);
    backend.wake_hwnd.store(0, Ordering::Release);
    let remaining_hwnd = backend.hwnd.replace(ptr::null_mut());
    if !remaining_hwnd.is_null() {
        unsafe { DestroyWindow(remaining_hwnd) };
    }
    state.deferred_error.map_or(Ok(()), Err)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam as *const CREATESTRUCTW;
        if !create.is_null() {
            let state = unsafe { (*create).lpCreateParams } as *mut HostState;
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        }
    }
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut HostState;
    if state.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        dispatch_message(&mut *state, hwnd, message, wparam, lparam)
    }));
    match result {
        Ok(value) => value,
        Err(_) => {
            unsafe { (*state).exit = true };
            0
        }
    }
}

unsafe fn dispatch_message(
    state: &mut HostState,
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            if let Err(error) = paint(state, hwnd) {
                if state.deferred_error.is_none() {
                    state.deferred_error = Some(error);
                }
                state.exit = true;
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_CLOSE => {
            dispatch_event(state, PixelWindowEvent::CloseRequested);
            0
        }
        WM_DESTROY => {
            state.exit = true;
            0
        }
        WM_NCDESTROY => {
            state.backend.alive.store(false, Ordering::Release);
            state.backend.wake_hwnd.store(0, Ordering::Release);
            state.backend.hwnd.set(ptr::null_mut());
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            0
        }
        WM_SIZE => {
            if wparam != 1 {
                update_metrics(state, GeometryChange::Resized, true);
            }
            0
        }
        WM_DPICHANGED => {
            apply_dpi_suggested_rect(hwnd, lparam);
            update_metrics(state, GeometryChange::ScaleFactorChanged, true);
            0
        }
        WM_SETFOCUS => {
            dispatch_event(state, PixelWindowEvent::FocusChanged(true));
            if state.backend.ime_allowed.get() {
                dispatch_event(
                    state,
                    PixelWindowEvent::Ime(crate::contract::ime::ImeEvent::Enabled),
                );
            }
            0
        }
        WM_KILLFOCUS => {
            state.ime_composing = false;
            crate::selected::ime::refresh_from_message(hwnd, WM_IME_ENDCOMPOSITION);
            dispatch_event(
                state,
                PixelWindowEvent::Ime(crate::contract::ime::ImeEvent::Disabled),
            );
            dispatch_event(state, PixelWindowEvent::FocusChanged(false));
            0
        }
        WM_IME_SETCONTEXT => {
            const IS_SHOWUICOMPOSITIONWINDOW: isize = 0x0002;
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam & !IS_SHOWUICOMPOSITIONWINDOW) }
        }
        WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION => {
            crate::selected::ime::refresh_from_message(hwnd, message);
            let composition = crate::selected::ime::composition();
            state.ime_composing = message != WM_IME_ENDCOMPOSITION && composition.is_some();
            let (text, cursor) = composition.map_or_else(
                || (String::new(), None),
                |composition| {
                    let cursor = composition.cursor;
                    (composition.text, Some((cursor, cursor)))
                },
            );
            dispatch_event(
                state,
                PixelWindowEvent::Ime(crate::contract::ime::ImeEvent::Preedit { text, cursor }),
            );
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_IME_CHAR => {
            if let crate::contract::input::KeyClassification::TextCommit(text) =
                state.ime_decoder.push(wparam as u16)
            {
                dispatch_event(
                    state,
                    PixelWindowEvent::Ime(crate::contract::ime::ImeEvent::Commit(text)),
                );
            }
            0
        }
        WAKE_MESSAGE => {
            dispatch_event(state, PixelWindowEvent::Wake);
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            if let Some(event) = key_event(wparam, lparam, message) {
                dispatch_event(state, PixelWindowEvent::Keyboard(event));
                0
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_CHAR => {
            if state.ime_composing {
                return 0;
            }
            if let crate::contract::input::KeyClassification::TextCommit(text) =
                state.decoder.push(wparam as u16)
                && text.chars().any(|character| !character.is_control())
            {
                let event = NormalizedKeyEvent {
                    logical: LogicalKey::Character(text.clone()),
                    physical: PhysicalKeyCode::Other,
                    text: Some(text),
                    state: KeyPressState::Pressed,
                    repeat: false,
                    modifiers: modifiers(),
                };
                dispatch_event(state, PixelWindowEvent::Keyboard(event));
            }
            0
        }
        WM_MOUSEMOVE => {
            if !state.tracking_mouse {
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                state.tracking_mouse = unsafe { TrackMouseEvent(&mut tracking) } != 0;
            }
            let scale_factor = state.backend.metrics.borrow().scale_factor;
            dispatch_event(
                state,
                PixelWindowEvent::PointerMoved {
                    position: point(lparam, scale_factor),
                    modifiers: modifiers(),
                },
            );
            0
        }
        MOUSE_LEAVE_MESSAGE => {
            state.tracking_mouse = false;
            dispatch_event(state, PixelWindowEvent::PointerLeft);
            0
        }
        WM_CAPTURECHANGED => {
            if state.backend.capture_active.replace(false) && lparam as HWND != hwnd {
                dispatch_event(state, PixelWindowEvent::PointerCaptureLost);
            }
            0
        }
        WM_CANCELMODE => {
            if state.backend.capture_active.replace(false) {
                unsafe { ReleaseCapture() };
                dispatch_event(state, PixelWindowEvent::PointerCaptureLost);
            }
            0
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
        | WM_MBUTTONUP => {
            let button = match message {
                WM_LBUTTONDOWN | WM_LBUTTONUP => PointerButton::Left,
                WM_RBUTTONDOWN | WM_RBUTTONUP => PointerButton::Right,
                _ => PointerButton::Middle,
            };
            let pressed = matches!(message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN);
            let scale_factor = state.backend.metrics.borrow().scale_factor;
            dispatch_event(
                state,
                PixelWindowEvent::PointerButton {
                    button,
                    state: if pressed {
                        PointerButtonState::Pressed
                    } else {
                        PointerButtonState::Released
                    },
                    position: Some(point(lparam, scale_factor)),
                    modifiers: modifiers(),
                },
            );
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) as u16 as i16) as f32 / 120.0;
            dispatch_event(
                state,
                PixelWindowEvent::MouseWheel {
                    delta: WheelDelta::Lines { x: 0.0, y: delta },
                    position: None,
                    modifiers: modifiers(),
                },
            );
            0
        }
        WM_TIMER if wparam == WAIT_TIMER_ID => {
            unsafe { KillTimer(hwnd, WAIT_TIMER_ID) };
            dispatch_event(state, PixelWindowEvent::Wake);
            0
        }
        CAPTURE_MESSAGE => {
            if unsafe { GetCapture() } == hwnd {
                unsafe { ReleaseCapture() };
            }
            0
        }
        IME_ALLOWED_MESSAGE => {
            apply_ime_allowed(hwnd, wparam != 0);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn paint(state: &mut HostState, hwnd: HWND) -> Result<(), PixelWindowError> {
    let mut paint: PAINTSTRUCT = unsafe { mem::zeroed() };
    let dc = unsafe { BeginPaint(hwnd, &mut paint) };
    if dc.is_null() {
        return Err(last_error("pixel_window_begin_paint_failed"));
    }

    let result = (|| {
        let metrics = *state.backend.metrics.borrow();
        if !metrics.is_drawable() {
            return Ok(());
        }
        let count = u64::from(metrics.physical_width)
            .checked_mul(u64::from(metrics.physical_height))
            .and_then(|count| usize::try_from(count).ok())
            .ok_or_else(|| {
                PixelWindowError::failed(
                    "pixel_window_frame_size_overflow",
                    "physical frame dimensions do not fit in the host buffer",
                )
            })?;
        state.pixels.resize(count, 0);
        let mut frame = XrgbPixelFrame::new(
            &mut state.pixels,
            metrics.physical_width,
            metrics.physical_height,
            metrics.scale_factor,
        );
        let render_result = state.application.render(&state.window, &mut frame);
        apply_directive(state, render_result);

        let paint_rect = Win32Rect {
            left: paint.rcPaint.left,
            top: paint.rcPaint.top,
            right: paint.rcPaint.right,
            bottom: paint.rcPaint.bottom,
        };
        let Some(geometry) =
            stretch_geometry_for_paint(paint_rect, metrics.physical_width, metrics.physical_height)
        else {
            return Ok(());
        };
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: metrics.physical_width as i32,
                // Negative height means top-down DIB: source Y equals client Y.
                biHeight: -(metrics.physical_height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..unsafe { mem::zeroed() }
            },
            ..unsafe { mem::zeroed() }
        };
        let source = geometry.source;
        let destination = geometry.destination;
        let copied = unsafe {
            StretchDIBits(
                dc,
                destination.left,
                destination.top,
                destination.right - destination.left,
                destination.bottom - destination.top,
                source.left,
                source.top,
                source.right - source.left,
                source.bottom - source.top,
                state.pixels.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            )
        };
        if copied <= 0 {
            return Err(last_error("pixel_window_surface_present_failed"));
        }
        Ok(())
    })();
    let ended = unsafe { EndPaint(hwnd, &paint) };
    match result {
        Err(error) => Err(error),
        Ok(()) if ended == 0 => Err(last_error("pixel_window_end_paint_failed")),
        Ok(()) => Ok(()),
    }
}

fn update_metrics(state: &mut HostState, change: GeometryChange, notify: bool) {
    let hwnd = state.backend.hwnd.get();
    if hwnd.is_null() {
        return;
    }
    let mut rect: RECT = unsafe { mem::zeroed() };
    if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
        state.deferred_error = Some(last_error("pixel_window_client_rect_failed"));
        state.exit = true;
        return;
    }
    let scale = f64::from(unsafe { GetDpiForWindow(hwnd) }.max(96)) / 96.0;
    let width = (rect.right - rect.left).max(0) as u32;
    let height = (rect.bottom - rect.top).max(0) as u32;
    let metrics = PixelWindowMetrics {
        logical_size: LogicalSize::new(width as f64 / scale, height as f64 / scale),
        physical_width: width,
        physical_height: height,
        scale_factor: scale,
    };
    *state.backend.metrics.borrow_mut() = metrics;
    if notify && state.opened && metrics.is_drawable() {
        dispatch_event(state, PixelWindowEvent::GeometryChanged { change, metrics });
    }
}

fn apply_dpi_suggested_rect(hwnd: HWND, lparam: LPARAM) {
    let suggested = lparam as *const RECT;
    if suggested.is_null() {
        return;
    }
    let rect = unsafe { &*suggested };
    if let Some((x, y, width, height)) = suggested_rect_geometry(rect) {
        unsafe {
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        };
    }
}

fn suggested_rect_geometry(rect: &RECT) -> Option<(i32, i32, i32, i32)> {
    let width = rect.right.checked_sub(rect.left)?;
    let height = rect.bottom.checked_sub(rect.top)?;
    (width > 0 && height > 0).then_some((rect.left, rect.top, width, height))
}

fn apply_ime_allowed(hwnd: HWND, allowed: bool) {
    let flags = if allowed { IACE_DEFAULT } else { 0 };
    unsafe { ImmAssociateContextEx(hwnd, ptr::null_mut(), flags) };
}

fn dispatch_event(state: &mut HostState, event: PixelWindowEvent) {
    if !state.opened || state.exit {
        return;
    }
    let result = state.application.event(&state.window, event);
    apply_directive(state, result);
}

fn apply_directive(state: &mut HostState, result: Result<PixelWindowDirective, PixelWindowError>) {
    match result {
        Ok(PixelWindowDirective::Exit) => state.exit = true,
        Ok(PixelWindowDirective::WaitUntil(deadline)) => {
            let hwnd = state.backend.hwnd.get();
            let now = Instant::now();
            if deadline <= now {
                unsafe { PostMessageW(hwnd, WAKE_MESSAGE, 0, 0) };
            } else {
                let millis = deadline
                    .duration_since(now)
                    .as_millis()
                    .clamp(1, u32::MAX as u128) as u32;
                if unsafe { SetTimer(hwnd, WAIT_TIMER_ID, millis, None) } == 0 {
                    state.deferred_error = Some(PixelWindowError::failed(
                        "pixel_window_timer_failed",
                        "native pixel window timer failed",
                    ));
                    state.exit = true;
                }
            }
        }
        Ok(PixelWindowDirective::Wait) => {
            unsafe { KillTimer(state.backend.hwnd.get(), WAIT_TIMER_ID) };
        }
        Ok(_) => {}
        Err(error) => {
            state.deferred_error = Some(error);
            state.exit = true;
        }
    }
}

fn key_event(wparam: WPARAM, lparam: LPARAM, message: u32) -> Option<NormalizedKeyEvent> {
    let code = wparam as u16;
    let modifiers = modifiers();
    let printable = code == VK_SPACE
        || (b'A' as u16..=b'Z' as u16).contains(&code)
        || (b'0' as u16..=b'9' as u16).contains(&code);
    if printable && !modifiers.control && !modifiers.alt {
        return None;
    }
    let named = named_key(code);
    let logical = if let Some(named) = named {
        LogicalKey::Named(named)
    } else if (b'A' as u16..=b'Z' as u16).contains(&code) {
        LogicalKey::Character(char::from_u32(u32::from(code) + 32)?.to_string())
    } else if (b'0' as u16..=b'9' as u16).contains(&code) {
        LogicalKey::Character(char::from_u32(u32::from(code))?.to_string())
    } else {
        LogicalKey::Unidentified
    };
    let physical = match code {
        value if (b'A' as u16..=b'Z' as u16).contains(&value) => {
            PhysicalKeyCode::Letter(char::from_u32(u32::from(value) + 32)?)
        }
        value if (b'0' as u16..=b'9' as u16).contains(&value) => {
            PhysicalKeyCode::Digit((value - b'0' as u16) as u8)
        }
        value if value == VK_BACK => PhysicalKeyCode::Backspace,
        value if value == VK_RETURN => PhysicalKeyCode::Enter,
        value if value == VK_SPACE => PhysicalKeyCode::Space,
        value if value == VK_TAB => PhysicalKeyCode::Tab,
        _ => PhysicalKeyCode::Other,
    };
    Some(NormalizedKeyEvent {
        logical,
        physical,
        text: None,
        state: if matches!(message, WM_KEYUP | WM_SYSKEYUP) {
            KeyPressState::Released
        } else {
            KeyPressState::Pressed
        },
        repeat: lparam & (1_isize << 30) != 0,
        modifiers,
    })
}

fn named_key(code: u16) -> Option<NamedKey> {
    Some(match code {
        value if value == VK_BACK => NamedKey::Backspace,
        value if value == VK_DELETE => NamedKey::Delete,
        value if value == VK_DOWN => NamedKey::ArrowDown,
        value if value == VK_END => NamedKey::End,
        value if value == VK_ESCAPE => NamedKey::Escape,
        value if value == VK_HOME => NamedKey::Home,
        value if value == VK_INSERT => NamedKey::Insert,
        value if value == VK_NEXT => NamedKey::PageDown,
        value if value == VK_PRIOR => NamedKey::PageUp,
        value if value == VK_RETURN => NamedKey::Enter,
        value if value == VK_RIGHT => NamedKey::ArrowRight,
        value if value == VK_SPACE => NamedKey::Space,
        value if value == VK_TAB => NamedKey::Tab,
        value if value == VK_UP => NamedKey::ArrowUp,
        value if value == VK_F1 => NamedKey::F1,
        value if value == VK_F2 => NamedKey::F2,
        value if value == VK_F3 => NamedKey::F3,
        value if value == VK_F4 => NamedKey::F4,
        value if value == VK_F5 => NamedKey::F5,
        value if value == VK_F6 => NamedKey::F6,
        value if value == VK_F7 => NamedKey::F7,
        value if value == VK_F8 => NamedKey::F8,
        value if value == VK_F9 => NamedKey::F9,
        value if value == VK_F10 => NamedKey::F10,
        value if value == VK_F11 => NamedKey::F11,
        value if value == VK_F12 => NamedKey::F12,
        _ => return None,
    })
}

fn modifiers() -> ModifierState {
    let down = |key: u16| unsafe { GetKeyState(i32::from(key)) } < 0;
    ModifierState {
        control: down(VK_CONTROL),
        shift: down(VK_SHIFT),
        alt: down(VK_MENU),
        meta: down(VK_LWIN),
    }
}

fn point(lparam: LPARAM, scale: f64) -> LogicalPoint {
    let packed = lparam as u32;
    let x = i32::from((packed & 0xffff) as u16 as i16);
    let y = i32::from((packed >> 16) as u16 as i16);
    LogicalPoint::new(f64::from(x) / scale, f64::from(y) / scale)
}

fn closed_error() -> PixelWindowError {
    PixelWindowError::failed("pixel_window_event_loop_closed", "native window is closed")
}

fn last_error(code: &'static str) -> PixelWindowError {
    PixelWindowError::failed(code, std::io::Error::last_os_error())
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_suggested_rect_requires_positive_checked_extents() {
        assert_eq!(
            suggested_rect_geometry(&RECT {
                left: 10,
                top: 20,
                right: 810,
                bottom: 620,
            }),
            Some((10, 20, 800, 600))
        );
        assert_eq!(
            suggested_rect_geometry(&RECT {
                left: 10,
                top: 20,
                right: 10,
                bottom: 620,
            }),
            None
        );
        assert_eq!(
            suggested_rect_geometry(&RECT {
                left: i32::MIN,
                top: 0,
                right: i32::MAX,
                bottom: 1,
            }),
            None
        );
    }

    #[test]
    fn damage_rect_is_clipped_and_invalid_dimensions_fall_back_full() {
        assert_eq!(
            win32_invalidation_for_damage(PixelRect::new(10, 20, 120, 140), 80, 90),
            RedrawInvalidation::Rect(Win32Rect {
                left: 10,
                top: 20,
                right: 80,
                bottom: 90,
            })
        );
        assert_eq!(
            win32_invalidation_for_damage(PixelRect::new(2, 3, 2, 10), 80, 90),
            RedrawInvalidation::Empty
        );
        assert_eq!(
            win32_invalidation_for_damage(PixelRect::new(0, 0, 10, 10), 0, 90),
            RedrawInvalidation::Full
        );
        assert_eq!(
            win32_invalidation_for_damage(
                PixelRect::new(WIN32_MAX_COORD, 0, WIN32_MAX_COORD + 1, 10),
                WIN32_MAX_COORD,
                90,
            ),
            RedrawInvalidation::Full
        );
    }

    #[test]
    fn paint_rect_uses_same_y_for_top_down_source_and_destination() {
        let geometry = stretch_geometry_for_paint(
            Win32Rect {
                left: -10,
                top: 20,
                right: 50,
                bottom: 80,
            },
            100,
            100,
        )
        .expect("non-empty paint region");
        assert_eq!(
            geometry,
            StretchDibGeometry {
                source: Win32Rect {
                    left: 0,
                    top: 20,
                    right: 50,
                    bottom: 80,
                },
                destination: Win32Rect {
                    left: 0,
                    top: 20,
                    right: 50,
                    bottom: 80,
                },
            }
        );
        assert!(
            stretch_geometry_for_paint(
                Win32Rect {
                    left: 90,
                    top: 90,
                    right: 90,
                    bottom: 100,
                },
                100,
                100,
            )
            .is_none()
        );
    }
}
