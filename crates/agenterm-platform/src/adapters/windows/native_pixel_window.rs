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

use crate::selected::reentrant_dispatch::{BoundedQueue, QueueError};

use windows_sys::Win32::Graphics::Gdi::ValidateRect;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, DIB_RGB_COLORS, EndPaint, HDC,
        InvalidateRect, PAINTSTRUCT, SRCCOPY, StretchDIBits,
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
            CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
            DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW,
            GetWindowRect, IDC_ARROW, IDC_SIZEWE, IsIconic, IsWindowVisible, IsZoomed, KillTimer,
            LoadCursorW, MSG, PM_REMOVE, PeekMessageW, PostMessageW, RegisterClassW, SW_HIDE,
            SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
            SWP_NOZORDER, SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
            SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WM_APP, WM_CANCELMODE,
            WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
            WM_IME_CHAR, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_SETCONTEXT,
            WM_IME_STARTCOMPOSITION, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN,
            WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE,
            WM_NCDESTROY, WM_PAINT, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETFOCUS, WM_SIZE,
            WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    },
};

#[cfg(test)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WM_NCCALCSIZE, WM_SETTEXT, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING,
};

use crate::contract::{
    input::{
        KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent, PhysicalKeyCode,
        Utf16TextDecoder,
    },
    pixel_present::{
        PixelPresentLedger, PixelPresentOutcome, PixelPresentReceipt, PixelPresentRegion,
        PixelPresentStats, elapsed_ns_since,
    },
    window_host::{
        GeometryChange, LogicalPoint, LogicalRect, LogicalSize, PixelBackingRetention,
        PixelFrameState, PixelPointerCursor, PixelRect, PixelWindow, PixelWindowApplication,
        PixelWindowBackend, PixelWindowDirective, PixelWindowError, PixelWindowEvent,
        PixelWindowMetrics, PixelWindowOptions, PointerButton, PointerButtonState, WheelDelta,
        WindowSemanticFlags, WindowWaker, XrgbPixelFrame,
    },
};

const WAKE_MESSAGE: u32 = WM_APP + 0x41;
const CAPTURE_MESSAGE: u32 = WM_APP + 0x42;
const IME_ALLOWED_MESSAGE: u32 = WM_APP + 0x43;
const MOUSE_LEAVE_MESSAGE: u32 = 0x02a3;
const WAIT_TIMER_ID: usize = 0x41;
const CLASS_NAME: &str = "AgenTermNativePixelWindow";
const WIN32_MAX_COORD: u32 = i32::MAX as u32;
const ERROR_CLASS_ALREADY_EXISTS_CODE: i32 = 1410;
const MAX_NATIVE_DEFERRED: usize = 256;
const MAX_NATIVE_DRAIN: usize = 512;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DispatchPhase {
    Idle,
    NativeMessage,
    ApplicationOpened,
    ApplicationEvent,
    Rendering,
    AboutToWait,
    Draining,
    Closing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeMessageClass {
    Default,
    Paint,
    Stateful,
}

enum NativeCommand {
    SetTitle(String),
    Show(i32),
    Focus,
    SetWindowSize { width: i32, height: i32 },
    ApplyDpiRect(Win32Rect),
    SetImeCursor { x: i32, y: i32 },
    SetImeAllowed(bool),
    ReleaseCapture,
}

enum PendingNativeEvent {
    CloseRequested,
    Destroy,
    Size {
        minimized: bool,
    },
    DpiChanged {
        suggested: Option<Win32Rect>,
    },
    FocusChanged(bool),
    ImeComposition {
        message: u32,
        active: bool,
        text: String,
        cursor: Option<(usize, usize)>,
    },
    ImeChar(u16),
    Keyboard(NormalizedKeyEvent),
    Char {
        unit: u16,
        modifiers: ModifierState,
    },
    PointerMoved {
        position: LogicalPoint,
        modifiers: ModifierState,
    },
    PointerLeft,
    CaptureChanged {
        new_owner: HWND,
    },
    CancelMode,
    PointerButton {
        button: PointerButton,
        state: PointerButtonState,
        position: LogicalPoint,
        modifiers: ModifierState,
    },
    MouseWheel {
        delta: WheelDelta,
        modifiers: ModifierState,
    },
    Wake,
    WaitTimer,
    CaptureRelease,
    ImeAllowed(bool),
}

enum DeferredNative {
    Command(NativeCommand),
    Event(PendingNativeEvent),
}

struct NativeControl {
    phase: Cell<DispatchPhase>,
    deferred: BoundedQueue<DeferredNative, MAX_NATIVE_DEFERRED>,
    paint_pending: Cell<bool>,
    closed: Cell<bool>,
    exit_requested: Cell<bool>,
    last_dpi_rect: Cell<Option<Win32Rect>>,
    failure_latched: Cell<bool>,
    failure: RefCell<Option<PixelWindowError>>,
}

impl NativeControl {
    fn new() -> Self {
        Self {
            phase: Cell::new(DispatchPhase::Idle),
            deferred: BoundedQueue::new(),
            paint_pending: Cell::new(false),
            closed: Cell::new(false),
            exit_requested: Cell::new(false),
            last_dpi_rect: Cell::new(None),
            failure_latched: Cell::new(false),
            failure: RefCell::new(None),
        }
    }

    fn has_deferred(&self) -> bool {
        self.deferred.is_empty().map(|empty| !empty).unwrap_or(true)
    }

    fn record_failure(&self, error: PixelWindowError) {
        self.failure_latched.set(true);
        self.exit_requested.set(true);
        if let Ok(mut failure) = self.failure.try_borrow_mut()
            && failure.is_none()
        {
            *failure = Some(error);
        }
    }

    fn take_failure(&self) -> Option<PixelWindowError> {
        let latched = self.failure_latched.replace(false);
        if let Ok(mut failure) = self.failure.try_borrow_mut() {
            return failure.take().or_else(|| {
                latched.then(|| {
                    PixelWindowError::failed(
                        "pixel_window_native_control_failure",
                        "native window control state became unavailable",
                    )
                })
            });
        }
        latched.then(|| {
            PixelWindowError::failed(
                "pixel_window_native_control_failure",
                "native window control state became unavailable",
            )
        })
    }

    fn enqueue(&self, item: DeferredNative) -> Result<(), PixelWindowError> {
        if self.closed.get() {
            return Err(closed_error());
        }
        self.deferred.push(item).map_err(|cause| {
            let code = match cause {
                QueueError::Borrowed => "pixel_window_native_queue_borrow_failed",
                QueueError::Full => "pixel_window_native_queue_overflow",
            };
            let error = native_queue_failure(code);
            self.record_failure(native_queue_failure(code));
            error
        })
    }

    fn enqueue_dpi_rect(&self, rect: Win32Rect) -> Result<(), PixelWindowError> {
        if self.last_dpi_rect.get() == Some(rect) {
            return Ok(());
        }
        self.last_dpi_rect.set(Some(rect));
        self.enqueue(DeferredNative::Command(NativeCommand::ApplyDpiRect(rect)))
    }

    fn pop(&self) -> Option<DeferredNative> {
        match self.deferred.pop() {
            Ok(item) => item,
            Err(QueueError::Borrowed) => {
                self.record_failure(native_queue_failure(
                    "pixel_window_native_queue_borrow_failed",
                ));
                None
            }
            Err(QueueError::Full) => unreachable!("pop cannot report a full queue"),
        }
    }

    fn push_front(&self, item: DeferredNative) -> Result<(), PixelWindowError> {
        self.deferred.push_front(item).map_err(|cause| {
            native_queue_failure(match cause {
                QueueError::Borrowed => "pixel_window_native_queue_borrow_failed",
                QueueError::Full => "pixel_window_native_queue_overflow",
            })
        })
    }

    fn clear_deferred(&self) {
        let _ = self.deferred.clear();
    }
}

struct NativeWindowUserData {
    host: RefCell<HostState>,
    control: Rc<NativeControl>,
    backend: Rc<Backend>,
}

struct NativeMessageSnapshot {
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    event: Option<PendingNativeEvent>,
    call_default: bool,
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

struct PaintSession {
    hwnd: HWND,
    paint: PAINTSTRUCT,
    dc: HDC,
    ended: bool,
}

impl PaintSession {
    fn begin(hwnd: HWND) -> Result<Self, PixelWindowError> {
        let mut paint: PAINTSTRUCT = unsafe { mem::zeroed() };
        let dc = unsafe { BeginPaint(hwnd, &mut paint) };
        if dc.is_null() {
            return Err(last_error("pixel_window_begin_paint_failed"));
        }
        Ok(Self {
            hwnd,
            paint,
            dc,
            ended: false,
        })
    }

    fn dc(&self) -> HDC {
        self.dc
    }

    fn paint_rect(&self) -> Win32Rect {
        Win32Rect {
            left: self.paint.rcPaint.left,
            top: self.paint.rcPaint.top,
            right: self.paint.rcPaint.right,
            bottom: self.paint.rcPaint.bottom,
        }
    }

    fn finish(mut self) -> Result<(), PixelWindowError> {
        let ended = unsafe { EndPaint(self.hwnd, &self.paint) };
        self.ended = true;
        if ended == 0 {
            Err(last_error("pixel_window_end_paint_failed"))
        } else {
            Ok(())
        }
    }
}

impl Drop for PaintSession {
    fn drop(&mut self) {
        if !self.ended {
            unsafe { EndPaint(self.hwnd, &self.paint) };
            self.ended = true;
        }
    }
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
    present: RefCell<PixelPresentLedger>,
    alive: Arc<AtomicBool>,
    control: Rc<NativeControl>,
    capture_active: Cell<bool>,
    ime_allowed: Cell<bool>,
}

impl Backend {
    fn submit_command(&self, command: NativeCommand) -> Result<(), PixelWindowError> {
        if self.control.closed.get() {
            return Err(closed_error());
        }
        if self.control.phase.get() == DispatchPhase::Idle && !self.control.has_deferred() {
            return apply_native_command(self, command);
        }
        self.control.enqueue(DeferredNative::Command(command))
    }

    fn submit_void_command(&self, command: NativeCommand) {
        if let Err(error) = self.submit_command(command)
            && !self.control.closed.get()
        {
            self.control.record_failure(error);
        }
    }
}

impl PixelWindowBackend for Backend {
    fn request_redraw(&self) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_null() {
            unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
        }
    }

    fn present_stats(&self) -> PixelPresentStats {
        self.present.borrow().snapshot()
    }

    fn last_present(&self) -> Option<PixelPresentReceipt> {
        self.present.borrow().last()
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
        self.submit_void_command(NativeCommand::Show(if minimized {
            SW_MINIMIZE
        } else {
            SW_RESTORE
        }));
    }

    fn set_maximized(&self, maximized: bool) {
        self.submit_void_command(NativeCommand::Show(if maximized {
            SW_MAXIMIZE
        } else {
            SW_RESTORE
        }));
    }

    fn set_visible(&self, visible: bool) {
        self.submit_void_command(NativeCommand::Show(if visible { SW_SHOW } else { SW_HIDE }));
    }

    fn focus(&self) {
        self.submit_void_command(NativeCommand::Focus);
    }

    fn set_title(&self, title: &str) {
        self.submit_void_command(NativeCommand::SetTitle(title.to_owned()));
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
        let width = crate::numeric::round_f64(size.width * metrics.scale_factor) as i32 + chrome_w;
        let height =
            crate::numeric::round_f64(size.height * metrics.scale_factor) as i32 + chrome_h;
        self.submit_command(NativeCommand::SetWindowSize {
            width: width.max(1),
            height: height.max(1),
        })
    }

    fn set_pointer_capture(&self, captured: bool) -> Result<(), PixelWindowError> {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return Err(closed_error());
        }
        if captured {
            // SetCapture returns the previous owner, not a success flag. The
            // ownership check is the authoritative result. If a previous
            // owner synchronously reenters this HWND, window_proc takes the
            // RefCell/queue path; it cannot create a second HostState borrow.
            let _previous_owner = unsafe { SetCapture(hwnd) };
            if unsafe { GetCapture() } != hwnd {
                self.capture_active.set(false);
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
        if !hwnd.is_null()
            && unsafe { PostMessageW(hwnd, IME_ALLOWED_MESSAGE, usize::from(allowed), 0) } == 0
        {
            self.control
                .record_failure(last_error("pixel_window_ime_allowed_post_failed"));
        }
    }

    fn set_ime_cursor_area(&self, area: LogicalRect) -> Result<(), PixelWindowError> {
        let scale = self.metrics.borrow().scale_factor;
        self.submit_command(NativeCommand::SetImeCursor {
            x: crate::numeric::round_f64(area.origin.x * scale) as i32,
            y: crate::numeric::round_f64(area.origin.y * scale) as i32,
        })
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
    frame_state: PixelFrameState,
    frame_width: u32,
    frame_height: u32,
    frame_scale_bits: u64,
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
        // Pixel backing and explicit invalidation own redraw. CS_HREDRAW /
        // CS_VREDRAW would make User32 invalidate the complete client on every
        // live-resize step, defeating retained pixels and partial present.
        style: 0,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        lpszClassName: class_name.as_ptr(),
        ..unsafe { mem::zeroed() }
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_CLASS_ALREADY_EXISTS_CODE) {
            return Err(PixelWindowError::failed(
                "pixel_window_class_register_failed",
                error,
            ));
        }
    }

    let scale = 1.0;
    let initial_metrics = PixelWindowMetrics {
        logical_size: options.initial_logical_size,
        physical_width: crate::numeric::round_f64(options.initial_logical_size.width).max(1.0)
            as u32,
        physical_height: crate::numeric::round_f64(options.initial_logical_size.height).max(1.0)
            as u32,
        scale_factor: scale,
    };
    let alive = Arc::new(AtomicBool::new(true));
    let control = Rc::new(NativeControl::new());
    let backend = Rc::new(Backend {
        hwnd: Cell::new(ptr::null_mut()),
        wake_hwnd: Arc::new(AtomicIsize::new(0)),
        metrics: RefCell::new(initial_metrics),
        present: RefCell::new(PixelPresentLedger::new()),
        alive: Arc::clone(&alive),
        control: control.clone(),
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
    let state = HostState {
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
        frame_state: PixelFrameState::new(PixelBackingRetention::RetainedAcrossFrames),
        frame_width: 0,
        frame_height: 0,
        frame_scale_bits: 0,
    };
    let user_data = Box::new(NativeWindowUserData {
        host: RefCell::new(state),
        control: control.clone(),
        backend: backend.clone(),
    });
    let user_data_ptr = Box::into_raw(user_data);
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
            user_data_ptr.cast::<c_void>(),
        )
    };
    if hwnd.is_null() {
        unsafe { drop(Box::from_raw(user_data_ptr)) };
        return Err(last_error("pixel_window_create_failed"));
    }
    let user_data = unsafe { Box::from_raw(user_data_ptr) };
    backend.hwnd.set(hwnd);
    backend.wake_hwnd.store(hwnd as isize, Ordering::Release);
    apply_ime_allowed(hwnd, options.ime_allowed);
    {
        let Ok(mut state) = user_data.host.try_borrow_mut() else {
            control.record_failure(native_queue_failure("pixel_window_host_borrow_failed"));
            return Err(native_queue_failure("pixel_window_host_borrow_failed"));
        };
        update_metrics(&mut state, GeometryChange::Resized, false);
        let previous = control.phase.replace(DispatchPhase::ApplicationOpened);
        let state = &mut *state;
        let HostState {
            application,
            window,
            ..
        } = state;
        let opened = catch_application("opened", || application.opened(window));
        apply_directive(state, opened);
        state.opened = true;
        control.phase.set(previous);
    }
    if let Err(error) = control.enqueue(DeferredNative::Command(NativeCommand::Show(
        if options.no_activate {
            SW_SHOWNOACTIVATE
        } else {
            SW_SHOW
        },
    ))) {
        control.record_failure(error);
    }
    backend.request_redraw();
    drain_native(&user_data, hwnd);

    while !host_should_exit(&user_data) {
        let mut message: MSG = unsafe { mem::zeroed() };
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if result == -1 {
            control.record_failure(last_error("pixel_window_message_loop_failed"));
            break;
        }
        if result == 0 || message.message == WM_QUIT {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        drain_native(&user_data, hwnd);
        while !host_should_exit(&user_data)
            && unsafe { PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) } != 0
        {
            if message.message == WM_QUIT {
                control.exit_requested.set(true);
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            drain_native(&user_data, hwnd);
        }
        if host_should_exit(&user_data) {
            break;
        }
        {
            let Ok(mut state) = user_data.host.try_borrow_mut() else {
                control.record_failure(native_queue_failure("pixel_window_host_borrow_failed"));
                break;
            };
            let previous = control.phase.replace(DispatchPhase::AboutToWait);
            let state = &mut *state;
            let HostState {
                application,
                window,
                ..
            } = state;
            let directive = catch_application("about_to_wait", || {
                application.about_to_wait(window, Instant::now())
            });
            apply_directive(state, directive);
            control.phase.set(previous);
        }
        drain_native(&user_data, hwnd);
    }

    control.phase.set(DispatchPhase::Closing);
    alive.store(false, Ordering::Release);
    backend.wake_hwnd.store(0, Ordering::Release);
    let remaining_hwnd = backend.hwnd.replace(ptr::null_mut());
    if !remaining_hwnd.is_null() {
        unsafe { DestroyWindow(remaining_hwnd) };
    }
    surface_control_failure(&user_data);
    user_data
        .host
        .try_borrow_mut()
        .ok()
        .and_then(|mut state| state.deferred_error.take())
        .map_or(Ok(()), Err)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        window_proc_inner(hwnd, message, wparam, lparam)
    })) {
        Ok(result) => result,
        Err(_) => {
            let user_data =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut NativeWindowUserData;
            if !user_data.is_null() {
                let user_data = unsafe { &*user_data };
                let error = match user_data.control.phase.get() {
                    DispatchPhase::Rendering
                    | DispatchPhase::ApplicationOpened
                    | DispatchPhase::ApplicationEvent
                    | DispatchPhase::AboutToWait => PixelWindowError::failed(
                        "pixel_window_application_panic",
                        "application callback panicked",
                    ),
                    _ => PixelWindowError::failed(
                        "pixel_window_host_panic",
                        "native pixel-window callback panicked",
                    ),
                };
                user_data.control.record_failure(error);
            }
            0
        }
    }
}

unsafe fn window_proc_inner(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam as *const CREATESTRUCTW;
        if !create.is_null() {
            let user_data = unsafe { (*create).lpCreateParams } as *mut NativeWindowUserData;
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, user_data as isize) };
        }
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }

    if message == WM_ERASEBKGND {
        return 1;
    }
    if message == WM_IME_SETCONTEXT {
        const IS_SHOWUICOMPOSITIONWINDOW: isize = 0x0002;
        return unsafe {
            DefWindowProcW(hwnd, message, wparam, lparam & !IS_SHOWUICOMPOSITIONWINDOW)
        };
    }

    let class = classify_native_message(message);
    if class == NativeMessageClass::Default {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }

    let user_data = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut NativeWindowUserData;
    if user_data.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let user_data = unsafe { &*user_data };
    if message == WM_NCDESTROY {
        return unsafe { handle_nc_destroy(user_data, hwnd, message, wparam, lparam) };
    }

    if class == NativeMessageClass::Paint {
        return match user_data.host.try_borrow_mut() {
            Ok(mut state) => {
                let previous = user_data
                    .control
                    .phase
                    .replace(DispatchPhase::NativeMessage);
                let result = paint(&mut state, hwnd);
                user_data.control.phase.set(previous);
                match result {
                    Ok(()) => 0,
                    Err(error) => {
                        state.frame_state.invalidate();
                        if state.deferred_error.is_none() {
                            state.deferred_error = Some(error);
                        }
                        state.exit = true;
                        user_data.control.exit_requested.set(true);
                        0
                    }
                }
            }
            Err(_) => {
                consume_reentrant_paint(user_data, hwnd);
                0
            }
        };
    }

    let snapshot =
        match unsafe { snapshot_native_message(hwnd, message, wparam, lparam, &user_data.backend) }
        {
            Some(snapshot) => snapshot,
            None => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        };
    let call_default = snapshot.call_default;
    match user_data.host.try_borrow_mut() {
        Ok(mut state) => {
            let previous = user_data
                .control
                .phase
                .replace(DispatchPhase::NativeMessage);
            let result = dispatch_snapshot(&mut state, hwnd, snapshot);
            user_data.control.phase.set(previous);
            result
        }
        Err(_) => {
            if let Some(event) = snapshot.event
                && let Err(error) = user_data.control.enqueue(DeferredNative::Event(event))
            {
                user_data.control.record_failure(error);
            }
            if call_default {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            } else {
                0
            }
        }
    }
}

fn classify_native_message(message: u32) -> NativeMessageClass {
    match message {
        WM_PAINT => NativeMessageClass::Paint,
        WM_CLOSE
        | WM_DESTROY
        | WM_NCDESTROY
        | WM_SIZE
        | WM_DPICHANGED
        | WM_SETFOCUS
        | WM_KILLFOCUS
        | WM_IME_STARTCOMPOSITION
        | WM_IME_COMPOSITION
        | WM_IME_ENDCOMPOSITION
        | WM_IME_CHAR
        | WAKE_MESSAGE
        | WM_KEYDOWN
        | WM_SYSKEYDOWN
        | WM_KEYUP
        | WM_SYSKEYUP
        | WM_CHAR
        | WM_MOUSEMOVE
        | MOUSE_LEAVE_MESSAGE
        | WM_CAPTURECHANGED
        | WM_CANCELMODE
        | WM_LBUTTONDOWN
        | WM_LBUTTONUP
        | WM_RBUTTONDOWN
        | WM_RBUTTONUP
        | WM_MBUTTONDOWN
        | WM_MBUTTONUP
        | WM_MOUSEWHEEL
        | WM_TIMER
        | CAPTURE_MESSAGE
        | IME_ALLOWED_MESSAGE => NativeMessageClass::Stateful,
        // Pointer-backed and all unknown messages remain in the original
        // callback/default-processing path, before any HostState borrow.
        _ => NativeMessageClass::Default,
    }
}

fn copied_suggested_rect(lparam: LPARAM) -> Option<Win32Rect> {
    let suggested = lparam as *const RECT;
    if suggested.is_null() {
        return None;
    }
    let rect = unsafe { *suggested };
    suggested_rect_geometry(&rect).map(|(x, y, width, height)| Win32Rect {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    })
}

fn current_scale(backend: &Backend) -> f64 {
    backend
        .metrics
        .try_borrow()
        .map(|metrics| metrics.scale_factor.max(f64::EPSILON))
        .unwrap_or(1.0)
}

unsafe fn snapshot_native_message(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    backend: &Backend,
) -> Option<NativeMessageSnapshot> {
    let scale = current_scale(backend);
    let snapshot = match message {
        WM_CLOSE => PendingNativeEvent::CloseRequested,
        WM_DESTROY => PendingNativeEvent::Destroy,
        WM_SIZE => PendingNativeEvent::Size {
            minimized: wparam == 1,
        },
        WM_DPICHANGED => PendingNativeEvent::DpiChanged {
            suggested: copied_suggested_rect(lparam),
        },
        WM_SETFOCUS => PendingNativeEvent::FocusChanged(true),
        WM_KILLFOCUS => PendingNativeEvent::FocusChanged(false),
        WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION => {
            crate::selected::ime::refresh_from_message(hwnd, message);
            let composition = crate::selected::ime::composition();
            let (active, text, cursor) = composition.map_or_else(
                || (false, String::new(), None),
                |composition| {
                    let cursor = composition.cursor;
                    (true, composition.text, Some((cursor, cursor)))
                },
            );
            PendingNativeEvent::ImeComposition {
                message,
                active: message != WM_IME_ENDCOMPOSITION && active,
                text,
                cursor,
            }
        }
        WM_IME_CHAR => PendingNativeEvent::ImeChar(wparam as u16),
        WAKE_MESSAGE => PendingNativeEvent::Wake,
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            let event = key_event(wparam, lparam, message);
            let call_default = event.is_none();
            return Some(NativeMessageSnapshot {
                message,
                wparam,
                lparam,
                event: event.map(PendingNativeEvent::Keyboard),
                call_default,
            });
        }
        WM_CHAR => PendingNativeEvent::Char {
            unit: wparam as u16,
            modifiers: modifiers(),
        },
        WM_MOUSEMOVE => PendingNativeEvent::PointerMoved {
            position: point(lparam, scale),
            modifiers: modifiers(),
        },
        MOUSE_LEAVE_MESSAGE => PendingNativeEvent::PointerLeft,
        WM_CAPTURECHANGED => PendingNativeEvent::CaptureChanged {
            new_owner: lparam as HWND,
        },
        WM_CANCELMODE => PendingNativeEvent::CancelMode,
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
        | WM_MBUTTONUP => {
            let button = match message {
                WM_LBUTTONDOWN | WM_LBUTTONUP => PointerButton::Left,
                WM_RBUTTONDOWN | WM_RBUTTONUP => PointerButton::Right,
                _ => PointerButton::Middle,
            };
            let pressed = matches!(message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN);
            PendingNativeEvent::PointerButton {
                button,
                state: if pressed {
                    PointerButtonState::Pressed
                } else {
                    PointerButtonState::Released
                },
                position: point(lparam, scale),
                modifiers: modifiers(),
            }
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) as u16 as i16) as f32 / 120.0;
            PendingNativeEvent::MouseWheel {
                delta: WheelDelta::Lines { x: 0.0, y: delta },
                modifiers: modifiers(),
            }
        }
        WM_TIMER if wparam == WAIT_TIMER_ID => PendingNativeEvent::WaitTimer,
        CAPTURE_MESSAGE => PendingNativeEvent::CaptureRelease,
        IME_ALLOWED_MESSAGE => PendingNativeEvent::ImeAllowed(wparam != 0),
        _ => return None,
    };
    Some(NativeMessageSnapshot {
        message,
        wparam,
        lparam,
        event: Some(snapshot),
        call_default: matches!(
            message,
            WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION
        ),
    })
}

fn dispatch_pending_event(state: &mut HostState, hwnd: HWND, event: PendingNativeEvent) {
    match event {
        PendingNativeEvent::CloseRequested => {
            dispatch_event(state, PixelWindowEvent::CloseRequested);
        }
        PendingNativeEvent::Destroy => state.exit = true,
        PendingNativeEvent::Size { minimized } => {
            if !minimized {
                update_metrics(state, GeometryChange::Resized, true);
            }
        }
        PendingNativeEvent::DpiChanged { suggested } => {
            if let Some(rect) = suggested
                && let Err(error) = state.backend.control.enqueue_dpi_rect(rect)
            {
                state.backend.control.record_failure(error);
            }
            update_metrics(state, GeometryChange::ScaleFactorChanged, true);
        }
        PendingNativeEvent::FocusChanged(focused) => {
            if focused {
                dispatch_event(state, PixelWindowEvent::FocusChanged(true));
                if state.backend.ime_allowed.get() {
                    dispatch_event(
                        state,
                        PixelWindowEvent::Ime(crate::contract::ime::ImeEvent::Enabled),
                    );
                }
            } else {
                state.ime_composing = false;
                crate::selected::ime::refresh_from_message(hwnd, WM_IME_ENDCOMPOSITION);
                dispatch_event(
                    state,
                    PixelWindowEvent::Ime(crate::contract::ime::ImeEvent::Disabled),
                );
                dispatch_event(state, PixelWindowEvent::FocusChanged(false));
            }
        }
        PendingNativeEvent::ImeComposition {
            message,
            active,
            text,
            cursor,
        } => {
            state.ime_composing = active;
            dispatch_event(
                state,
                PixelWindowEvent::Ime(crate::contract::ime::ImeEvent::Preedit { text, cursor }),
            );
            let _ = message;
        }
        PendingNativeEvent::ImeChar(unit) => {
            if let crate::contract::input::KeyClassification::TextCommit(text) =
                state.ime_decoder.push(unit)
            {
                dispatch_event(
                    state,
                    PixelWindowEvent::Ime(crate::contract::ime::ImeEvent::Commit(text)),
                );
            }
        }
        PendingNativeEvent::Keyboard(event) => {
            dispatch_event(state, PixelWindowEvent::Keyboard(event));
        }
        PendingNativeEvent::Char { unit, modifiers } => {
            if state.ime_composing {
                return;
            }
            if let crate::contract::input::KeyClassification::TextCommit(text) =
                state.decoder.push(unit)
                && text.chars().any(|character| !character.is_control())
            {
                let event = NormalizedKeyEvent {
                    logical: LogicalKey::Character(text.clone()),
                    physical: PhysicalKeyCode::Other,
                    text: Some(text),
                    state: KeyPressState::Pressed,
                    repeat: false,
                    modifiers,
                };
                dispatch_event(state, PixelWindowEvent::Keyboard(event));
            }
        }
        PendingNativeEvent::PointerMoved {
            position,
            modifiers,
        } => {
            if !state.tracking_mouse {
                let mut tracking = TRACKMOUSEEVENT {
                    cbSize: mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                state.tracking_mouse = unsafe { TrackMouseEvent(&mut tracking) } != 0;
            }
            dispatch_event(
                state,
                PixelWindowEvent::PointerMoved {
                    position,
                    modifiers,
                },
            );
        }
        PendingNativeEvent::PointerLeft => {
            state.tracking_mouse = false;
            dispatch_event(state, PixelWindowEvent::PointerLeft);
        }
        PendingNativeEvent::CaptureChanged { new_owner } => {
            if state.backend.capture_active.replace(false) && new_owner != hwnd {
                dispatch_event(state, PixelWindowEvent::PointerCaptureLost);
            }
        }
        PendingNativeEvent::CancelMode => {
            if state.backend.capture_active.replace(false) {
                dispatch_event(state, PixelWindowEvent::PointerCaptureLost);
                if let Err(error) = state
                    .backend
                    .control
                    .enqueue(DeferredNative::Command(NativeCommand::ReleaseCapture))
                {
                    state.backend.control.record_failure(error);
                }
            }
        }
        PendingNativeEvent::PointerButton {
            button,
            state: button_state,
            position,
            modifiers,
        } => dispatch_event(
            state,
            PixelWindowEvent::PointerButton {
                button,
                state: button_state,
                position: Some(position),
                modifiers,
            },
        ),
        PendingNativeEvent::MouseWheel { delta, modifiers } => dispatch_event(
            state,
            PixelWindowEvent::MouseWheel {
                delta,
                position: None,
                modifiers,
            },
        ),
        PendingNativeEvent::Wake => dispatch_event(state, PixelWindowEvent::Wake),
        PendingNativeEvent::WaitTimer => {
            unsafe { KillTimer(hwnd, WAIT_TIMER_ID) };
            dispatch_event(state, PixelWindowEvent::Wake);
        }
        PendingNativeEvent::CaptureRelease => {
            if unsafe { GetCapture() } == hwnd
                && let Err(error) = state
                    .backend
                    .control
                    .enqueue(DeferredNative::Command(NativeCommand::ReleaseCapture))
            {
                state.backend.control.record_failure(error);
            }
        }
        PendingNativeEvent::ImeAllowed(allowed) => {
            if let Err(error) = state.backend.control.enqueue(DeferredNative::Command(
                NativeCommand::SetImeAllowed(allowed),
            )) {
                state.backend.control.record_failure(error);
            }
        }
    }
}

fn dispatch_snapshot(
    state: &mut HostState,
    hwnd: HWND,
    snapshot: NativeMessageSnapshot,
) -> LRESULT {
    if let Some(event) = snapshot.event {
        dispatch_pending_event(state, hwnd, event);
    }
    if snapshot.call_default {
        unsafe { DefWindowProcW(hwnd, snapshot.message, snapshot.wparam, snapshot.lparam) }
    } else {
        0
    }
}

unsafe fn handle_nc_destroy(
    user_data: &NativeWindowUserData,
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    user_data.control.closed.set(true);
    user_data.control.exit_requested.set(true);
    user_data.control.paint_pending.set(false);
    user_data.control.clear_deferred();
    user_data.backend.alive.store(false, Ordering::Release);
    user_data.backend.wake_hwnd.store(0, Ordering::Release);
    user_data.backend.hwnd.set(ptr::null_mut());
    if let Ok(mut state) = user_data.host.try_borrow_mut() {
        state.frame_state.invalidate();
        state.exit = true;
    }
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn consume_reentrant_paint(user_data: &NativeWindowUserData, hwnd: HWND) {
    user_data.control.paint_pending.set(true);
    if unsafe { ValidateRect(hwnd, ptr::null()) } == 0 {
        user_data
            .control
            .record_failure(last_error("pixel_window_reentrant_paint_validate_failed"));
    }
}

fn apply_native_command(backend: &Backend, command: NativeCommand) -> Result<(), PixelWindowError> {
    let hwnd = backend.hwnd.get();
    if hwnd.is_null() {
        return Err(closed_error());
    }
    match command {
        NativeCommand::SetTitle(title) => {
            let title = wide_null(&title);
            if unsafe { SetWindowTextW(hwnd, title.as_ptr()) } == 0 {
                return Err(last_error("pixel_window_title_failed"));
            }
        }
        NativeCommand::Show(command) => unsafe {
            ShowWindow(hwnd, command);
        },
        NativeCommand::Focus => unsafe {
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
        },
        NativeCommand::SetWindowSize { width, height } => {
            if unsafe {
                SetWindowPos(
                    hwnd,
                    ptr::null_mut(),
                    0,
                    0,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )
            } == 0
            {
                return Err(last_error("pixel_window_resize_failed"));
            }
        }
        NativeCommand::ApplyDpiRect(rect) => {
            let width = rect.right.checked_sub(rect.left).ok_or_else(|| {
                PixelWindowError::failed(
                    "pixel_window_dpi_rect_invalid",
                    "DPI suggested rectangle width overflowed",
                )
            })?;
            let height = rect.bottom.checked_sub(rect.top).ok_or_else(|| {
                PixelWindowError::failed(
                    "pixel_window_dpi_rect_invalid",
                    "DPI suggested rectangle height overflowed",
                )
            })?;
            if width <= 0 || height <= 0 {
                return Ok(());
            }
            if unsafe {
                SetWindowPos(
                    hwnd,
                    ptr::null_mut(),
                    rect.left,
                    rect.top,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )
            } == 0
            {
                return Err(last_error("pixel_window_dpi_resize_failed"));
            }
        }
        NativeCommand::SetImeCursor { x, y } => {
            crate::selected::ime::set_anchor_position(x, y);
        }
        NativeCommand::SetImeAllowed(allowed) => apply_ime_allowed(hwnd, allowed),
        NativeCommand::ReleaseCapture => {
            if unsafe { GetCapture() } == hwnd && unsafe { ReleaseCapture() } == 0 {
                return Err(last_error("pixel_window_pointer_release_failed"));
            }
        }
    }
    Ok(())
}

fn apply_deferred_item(
    user_data: &NativeWindowUserData,
    hwnd: HWND,
    item: DeferredNative,
) -> Result<(), PixelWindowError> {
    match item {
        DeferredNative::Command(command) => apply_native_command(&user_data.backend, command),
        DeferredNative::Event(event) => {
            let Ok(mut state) = user_data.host.try_borrow_mut() else {
                user_data.control.push_front(DeferredNative::Event(event))?;
                return Err(native_queue_failure("pixel_window_drain_borrow_failed"));
            };
            let previous = user_data
                .control
                .phase
                .replace(DispatchPhase::NativeMessage);
            dispatch_pending_event(&mut state, hwnd, event);
            user_data.control.phase.set(previous);
            Ok(())
        }
    }
}

fn drain_native(user_data: &NativeWindowUserData, hwnd: HWND) {
    if user_data.control.closed.get() {
        surface_control_failure(user_data);
        return;
    }
    let previous = user_data.control.phase.replace(DispatchPhase::Draining);
    let mut steps = 0;
    while steps < MAX_NATIVE_DRAIN
        && !user_data.control.exit_requested.get()
        && !host_state_exit(user_data)
    {
        let Some(item) = user_data.control.pop() else {
            break;
        };
        steps += 1;
        let (panic_code, panic_message) = match &item {
            DeferredNative::Command(_) => (
                "pixel_window_native_command_panic",
                "deferred native command panicked",
            ),
            DeferredNative::Event(_) => (
                "pixel_window_deferred_event_panic",
                "deferred native event panicked",
            ),
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            apply_deferred_item(user_data, hwnd, item)
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => user_data.control.record_failure(error),
            Err(_) => user_data
                .control
                .record_failure(PixelWindowError::failed(panic_code, panic_message)),
        }
    }
    if steps == MAX_NATIVE_DRAIN && user_data.control.has_deferred() {
        user_data.control.record_failure(native_queue_failure(
            "pixel_window_native_drain_nonconvergent",
        ));
    }
    if !user_data.control.exit_requested.get()
        && user_data.control.paint_pending.replace(false)
        && unsafe { InvalidateRect(hwnd, ptr::null(), 0) } == 0
    {
        user_data
            .control
            .record_failure(last_error("pixel_window_reentrant_paint_reschedule_failed"));
    }
    user_data.control.phase.set(previous);
    surface_control_failure(user_data);
}

fn host_state_exit(user_data: &NativeWindowUserData) -> bool {
    user_data
        .host
        .try_borrow()
        .map(|state| state.exit)
        .unwrap_or(false)
}

fn host_should_exit(user_data: &NativeWindowUserData) -> bool {
    user_data.control.exit_requested.get() || host_state_exit(user_data)
}

fn surface_control_failure(user_data: &NativeWindowUserData) {
    let Ok(mut state) = user_data.host.try_borrow_mut() else {
        return;
    };
    if let Some(error) = user_data.control.take_failure() {
        state.frame_state.invalidate();
        if state.deferred_error.is_none() {
            state.deferred_error = Some(error);
        }
        state.exit = true;
    }
    if user_data.control.exit_requested.get() {
        state.exit = true;
    }
}

fn paint(state: &mut HostState, hwnd: HWND) -> Result<(), PixelWindowError> {
    let session = PaintSession::begin(hwnd)?;
    let dc = session.dc();

    let result = (|| {
        let metrics = *state.backend.metrics.borrow();
        sync_frame_geometry(state, metrics);
        if paint_should_skip(unsafe { IsIconic(hwnd) } != 0, metrics) {
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
        let previous_phase = state
            .backend
            .control
            .phase
            .replace(DispatchPhase::Rendering);
        let render_result = {
            let mut frame = XrgbPixelFrame::new(
                &mut state.pixels,
                metrics.physical_width,
                metrics.physical_height,
                metrics.scale_factor,
                &mut state.frame_state,
            );
            match state.application.render(&state.window, &mut frame) {
                Ok(directive) => frame
                    .write_receipt()
                    .map(|receipt| (directive, receipt))
                    .map_err(|error| {
                        PixelWindowError::failed("pixel_window_frame_commit_failed", error)
                    }),
                Err(error) => Err(error),
            }
        };
        state.backend.control.phase.set(previous_phase);
        let directive = match render_result {
            Ok((directive, _receipt)) => directive,
            Err(error) => {
                state.frame_state.invalidate();
                apply_directive(state, Err(error));
                return Ok(());
            }
        };
        apply_directive(state, Ok(directive));
        if state.backend.control.closed.get() {
            return Ok(());
        }

        let paint_rect = session.paint_rect();
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
        let (region, requested_pixels, _) = present_pixels_for_geometry(
            geometry,
            metrics.physical_width,
            metrics.physical_height,
            0,
        );
        let started = Instant::now();
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
        let (outcome, completed_scanlines, present_error) =
            match validate_stretch_copy(copied, source.bottom - source.top) {
                Ok(copied) => (PixelPresentOutcome::Succeeded, copied, None),
                Err(error) => (PixelPresentOutcome::Failed, 0, Some(error)),
            };
        let elapsed_ns = elapsed_ns_since(started);
        let (_, _, completed_pixels) = present_pixels_for_geometry(
            geometry,
            metrics.physical_width,
            metrics.physical_height,
            completed_scanlines,
        );
        state.backend.present.borrow_mut().record(
            elapsed_ns,
            requested_pixels,
            completed_pixels,
            region,
            outcome,
        );
        if let Some(error) = present_error {
            return Err(error);
        };
        Ok(())
    })();
    let end_result = session.finish();
    let result = match result {
        Err(error) => Err(error),
        Ok(()) => end_result,
    };
    if result.is_err() {
        state.frame_state.invalidate();
    }
    result
}

fn validate_stretch_copy(
    copied_scanlines: i32,
    requested_scanlines: i32,
) -> Result<i32, PixelWindowError> {
    if copied_scanlines <= 0 {
        return Err(last_error("pixel_window_surface_present_failed"));
    }
    if copied_scanlines < requested_scanlines {
        return Err(PixelWindowError::failed(
            "pixel_window_surface_present_short",
            format!("StretchDIBits copied {copied_scanlines} of {requested_scanlines} scanlines"),
        ));
    }
    Ok(copied_scanlines)
}

fn present_pixels_for_geometry(
    geometry: StretchDibGeometry,
    frame_width: u32,
    frame_height: u32,
    copied_scanlines: i32,
) -> (PixelPresentRegion, u64, u64) {
    let width = u64::try_from(geometry.source.right - geometry.source.left).unwrap_or(0);
    let height = u64::try_from(geometry.source.bottom - geometry.source.top).unwrap_or(0);
    let requested_pixels = width.saturating_mul(height);
    let copied_scanlines = u64::try_from(copied_scanlines.max(0))
        .unwrap_or(0)
        .min(height);
    let completed_pixels = width.saturating_mul(copied_scanlines);
    let region = if width == u64::from(frame_width) && height == u64::from(frame_height) {
        PixelPresentRegion::Full
    } else {
        PixelPresentRegion::Partial
    };
    (region, requested_pixels, completed_pixels)
}

fn frame_geometry_changed(
    previous_width: u32,
    previous_height: u32,
    previous_scale_bits: u64,
    metrics: PixelWindowMetrics,
) -> bool {
    previous_width != metrics.physical_width
        || previous_height != metrics.physical_height
        || previous_scale_bits != metrics.scale_factor.to_bits()
}

fn sync_frame_geometry(state: &mut HostState, metrics: PixelWindowMetrics) {
    if frame_geometry_changed(
        state.frame_width,
        state.frame_height,
        state.frame_scale_bits,
        metrics,
    ) {
        state.frame_state.advance_generation();
        state.frame_width = metrics.physical_width;
        state.frame_height = metrics.physical_height;
        state.frame_scale_bits = metrics.scale_factor.to_bits();
    }
}

fn paint_should_skip(minimized: bool, metrics: PixelWindowMetrics) -> bool {
    minimized || !metrics.is_drawable()
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
    sync_frame_geometry(state, metrics);
    if notify && state.opened && metrics.is_drawable() {
        dispatch_event(state, PixelWindowEvent::GeometryChanged { change, metrics });
    }
}

fn suggested_rect_geometry(rect: &RECT) -> Option<(i32, i32, i32, i32)> {
    let width = rect.right.checked_sub(rect.left)?;
    let height = rect.bottom.checked_sub(rect.top)?;
    (width > 0 && height > 0).then_some((rect.left, rect.top, width, height))
}

fn apply_ime_allowed(hwnd: HWND, allowed: bool) {
    let flags = if allowed { IACE_DEFAULT } else { 0 };
    // IMM32 is optional on Windows installations without East Asian input
    // support. PixelWindowBackend::set_ime_allowed is deliberately best-effort
    // and has no failure channel; absence must not terminate an otherwise
    // functional terminal window.
    unsafe { ImmAssociateContextEx(hwnd, ptr::null_mut(), flags) };
}

fn dispatch_event(state: &mut HostState, event: PixelWindowEvent) {
    if !state.opened || state.exit {
        return;
    }
    let control = state.backend.control.clone();
    let previous = control.phase.replace(DispatchPhase::ApplicationEvent);
    let result = catch_application("event", || state.application.event(&state.window, event));
    apply_directive(state, result);
    control.phase.set(previous);
}

fn catch_application<T>(
    callback_name: &'static str,
    callback: impl FnOnce() -> Result<T, PixelWindowError>,
) -> Result<T, PixelWindowError> {
    match catch_unwind(AssertUnwindSafe(callback)) {
        Ok(result) => result,
        Err(_) => Err(PixelWindowError::failed(
            "pixel_window_application_panic",
            format!("application callback `{callback_name}` panicked"),
        )),
    }
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

fn native_queue_failure(code: &'static str) -> PixelWindowError {
    PixelWindowError::failed(code, "native window deferred control queue failed")
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

    #[test]
    fn present_pixels_distinguish_full_partial_and_short_native_copy() {
        let full = StretchDibGeometry {
            source: Win32Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 50,
            },
            destination: Win32Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 50,
            },
        };
        assert_eq!(
            present_pixels_for_geometry(full, 100, 50, 50),
            (PixelPresentRegion::Full, 5_000, 5_000)
        );
        assert_eq!(
            present_pixels_for_geometry(full, 100, 50, 7),
            (PixelPresentRegion::Full, 5_000, 700)
        );

        let partial = StretchDibGeometry {
            source: Win32Rect {
                left: 10,
                top: 20,
                right: 30,
                bottom: 30,
            },
            destination: Win32Rect {
                left: 10,
                top: 20,
                right: 30,
                bottom: 30,
            },
        };
        assert_eq!(
            present_pixels_for_geometry(partial, 100, 50, -1),
            (PixelPresentRegion::Partial, 200, 0)
        );
    }

    #[test]
    fn frame_geometry_change_detects_scale_without_size_change() {
        let metrics = PixelWindowMetrics {
            logical_size: LogicalSize::new(100.0, 80.0),
            physical_width: 100,
            physical_height: 80,
            scale_factor: 1.5,
        };
        assert!(!frame_geometry_changed(100, 80, 1.5_f64.to_bits(), metrics));
        assert!(frame_geometry_changed(100, 80, 1.0_f64.to_bits(), metrics));
        assert!(frame_geometry_changed(99, 80, 1.5_f64.to_bits(), metrics));
    }

    #[test]
    fn minimized_or_nondrawable_metrics_suppress_paint() {
        let metrics = PixelWindowMetrics {
            logical_size: LogicalSize::new(100.0, 80.0),
            physical_width: 100,
            physical_height: 80,
            scale_factor: 1.0,
        };
        assert!(paint_should_skip(true, metrics));
        assert!(!paint_should_skip(false, metrics));
        assert!(paint_should_skip(
            false,
            PixelWindowMetrics {
                physical_width: 0,
                physical_height: 80,
                ..metrics
            }
        ));
    }

    #[test]
    fn short_stretch_copy_is_a_typed_failure() {
        let error = validate_stretch_copy(7, 50).expect_err("short copy must fail");
        match error {
            PixelWindowError::Failed { code, .. } => {
                assert_eq!(code, "pixel_window_surface_present_short");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(validate_stretch_copy(50, 50).expect("complete copy"), 50);
    }

    #[test]
    fn native_control_defers_in_callback_phase_and_preserves_fifo() {
        let control = NativeControl::new();
        control.phase.set(DispatchPhase::ApplicationEvent);
        control
            .enqueue(DeferredNative::Command(NativeCommand::SetTitle(
                "queued".to_owned(),
            )))
            .expect("callback command fits");
        control
            .enqueue(DeferredNative::Event(PendingNativeEvent::FocusChanged(
                true,
            )))
            .expect("reentrant event fits");
        assert!(control.has_deferred());
        assert!(matches!(control.pop(), Some(DeferredNative::Command(_))));
        assert!(matches!(control.pop(), Some(DeferredNative::Event(_))));
        assert!(!control.has_deferred());
    }

    #[test]
    fn native_control_overflow_is_a_typed_failure() {
        let control = NativeControl::new();
        for _ in 0..MAX_NATIVE_DEFERRED {
            control
                .enqueue(DeferredNative::Event(PendingNativeEvent::Wake))
                .expect("queue capacity");
        }
        let error = control
            .enqueue(DeferredNative::Event(PendingNativeEvent::Wake))
            .expect_err("overflow must fail");
        match error {
            PixelWindowError::Failed { code, .. } => {
                assert_eq!(code, "pixel_window_native_queue_overflow");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(control.exit_requested.get());
        assert!(control.take_failure().is_some());
    }

    #[test]
    fn application_panic_becomes_a_typed_failure() {
        let error = catch_application::<()>("test", || panic!("synthetic callback panic"))
            .expect_err("callback panic must not escape the host boundary");
        match error {
            PixelWindowError::Failed { code, .. } => {
                assert_eq!(code, "pixel_window_application_panic");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn default_window_messages_are_classified_before_state_borrow() {
        assert_eq!(
            classify_native_message(WM_SETTEXT),
            NativeMessageClass::Default
        );
        assert_eq!(
            classify_native_message(WM_WINDOWPOSCHANGING),
            NativeMessageClass::Default
        );
        assert_eq!(
            classify_native_message(WM_WINDOWPOSCHANGED),
            NativeMessageClass::Default
        );
        assert_eq!(
            classify_native_message(WM_NCCALCSIZE),
            NativeMessageClass::Default
        );
        assert_eq!(
            classify_native_message(WM_SIZE),
            NativeMessageClass::Stateful
        );
        assert_eq!(
            classify_native_message(WM_CAPTURECHANGED),
            NativeMessageClass::Stateful
        );
        assert_eq!(classify_native_message(WM_PAINT), NativeMessageClass::Paint);
        assert_eq!(
            classify_native_message(WM_QUIT),
            NativeMessageClass::Default
        );
    }

    #[test]
    fn dpi_lparam_is_copied_before_deferred_processing() {
        let mut rect = RECT {
            left: 10,
            top: 20,
            right: 810,
            bottom: 620,
        };
        let copied = copied_suggested_rect((&mut rect as *mut RECT).cast::<c_void>() as LPARAM)
            .expect("valid DPI rectangle");
        rect.right = 11;
        assert_ne!(copied.right, rect.right);
        assert_eq!(
            copied,
            Win32Rect {
                left: 10,
                top: 20,
                right: 810,
                bottom: 620,
            }
        );
    }
}
