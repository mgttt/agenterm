//! Win32 native control-window host. Native handles never cross this module.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io, mem, ptr,
    rc::Rc,
    time::Instant,
};

use windows_sys::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen,
        CreateSolidBrush, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_RIGHT, DT_SINGLELINE, DT_VCENTER,
        DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetStockObject, InvalidateRect,
        LineTo, MoveToEx, PAINTSTRUCT, PS_SOLID, Rectangle, SRCCOPY, ScreenToClient, SelectObject,
        SetBkMode, SetTextColor, TRANSPARENT, TextOutW, UpdateWindow, WHITE_BRUSH,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{
            EnableWindow, GetCapture, GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus,
            VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F2, VK_F3, VK_F4,
            VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10, VK_F11, VK_F12, VK_HOME, VK_INSERT, VK_LEFT,
            VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
        },
        WindowsAndMessaging::{
            AppendMenuW, CS_DBLCLKS, CW_USEDEFAULT, CheckMenuItem, CreateWindowExW, DefWindowProcW,
            DestroyWindow, DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_PASSWORD,
            ES_WANTRETURN, EnableMenuItem, GWLP_USERDATA, GetClientRect, GetMessageW,
            GetSystemMenu, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
            IDC_ARROW, IDC_HAND, IDC_IBEAM, IDC_SIZENS, IDC_SIZEWE, ISMEX_NOSEND, InSendMessageEx,
            IsIconic, IsWindow, IsWindowVisible, IsZoomed, LoadCursorW, MF_BYCOMMAND, MF_CHECKED,
            MF_ENABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, ModifyMenuW,
            MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW, SIZE_MINIMIZED, SW_HIDE,
            SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SendMessageW,
            SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowTextW,
            ShowWindow, TranslateMessage, WM_APP, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_COMMAND,
            WM_COPY, WM_DESTROY, WM_ERASEBKGND, WM_INITMENUPOPUP, WM_KEYDOWN, WM_KEYUP,
            WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
            WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_PASTE,
            WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETFOCUS, WM_SIZE, WM_SYSCOMMAND, WM_TIMER, WNDCLASSW,
            WS_CHILD, WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
            WS_VISIBLE, WS_VSCROLL,
        },
    },
};

use crate::{
    contract::input::{
        KeyClassification, KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent,
        PhysicalKeyCode, Utf16TextDecoder,
    },
    control_window::{
        ButtonState, ControlCanvas, ControlCursor, ControlId, ControlKind, ControlWheelDelta,
        ControlWindow, ControlWindowApplication, ControlWindowBackend, ControlWindowDirective,
        ControlWindowError, ControlWindowEvent, ControlWindowOptions, ControlWindowQuery,
        ControlWindowRenderActivity, ControlWindowState, FocusTarget, MenuCommandId, PixelPoint,
        PixelRect, PixelSize, PointerButton, Rgb8, TextHorizontalAlignment, TextOptions,
        WindowPresentation,
    },
};

const TIMER_ID: usize = 1;
const DEFERRED_CLOSE: u32 = WM_APP + 1;
const AUTOMATION_SHORTCUT: u32 = WM_APP + 2;
const AUTOMATION_FOCUS_QUERY: u32 = WM_APP + 3;
const AUTOMATION_RENDER_ACTIVITY_SAMPLE: u32 = WM_APP + 4;

// windows-sys aliases SetWindowLongPtrW to SetWindowLongW on 32-bit targets.
#[cfg(target_pointer_width = "32")]
type NativeLongPtr = i32;
#[cfg(target_pointer_width = "64")]
type NativeLongPtr = isize;
const _: () = assert!(std::mem::size_of::<NativeLongPtr>() == std::mem::size_of::<*mut ()>());

struct Backend {
    window: Cell<HWND>,
    controls: HashMap<ControlId, HWND>,
    system_menu: HashMap<MenuCommandId, u32>,
    redraw_requests: Cell<u64>,
    parent_paints: Cell<u64>,
    control_bounds_updates: Cell<u64>,
    control_bounds_skips: Cell<u64>,
    control_visibility_updates: Cell<u64>,
    control_visibility_skips: Cell<u64>,
}

impl Backend {
    fn record_parent_paint(&self) {
        increment(&self.parent_paints);
    }

    fn control(&self, id: ControlId) -> Result<HWND, ControlWindowError> {
        self.controls.get(&id).copied().ok_or_else(|| {
            ControlWindowError::failed(
                "control_window_unknown_control",
                format!("unknown control {}", id.0),
            )
        })
    }

    fn focus_target(&self, hwnd: HWND) -> FocusTarget {
        if hwnd == self.window.get() {
            return FocusTarget::Window;
        }
        self.controls
            .iter()
            .find_map(|(id, control)| (*control == hwnd).then_some(FocusTarget::Control(*id)))
            .unwrap_or(FocusTarget::None)
    }

    fn menu_id(&self, id: MenuCommandId) -> Result<u32, ControlWindowError> {
        self.system_menu.get(&id).copied().ok_or_else(|| {
            ControlWindowError::failed(
                "control_window_unknown_system_menu_command",
                format!("unknown system menu command {}", id.0),
            )
        })
    }
}

impl ControlWindowBackend for Backend {
    fn request_redraw(&self) {
        increment(&self.redraw_requests);
        unsafe {
            InvalidateRect(self.window.get(), ptr::null(), 0);
        }
    }
    fn render_activity(&self) -> ControlWindowRenderActivity {
        ControlWindowRenderActivity {
            redraw_requests: self.redraw_requests.get(),
            parent_paints: self.parent_paints.get(),
            control_bounds_updates: self.control_bounds_updates.get(),
            control_bounds_skips: self.control_bounds_skips.get(),
            control_visibility_updates: self.control_visibility_updates.get(),
            control_visibility_skips: self.control_visibility_skips.get(),
        }
    }
    fn close(&self) {
        unsafe {
            PostMessageW(self.window.get(), DEFERRED_CLOSE, 0, 0);
        }
    }
    fn focus(&self) {
        unsafe {
            SetForegroundWindow(self.window.get());
            SetFocus(self.window.get());
        }
    }
    fn client_size(&self) -> PixelSize {
        client_size(self.window.get())
    }
    fn state(&self) -> ControlWindowState {
        let window = self.window.get();
        ControlWindowState {
            minimized: unsafe { IsIconic(window) } != 0,
            maximized: unsafe { IsZoomed(window) } != 0,
            visible: unsafe { IsWindowVisible(window) } != 0,
        }
    }
    fn focused_target(&self) -> FocusTarget {
        self.focus_target(unsafe { GetFocus() })
    }
    fn set_client_size(&self, size: PixelSize) -> Result<(), ControlWindowError> {
        if size.width == 0 || size.height == 0 {
            return Err(ControlWindowError::failed(
                "control_window_invalid_client_size",
                "client size must be non-zero",
            ));
        }
        let window = self.window.get();
        let current_client = client_size(window);
        let mut outer: RECT = unsafe { mem::zeroed() };
        if unsafe { GetWindowRect(window, &mut outer) } == 0 {
            return Err(last_error("control_window_get_window_rect_failed"));
        }
        let outer_width = (outer.right - outer.left).max(0);
        let outer_height = (outer.bottom - outer.top).max(0);
        let width = outer_width
            .saturating_add(i32_size(size.width))
            .saturating_sub(i32_size(current_client.width));
        let height = outer_height
            .saturating_add(i32_size(size.height))
            .saturating_sub(i32_size(current_client.height));
        if unsafe { MoveWindow(window, outer.left, outer.top, width, height, 1) } == 0 {
            Err(last_error("control_window_resize_failed"))
        } else {
            Ok(())
        }
    }
    fn set_presentation(&self, presentation: WindowPresentation) {
        unsafe {
            ShowWindow(
                self.window.get(),
                match presentation {
                    WindowPresentation::Minimized => SW_MINIMIZE,
                    WindowPresentation::Maximized => SW_MAXIMIZE,
                    WindowPresentation::Restored => SW_RESTORE,
                },
            );
        }
    }
    fn show_without_activation(&self) {
        unsafe {
            ShowWindow(self.window.get(), SW_SHOWNOACTIVATE);
        }
    }
    fn set_title(&self, title: &str) -> Result<(), ControlWindowError> {
        let w = wide(title);
        if unsafe { SetWindowTextW(self.window.get(), w.as_ptr()) } == 0 {
            Err(last_error("control_window_set_title_failed"))
        } else {
            Ok(())
        }
    }
    fn set_control_text(&self, id: ControlId, text: &str) -> Result<(), ControlWindowError> {
        let w = wide(text);
        if unsafe { SetWindowTextW(self.control(id)?, w.as_ptr()) } == 0 {
            Err(last_error("control_window_set_control_text_failed"))
        } else {
            Ok(())
        }
    }
    fn control_text(&self, id: ControlId) -> Result<String, ControlWindowError> {
        let hwnd = self.control(id)?;
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        let mut value = vec![0u16; usize::try_from(len).unwrap_or(0) + 1];
        let copied = unsafe {
            GetWindowTextW(
                hwnd,
                value.as_mut_ptr(),
                i32::try_from(value.len()).unwrap_or(i32::MAX),
            )
        };
        if copied == 0 && len > 0 {
            return Err(last_error("control_window_get_control_text_failed"));
        }
        Ok(String::from_utf16_lossy(
            &value[..usize::try_from(copied).unwrap_or(0)],
        ))
    }
    fn copy_control_selection(&self, id: ControlId) -> Result<(), ControlWindowError> {
        unsafe {
            SendMessageW(self.control(id)?, WM_COPY, 0, 0);
        }
        Ok(())
    }
    fn paste_control_selection(&self, id: ControlId) -> Result<(), ControlWindowError> {
        unsafe {
            SendMessageW(self.control(id)?, WM_PASTE, 0, 0);
        }
        Ok(())
    }
    fn set_control_bounds(&self, id: ControlId, b: PixelRect) -> Result<(), ControlWindowError> {
        let control = self.control(id)?;
        let mut current: RECT = unsafe { mem::zeroed() };
        if unsafe { GetWindowRect(control, &mut current) } == 0 {
            return Err(last_error("control_window_get_control_rect_failed"));
        }
        let mut origin = POINT {
            x: current.left,
            y: current.top,
        };
        if unsafe { ScreenToClient(self.window.get(), &mut origin) } == 0 {
            return Err(last_error("control_window_map_control_rect_failed"));
        }
        if control_bounds_match(origin, current, b) {
            increment(&self.control_bounds_skips);
            return Ok(());
        }
        if unsafe {
            MoveWindow(
                control,
                b.origin.x,
                b.origin.y,
                i32_size(b.size.width),
                i32_size(b.size.height),
                1,
            )
        } == 0
        {
            Err(last_error("control_window_layout_failed"))
        } else {
            increment(&self.control_bounds_updates);
            Ok(())
        }
    }
    fn set_control_enabled(&self, id: ControlId, enabled: bool) -> Result<(), ControlWindowError> {
        unsafe {
            EnableWindow(self.control(id)?, enabled as i32);
        }
        Ok(())
    }
    fn set_control_visible(&self, id: ControlId, visible: bool) -> Result<(), ControlWindowError> {
        let control = self.control(id)?;
        if (unsafe { IsWindowVisible(control) } != 0) == visible {
            increment(&self.control_visibility_skips);
            return Ok(());
        }
        unsafe {
            ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE });
        }
        increment(&self.control_visibility_updates);
        Ok(())
    }
    fn set_system_menu_text(
        &self,
        id: MenuCommandId,
        text: &str,
    ) -> Result<(), ControlWindowError> {
        let native_id = self.menu_id(id)?;
        let text = wide(text);
        let menu = unsafe { GetSystemMenu(self.window.get(), 0) };
        if unsafe {
            ModifyMenuW(
                menu,
                native_id,
                MF_BYCOMMAND | MF_STRING,
                native_id as usize,
                text.as_ptr(),
            )
        } == 0
        {
            Err(last_error("control_window_system_menu_text_failed"))
        } else {
            Ok(())
        }
    }
    fn set_system_menu_enabled(
        &self,
        id: MenuCommandId,
        enabled: bool,
    ) -> Result<(), ControlWindowError> {
        let native_id = self.menu_id(id)?;
        let result = unsafe {
            EnableMenuItem(
                GetSystemMenu(self.window.get(), 0),
                native_id,
                MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED },
            )
        };
        if result == -1 {
            Err(last_error("control_window_system_menu_enabled_failed"))
        } else {
            Ok(())
        }
    }
    fn set_system_menu_checked(
        &self,
        id: MenuCommandId,
        checked: bool,
    ) -> Result<(), ControlWindowError> {
        let native_id = self.menu_id(id)?;
        let result = unsafe {
            CheckMenuItem(
                GetSystemMenu(self.window.get(), 0),
                native_id,
                MF_BYCOMMAND | if checked { MF_CHECKED } else { MF_UNCHECKED },
            )
        };
        if result == u32::MAX {
            Err(last_error("control_window_system_menu_checked_failed"))
        } else {
            Ok(())
        }
    }
    fn focus_control(&self, id: ControlId) -> Result<(), ControlWindowError> {
        unsafe {
            SetFocus(self.control(id)?);
        }
        Ok(())
    }
    fn set_pointer_capture(&self, capture: bool) -> Result<(), ControlWindowError> {
        if capture {
            unsafe {
                SetCapture(self.window.get());
            }
            if !self.pointer_capture_owned()? {
                return Err(ControlWindowError::failed(
                    "control_window_pointer_capture_acquire_failed",
                    "SetCapture returned without transferring pointer capture to this window",
                ));
            }
        } else if self.pointer_capture_owned()? {
            if unsafe { ReleaseCapture() } == 0 {
                return Err(last_error("control_window_pointer_capture_release_failed"));
            }
            if self.pointer_capture_owned()? {
                return Err(ControlWindowError::failed(
                    "control_window_pointer_capture_release_failed",
                    "ReleaseCapture returned without releasing pointer capture from this window",
                ));
            }
        }
        Ok(())
    }
    fn pointer_capture_owned(&self) -> Result<bool, ControlWindowError> {
        let window = self.window.get();
        validate_pointer_capture_window(window, unsafe { IsWindow(window) } != 0)?;
        Ok(pointer_capture_is_owned(window, unsafe { GetCapture() }))
    }
    fn set_cursor(&self, cursor: ControlCursor) -> Result<(), ControlWindowError> {
        let id = match cursor {
            ControlCursor::Arrow => IDC_ARROW,
            ControlCursor::Hand => IDC_HAND,
            ControlCursor::Text => IDC_IBEAM,
            ControlCursor::ResizeHorizontal => IDC_SIZEWE,
            ControlCursor::ResizeVertical => IDC_SIZENS,
        };
        let value = unsafe { LoadCursorW(ptr::null_mut(), id) };
        if value.is_null() {
            Err(last_error("control_window_cursor_load_failed"))
        } else {
            unsafe {
                SetCursor(value);
            }
            Ok(())
        }
    }
    #[cfg(feature = "font")]
    fn create_font(
        &self,
        request: crate::font::FontRequest<'_>,
    ) -> Result<crate::font::NativeFont, ControlWindowError> {
        let handle =
            unsafe { crate::font::OpaqueWindowHandle::from_raw(self.window.get() as isize) };
        crate::font::create_terminal_font(handle, request).map_err(|error| {
            ControlWindowError::failed(error.code(), "native control-window font creation failed")
        })
    }
    #[cfg(feature = "screenshot")]
    fn capture_png(
        &self,
        path: &std::path::Path,
        area: crate::screenshot::NativeCaptureArea,
    ) -> Result<(), ControlWindowError> {
        // Flush a redraw requested by the application before sampling native pixels. This keeps
        // structured state and screenshots from describing adjacent frames.
        unsafe {
            UpdateWindow(self.window.get());
        }
        let window = unsafe {
            crate::screenshot::ScreenshotWindowHandle::from_raw(self.window.get() as isize)
        }
        .ok_or_else(|| {
            ControlWindowError::failed(
                "control_window_screenshot_handle_unavailable",
                "native control-window screenshot handle is unavailable",
            )
        })?;
        crate::screenshot::capture_native_window_png(window, path, area)
            .map(|_| ())
            .map_err(|error| {
                ControlWindowError::failed(
                    "control_window_screenshot_failed",
                    format!("{}: {error}", error.code()),
                )
            })
    }
}

struct State {
    window: ControlWindow,
    backend: Rc<Backend>,
    application: Box<dyn ControlWindowApplication>,
    system_menu_commands: HashMap<usize, MenuCommandId>,
    text_decoder: Utf16TextDecoder,
    deferred_error: Option<ControlWindowError>,
    destroying: bool,
}

pub(crate) fn run_control_window(
    options: ControlWindowOptions,
    application: Box<dyn ControlWindowApplication>,
) -> Result<(), ControlWindowError> {
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        return Err(last_error("control_window_module_handle_failed"));
    }
    let class = wide("AgentermPlatformControlWindow");
    let wc = WNDCLASSW {
        style: CS_DBLCLKS,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
        hbrBackground: unsafe { GetStockObject(WHITE_BRUSH) } as _,
        lpszClassName: class.as_ptr(),
        ..unsafe { mem::zeroed() }
    };
    if unsafe { RegisterClassW(&wc) } == 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(1410) {
            return Err(ControlWindowError::failed(
                "control_window_class_register_failed",
                e,
            ));
        }
    }
    let title = wide(&options.title);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            top_level_window_style(),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            i32_size(options.initial_size.width),
            i32_size(options.initial_size.height),
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        )
    };
    if hwnd.is_null() {
        return Err(last_error("control_window_create_failed"));
    }
    let mut controls = HashMap::new();
    for spec in &options.controls {
        let (class_name, style, ex_style) = match spec.kind {
            ControlKind::Button => ("BUTTON", 0u32, 0u32),
            ControlKind::Label => ("STATIC", 0, 0),
            ControlKind::TextInput {
                multiline,
                password,
                vertical_scroll,
                want_return,
            } => (
                "EDIT",
                (if multiline { ES_MULTILINE as u32 } else { 0 })
                    | (if password { ES_PASSWORD as u32 } else { 0 })
                    | (if vertical_scroll {
                        ES_AUTOVSCROLL as u32 | WS_VSCROLL
                    } else {
                        0
                    })
                    | (if want_return { ES_WANTRETURN as u32 } else { 0 }),
                WS_EX_CLIENTEDGE,
            ),
        };
        let class_name = wide(class_name);
        let text = wide(&spec.text);
        let mut child_style = WS_CHILD
            | if spec.visible { WS_VISIBLE } else { 0 }
            | if spec.tab_stop { WS_TABSTOP } else { 0 }
            | style;
        if matches!(spec.kind, ControlKind::Button) {
            child_style |= 0;
        }
        let child = unsafe {
            CreateWindowExW(
                ex_style,
                class_name.as_ptr(),
                text.as_ptr(),
                child_style,
                spec.bounds.origin.x,
                spec.bounds.origin.y,
                i32_size(spec.bounds.size.width),
                i32_size(spec.bounds.size.height),
                hwnd,
                spec.id.0 as usize as _,
                instance,
                ptr::null_mut(),
            )
        };
        if child.is_null() {
            let error = last_error("control_window_create_control_failed");
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(error);
        }
        unsafe {
            EnableWindow(child, spec.enabled as i32);
        }
        controls.insert(spec.id, child);
    }
    let mut system_menu = HashMap::new();
    for item in &options.system_menu {
        let native_id = native_menu_id(item.id)?;
        if system_menu.insert(item.id, native_id).is_some() {
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(ControlWindowError::failed(
                "control_window_duplicate_system_menu_command",
                format!("duplicate system menu command {}", item.id.0),
            ));
        }
    }
    let backend = Rc::new(Backend {
        window: Cell::new(hwnd),
        controls,
        system_menu,
        redraw_requests: Cell::new(0),
        parent_paints: Cell::new(0),
        control_bounds_updates: Cell::new(0),
        control_bounds_skips: Cell::new(0),
        control_visibility_updates: Cell::new(0),
        control_visibility_skips: Cell::new(0),
    });
    let window = ControlWindow(backend.clone());
    let mut state = Box::new(State {
        window: window.clone(),
        backend: backend.clone(),
        application,
        system_menu_commands: HashMap::new(),
        text_decoder: Utf16TextDecoder::default(),
        deferred_error: None,
        destroying: false,
    });
    unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (&mut *state as *mut State) as NativeLongPtr,
        );
    }
    let menu = unsafe { GetSystemMenu(hwnd, 0) };
    for item in &options.system_menu {
        let native_id = usize::from(item.id.0);
        unsafe {
            if item.separator_before && AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null()) == 0 {
                let error = last_error("control_window_system_menu_separator_failed");
                DestroyWindow(hwnd);
                return Err(error);
            }
            let text = wide(&item.text);
            if AppendMenuW(menu, MF_STRING, native_id, text.as_ptr()) == 0 {
                let error = last_error("control_window_system_menu_item_failed");
                DestroyWindow(hwnd);
                return Err(error);
            }
            if EnableMenuItem(
                menu,
                native_id as u32,
                MF_BYCOMMAND | if item.enabled { MF_ENABLED } else { MF_GRAYED },
            ) == -1
            {
                let error = last_error("control_window_system_menu_state_failed");
                DestroyWindow(hwnd);
                return Err(error);
            }
            if CheckMenuItem(
                menu,
                native_id as u32,
                MF_BYCOMMAND
                    | if item.checked {
                        MF_CHECKED
                    } else {
                        MF_UNCHECKED
                    },
            ) == u32::MAX
            {
                let error = last_error("control_window_system_menu_checked_failed");
                DestroyWindow(hwnd);
                return Err(error);
            }
        }
        state.system_menu_commands.insert(native_id, item.id);
    }
    if options.poll_interval_ms > 0
        && unsafe { SetTimer(hwnd, TIMER_ID, options.poll_interval_ms, None) } == 0
    {
        let error = last_error("control_window_timer_failed");
        unsafe {
            DestroyWindow(hwnd);
        }
        return Err(error);
    }
    let directive = state.application.opened(&window)?;
    apply_directive(&mut state, directive);
    unsafe {
        ShowWindow(
            hwnd,
            if options.no_activate {
                SW_SHOWNOACTIVATE
            } else {
                SW_SHOW
            },
        );
    }
    let mut msg: MSG = unsafe { mem::zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) };
        if result == -1 {
            return Err(last_error("control_window_message_loop_failed"));
        }
        if result == 0 {
            break;
        }
        if msg.message == WM_KEYDOWN || msg.message == WM_KEYUP {
            let directive = dispatch(
                &mut state,
                ControlWindowEvent::KeyPreview {
                    target: backend.focus_target(msg.hwnd),
                    event: key_event(msg.wParam as u32, msg.message == WM_KEYDOWN, msg.lParam),
                },
            );
            if matches!(
                directive,
                Some(ControlWindowDirective::Consumed | ControlWindowDirective::ConsumedAndRedraw)
            ) {
                continue;
            }
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    state.deferred_error.take().map_or(Ok(()), Err)
}

fn native_menu_id(id: MenuCommandId) -> Result<u32, ControlWindowError> {
    let native_id = u32::from(id.0);
    if native_id == 0 || native_id >= 0xF000 {
        Err(ControlWindowError::failed(
            "control_window_invalid_system_menu_command",
            format!("system menu command {} must be in 1..0xF000", id.0),
        ))
    } else {
        Ok(native_id)
    }
}

thread_local! {
    // Guards against re-entering the state-dispatch path for the same
    // thread while a message is already being dispatched. `Backend`
    // methods such as `set_presentation`/`set_client_size` call
    // `ShowWindow`/`MoveWindow` on this same `hwnd`, and Win32 delivers the
    // resulting WM_SIZE/WM_WINDOWPOSCHANGED/... messages synchronously and
    // reentrantly. Without this guard, the reentrant call would dereference
    // `state_ptr` into a second `&mut State` while the outer dispatch still
    // holds one live through `state.application` — an aliasing violation
    // regardless of whether it happens to behave today. Reentrant messages
    // fall back to the default window procedure immediately so the Win32
    // call keeps its synchronous contract, and are queued for the normal
    // dispatch path once the outer dispatch has released `&mut State`.
    static DISPATCHING: Cell<bool> = const { Cell::new(false) };
    static PENDING_MESSAGES: RefCell<Vec<(u32, WPARAM, LPARAM)>> =
        const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if state_ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    if msg == WM_NCDESTROY {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    if DISPATCHING.with(Cell::get) {
        PENDING_MESSAGES.with(|pending| {
            pending.borrow_mut().push((msg, wp, lp));
        });
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    DISPATCHING.with(|flag| flag.set(true));
    let result = dispatch_window_message(hwnd, msg, wp, lp, unsafe { &mut *state_ptr }, true);
    loop {
        let pending = PENDING_MESSAGES.with(|messages| std::mem::take(&mut *messages.borrow_mut()));
        if pending.is_empty() {
            break;
        }
        for (pending_msg, pending_wp, pending_lp) in pending {
            // Replay only delivers the application event: the queue path above
            // already handed this message to DefWindowProcW when it arrived.
            // Default-processing it a second time duplicates every message
            // DefWindowProcW *generates* — WM_IME_CHAR synthesizes a WM_CHAR,
            // so composed text arrived twice ("测试" as "测试测试").
            let _ = dispatch_window_message(
                hwnd,
                pending_msg,
                pending_wp,
                pending_lp,
                unsafe { &mut *state_ptr },
                false,
            );
        }
    }
    DISPATCHING.with(|flag| flag.set(false));
    result
}

fn dispatch_window_message(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    state: &mut State,
    default_process: bool,
) -> LRESULT {
    let event = match msg {
        WM_TIMER => Some(ControlWindowEvent::Poll {
            now: Instant::now(),
        }),
        WM_SIZE => Some(ControlWindowEvent::Resized {
            size: client_size(hwnd),
            minimized: u32::try_from(wp).ok() == Some(SIZE_MINIMIZED),
        }),
        WM_CLOSE => Some(ControlWindowEvent::CloseRequested),
        WM_SETFOCUS => Some(ControlWindowEvent::FocusChanged(true)),
        WM_KILLFOCUS => Some(ControlWindowEvent::FocusChanged(false)),
        WM_CAPTURECHANGED => Some(ControlWindowEvent::CaptureChanged(false)),
        WM_KEYDOWN | WM_KEYUP if unsafe { InSendMessageEx(ptr::null()) } != ISMEX_NOSEND => {
            Some(ControlWindowEvent::KeyPreview {
                target: state.window.focused_target(),
                event: key_event(wp as u32, msg == WM_KEYDOWN, lp),
            })
        }
        WM_CHAR => match state.text_decoder.push(wp as u16) {
            KeyClassification::TextCommit(text) => Some(ControlWindowEvent::TextInput(text)),
            _ => None,
        },
        WM_MOUSEMOVE => Some(ControlWindowEvent::PointerMoved {
            position: point_from_lparam(lp),
            modifiers: current_modifiers(),
        }),
        WM_LBUTTONDOWN => Some(pointer_event(
            PointerButton::Left,
            ButtonState::Pressed,
            lp,
            1,
        )),
        WM_LBUTTONDBLCLK => Some(pointer_event(
            PointerButton::Left,
            ButtonState::Pressed,
            lp,
            2,
        )),
        WM_LBUTTONUP => Some(pointer_event(
            PointerButton::Left,
            ButtonState::Released,
            lp,
            1,
        )),
        WM_RBUTTONDOWN => Some(pointer_event(
            PointerButton::Right,
            ButtonState::Pressed,
            lp,
            1,
        )),
        WM_RBUTTONUP => Some(pointer_event(
            PointerButton::Right,
            ButtonState::Released,
            lp,
            1,
        )),
        WM_MBUTTONDOWN => Some(pointer_event(
            PointerButton::Middle,
            ButtonState::Pressed,
            lp,
            1,
        )),
        WM_MBUTTONUP => Some(pointer_event(
            PointerButton::Middle,
            ButtonState::Released,
            lp,
            1,
        )),
        WM_MOUSEWHEEL => {
            let mut p = POINT {
                x: low_i16(lp) as i32,
                y: high_i16(lp) as i32,
            };
            unsafe {
                ScreenToClient(hwnd, &mut p);
            }
            Some(ControlWindowEvent::Wheel {
                delta: ControlWheelDelta::Lines(f32::from(high_i16(wp as isize)) / 120.0),
                position: PixelPoint::new(p.x, p.y),
                modifiers: current_modifiers(),
            })
        }
        WM_COMMAND => Some(ControlWindowEvent::Command(ControlId((wp & 0xffff) as u32))),
        WM_INITMENUPOPUP => Some(ControlWindowEvent::SystemMenuOpening),
        WM_SYSCOMMAND if state.system_menu_commands.contains_key(&wp) => Some(
            ControlWindowEvent::SystemMenu(state.system_menu_commands[&wp]),
        ),
        AUTOMATION_SHORTCUT => {
            let modifiers = lp as usize;
            let directive = dispatch(
                state,
                ControlWindowEvent::AutomationShortcut {
                    key: wp as u32,
                    modifiers: ModifierState {
                        control: modifiers & 1 != 0,
                        shift: modifiers & 2 != 0,
                        alt: modifiers & 4 != 0,
                        meta: false,
                    },
                },
            );
            return if matches!(
                directive,
                Some(ControlWindowDirective::Consumed | ControlWindowDirective::ConsumedAndRedraw)
            ) {
                1
            } else {
                0
            };
        }
        AUTOMATION_FOCUS_QUERY => {
            return state
                .application
                .query(&state.window, ControlWindowQuery::AutomationFocusSurface);
        }
        AUTOMATION_RENDER_ACTIVITY_SAMPLE => {
            let activity = state.window.render_activity();
            dispatch(state, ControlWindowEvent::RenderActivitySample(activity));
            return 1;
        }
        WM_PAINT => {
            paint(state, hwnd);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        DEFERRED_CLOSE => {
            state.destroying = true;
            unsafe {
                DestroyWindow(hwnd);
            }
            return 0;
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            return 0;
        }
        _ => None,
    };
    if let Some(event) = event {
        dispatch(state, event);
        return 0;
    }
    if default_process {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    0
}

fn dispatch(state: &mut State, event: ControlWindowEvent) -> Option<ControlWindowDirective> {
    if state.destroying {
        return None;
    }
    match state.application.event(&state.window, event) {
        Ok(d) => {
            apply_directive(state, d);
            Some(d)
        }
        Err(e) => {
            state.deferred_error = Some(e);
            state.window.close();
            None
        }
    }
}
fn apply_directive(state: &mut State, directive: ControlWindowDirective) {
    match directive {
        ControlWindowDirective::Continue => {}
        ControlWindowDirective::Redraw => state.window.request_redraw(),
        ControlWindowDirective::Consumed => {}
        ControlWindowDirective::ConsumedAndRedraw => state.window.request_redraw(),
        ControlWindowDirective::Close => state.window.close(),
    }
}

fn paint(state: &mut State, hwnd: HWND) {
    state.backend.record_parent_paint();
    let mut ps: PAINTSTRUCT = unsafe { mem::zeroed() };
    let target = unsafe { BeginPaint(hwnd, &mut ps) };
    let size = client_size(hwnd);
    if size.width == 0 || size.height == 0 {
        unsafe {
            EndPaint(hwnd, &ps);
        }
        return;
    }
    let memory = unsafe { CreateCompatibleDC(target) };
    let bitmap = unsafe {
        CreateCompatibleBitmap(
            target,
            i32_size(size.width.max(1)),
            i32_size(size.height.max(1)),
        )
    };
    if memory.is_null() || bitmap.is_null() {
        state.deferred_error = Some(last_error("control_window_back_buffer_failed"));
        unsafe {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !memory.is_null() {
                DeleteDC(memory);
            }
            EndPaint(hwnd, &ps);
        }
        state.window.close();
        return;
    }
    let old = unsafe { SelectObject(memory, bitmap) };
    let paint_result = {
        let mut canvas = WinCanvas {
            dc: memory,
            size,
            #[cfg(feature = "font")]
            original_font: None,
        };
        state.application.paint(&state.window, &mut canvas)
    };
    match paint_result {
        Ok(d) => apply_directive(state, d),
        Err(e) => {
            state.deferred_error = Some(e);
            state.window.close();
        }
    }
    let presented = unsafe {
        BitBlt(
            target,
            0,
            0,
            i32_size(size.width),
            i32_size(size.height),
            memory,
            0,
            0,
            SRCCOPY,
        )
    };
    unsafe {
        SelectObject(memory, old);
        DeleteObject(bitmap);
        DeleteDC(memory);
        EndPaint(hwnd, &ps);
    }
    if presented == 0 {
        state.deferred_error = Some(last_error("control_window_present_failed"));
        state.window.close();
    }
}

fn increment(counter: &Cell<u64>) {
    counter.set(counter.get().saturating_add(1));
}

struct WinCanvas {
    dc: *mut core::ffi::c_void,
    size: PixelSize,
    #[cfg(feature = "font")]
    original_font: Option<*mut core::ffi::c_void>,
}
impl ControlCanvas for WinCanvas {
    fn size(&self) -> PixelSize {
        self.size
    }
    fn clear(&mut self, c: Rgb8) {
        self.fill_rect(PixelRect::new(0, 0, self.size.width, self.size.height), c);
    }
    fn fill_rect(&mut self, r: PixelRect, c: Rgb8) {
        let brush = unsafe { CreateSolidBrush(color(c)) };
        let rect = to_rect(r);
        unsafe {
            FillRect(self.dc, &rect, brush);
            DeleteObject(brush);
        }
    }
    fn stroke_rect(&mut self, r: PixelRect, c: Rgb8, width: u32) {
        let pen = unsafe { CreatePen(PS_SOLID, i32_size(width.max(1)), color(c)) };
        let old_pen = unsafe { SelectObject(self.dc, pen) };
        let old_brush = unsafe { SelectObject(self.dc, GetStockObject(5)) };
        let rect = to_rect(r);
        unsafe {
            Rectangle(self.dc, rect.left, rect.top, rect.right, rect.bottom);
            SelectObject(self.dc, old_brush);
            SelectObject(self.dc, old_pen);
            DeleteObject(pen);
        }
    }
    fn line(&mut self, a: PixelPoint, b: PixelPoint, c: Rgb8, width: u32) {
        let pen = unsafe { CreatePen(PS_SOLID, i32_size(width.max(1)), color(c)) };
        let old = unsafe { SelectObject(self.dc, pen) };
        unsafe {
            MoveToEx(self.dc, a.x, a.y, ptr::null_mut());
            LineTo(self.dc, b.x, b.y);
            SelectObject(self.dc, old);
            DeleteObject(pen);
        }
    }
    fn text(&mut self, p: PixelPoint, text: &str, c: Rgb8) {
        let w: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            SetBkMode(self.dc, TRANSPARENT as i32);
            SetTextColor(self.dc, color(c));
            TextOutW(
                self.dc,
                p.x,
                p.y,
                w.as_ptr(),
                i32::try_from(w.len()).unwrap_or(i32::MAX),
            );
        }
    }
    fn text_rect(&mut self, rect: PixelRect, text: &str, c: Rgb8, options: TextOptions) {
        let mut text = text.encode_utf16().collect::<Vec<_>>();
        let mut rect = to_rect(rect);
        let mut flags = match options.horizontal {
            TextHorizontalAlignment::Left => DT_LEFT,
            TextHorizontalAlignment::Center => DT_CENTER,
            TextHorizontalAlignment::Right => DT_RIGHT,
        };
        if options.vertical_center {
            flags |= DT_VCENTER;
        }
        if options.single_line {
            flags |= DT_SINGLELINE;
        }
        if options.end_ellipsis {
            flags |= DT_END_ELLIPSIS;
        }
        unsafe {
            SetBkMode(self.dc, TRANSPARENT as i32);
            SetTextColor(self.dc, color(c));
            DrawTextW(
                self.dc,
                text.as_mut_ptr(),
                i32::try_from(text.len()).unwrap_or(i32::MAX),
                &mut rect,
                flags,
            );
        }
    }
    #[cfg(feature = "font")]
    fn set_font(&mut self, font: &crate::font::NativeFont) -> Result<(), ControlWindowError> {
        let previous =
            unsafe { SelectObject(self.dc, font.raw_handle() as *mut core::ffi::c_void) };
        if previous.is_null() {
            return Err(last_error("control_window_select_font_failed"));
        }
        if self.original_font.is_none() {
            self.original_font = Some(previous);
        }
        Ok(())
    }
}

impl Drop for WinCanvas {
    fn drop(&mut self) {
        #[cfg(feature = "font")]
        if let Some(font) = self.original_font.take() {
            unsafe {
                SelectObject(self.dc, font);
            }
        }
    }
}

fn pointer_event(
    button: PointerButton,
    state: ButtonState,
    lp: LPARAM,
    clicks: u8,
) -> ControlWindowEvent {
    ControlWindowEvent::PointerButton {
        button,
        state,
        position: point_from_lparam(lp),
        clicks,
        modifiers: current_modifiers(),
    }
}
fn key_event(key: u32, pressed: bool, lp: LPARAM) -> NormalizedKeyEvent {
    let (logical, physical) = normalized_key_identity(key);
    NormalizedKeyEvent {
        logical,
        physical,
        text: None,
        state: if pressed {
            KeyPressState::Pressed
        } else {
            KeyPressState::Released
        },
        repeat: pressed && ((lp as usize >> 30) & 1) != 0,
        modifiers: current_modifiers(),
    }
}

fn normalized_key_identity(key: u32) -> (LogicalKey, PhysicalKeyCode) {
    if (0x41..=0x5a).contains(&key) {
        let character = char::from_u32(key)
            .expect("ASCII virtual-key letter is a scalar")
            .to_ascii_lowercase();
        return (
            LogicalKey::Character(character.to_string()),
            PhysicalKeyCode::Letter(character),
        );
    }
    if (0x30..=0x39).contains(&key) {
        let digit = u8::try_from(key - 0x30).expect("virtual-key digit fits u8");
        return (
            LogicalKey::Character(char::from(b'0' + digit).to_string()),
            PhysicalKeyCode::Digit(digit),
        );
    }
    let named = match key as u16 {
        VK_BACK => Some((NamedKey::Backspace, PhysicalKeyCode::Backspace)),
        VK_RETURN => Some((NamedKey::Enter, PhysicalKeyCode::Enter)),
        VK_SPACE => Some((NamedKey::Space, PhysicalKeyCode::Space)),
        VK_TAB => Some((NamedKey::Tab, PhysicalKeyCode::Tab)),
        VK_DELETE => Some((NamedKey::Delete, PhysicalKeyCode::Other)),
        VK_DOWN => Some((NamedKey::ArrowDown, PhysicalKeyCode::Other)),
        VK_END => Some((NamedKey::End, PhysicalKeyCode::Other)),
        VK_ESCAPE => Some((NamedKey::Escape, PhysicalKeyCode::Other)),
        VK_F1 => Some((NamedKey::F1, PhysicalKeyCode::Other)),
        VK_F2 => Some((NamedKey::F2, PhysicalKeyCode::Other)),
        VK_F3 => Some((NamedKey::F3, PhysicalKeyCode::Other)),
        VK_F4 => Some((NamedKey::F4, PhysicalKeyCode::Other)),
        VK_F5 => Some((NamedKey::F5, PhysicalKeyCode::Other)),
        VK_F6 => Some((NamedKey::F6, PhysicalKeyCode::Other)),
        VK_F7 => Some((NamedKey::F7, PhysicalKeyCode::Other)),
        VK_F8 => Some((NamedKey::F8, PhysicalKeyCode::Other)),
        VK_F9 => Some((NamedKey::F9, PhysicalKeyCode::Other)),
        VK_F10 => Some((NamedKey::F10, PhysicalKeyCode::Other)),
        VK_F11 => Some((NamedKey::F11, PhysicalKeyCode::Other)),
        VK_F12 => Some((NamedKey::F12, PhysicalKeyCode::Other)),
        VK_HOME => Some((NamedKey::Home, PhysicalKeyCode::Other)),
        VK_INSERT => Some((NamedKey::Insert, PhysicalKeyCode::Other)),
        VK_LEFT => Some((NamedKey::ArrowLeft, PhysicalKeyCode::Other)),
        VK_NEXT => Some((NamedKey::PageDown, PhysicalKeyCode::Other)),
        VK_PRIOR => Some((NamedKey::PageUp, PhysicalKeyCode::Other)),
        VK_RIGHT => Some((NamedKey::ArrowRight, PhysicalKeyCode::Other)),
        VK_UP => Some((NamedKey::ArrowUp, PhysicalKeyCode::Other)),
        _ => None,
    };
    named.map_or(
        (LogicalKey::Unidentified, PhysicalKeyCode::Other),
        |(logical, physical)| (LogicalKey::Named(logical), physical),
    )
}
fn key_down(key: u16) -> bool {
    (unsafe { GetKeyState(i32::from(key)) }) < 0
}
fn current_modifiers() -> ModifierState {
    ModifierState {
        control: key_down(VK_CONTROL),
        shift: key_down(VK_SHIFT),
        alt: key_down(VK_MENU),
        meta: false,
    }
}
fn point_from_lparam(lp: LPARAM) -> PixelPoint {
    PixelPoint::new(low_i16(lp) as i32, high_i16(lp) as i32)
}
fn low_i16(v: isize) -> i16 {
    (v as u16) as i16
}
fn high_i16(v: isize) -> i16 {
    ((v >> 16) as u16) as i16
}
fn client_size(hwnd: HWND) -> PixelSize {
    let mut r: RECT = unsafe { mem::zeroed() };
    unsafe {
        GetClientRect(hwnd, &mut r);
    }
    PixelSize::new(
        (r.right - r.left).max(0) as u32,
        (r.bottom - r.top).max(0) as u32,
    )
}
fn to_rect(r: PixelRect) -> RECT {
    RECT {
        left: r.origin.x,
        top: r.origin.y,
        right: r.origin.x.saturating_add(i32_size(r.size.width)),
        bottom: r.origin.y.saturating_add(i32_size(r.size.height)),
    }
}
fn color(c: Rgb8) -> COLORREF {
    u32::from(c.red) | (u32::from(c.green) << 8) | (u32::from(c.blue) << 16)
}
fn i32_size(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}
fn control_bounds_match(origin: POINT, current: RECT, requested: PixelRect) -> bool {
    origin.x == requested.origin.x
        && origin.y == requested.origin.y
        && current.right.saturating_sub(current.left) == i32_size(requested.size.width)
        && current.bottom.saturating_sub(current.top) == i32_size(requested.size.height)
}
fn pointer_capture_is_owned(window: HWND, captured: HWND) -> bool {
    !window.is_null() && captured == window
}
fn validate_pointer_capture_window(
    window: HWND,
    is_window: bool,
) -> Result<(), ControlWindowError> {
    if window.is_null() || !is_window {
        Err(ControlWindowError::failed(
            "control_window_pointer_capture_query_failed",
            "native control-window handle is unavailable",
        ))
    } else {
        Ok(())
    }
}
fn top_level_window_style() -> u32 {
    WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN
}
fn wide(v: &str) -> Vec<u16> {
    v.encode_utf16().chain(std::iter::once(0)).collect()
}
fn last_error(code: &'static str) -> ControlWindowError {
    ControlWindowError::failed(code, io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_window_clips_native_children_from_parent_paint() {
        assert_ne!(top_level_window_style() & WS_CLIPCHILDREN, 0);
        assert_eq!(
            top_level_window_style() & WS_OVERLAPPEDWINDOW,
            WS_OVERLAPPEDWINDOW
        );
    }

    #[test]
    fn unchanged_control_bounds_skip_native_repaint() {
        let requested = PixelRect::new(20, 30, 120, 40);
        assert!(control_bounds_match(
            POINT { x: 20, y: 30 },
            RECT {
                left: 200,
                top: 300,
                right: 320,
                bottom: 340,
            },
            requested,
        ));
        assert!(!control_bounds_match(
            POINT { x: 21, y: 30 },
            RECT {
                left: 200,
                top: 300,
                right: 320,
                bottom: 340,
            },
            requested,
        ));
    }

    #[test]
    fn pointer_capture_ownership_requires_the_exact_window() {
        let window = 1usize as HWND;
        let other = 2usize as HWND;
        assert!(pointer_capture_is_owned(window, window));
        assert!(!pointer_capture_is_owned(window, other));
        assert!(!pointer_capture_is_owned(window, ptr::null_mut()));
        assert!(!pointer_capture_is_owned(ptr::null_mut(), ptr::null_mut()));
    }

    #[test]
    fn unavailable_pointer_capture_window_is_a_typed_query_failure() {
        for (window, is_window) in [(ptr::null_mut(), false), (1usize as HWND, false)] {
            assert!(matches!(
                validate_pointer_capture_window(window, is_window),
                Err(ControlWindowError::Failed { code, .. })
                    if code == "control_window_pointer_capture_query_failed"
            ));
        }
        assert!(validate_pointer_capture_window(1usize as HWND, true).is_ok());
    }

    #[test]
    fn named_and_physical_key_identity_is_stable() {
        assert_eq!(
            normalized_key_identity(u32::from(VK_TAB)),
            (LogicalKey::Named(NamedKey::Tab), PhysicalKeyCode::Tab)
        );
        assert_eq!(
            normalized_key_identity(u32::from(VK_LEFT)),
            (
                LogicalKey::Named(NamedKey::ArrowLeft),
                PhysicalKeyCode::Other
            )
        );
        assert_eq!(
            normalized_key_identity(u32::from(VK_F12)),
            (LogicalKey::Named(NamedKey::F12), PhysicalKeyCode::Other)
        );
        assert_eq!(
            normalized_key_identity(u32::from(b'A')),
            (
                LogicalKey::Character("a".to_owned()),
                PhysicalKeyCode::Letter('a')
            )
        );
        assert_eq!(
            normalized_key_identity(u32::from(b'7')),
            (
                LogicalKey::Character("7".to_owned()),
                PhysicalKeyCode::Digit(7)
            )
        );
    }

    #[test]
    fn system_menu_commands_preserve_stable_ids_and_reject_reserved_values() {
        assert_eq!(native_menu_id(MenuCommandId(0x1f00)).unwrap(), 0x1f00);
        for invalid in [0, 0xf000, u16::MAX] {
            assert!(matches!(
                native_menu_id(MenuCommandId(invalid)),
                Err(ControlWindowError::Failed { code, .. })
                    if code == "control_window_invalid_system_menu_command"
            ));
        }
    }
}
