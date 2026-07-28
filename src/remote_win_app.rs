use std::{
    ffi::c_void,
    mem,
    process::Command,
    ptr, thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use unicode_width::UnicodeWidthChar;
use windows_sys::Win32::{
    Foundation::{COLORREF, GlobalFree, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
        DEFAULT_CHARSET, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteObject,
        DrawTextW, EndPaint, FF_MODERN, FIXED_PITCH, FW_NORMAL, FillRect, FrameRect, GetDC,
        GetTextMetricsW, HDC, HFONT, HGDIOBJ, LOGPIXELSY, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
        ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT,
        UpdateWindow,
    },
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        LibraryLoader::GetModuleHandleW,
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
    UI::{
        Input::KeyboardAndMouse::{
            GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_DOWN,
            VK_END, VK_ESCAPE, VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9,
            VK_F10, VK_F11, VK_F12, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_UP,
        },
        WindowsAndMessaging::{
            CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
            DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN, EnableMenuItem,
            GWLP_USERDATA, GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW,
            GetSystemMenu, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, IDC_ARROW,
            IDC_SIZEWE, InsertMenuW, LoadCursorW, LoadIconW, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED,
            MF_SEPARATOR, MF_STRING, MSG, MoveWindow, PostQuitMessage, RegisterClassW, SC_CLOSE,
            SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SWP_SHOWWINDOW, SendMessageW, SetCursor, SetForegroundWindow, SetTimer,
            SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage,
            WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_COPY, WM_CREATE, WM_DESTROY,
            WM_ERASEBKGND, WM_INITMENUPOPUP, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_PASTE, WM_SETCURSOR,
            WM_SETFOCUS, WM_SIZE, WM_SYSCOMMAND, WM_TIMER, WNDCLASSW, WS_BORDER, WS_CHILD,
            WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
};

use crate::{
    client::{ipc_address, ipc_socket_addr},
    commands::tmux_key_bytes,
    settings::{AppConfig, clamp_tabs_width, load_config, save_config},
    theme::{Rgb, ThemeId, ThemePalette},
    ui_bridge::{
        UI_TAB_NOTE_MAX_BYTES, UI_TAB_TITLE_MAX_BYTES, UiCellStyle, UiColor, UiScreenSnapshot,
        UiTabBootstrap,
    },
    ui_client::{UiClientModel, tab_by_id},
    ui_geometry::{
        PixelRect, TERMINAL_SCROLLBAR_WIDTH, TerminalScrollbarGeometry, WorkspaceLayout,
        WorkspaceLayoutInput, scrollback_for_thumb_top, tabs_width_from_drag,
        terminal_scrollbar_geometry, workspace_layout,
    },
};

const TIMER_ID: usize = 1;
const EDIT_ID: usize = 2101;
const SEND_ID: usize = 2102;
const NEW_ID: usize = 2103;
const TABS_ID: usize = 2104;
const TAB_TITLE_EDIT_ID: usize = 2105;
const TAB_NOTE_EDIT_ID: usize = 2106;
const TAB_SAVE_ID: usize = 2107;
const TAB_CANCEL_ID: usize = 2108;
const CLOSE_KEEP_ID: usize = 2109;
const CLOSE_STOP_ID: usize = 2110;
const CLOSE_CANCEL_ID: usize = 2111;
const SETTINGS_ID: usize = 2112;
const SETTINGS_FONT_ID: usize = 2113;
const SETTINGS_SIZE_ID: usize = 2114;
const SETTINGS_DARK_ID: usize = 2115;
const SETTINGS_LIGHT_ID: usize = 2116;
const SETTINGS_APPLY_ID: usize = 2117;
const SETTINGS_CANCEL_ID: usize = 2118;
const SYSTEM_MENU_COPY_ID: usize = 0x1f00;
const SYSTEM_MENU_PASTE_ID: usize = 0x1f10;
const CLIPBOARD_UNICODE_TEXT: u32 = 13;
const TERMINAL_PASTE_LIMIT_BYTES: usize = 256 * 1024;
const WM_APP_AUTOMATION_SHORTCUT: u32 = 0x8000 + 2;
const WM_APP_FOCUS_QUERY: u32 = 0x8000 + 3;
const SIDEBAR_ROW_HEIGHT: i32 = 44;
const TOOLBAR_HEIGHT: i32 = 44;
const STATUS_HEIGHT: i32 = 26;
const COMPOSER_HEIGHT: i32 = 104;
const MARGIN: i32 = 6;
const RECONNECT_INTERVAL: Duration = Duration::from_millis(500);
const START_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) fn run_remote_gui(no_activate: bool) -> Result<()> {
    let client_id = format!(
        "agenterm-gui:{}:{}",
        std::process::id(),
        crate::client::unix_time_ms()
    );
    let client = connect_or_start_server(&client_id)?;
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        anyhow::bail!("GetModuleHandleW failed");
    }
    let class_name = wide("AgenTermRemoteUiClass");
    let mut window_class: WNDCLASSW = unsafe { mem::zeroed() };
    window_class.style = CS_HREDRAW | CS_VREDRAW;
    window_class.lpfnWndProc = Some(window_proc);
    window_class.hInstance = instance as HINSTANCE;
    window_class.hCursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
    window_class.hIcon =
        unsafe { LoadIconW(instance as HINSTANCE, ptr::without_provenance::<u16>(1)) };
    window_class.lpszClassName = class_name.as_ptr();
    if unsafe { RegisterClassW(&window_class) } == 0 {
        anyhow::bail!("RegisterClassW failed for replaceable UI");
    }

    let title = wide(&format!(
        "AgenTerm-{}:{}",
        env!("CARGO_PKG_VERSION"),
        ipc_socket_addr()?.port()
    ));
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1180,
            760,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    if window.is_null() {
        anyhow::bail!("CreateWindowExW failed for replaceable UI");
    }
    install_system_menu(window)?;
    let edit = unsafe {
        CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_TABSTOP
                | WS_VSCROLL
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_WANTRETURN as u32,
            0,
            0,
            100,
            60,
            window,
            EDIT_ID as *mut c_void,
            instance,
            ptr::null(),
        )
    };
    let send = create_button(window, instance, SEND_ID, "Send");
    let new_tab = create_button(window, instance, NEW_ID, "New");
    let tabs = create_button(window, instance, TABS_ID, "Tabs");
    let settings = create_button(window, instance, SETTINGS_ID, "Settings");
    let tab_title_edit = create_hidden_edit(window, instance, TAB_TITLE_EDIT_ID);
    let tab_note_edit = create_hidden_edit(window, instance, TAB_NOTE_EDIT_ID);
    let tab_save = create_hidden_button(window, instance, TAB_SAVE_ID, "Save");
    let tab_cancel = create_hidden_button(window, instance, TAB_CANCEL_ID, "Cancel");
    let close_keep = create_hidden_button(window, instance, CLOSE_KEEP_ID, "Keep Server Running");
    let close_stop = create_hidden_button(window, instance, CLOSE_STOP_ID, "Stop Server && Exit");
    let close_cancel = create_hidden_button(window, instance, CLOSE_CANCEL_ID, "Cancel");
    let settings_font = create_hidden_edit(window, instance, SETTINGS_FONT_ID);
    let settings_size = create_hidden_edit(window, instance, SETTINGS_SIZE_ID);
    let settings_dark = create_hidden_button(window, instance, SETTINGS_DARK_ID, "Dark");
    let settings_light = create_hidden_button(window, instance, SETTINGS_LIGHT_ID, "Light");
    let settings_apply = create_hidden_button(window, instance, SETTINGS_APPLY_ID, "Apply");
    let settings_cancel = create_hidden_button(window, instance, SETTINGS_CANCEL_ID, "Cancel");
    if edit.is_null()
        || send.is_null()
        || new_tab.is_null()
        || tabs.is_null()
        || settings.is_null()
        || tab_title_edit.is_null()
        || tab_note_edit.is_null()
        || tab_save.is_null()
        || tab_cancel.is_null()
        || close_keep.is_null()
        || close_stop.is_null()
        || close_cancel.is_null()
        || settings_font.is_null()
        || settings_size.is_null()
        || settings_dark.is_null()
        || settings_light.is_null()
        || settings_apply.is_null()
        || settings_cancel.is_null()
    {
        unsafe { DestroyWindow(window) };
        anyhow::bail!("failed to create replaceable UI controls");
    }

    let state = Box::new(RemoteWindowState::new(
        window,
        RemoteControls {
            edit,
            send,
            new_tab,
            tabs_button: tabs,
            settings,
            tab_title_edit,
            tab_note_edit,
            tab_save,
            tab_cancel,
            close_keep,
            close_stop,
            close_cancel,
            settings_font,
            settings_size,
            settings_dark,
            settings_light,
            settings_apply,
            settings_cancel,
        },
        client_id,
        client,
    )?);
    unsafe {
        SetWindowLongPtrW(window, GWLP_USERDATA, Box::into_raw(state) as isize);
        SetTimer(window, TIMER_ID, 100, None);
    }
    if let Some(state) = state_mut(window) {
        state.layout();
        state.load_composer();
        state.resize_active_terminal();
    }
    unsafe {
        if no_activate {
            show_without_activation(window);
        } else {
            ShowWindow(window, SW_SHOW);
            SetForegroundWindow(window);
        }
        UpdateWindow(window);
    }

    let mut message: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        if message.message == WM_KEYDOWN
            && let Some(state) = state_mut(window)
            && state.handle_keyboard_navigation(message.wParam as u32)
        {
            unsafe { windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0) };
            continue;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn connect_or_start_server(client_id: &str) -> Result<UiClientModel> {
    match UiClientModel::connect(client_id.to_owned()) {
        Ok(client) => return Ok(client),
        Err(error) => {
            if std::net::TcpStream::connect_timeout(&ipc_socket_addr()?, Duration::from_millis(100))
                .is_ok()
            {
                return Err(error).context("running server rejected replaceable UI");
            }
        }
    }
    start_server_process()?;
    let deadline = Instant::now() + START_TIMEOUT;
    let mut last_error = None;
    while Instant::now() < deadline {
        match UiClientModel::connect(client_id.to_owned()) {
            Ok(client) => return Ok(client),
            Err(error) => last_error = Some(error),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("server did not become ready")))
        .context("could not start independent AgenTerm server")
}

fn start_server_process() -> Result<()> {
    let current = std::env::current_exe().context("could not locate agenterm.exe")?;
    let server = current.with_file_name("agenterm-server.exe");
    if !server.is_file() {
        anyhow::bail!(
            "independent server executable is not beside the GUI: {}",
            server.display()
        );
    }
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new(server)
        .arg("--address")
        .arg(ipc_address())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("failed to launch independent AgenTerm server")?;
    Ok(())
}

struct RemoteControls {
    edit: HWND,
    send: HWND,
    new_tab: HWND,
    tabs_button: HWND,
    settings: HWND,
    tab_title_edit: HWND,
    tab_note_edit: HWND,
    tab_save: HWND,
    tab_cancel: HWND,
    close_keep: HWND,
    close_stop: HWND,
    close_cancel: HWND,
    settings_font: HWND,
    settings_size: HWND,
    settings_dark: HWND,
    settings_light: HWND,
    settings_apply: HWND,
    settings_cancel: HWND,
}

#[derive(Clone, Copy)]
enum RemoteCloseChoice {
    KeepServerRunning,
    StopServerAndExit,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemoteFocusSurface {
    Terminal,
    Composer,
    Tabs,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RemoteTerminalPoint {
    row: u32,
    column: u32,
}

#[derive(Clone, Debug)]
struct RemoteTerminalSelection {
    tab_id: String,
    generation: u64,
    anchor: RemoteTerminalPoint,
    active: RemoteTerminalPoint,
    dragging: bool,
}

#[derive(Clone, Copy)]
struct RemoteScrollDrag {
    thumb_grab_offset: i32,
}

impl RemoteTerminalSelection {
    fn bounds(&self) -> (RemoteTerminalPoint, RemoteTerminalPoint) {
        if self.anchor <= self.active {
            (self.anchor, self.active)
        } else {
            (self.active, self.anchor)
        }
    }

    fn is_empty(&self) -> bool {
        self.anchor == self.active
    }
}

fn remote_surface_navigation(
    source: RemoteFocusSurface,
    control: bool,
    shift: bool,
    alt: bool,
    key: u32,
) -> Option<RemoteFocusSurface> {
    if !control || shift || alt {
        return None;
    }
    match (source, key) {
        (RemoteFocusSurface::Terminal, key) if key == u32::from(VK_DOWN) => {
            Some(RemoteFocusSurface::Composer)
        }
        (RemoteFocusSurface::Composer, key) if key == u32::from(VK_UP) => {
            Some(RemoteFocusSurface::Terminal)
        }
        (RemoteFocusSurface::Terminal, key) if key == u32::from(VK_LEFT) => {
            Some(RemoteFocusSurface::Tabs)
        }
        (RemoteFocusSurface::Tabs, key) if key == u32::from(VK_RIGHT) => {
            Some(RemoteFocusSurface::Terminal)
        }
        _ => None,
    }
}

struct RemoteWindowState {
    window: HWND,
    edit: HWND,
    send: HWND,
    new_tab: HWND,
    tabs_button: HWND,
    settings: HWND,
    tab_title_edit: HWND,
    tab_note_edit: HWND,
    tab_save: HWND,
    tab_cancel: HWND,
    close_keep: HWND,
    close_stop: HWND,
    close_cancel: HWND,
    settings_font: HWND,
    settings_size: HWND,
    settings_dark: HWND,
    settings_light: HWND,
    settings_apply: HWND,
    settings_cancel: HWND,
    client_id: String,
    client: Option<UiClientModel>,
    reconnect_after: Instant,
    last_error: Option<String>,
    tabs_visible: bool,
    config: AppConfig,
    font: HFONT,
    cell_width: i32,
    cell_height: i32,
    pending_high_surrogate: Option<u16>,
    last_active_id: Option<String>,
    tabs_resize_dragging: bool,
    editing_tab_id: Option<String>,
    window_close_pending: bool,
    settings_open: bool,
    settings_theme_draft: ThemeId,
    terminal_selection: Option<RemoteTerminalSelection>,
    scroll_drag: Option<RemoteScrollDrag>,
    focus_surface: RemoteFocusSurface,
}

impl RemoteWindowState {
    fn new(
        window: HWND,
        controls: RemoteControls,
        client_id: String,
        client: UiClientModel,
    ) -> Result<Self> {
        let RemoteControls {
            edit,
            send,
            new_tab,
            tabs_button,
            settings,
            tab_title_edit,
            tab_note_edit,
            tab_save,
            tab_cancel,
            close_keep,
            close_stop,
            close_cancel,
            settings_font,
            settings_size,
            settings_dark,
            settings_light,
            settings_apply,
            settings_cancel,
        } = controls;
        let config = load_config();
        let settings_theme_draft = config.color_theme;
        let (font, cell_width, cell_height) = create_terminal_font(window, &config)?;
        let last_active_id = client.snapshot().active_tab_id.clone();
        Ok(Self {
            window,
            edit,
            send,
            new_tab,
            tabs_button,
            settings,
            tab_title_edit,
            tab_note_edit,
            tab_save,
            tab_cancel,
            close_keep,
            close_stop,
            close_cancel,
            settings_font,
            settings_size,
            settings_dark,
            settings_light,
            settings_apply,
            settings_cancel,
            client_id,
            client: Some(client),
            reconnect_after: Instant::now(),
            last_error: None,
            tabs_visible: config.tabs_visible,
            config,
            font,
            cell_width,
            cell_height,
            pending_high_surrogate: None,
            last_active_id,
            tabs_resize_dragging: false,
            editing_tab_id: None,
            window_close_pending: false,
            settings_open: false,
            settings_theme_draft,
            terminal_selection: None,
            scroll_drag: None,
            focus_surface: RemoteFocusSurface::Terminal,
        })
    }

    fn tick(&mut self) -> bool {
        let result = self
            .client
            .as_mut()
            .context("replaceable UI is disconnected")
            .and_then(|client| {
                let heartbeat = client.heartbeat_if_due()?;
                let delta = client.poll_deltas()?;
                Ok(heartbeat || delta)
            });
        match result {
            Ok(changed) => {
                self.reconcile_tab_editor();
                self.reconcile_terminal_selection();
                let active = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                if active != self.last_active_id {
                    self.last_active_id = active;
                    self.load_composer();
                }
                self.last_error = None;
                changed
            }
            Err(error) => {
                self.last_error = Some(format!("{error:#}"));
                if Instant::now() >= self.reconnect_after {
                    self.reconnect_after = Instant::now() + RECONNECT_INTERVAL;
                    match UiClientModel::connect(self.client_id.clone()) {
                        Ok(client) => {
                            self.client = Some(client);
                            self.editing_tab_id = None;
                            self.terminal_selection = None;
                            self.show_tab_editor(false);
                            self.last_active_id = self
                                .client
                                .as_ref()
                                .and_then(|client| client.snapshot().active_tab_id.clone());
                            self.last_error = None;
                            self.load_composer();
                            self.resize_active_terminal();
                            return true;
                        }
                        Err(reconnect_error) => {
                            self.last_error = Some(format!("Disconnected: {reconnect_error:#}"));
                        }
                    }
                }
                true
            }
        }
    }

    fn active_tab(&self) -> Option<&UiTabBootstrap> {
        let client = self.client.as_ref()?;
        let active = client.snapshot().active_tab_id.as_deref()?;
        tab_by_id(client.snapshot(), active)
    }

    fn reconcile_tab_editor(&mut self) {
        let still_exists = self.editing_tab_id.as_deref().is_some_and(|id| {
            self.client
                .as_ref()
                .is_some_and(|client| client.snapshot().tabs.iter().any(|tab| tab.id == id))
        });
        if self.editing_tab_id.is_some() && !still_exists {
            self.editing_tab_id = None;
            self.show_tab_editor(false);
        }
    }

    fn reconcile_terminal_selection(&mut self) {
        let still_current = self.terminal_selection.as_ref().is_some_and(|selection| {
            self.active_tab().is_some_and(|tab| {
                tab.id == selection.tab_id && tab.screen.generation == selection.generation
            })
        });
        if self.terminal_selection.is_some() && !still_current {
            self.terminal_selection = None;
        }
    }

    fn workspace_geometry(&self) -> WorkspaceLayout {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        workspace_layout(WorkspaceLayoutInput {
            client_width: client.right,
            client_height: client.bottom,
            tabs_visible: self.tabs_visible,
            configured_tabs_width: i32::from(self.config.tabs_width),
            composer_height: COMPOSER_HEIGHT,
            status_height: STATUS_HEIGHT,
        })
    }

    fn layout_rects(&self) -> (RECT, RECT, RECT, RECT) {
        let geometry = self.workspace_geometry();
        (
            win_rect(geometry.sidebar),
            win_rect(geometry.terminal),
            win_rect(geometry.composer),
            win_rect(geometry.status),
        )
    }

    fn layout(&mut self) {
        let (sidebar, _, composer, _) = self.layout_rects();
        let toolbar_top = sidebar.bottom.saturating_sub(TOOLBAR_HEIGHT);
        unsafe {
            MoveWindow(
                self.new_tab,
                sidebar.left + MARGIN,
                toolbar_top + MARGIN,
                72,
                32,
                1,
            );
            MoveWindow(
                self.tabs_button,
                sidebar.left + 84,
                toolbar_top + MARGIN,
                66,
                32,
                1,
            );
            MoveWindow(
                self.settings,
                sidebar.left + 156,
                toolbar_top + MARGIN,
                86,
                32,
                1,
            );
            let send_width = 76;
            MoveWindow(
                self.edit,
                composer.left + MARGIN,
                composer.top + 26,
                (composer.right - composer.left - send_width - MARGIN * 3).max(80),
                (composer.bottom - composer.top - 34).max(30),
                1,
            );
            MoveWindow(
                self.send,
                composer.right - send_width - MARGIN,
                composer.top + 26,
                send_width,
                34,
                1,
            );
            ShowWindow(
                self.new_tab,
                if self.tabs_visible && !self.window_close_pending && !self.settings_open {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
        }
        self.layout_tab_editor();
        self.layout_close_controls();
        self.layout_settings_controls();
    }

    fn layout_tab_editor(&self) {
        let Some(tab_id) = self.editing_tab_id.as_deref() else {
            self.show_tab_editor(false);
            return;
        };
        let Some(client) = &self.client else {
            self.show_tab_editor(false);
            return;
        };
        let Some(position) = client
            .snapshot()
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id)
        else {
            self.show_tab_editor(false);
            return;
        };
        let (sidebar, _, _, _) = self.layout_rects();
        let top = i32::try_from(position)
            .unwrap_or(i32::MAX)
            .saturating_mul(SIDEBAR_ROW_HEIGHT)
            + 4;
        let actions_width = 88;
        let left = sidebar.left + 6;
        let edit_width = (sidebar.right - left - actions_width - 8).max(44);
        unsafe {
            MoveWindow(self.tab_title_edit, left, top, edit_width, 18, 1);
            MoveWindow(self.tab_note_edit, left, top + 20, edit_width, 18, 1);
            MoveWindow(self.tab_save, left + edit_width + 4, top, 40, 38, 1);
            MoveWindow(self.tab_cancel, left + edit_width + 46, top, 42, 38, 1);
        }
        self.show_tab_editor(self.tabs_visible && !self.window_close_pending);
    }

    fn show_tab_editor(&self, visible: bool) {
        let command = if visible { SW_SHOW } else { SW_HIDE };
        unsafe {
            ShowWindow(self.tab_title_edit, command);
            ShowWindow(self.tab_note_edit, command);
            ShowWindow(self.tab_save, command);
            ShowWindow(self.tab_cancel, command);
        }
    }

    fn close_modal_geometry(&self) -> (RECT, [RECT; 3]) {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let width = (client.right - 32).clamp(360, 620);
        let height = 230;
        let left = ((client.right - width) / 2).max(0);
        let top = ((client.bottom - height) / 2).max(0);
        let modal = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let gap = 8;
        let button_left = left + 20;
        let button_width = ((width - 40 - gap * 2) / 3).max(90);
        let button_top = top + 158;
        let button_bottom = button_top + 40;
        let keep = RECT {
            left: button_left,
            top: button_top,
            right: button_left + button_width,
            bottom: button_bottom,
        };
        let stop = RECT {
            left: keep.right + gap,
            top: button_top,
            right: keep.right + gap + button_width,
            bottom: button_bottom,
        };
        let cancel = RECT {
            left: stop.right + gap,
            top: button_top,
            right: stop.right + gap + button_width,
            bottom: button_bottom,
        };
        (modal, [keep, stop, cancel])
    }

    fn layout_close_controls(&self) {
        let (_, buttons) = self.close_modal_geometry();
        for (control, rect) in [
            (self.close_keep, buttons[0]),
            (self.close_stop, buttons[1]),
            (self.close_cancel, buttons[2]),
        ] {
            unsafe {
                MoveWindow(
                    control,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    1,
                );
            }
        }
        self.show_close_controls(self.window_close_pending);
    }

    fn show_close_controls(&self, visible: bool) {
        let command = if visible { SW_SHOW } else { SW_HIDE };
        unsafe {
            ShowWindow(self.close_keep, command);
            ShowWindow(self.close_stop, command);
            ShowWindow(self.close_cancel, command);
        }
    }

    fn show_workspace_controls(&self, visible: bool) {
        let command = if visible { SW_SHOW } else { SW_HIDE };
        unsafe {
            ShowWindow(self.edit, command);
            ShowWindow(self.send, command);
            ShowWindow(self.tabs_button, command);
            ShowWindow(self.settings, command);
            ShowWindow(
                self.new_tab,
                if visible && self.tabs_visible {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
        }
        self.show_tab_editor(visible && self.tabs_visible && self.editing_tab_id.is_some());
    }

    fn settings_modal_geometry(&self) -> (RECT, [RECT; 6]) {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let width = (client.right - 32).clamp(420, 540);
        let height = 330;
        let left = ((client.right - width) / 2).max(0);
        let top = ((client.bottom - height) / 2).max(0);
        let modal = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let font = RECT {
            left: left + 32,
            top: top + 92,
            right: left + width - 126,
            bottom: top + 124,
        };
        let size = RECT {
            left: left + width - 110,
            top: top + 92,
            right: left + width - 32,
            bottom: top + 124,
        };
        let dark = RECT {
            left: left + 32,
            top: top + 180,
            right: left + 178,
            bottom: top + 214,
        };
        let light = RECT {
            left: left + 190,
            top: top + 180,
            right: left + 336,
            bottom: top + 214,
        };
        let apply = RECT {
            left: left + width - 126,
            top: top + 266,
            right: left + width - 32,
            bottom: top + 302,
        };
        let cancel = RECT {
            left: apply.left - 106,
            top: top + 266,
            right: apply.left - 12,
            bottom: top + 302,
        };
        (modal, [font, size, dark, light, apply, cancel])
    }

    fn layout_settings_controls(&self) {
        let (_, controls) = self.settings_modal_geometry();
        for (control, rect) in [
            (self.settings_font, controls[0]),
            (self.settings_size, controls[1]),
            (self.settings_dark, controls[2]),
            (self.settings_light, controls[3]),
            (self.settings_apply, controls[4]),
            (self.settings_cancel, controls[5]),
        ] {
            unsafe {
                MoveWindow(
                    control,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    1,
                );
            }
        }
        self.show_settings_controls(self.settings_open);
    }

    fn show_settings_controls(&self, visible: bool) {
        let command = if visible { SW_SHOW } else { SW_HIDE };
        unsafe {
            ShowWindow(self.settings_font, command);
            ShowWindow(self.settings_size, command);
            ShowWindow(self.settings_dark, command);
            ShowWindow(self.settings_light, command);
            ShowWindow(self.settings_apply, command);
            ShowWindow(self.settings_cancel, command);
        }
    }

    fn resize_active_terminal(&mut self) {
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return;
        };
        let (_, terminal, _, _) = self.layout_rects();
        let rows = ((terminal.bottom - terminal.top) / self.cell_height)
            .clamp(1, 512)
            .try_into()
            .unwrap_or(1);
        let columns = ((terminal.right - terminal.left - TERMINAL_SCROLLBAR_WIDTH)
            / self.cell_width)
            .clamp(1, 512)
            .try_into()
            .unwrap_or(1);
        if let Some(client) = self.client.as_mut()
            && let Err(error) = client.resize(&tab_id, rows, columns)
        {
            self.last_error = Some(format!("PTY resize failed: {error:#}"));
        }
    }

    fn load_composer(&self) {
        let text = self
            .active_tab()
            .and_then(|tab| tab.composer.text.as_deref())
            .unwrap_or_default();
        unsafe { SetWindowTextW(self.edit, wide(text).as_ptr()) };
    }

    fn sync_composer(&mut self) -> Result<()> {
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return Ok(());
        };
        let text = window_text(self.edit);
        self.client
            .as_mut()
            .context("UI is disconnected")?
            .run_control(vec![
                "set-composer".to_owned(),
                "-t".to_owned(),
                tab_id,
                "--".to_owned(),
                text,
            ])?;
        Ok(())
    }

    fn send_composer(&mut self) {
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return;
        };
        let text = window_text(self.edit);
        let result =
            self.client
                .as_mut()
                .context("UI is disconnected")
                .and_then(|client| -> Result<()> {
                    client.run_control(vec![
                        "set-composer".to_owned(),
                        "-t".to_owned(),
                        tab_id.clone(),
                        "--".to_owned(),
                        text,
                    ])?;
                    client.run_control(vec![
                        "send-composer".to_owned(),
                        "-t".to_owned(),
                        tab_id,
                    ])?;
                    Ok(())
                });
        match result {
            Ok(()) => unsafe {
                SetWindowTextW(self.edit, wide("").as_ptr());
            },
            Err(error) => self.last_error = Some(format!("Composer send failed: {error:#}")),
        }
    }

    fn new_tab(&mut self) {
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        let result =
            self.client
                .as_mut()
                .context("UI is disconnected")
                .and_then(|client| -> Result<()> {
                    client.run_control(vec![
                        "new-window".to_owned(),
                        "-P".to_owned(),
                        "-F".to_owned(),
                        "#{window_id}".to_owned(),
                    ])?;
                    client.poll_deltas()?;
                    Ok(())
                });
        if let Err(error) = result {
            self.last_error = Some(format!("New tab failed: {error:#}"));
        }
    }

    fn select_tab_at(&mut self, y: i32) {
        if !self.tabs_visible || y < 0 {
            return;
        }
        let index = usize::try_from(y / SIDEBAR_ROW_HEIGHT).unwrap_or(usize::MAX);
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().tabs.get(index))
            .map(|tab| tab.id.clone())
        else {
            return;
        };
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.select_tab(&tab_id)?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Tab selection failed: {error:#}"));
        } else {
            self.focus_surface = RemoteFocusSurface::Tabs;
            self.last_active_id = Some(tab_id);
            self.load_composer();
            self.resize_active_terminal();
        }
    }

    fn tab_at_y(&self, y: i32) -> Option<&UiTabBootstrap> {
        if !self.tabs_visible || y < 0 {
            return None;
        }
        let index = usize::try_from(y / SIDEBAR_ROW_HEIGHT).ok()?;
        self.client.as_ref()?.snapshot().tabs.get(index)
    }

    fn tab_edit_action_contains(&self, x: i32, y: i32) -> bool {
        if self.tab_at_y(y).is_none() {
            return false;
        }
        let sidebar = self.layout_rects().0;
        x >= sidebar.right - 52 && x < sidebar.right - 6
    }

    fn begin_tab_edit_at(&mut self, y: i32) {
        let Some(tab) = self.tab_at_y(y).cloned() else {
            return;
        };
        self.focus_surface = RemoteFocusSurface::Tabs;
        self.editing_tab_id = Some(tab.id);
        unsafe {
            SetWindowTextW(self.tab_title_edit, wide(&tab.title).as_ptr());
            SetWindowTextW(self.tab_note_edit, wide(&tab.note).as_ptr());
        }
        self.layout_tab_editor();
        unsafe { SetFocus(self.tab_title_edit) };
    }

    fn finish_tab_edit(&mut self, save: bool) {
        let Some(tab_id) = self.editing_tab_id.clone() else {
            return;
        };
        if save {
            let title = window_text(self.tab_title_edit);
            let note = window_text(self.tab_note_edit);
            if title.trim().is_empty() {
                self.last_error = Some("Tab title cannot be empty".to_owned());
                return;
            }
            if title.len() > UI_TAB_TITLE_MAX_BYTES {
                self.last_error = Some(format!(
                    "Tab title exceeds the {UI_TAB_TITLE_MAX_BYTES}-byte UI limit"
                ));
                return;
            }
            if note.len() > UI_TAB_NOTE_MAX_BYTES {
                self.last_error = Some(format!(
                    "Tab note exceeds the {UI_TAB_NOTE_MAX_BYTES}-byte UI limit"
                ));
                return;
            }
            let result = self
                .client
                .as_mut()
                .context("UI is disconnected")
                .and_then(|client| {
                    client.run_control(vec![
                        "rename-window".to_owned(),
                        "-t".to_owned(),
                        tab_id.clone(),
                        title,
                    ])?;
                    client.run_control(vec![
                        "set-tab-note".to_owned(),
                        "-t".to_owned(),
                        tab_id,
                        note,
                    ])?;
                    client.poll_deltas()?;
                    Ok(())
                });
            if let Err(error) = result {
                self.last_error = Some(format!("Tab edit failed: {error:#}"));
                return;
            }
        }
        self.editing_tab_id = None;
        self.show_tab_editor(false);
        self.focus_surface = RemoteFocusSurface::Tabs;
        unsafe { SetFocus(self.window) };
    }

    fn open_settings(&mut self) {
        if self.settings_open || self.window_close_pending {
            return;
        }
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.finish_tab_edit(false);
        self.settings_open = true;
        self.settings_theme_draft = self.config.color_theme;
        unsafe {
            SetWindowTextW(
                self.settings_font,
                wide(&self.config.terminal_font_family).as_ptr(),
            );
            SetWindowTextW(
                self.settings_size,
                wide(&self.config.terminal_font_size.to_string()).as_ptr(),
            );
        }
        self.refresh_settings_theme_controls();
        self.show_workspace_controls(false);
        self.layout_settings_controls();
        unsafe { SetFocus(self.settings_font) };
    }

    fn preview_settings_theme(&mut self, theme: ThemeId) {
        if !self.settings_open {
            return;
        }
        self.settings_theme_draft = theme;
        self.refresh_settings_theme_controls();
    }

    fn refresh_settings_theme_controls(&self) {
        for (theme, control) in [
            (ThemeId::Dark, self.settings_dark),
            (ThemeId::Light, self.settings_light),
        ] {
            let state = if theme == self.settings_theme_draft {
                "Selected"
            } else {
                "Preview"
            };
            unsafe {
                SetWindowTextW(
                    control,
                    wide(&format!("{} · {state}", theme.label())).as_ptr(),
                );
            }
        }
    }

    fn apply_settings(&mut self) -> Result<()> {
        let family = window_text(self.settings_font).trim().to_owned();
        let size = window_text(self.settings_size)
            .trim()
            .parse::<u16>()
            .context("font size must be a number from 8 to 36")?;
        if family.is_empty() || family.len() > 256 || !(8..=36).contains(&size) {
            anyhow::bail!("font family is required (maximum 256 bytes) and size must be 8 to 36");
        }
        let mut next = self.config.clone();
        next.terminal_font_family = family;
        next.terminal_font_size = size;
        next.color_theme = self.settings_theme_draft;
        let (font, cell_width, cell_height) = create_terminal_font(self.window, &next)?;
        if let Err(error) = save_config(&next) {
            unsafe { DeleteObject(font as HGDIOBJ) };
            return Err(error).context("could not save settings");
        }
        unsafe { DeleteObject(self.font as HGDIOBJ) };
        self.font = font;
        self.cell_width = cell_width;
        self.cell_height = cell_height;
        self.config = next;
        self.last_error = None;
        Ok(())
    }

    fn finish_settings(&mut self, apply: bool) {
        if !self.settings_open {
            return;
        }
        if apply && let Err(error) = self.apply_settings() {
            self.last_error = Some(format!("Settings apply failed: {error:#}"));
            return;
        }
        self.settings_open = false;
        self.settings_theme_draft = self.config.color_theme;
        self.show_settings_controls(false);
        self.show_workspace_controls(true);
        self.focus_surface = RemoteFocusSurface::Terminal;
        self.layout();
        self.load_composer();
        self.resize_active_terminal();
        unsafe { SetFocus(self.window) };
    }

    fn request_window_close(&mut self) {
        if self.window_close_pending {
            return;
        }
        self.cancel_terminal_selection();
        if self.settings_open {
            self.finish_settings(false);
        }
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.finish_tab_edit(false);
        self.window_close_pending = true;
        self.show_workspace_controls(false);
        self.layout_close_controls();
        unsafe { SetFocus(self.window) };
    }

    fn finish_window_close(&mut self, choice: RemoteCloseChoice) {
        if !self.window_close_pending {
            return;
        }
        match choice {
            RemoteCloseChoice::KeepServerRunning => {
                self.window_close_pending = false;
                unsafe { DestroyWindow(self.window) };
            }
            RemoteCloseChoice::StopServerAndExit => {
                let result =
                    self.client
                        .as_mut()
                        .context("UI is disconnected")
                        .and_then(|client| {
                            client.run_control(vec!["shutdown".to_owned()])?;
                            Ok(())
                        });
                if let Err(error) = result {
                    self.last_error = Some(format!("Server shutdown failed: {error:#}"));
                    self.window_close_pending = false;
                    self.show_close_controls(false);
                    self.show_workspace_controls(true);
                    unsafe { SetFocus(self.window) };
                    return;
                }
                self.window_close_pending = false;
                unsafe { DestroyWindow(self.window) };
            }
            RemoteCloseChoice::Cancel => {
                self.window_close_pending = false;
                self.show_close_controls(false);
                self.show_workspace_controls(true);
                self.focus_surface = RemoteFocusSurface::Terminal;
                unsafe { SetFocus(self.window) };
            }
        }
    }

    fn terminal_point(&self, x: i32, y: i32, clamp: bool) -> Option<RemoteTerminalPoint> {
        let (_, terminal, _, _) = self.layout_rects();
        let tab = self.active_tab()?;
        if !clamp
            && (x < terminal.left
                || x >= terminal.right - TERMINAL_SCROLLBAR_WIDTH
                || y < terminal.top
                || y >= terminal.bottom)
        {
            return None;
        }
        let local_x = (x - terminal.left).clamp(
            0,
            (terminal.right - terminal.left - TERMINAL_SCROLLBAR_WIDTH - 1).max(0),
        );
        let local_y = (y - terminal.top).clamp(0, (terminal.bottom - terminal.top - 1).max(0));
        Some(RemoteTerminalPoint {
            row: u32::try_from(local_y / self.cell_height)
                .unwrap_or_default()
                .min(tab.screen.rows.saturating_sub(1)),
            column: u32::try_from(local_x / self.cell_width)
                .unwrap_or_default()
                .min(tab.screen.columns.saturating_sub(1)),
        })
    }

    fn begin_terminal_selection(&mut self, x: i32, y: i32) -> bool {
        let Some(point) = self.terminal_point(x, y, false) else {
            return false;
        };
        let Some(tab) = self.active_tab() else {
            return false;
        };
        self.terminal_selection = Some(RemoteTerminalSelection {
            tab_id: tab.id.clone(),
            generation: tab.screen.generation,
            anchor: point,
            active: point,
            dragging: true,
        });
        self.focus_surface = RemoteFocusSurface::Terminal;
        unsafe {
            SetFocus(self.window);
            SetCapture(self.window);
        }
        true
    }

    fn drag_terminal_selection(&mut self, x: i32, y: i32) -> bool {
        if !self
            .terminal_selection
            .as_ref()
            .is_some_and(|selection| selection.dragging)
        {
            return false;
        }
        let Some(point) = self.terminal_point(x, y, true) else {
            return false;
        };
        if let Some(selection) = self.terminal_selection.as_mut() {
            selection.active = point;
        }
        true
    }

    fn finish_terminal_selection(&mut self, x: i32, y: i32) -> bool {
        if !self.drag_terminal_selection(x, y) {
            return false;
        }
        unsafe { ReleaseCapture() };
        if let Some(selection) = self.terminal_selection.as_mut() {
            selection.dragging = false;
            if selection.is_empty() {
                self.terminal_selection = None;
            }
        }
        true
    }

    fn terminal_selection_capture_lost(&mut self) {
        if let Some(selection) = self.terminal_selection.as_mut()
            && selection.dragging
        {
            selection.dragging = false;
            if selection.is_empty() {
                self.terminal_selection = None;
            }
        }
    }

    fn cancel_terminal_selection(&mut self) {
        if self
            .terminal_selection
            .as_ref()
            .is_some_and(|selection| selection.dragging)
        {
            unsafe { ReleaseCapture() };
        }
        self.terminal_selection = None;
    }

    fn copy_terminal_selection(&mut self) -> Result<()> {
        let selection = self
            .terminal_selection
            .as_ref()
            .context("no terminal selection is active")?;
        if selection.is_empty() {
            anyhow::bail!("terminal selection is empty");
        }
        let tab = self
            .active_tab()
            .filter(|tab| {
                tab.id == selection.tab_id && tab.screen.generation == selection.generation
            })
            .context("terminal selection is stale")?;
        let text = screen_selection_text(&tab.screen, selection);
        if text.is_empty() {
            anyhow::bail!("terminal selection contains no text");
        }
        set_clipboard_text(self.window, &text)?;
        self.last_error = None;
        Ok(())
    }

    fn paste_terminal_clipboard(&mut self) -> Result<()> {
        let text = normalize_terminal_paste(&read_clipboard_text()?);
        if text.is_empty() {
            anyhow::bail!("clipboard text contains no pasteable characters");
        }
        if text.len() > TERMINAL_PASTE_LIMIT_BYTES {
            anyhow::bail!(
                "normalized clipboard text exceeds the {TERMINAL_PASTE_LIMIT_BYTES}-byte limit"
            );
        }
        self.terminal_input(text.as_bytes());
        self.last_error = None;
        Ok(())
    }

    fn is_edit_control(&self, window: HWND) -> bool {
        [
            self.edit,
            self.tab_title_edit,
            self.tab_note_edit,
            self.settings_font,
            self.settings_size,
        ]
        .contains(&window)
    }

    fn current_focus_surface(&self) -> RemoteFocusSurface {
        if unsafe { GetFocus() } == self.edit {
            RemoteFocusSurface::Composer
        } else {
            self.focus_surface
        }
    }

    fn set_focus_surface(&mut self, target: RemoteFocusSurface) -> bool {
        if self.window_close_pending || self.settings_open || self.editing_tab_id.is_some() {
            return false;
        }
        match target {
            RemoteFocusSurface::Terminal => {
                self.focus_surface = RemoteFocusSurface::Terminal;
                unsafe { SetFocus(self.window) };
            }
            RemoteFocusSurface::Composer => {
                if self.active_tab().is_none() {
                    return false;
                }
                self.focus_surface = RemoteFocusSurface::Composer;
                unsafe { SetFocus(self.edit) };
            }
            RemoteFocusSurface::Tabs => {
                if !self.tabs_visible {
                    self.tabs_visible = true;
                    self.config.tabs_visible = true;
                    if let Err(error) = save_config(&self.config) {
                        self.last_error = Some(format!("Tabs visibility save failed: {error:#}"));
                    }
                    self.layout();
                    self.resize_active_terminal();
                }
                self.focus_surface = RemoteFocusSurface::Tabs;
                unsafe { SetFocus(self.window) };
            }
        }
        true
    }

    fn handle_surface_navigation(
        &mut self,
        key: u32,
        control: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        let Some(target) =
            remote_surface_navigation(self.current_focus_surface(), control, shift, alt, key)
        else {
            return false;
        };
        self.set_focus_surface(target)
    }

    fn handle_keyboard_navigation(&mut self, key: u32) -> bool {
        self.handle_surface_navigation(
            key,
            unsafe { GetKeyState(VK_CONTROL as i32) } < 0,
            unsafe { GetKeyState(0x10) } < 0,
            unsafe { GetKeyState(0x12) } < 0,
        )
    }

    fn system_menu_state(&self) -> (bool, bool) {
        let focused = unsafe { GetFocus() };
        if self.is_edit_control(focused) {
            return (true, clipboard_has_unicode_text());
        }
        let terminal_ready = focused == self.window
            && !self.window_close_pending
            && !self.settings_open
            && self.active_tab().is_some_and(|tab| !tab.dead);
        (
            terminal_ready
                && self
                    .terminal_selection
                    .as_ref()
                    .is_some_and(|selection| !selection.is_empty()),
            terminal_ready && clipboard_has_unicode_text(),
        )
    }

    fn refresh_system_menu(&self) {
        let menu = unsafe { GetSystemMenu(self.window, 0) };
        if menu.is_null() {
            return;
        }
        let (copy, paste) = self.system_menu_state();
        unsafe {
            EnableMenuItem(
                menu,
                SYSTEM_MENU_COPY_ID as u32,
                MF_BYCOMMAND | if copy { MF_ENABLED } else { MF_GRAYED },
            );
            EnableMenuItem(
                menu,
                SYSTEM_MENU_PASTE_ID as u32,
                MF_BYCOMMAND | if paste { MF_ENABLED } else { MF_GRAYED },
            );
        }
    }

    fn system_menu_copy(&mut self) {
        let focused = unsafe { GetFocus() };
        if self.is_edit_control(focused) {
            unsafe { SendMessageW(focused, WM_COPY, 0, 0) };
        } else if let Err(error) = self.copy_terminal_selection() {
            self.last_error = Some(format!("Copy failed: {error:#}"));
        }
    }

    fn system_menu_paste(&mut self) {
        let focused = unsafe { GetFocus() };
        if self.is_edit_control(focused) {
            unsafe { SendMessageW(focused, WM_PASTE, 0, 0) };
        } else if let Err(error) = self.paste_terminal_clipboard() {
            self.last_error = Some(format!("Paste failed: {error:#}"));
        }
    }

    fn terminal_input(&mut self, bytes: &[u8]) {
        self.cancel_terminal_selection();
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return;
        };
        if let Some(client) = self.client.as_mut()
            && let Err(error) = client.send_input(&tab_id, bytes)
        {
            self.last_error = Some(format!("Terminal input failed: {error:#}"));
        }
    }

    fn scroll_terminal(&mut self, delta: i32) {
        self.cancel_terminal_selection();
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return;
        };
        let count = usize::try_from(delta.unsigned_abs())
            .unwrap_or(120)
            .div_ceil(120)
            .saturating_mul(3)
            .max(1);
        let action = if delta > 0 { "up" } else { "down" };
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.run_control(vec![
                    "scroll-pane".to_owned(),
                    "-t".to_owned(),
                    tab_id,
                    action.to_owned(),
                    count.to_string(),
                ])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Terminal scroll failed: {error:#}"));
        }
    }

    fn scrollbar_state(&self) -> Option<(TerminalScrollbarGeometry, usize, usize)> {
        let tab = self.active_tab()?;
        let geometry = terminal_scrollbar_geometry(
            self.workspace_geometry().terminal,
            usize::try_from(tab.screen.rows).unwrap_or_default(),
            tab.screen.scrollback_offset,
            tab.screen.max_scrollback,
        );
        Some((
            geometry,
            tab.screen.scrollback_offset,
            tab.screen.max_scrollback,
        ))
    }

    fn set_scrollback(&mut self, requested: usize) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let tab_id = tab.id.clone();
        let current = tab.screen.scrollback_offset;
        let target = requested.min(tab.screen.max_scrollback);
        if target == current {
            return;
        }
        self.cancel_terminal_selection();
        let action = if target > current { "up" } else { "down" };
        let count = target.abs_diff(current);
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.run_control(vec![
                    "scroll-pane".to_owned(),
                    "-t".to_owned(),
                    tab_id,
                    action.to_owned(),
                    count.to_string(),
                ])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Terminal scrollbar failed: {error:#}"));
        }
    }

    fn click_scrollbar(&mut self, x: i32, y: i32) -> bool {
        let Some((geometry, current, maximum)) = self.scrollbar_state() else {
            return false;
        };
        if !geometry.track.contains(x, y) {
            return false;
        }
        self.cancel_terminal_selection();
        if maximum == 0 {
            return true;
        }
        if geometry.thumb.contains(x, y) {
            self.scroll_drag = Some(RemoteScrollDrag {
                thumb_grab_offset: y - geometry.thumb.top,
            });
            unsafe { SetCapture(self.window) };
        } else {
            let page = self
                .active_tab()
                .map(|tab| usize::try_from(tab.screen.rows).unwrap_or(1))
                .unwrap_or(1)
                .max(1);
            self.set_scrollback(if y < geometry.thumb.top {
                current.saturating_add(page).min(maximum)
            } else {
                current.saturating_sub(page)
            });
        }
        true
    }

    fn drag_scrollbar(&mut self, y: i32) -> bool {
        let Some(drag) = self.scroll_drag else {
            return false;
        };
        let Some((geometry, _, maximum)) = self.scrollbar_state() else {
            self.end_scroll_drag();
            return false;
        };
        let offset = scrollback_for_thumb_top(geometry, y - drag.thumb_grab_offset, maximum);
        self.set_scrollback(offset);
        true
    }

    fn end_scroll_drag(&mut self) {
        if self.scroll_drag.take().is_some() {
            unsafe { ReleaseCapture() };
        }
    }

    fn scrollbar_capture_lost(&mut self) {
        self.scroll_drag = None;
    }

    fn resize_grip_contains(&self, x: i32, y: i32) -> bool {
        self.workspace_geometry()
            .resize_grip
            .is_some_and(|grip| grip.contains(x, y))
    }

    fn begin_tabs_resize(&mut self, x: i32, y: i32) -> bool {
        if !self.resize_grip_contains(x, y) {
            return false;
        }
        self.cancel_terminal_selection();
        self.tabs_resize_dragging = true;
        unsafe { SetCapture(self.window) };
        true
    }

    fn drag_tabs_resize(&mut self, x: i32) {
        if !self.tabs_resize_dragging {
            return;
        }
        let width = self.workspace_geometry().client.width();
        self.config.tabs_width = clamp_tabs_width(tabs_width_from_drag(x, width));
        self.layout();
        self.resize_active_terminal();
    }

    fn finish_tabs_resize(&mut self) {
        if !self.tabs_resize_dragging {
            return;
        }
        self.tabs_resize_dragging = false;
        unsafe { ReleaseCapture() };
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Tabs width save failed: {error:#}"));
        }
    }

    fn tabs_resize_capture_lost(&mut self) {
        if !self.tabs_resize_dragging {
            return;
        }
        self.tabs_resize_dragging = false;
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Tabs width save failed: {error:#}"));
        }
    }

    fn set_resize_cursor_if_needed(&self) -> bool {
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } == 0
            || unsafe { ScreenToClient(self.window, &mut point) } == 0
            || !self.resize_grip_contains(point.x, point.y)
        {
            return false;
        }
        unsafe { SetCursor(LoadCursorW(ptr::null_mut(), IDC_SIZEWE)) };
        true
    }

    fn terminal_char(&mut self, value: u16) {
        let scalar = if (0xd800..=0xdbff).contains(&value) {
            self.pending_high_surrogate = Some(value);
            return;
        } else if (0xdc00..=0xdfff).contains(&value) {
            let Some(high) = self.pending_high_surrogate.take() else {
                return;
            };
            0x1_0000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(value) - 0xdc00)
        } else {
            self.pending_high_surrogate = None;
            u32::from(value)
        };
        let mut encoded = [0_u8; 4];
        let bytes = match scalar {
            8 => b"\x7f".as_slice(),
            13 => b"\r".as_slice(),
            _ => char::from_u32(scalar)
                .map(|character| character.encode_utf8(&mut encoded).as_bytes())
                .unwrap_or_default(),
        };
        if !bytes.is_empty() {
            self.terminal_input(bytes);
        }
    }

    fn terminal_key(&mut self, key: u32) -> bool {
        let key = u16::try_from(key).unwrap_or_default();
        let control = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
        if control && key == u16::from(b'C') && self.terminal_selection.is_some() {
            if let Err(error) = self.copy_terminal_selection() {
                self.last_error = Some(format!("Copy failed: {error:#}"));
            }
            return true;
        }
        if control && key == u16::from(b'V') {
            if let Err(error) = self.paste_terminal_clipboard() {
                self.last_error = Some(format!("Paste failed: {error:#}"));
            }
            return true;
        }
        if control && (u16::from(b'A')..=u16::from(b'Z')).contains(&key) {
            self.terminal_input(&[(key as u8) - b'A' + 1]);
            return true;
        }
        let name = match key {
            VK_UP => "Up",
            VK_DOWN => "Down",
            VK_LEFT => "Left",
            VK_RIGHT => "Right",
            VK_HOME => "Home",
            VK_END => "End",
            VK_PRIOR => "PageUp",
            VK_NEXT => "PageDown",
            VK_ESCAPE => "Escape",
            VK_F1 => "F1",
            VK_F2 => "F2",
            VK_F3 => "F3",
            VK_F4 => "F4",
            VK_F5 => "F5",
            VK_F6 => "F6",
            VK_F7 => "F7",
            VK_F8 => "F8",
            VK_F9 => "F9",
            VK_F10 => "F10",
            VK_F11 => "F11",
            VK_F12 => "F12",
            _ => return false,
        };
        if let Some(bytes) = tmux_key_bytes(name) {
            self.terminal_input(&bytes);
            true
        } else {
            false
        }
    }

    fn paint(&self) {
        let palette = if self.settings_open {
            self.settings_theme_draft.palette()
        } else {
            self.config.color_theme.palette()
        };
        let mut paint: PAINTSTRUCT = unsafe { mem::zeroed() };
        let device = unsafe { BeginPaint(self.window, &mut paint) };
        if device.is_null() {
            return;
        }
        let (sidebar, terminal, composer, status) = self.layout_rects();
        fill(device, &sidebar, palette.sidebar.colorref());
        fill(device, &terminal, palette.terminal_background.colorref());
        fill(device, &composer, palette.composer.colorref());
        fill(device, &status, palette.status.colorref());
        unsafe {
            SelectObject(device, self.font as HGDIOBJ);
            SetBkMode(device, TRANSPARENT as i32);
        }
        self.paint_tabs(device, sidebar, palette);
        if let Some(tab) = self.active_tab() {
            paint_screen(
                device,
                terminal,
                &tab.screen,
                self.cell_width,
                self.cell_height,
                palette,
            );
            self.paint_terminal_selection(device, terminal, &tab.screen, palette);
            self.paint_terminal_scrollbar(device, palette);
            draw_text(
                device,
                RECT {
                    left: composer.left + MARGIN,
                    top: composer.top + 2,
                    right: composer.right - MARGIN,
                    bottom: composer.top + 24,
                },
                &format!("Input → {}  {}", tab.id, tab.title),
                palette.muted_text.colorref(),
            );
        }
        let status_text = if let Some(error) = &self.last_error {
            error.clone()
        } else if let Some(client) = &self.client {
            let cwd = self
                .active_tab()
                .and_then(|tab| tab.working_context.cwd.as_deref())
                .unwrap_or("-");
            format!(
                "Connected · server PID {} · {} · {}",
                client.server_pid(),
                client.client_id(),
                cwd
            )
        } else {
            "Disconnected · reconnecting".to_owned()
        };
        draw_text(
            device,
            RECT {
                left: status.left + MARGIN,
                top: status.top,
                right: status.right - MARGIN,
                bottom: status.bottom,
            },
            &status_text,
            if self.last_error.is_some() {
                palette.danger.colorref()
            } else {
                palette.muted_text.colorref()
            },
        );
        if !self.window_close_pending && !self.settings_open {
            let focus = match self.current_focus_surface() {
                RemoteFocusSurface::Terminal => terminal,
                RemoteFocusSurface::Composer => composer,
                RemoteFocusSurface::Tabs => sidebar,
            };
            frame(device, &focus, palette.focus_ring.colorref());
        }
        if self.window_close_pending {
            self.paint_window_close(device, palette);
        } else if self.settings_open {
            self.paint_settings(device, palette);
        }
        unsafe { EndPaint(self.window, &paint) };
    }

    fn paint_terminal_selection(
        &self,
        device: HDC,
        terminal: RECT,
        screen: &UiScreenSnapshot,
        palette: &ThemePalette,
    ) {
        let Some(selection) = self.terminal_selection.as_ref().filter(|selection| {
            selection.tab_id == screen.tab_id && selection.generation == screen.generation
        }) else {
            return;
        };
        let cells = screen_cells(screen);
        let (start, end) = selection.bounds();
        for row in start.row..=end.row.min(screen.rows.saturating_sub(1)) {
            let first = if row == start.row { start.column } else { 0 };
            let last = if row == end.row {
                end.column
            } else {
                screen.columns.saturating_sub(1)
            }
            .min(screen.columns.saturating_sub(1));
            fill(
                device,
                &RECT {
                    left: terminal.left
                        + i32::try_from(first).unwrap_or_default() * self.cell_width,
                    top: terminal.top + i32::try_from(row).unwrap_or_default() * self.cell_height,
                    right: terminal.left
                        + i32::try_from(last.saturating_add(1)).unwrap_or_default()
                            * self.cell_width,
                    bottom: terminal.top
                        + i32::try_from(row.saturating_add(1)).unwrap_or_default()
                            * self.cell_height,
                },
                palette.selection_background.colorref(),
            );
            for column in first..=last {
                let Some(text) = cells
                    .get(usize::try_from(row).unwrap_or(usize::MAX))
                    .and_then(|cells| cells.get(usize::try_from(column).unwrap_or(usize::MAX)))
                    .and_then(Option::as_deref)
                    .filter(|text| !text.is_empty())
                else {
                    continue;
                };
                let span = text
                    .chars()
                    .map(|character| UnicodeWidthChar::width(character).unwrap_or(1))
                    .sum::<usize>()
                    .max(1);
                draw_text(
                    device,
                    RECT {
                        left: terminal.left
                            + i32::try_from(column).unwrap_or_default() * self.cell_width,
                        top: terminal.top
                            + i32::try_from(row).unwrap_or_default() * self.cell_height,
                        right: terminal.left
                            + (i32::try_from(column).unwrap_or_default()
                                + i32::try_from(span).unwrap_or(1))
                                * self.cell_width,
                        bottom: terminal.top
                            + i32::try_from(row.saturating_add(1)).unwrap_or_default()
                                * self.cell_height,
                    },
                    text,
                    palette.selection_foreground.colorref(),
                );
            }
        }
    }

    fn paint_terminal_scrollbar(&self, device: HDC, palette: &ThemePalette) {
        let Some((geometry, _, _)) = self.scrollbar_state() else {
            return;
        };
        let track = win_rect(geometry.track);
        let thumb = win_rect(geometry.thumb);
        fill(device, &track, palette.scrollbar_track.colorref());
        fill(
            device,
            &thumb,
            if self.scroll_drag.is_some() {
                palette.scrollbar_thumb_active.colorref()
            } else {
                palette.scrollbar_thumb.colorref()
            },
        );
    }

    fn paint_window_close(&self, device: HDC, palette: &ThemePalette) {
        let (modal, _) = self.close_modal_geometry();
        fill(device, &modal, palette.modal.colorref());
        frame(device, &modal, palette.accent.colorref());
        draw_text(
            device,
            RECT {
                left: modal.left + 24,
                top: modal.top + 18,
                right: modal.right - 24,
                bottom: modal.top + 50,
            },
            "Close AgenTerm window?",
            palette.text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 24,
                top: modal.top + 56,
                right: modal.right - 24,
                bottom: modal.top + 86,
            },
            "Keep the server running to preserve live tabs and processes.",
            palette.muted_text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 24,
                top: modal.top + 90,
                right: modal.right - 24,
                bottom: modal.top + 120,
            },
            "Press Enter to keep it running, or Esc to cancel.",
            palette.muted_text.colorref(),
        );
    }

    fn paint_settings(&self, device: HDC, palette: &ThemePalette) {
        let (modal, _) = self.settings_modal_geometry();
        fill(device, &modal, palette.modal.colorref());
        frame(device, &modal, palette.accent.colorref());
        draw_text(
            device,
            RECT {
                left: modal.left + 28,
                top: modal.top + 18,
                right: modal.right - 28,
                bottom: modal.top + 50,
            },
            "Settings",
            palette.text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 32,
                top: modal.top + 58,
                right: modal.right - 126,
                bottom: modal.top + 88,
            },
            "Terminal font family",
            palette.muted_text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.right - 110,
                top: modal.top + 58,
                right: modal.right - 32,
                bottom: modal.top + 88,
            },
            "Size",
            palette.muted_text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 32,
                top: modal.top + 144,
                right: modal.right - 32,
                bottom: modal.top + 174,
            },
            "Color theme · preview is immediate; Apply persists",
            palette.muted_text.colorref(),
        );
    }

    fn paint_tabs(&self, device: HDC, sidebar: RECT, palette: &ThemePalette) {
        if !self.tabs_visible {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        for (position, tab) in client.snapshot().tabs.iter().enumerate() {
            let top = i32::try_from(position)
                .unwrap_or(i32::MAX)
                .saturating_mul(SIDEBAR_ROW_HEIGHT);
            if top + SIDEBAR_ROW_HEIGHT > sidebar.bottom - TOOLBAR_HEIGHT {
                break;
            }
            let row = RECT {
                left: sidebar.left + 5,
                top: top + 4,
                right: sidebar.right - 6,
                bottom: top + SIDEBAR_ROW_HEIGHT - 3,
            };
            let active = client.snapshot().active_tab_id.as_deref() == Some(tab.id.as_str());
            if active {
                fill(device, &row, palette.active.colorref());
                frame(device, &row, palette.active_border.colorref());
            }
            if self.editing_tab_id.as_deref() == Some(tab.id.as_str()) {
                continue;
            }
            let edit = RECT {
                left: row.right - 46,
                top: row.top + 5,
                right: row.right - 2,
                bottom: row.bottom - 5,
            };
            frame(device, &edit, palette.active_border.colorref());
            draw_text(device, edit, "Edit", palette.muted_text.colorref());
            let depth = tab_depth(&client.snapshot().tabs, tab);
            draw_text(
                device,
                RECT {
                    left: row.left + 7 + i32::try_from(depth).unwrap_or(0) * 12,
                    top: row.top,
                    right: edit.left - 4,
                    bottom: row.bottom,
                },
                &format!(
                    "{} {}{}",
                    tab.id,
                    tab.title,
                    if tab.dead { " [dead]" } else { "" }
                ),
                if tab.dead {
                    palette.muted_text.colorref()
                } else {
                    palette.text.colorref()
                },
            );
        }
    }
}

impl Drop for RemoteWindowState {
    fn drop(&mut self) {
        if let Some(client) = self.client.as_mut() {
            let _ = client.detach();
        }
        unsafe { DeleteObject(self.font as HGDIOBJ) };
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => 0,
        WM_TIMER => {
            if let Some(state) = state_mut(window)
                && state.tick()
            {
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                };
            }
            0
        }
        WM_SIZE => {
            if let Some(state) = state_mut(window) {
                state.layout();
                state.resize_active_terminal();
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                };
            }
            0
        }
        WM_PAINT => {
            if let Some(state) = state_ref(window) {
                state.paint();
                0
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        WM_ERASEBKGND => 1,
        WM_SETFOCUS => {
            if let Some(state) = state_ref(window)
                && unsafe { GetFocus() } != state.edit
            {
                unsafe { SetFocus(window) };
            }
            0
        }
        WM_LBUTTONDOWN => {
            if let Some(state) = state_mut(window) {
                if state.window_close_pending || state.settings_open {
                    return 0;
                }
                let x = (lparam as u32 & 0xffff) as i16 as i32;
                let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
                if state.begin_tabs_resize(x, y) {
                    return 0;
                }
                if state.click_scrollbar(x, y) {
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                    return 0;
                }
                let sidebar = state.layout_rects().0;
                if state.tabs_visible && x < sidebar.right {
                    if state.tab_edit_action_contains(x, y) {
                        state.begin_tab_edit_at(y);
                    } else {
                        state.finish_tab_edit(false);
                        state.select_tab_at(y);
                    }
                } else {
                    state.finish_tab_edit(false);
                    if state.begin_terminal_selection(x, y) {
                        unsafe {
                            windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                                window,
                                ptr::null(),
                                0,
                            )
                        };
                        return 0;
                    }
                    unsafe { SetFocus(window) };
                }
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                };
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some(state) = state_mut(window) {
                let x = (lparam as u32 & 0xffff) as i16 as i32;
                let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
                if state.scroll_drag.is_some() {
                    if state.drag_scrollbar(y) {
                        unsafe {
                            windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                                window,
                                ptr::null(),
                                0,
                            )
                        };
                    }
                } else if state.tabs_resize_dragging {
                    state.drag_tabs_resize(x);
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                } else if state.drag_terminal_selection(x, y) {
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                }
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_mut(window) {
                let x = (lparam as u32 & 0xffff) as i16 as i32;
                let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
                if state.scroll_drag.is_some() {
                    state.end_scroll_drag();
                } else if state.tabs_resize_dragging {
                    state.finish_tabs_resize();
                } else if state.finish_terminal_selection(x, y) {
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                }
            }
            0
        }
        WM_CAPTURECHANGED => {
            if let Some(state) = state_mut(window) {
                state.tabs_resize_capture_lost();
                state.scrollbar_capture_lost();
                state.terminal_selection_capture_lost();
            }
            0
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = state_mut(window) {
                let delta = ((wparam >> 16) & 0xffff) as u16 as i16 as i32;
                state.scroll_terminal(delta);
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                };
            }
            0
        }
        WM_SETCURSOR => {
            if let Some(state) = state_ref(window)
                && state.set_resize_cursor_if_needed()
            {
                1
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        WM_INITMENUPOPUP => {
            if let Some(state) = state_ref(window) {
                state.refresh_system_menu();
            }
            0
        }
        WM_SYSCOMMAND => match wparam & 0xfff0 {
            SYSTEM_MENU_COPY_ID => {
                if let Some(state) = state_mut(window) {
                    state.system_menu_copy();
                }
                0
            }
            SYSTEM_MENU_PASTE_ID => {
                if let Some(state) = state_mut(window) {
                    state.system_menu_paste();
                }
                0
            }
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        },
        WM_KEYDOWN => {
            if let Some(state) = state_mut(window) {
                if state.window_close_pending {
                    match wparam as u32 {
                        0x0d => state.finish_window_close(RemoteCloseChoice::KeepServerRunning),
                        key if key == u32::from(VK_ESCAPE) => {
                            state.finish_window_close(RemoteCloseChoice::Cancel)
                        }
                        _ => {}
                    }
                    return 0;
                }
                if state.settings_open {
                    if wparam as u32 == u32::from(VK_ESCAPE) {
                        state.finish_settings(false);
                    }
                    return 0;
                }
                if unsafe { GetFocus() } == window && state.terminal_key(wparam as u32) {
                    return 0;
                }
            }
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_APP_AUTOMATION_SHORTCUT => {
            if let Some(state) = state_mut(window) {
                let modifiers = lparam as usize;
                if state.handle_surface_navigation(
                    wparam as u32,
                    modifiers & 1 != 0,
                    modifiers & 2 != 0,
                    modifiers & 4 != 0,
                ) {
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                    return 1;
                }
            }
            0
        }
        WM_APP_FOCUS_QUERY => {
            state_ref(window).map_or(0, |state| match state.current_focus_surface() {
                RemoteFocusSurface::Terminal => 1,
                RemoteFocusSurface::Composer => 2,
                RemoteFocusSurface::Tabs => 3,
            })
        }
        WM_CHAR => {
            if let Some(state) = state_mut(window) {
                if state.window_close_pending || state.settings_open {
                    return 0;
                }
                if unsafe { GetFocus() } == window {
                    state.terminal_char(wparam as u16);
                }
            }
            0
        }
        WM_COMMAND => {
            if let Some(state) = state_mut(window) {
                match wparam & 0xffff {
                    SEND_ID => state.send_composer(),
                    NEW_ID => state.new_tab(),
                    SETTINGS_ID => state.open_settings(),
                    TABS_ID => {
                        state.finish_tab_edit(false);
                        state.tabs_visible = !state.tabs_visible;
                        state.config.tabs_visible = state.tabs_visible;
                        if let Err(error) = save_config(&state.config) {
                            state.last_error =
                                Some(format!("Tabs visibility save failed: {error:#}"));
                        }
                        state.layout();
                        state.resize_active_terminal();
                    }
                    TAB_SAVE_ID => state.finish_tab_edit(true),
                    TAB_CANCEL_ID => state.finish_tab_edit(false),
                    CLOSE_KEEP_ID => {
                        state.finish_window_close(RemoteCloseChoice::KeepServerRunning)
                    }
                    CLOSE_STOP_ID => {
                        state.finish_window_close(RemoteCloseChoice::StopServerAndExit)
                    }
                    CLOSE_CANCEL_ID => state.finish_window_close(RemoteCloseChoice::Cancel),
                    SETTINGS_DARK_ID => state.preview_settings_theme(ThemeId::Dark),
                    SETTINGS_LIGHT_ID => state.preview_settings_theme(ThemeId::Light),
                    SETTINGS_APPLY_ID => state.finish_settings(true),
                    SETTINGS_CANCEL_ID => state.finish_settings(false),
                    _ => {}
                }
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                };
            }
            0
        }
        WM_CLOSE => {
            if let Some(state) = state_mut(window) {
                state.request_window_close();
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                };
            } else {
                unsafe { DestroyWindow(window) };
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_NCDESTROY => {
            let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) };
            if pointer != 0 {
                unsafe {
                    SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                    drop(Box::from_raw(pointer as *mut RemoteWindowState));
                }
            }
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn create_button(window: HWND, instance: HINSTANCE, id: usize, text: &str) -> HWND {
    unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            70,
            32,
            window,
            id as *mut c_void,
            instance,
            ptr::null(),
        )
    }
}

fn create_hidden_edit(window: HWND, instance: HINSTANCE, id: usize) -> HWND {
    unsafe {
        CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_BORDER | WS_TABSTOP,
            0,
            0,
            100,
            18,
            window,
            id as *mut c_void,
            instance,
            ptr::null(),
        )
    }
}

fn create_hidden_button(window: HWND, instance: HINSTANCE, id: usize, text: &str) -> HWND {
    unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_TABSTOP,
            0,
            0,
            42,
            38,
            window,
            id as *mut c_void,
            instance,
            ptr::null(),
        )
    }
}

fn install_system_menu(window: HWND) -> Result<()> {
    let menu = unsafe { GetSystemMenu(window, 0) };
    if menu.is_null() {
        anyhow::bail!("GetSystemMenu failed");
    }
    if unsafe {
        InsertMenuW(
            menu,
            SC_CLOSE,
            MF_BYCOMMAND | MF_STRING,
            SYSTEM_MENU_COPY_ID,
            wide("Copy\tCtrl+C").as_ptr(),
        )
    } == 0
        || unsafe {
            InsertMenuW(
                menu,
                SC_CLOSE,
                MF_BYCOMMAND | MF_STRING,
                SYSTEM_MENU_PASTE_ID,
                wide("Paste\tCtrl+V").as_ptr(),
            )
        } == 0
        || unsafe { InsertMenuW(menu, SC_CLOSE, MF_BYCOMMAND | MF_SEPARATOR, 0, ptr::null()) } == 0
    {
        anyhow::bail!("could not add Copy and Paste to the window system menu");
    }
    Ok(())
}

fn set_clipboard_text(window: HWND, text: &str) -> Result<()> {
    if unsafe { OpenClipboard(window) } == 0 {
        anyhow::bail!("could not open the Windows clipboard");
    }
    if unsafe { EmptyClipboard() } == 0 {
        unsafe { CloseClipboard() };
        anyhow::bail!("could not clear the Windows clipboard");
    }
    let encoded = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let allocation = unsafe { GlobalAlloc(GMEM_MOVEABLE, encoded.len() * mem::size_of::<u16>()) };
    if allocation.is_null() {
        unsafe { CloseClipboard() };
        anyhow::bail!("could not allocate clipboard text");
    }
    let destination = unsafe { GlobalLock(allocation) } as *mut u16;
    if destination.is_null() {
        unsafe {
            GlobalFree(allocation);
            CloseClipboard();
        }
        anyhow::bail!("could not lock clipboard text");
    }
    unsafe {
        ptr::copy_nonoverlapping(encoded.as_ptr(), destination, encoded.len());
        GlobalUnlock(allocation);
    }
    if unsafe { SetClipboardData(CLIPBOARD_UNICODE_TEXT, allocation) }.is_null() {
        unsafe {
            GlobalFree(allocation);
            CloseClipboard();
        }
        anyhow::bail!("could not publish clipboard text");
    }
    unsafe { CloseClipboard() };
    Ok(())
}

fn clipboard_has_unicode_text() -> bool {
    (unsafe { IsClipboardFormatAvailable(CLIPBOARD_UNICODE_TEXT) }) != 0
}

fn read_clipboard_text() -> Result<String> {
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
            break;
        }
        if Instant::now() >= deadline {
            anyhow::bail!("could not open the Windows clipboard within 500 ms");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let result = (|| {
        if !clipboard_has_unicode_text() {
            anyhow::bail!("the clipboard does not contain Unicode text");
        }
        let allocation = unsafe { GetClipboardData(CLIPBOARD_UNICODE_TEXT) };
        if allocation.is_null() {
            anyhow::bail!("could not read Unicode clipboard data");
        }
        let allocation_size = unsafe { GlobalSize(allocation) };
        if allocation_size == 0 {
            anyhow::bail!("Unicode clipboard data has no readable allocation");
        }
        if allocation_size > (TERMINAL_PASTE_LIMIT_BYTES + 1) * mem::size_of::<u16>() {
            anyhow::bail!(
                "clipboard text exceeds the {TERMINAL_PASTE_LIMIT_BYTES}-byte terminal paste limit"
            );
        }
        let source = unsafe { GlobalLock(allocation) } as *const u16;
        if source.is_null() {
            anyhow::bail!("could not lock Unicode clipboard data");
        }
        let units =
            unsafe { std::slice::from_raw_parts(source, allocation_size / mem::size_of::<u16>()) };
        let length = units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len());
        let decoded = String::from_utf16(&units[..length])
            .context("Unicode clipboard data is not valid UTF-16");
        unsafe { GlobalUnlock(allocation) };
        decoded
    })();
    unsafe { CloseClipboard() };
    result
}

fn normalize_terminal_paste(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push('\r');
            }
            '\n' => normalized.push('\r'),
            '\t' => normalized.push('\t'),
            value if !value.is_control() => normalized.push(value),
            _ => {}
        }
    }
    normalized
}

fn screen_cells(screen: &UiScreenSnapshot) -> Vec<Vec<Option<String>>> {
    let rows = usize::try_from(screen.rows).unwrap_or_default();
    let columns = usize::try_from(screen.columns).unwrap_or_default();
    let mut cells: Vec<Vec<Option<String>>> = vec![vec![None; columns]; rows];
    for run in &screen.runs {
        let Some(row) = cells.get_mut(usize::try_from(run.row).unwrap_or(usize::MAX)) else {
            continue;
        };
        let mut column = usize::try_from(run.column).unwrap_or(usize::MAX);
        for character in run.text.chars() {
            let width = UnicodeWidthChar::width(character).unwrap_or(1);
            if width == 0 {
                if let Some(Some(previous)) = column
                    .checked_sub(1)
                    .and_then(|previous| row.get_mut(previous))
                {
                    previous.push(character);
                }
                continue;
            }
            if column >= row.len() {
                break;
            }
            row[column] = Some(character.to_string());
            for continuation in 1..width {
                if let Some(cell) = row.get_mut(column + continuation) {
                    *cell = Some(String::new());
                }
            }
            column = column.saturating_add(width);
        }
    }
    cells
}

fn screen_selection_text(screen: &UiScreenSnapshot, selection: &RemoteTerminalSelection) -> String {
    let cells = screen_cells(screen);
    let (start, end) = selection.bounds();
    let mut lines = Vec::new();
    for row in start.row..=end.row.min(screen.rows.saturating_sub(1)) {
        let first = if row == start.row { start.column } else { 0 };
        let last = if row == end.row {
            end.column
        } else {
            screen.columns.saturating_sub(1)
        }
        .min(screen.columns.saturating_sub(1));
        let mut line = String::new();
        for column in first..=last {
            match cells
                .get(usize::try_from(row).unwrap_or(usize::MAX))
                .and_then(|row| row.get(usize::try_from(column).unwrap_or(usize::MAX)))
            {
                Some(Some(text)) => line.push_str(text),
                Some(None) | None => line.push(' '),
            }
        }
        lines.push(line.trim_end_matches(' ').to_owned());
    }
    lines.join("\r\n")
}

fn create_terminal_font(window: HWND, config: &AppConfig) -> Result<(HFONT, i32, i32)> {
    let device = unsafe { GetDC(window) };
    if device.is_null() {
        anyhow::bail!("GetDC failed");
    }
    let dpi = unsafe {
        windows_sys::Win32::Graphics::Gdi::GetDeviceCaps(
            device,
            i32::try_from(LOGPIXELSY).unwrap_or(90),
        )
    };
    let height = -((i32::from(config.terminal_font_size) * dpi) / 72).max(1);
    let font = unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            u32::from(OUT_DEFAULT_PRECIS),
            u32::from(CLIP_DEFAULT_PRECIS),
            u32::from(CLEARTYPE_QUALITY),
            u32::from(FIXED_PITCH | FF_MODERN),
            wide(&config.terminal_font_family).as_ptr(),
        )
    };
    if font.is_null() {
        unsafe { ReleaseDC(window, device) };
        anyhow::bail!("CreateFontW failed");
    }
    let previous = unsafe { SelectObject(device, font as HGDIOBJ) };
    let mut metrics: TEXTMETRICW = unsafe { mem::zeroed() };
    let measured = unsafe { GetTextMetricsW(device, &mut metrics) };
    unsafe {
        SelectObject(device, previous);
        ReleaseDC(window, device);
    }
    if measured == 0 {
        unsafe { DeleteObject(font as HGDIOBJ) };
        anyhow::bail!("GetTextMetricsW failed");
    }
    Ok((font, metrics.tmAveCharWidth.max(1), metrics.tmHeight.max(1)))
}

fn paint_screen(
    device: HDC,
    terminal: RECT,
    screen: &UiScreenSnapshot,
    cell_width: i32,
    cell_height: i32,
    palette: &ThemePalette,
) {
    for run in &screen.runs {
        let left = terminal.left + i32::try_from(run.column).unwrap_or(0) * cell_width;
        let top = terminal.top + i32::try_from(run.row).unwrap_or(0) * cell_height;
        let right = left
            + i32::try_from(run.columns)
                .unwrap_or(0)
                .saturating_mul(cell_width);
        let rect = RECT {
            left,
            top,
            right: right.min(terminal.right),
            bottom: (top + cell_height).min(terminal.bottom),
        };
        if rect.left >= terminal.right || rect.top >= terminal.bottom {
            continue;
        }
        let (foreground, background) = style_colors(&run.style, palette);
        if background != palette.terminal_background.colorref() {
            fill(device, &rect, background);
        }
        unsafe { SetTextColor(device, foreground) };
        let text = wide_without_nul(&run.text);
        if !text.is_empty() {
            unsafe {
                windows_sys::Win32::Graphics::Gdi::ExtTextOutW(
                    device,
                    left,
                    top,
                    0,
                    ptr::null(),
                    text.as_ptr(),
                    u32::try_from(text.len()).unwrap_or(u32::MAX),
                    ptr::null(),
                );
            }
        }
    }
    if screen.cursor.visible {
        let cursor = RECT {
            left: terminal.left + i32::try_from(screen.cursor.column).unwrap_or(0) * cell_width,
            top: terminal.top + i32::try_from(screen.cursor.row).unwrap_or(0) * cell_height,
            right: terminal.left
                + (i32::try_from(screen.cursor.column).unwrap_or(0) + 1) * cell_width,
            bottom: terminal.top
                + (i32::try_from(screen.cursor.row).unwrap_or(0) + 1) * cell_height,
        };
        frame(device, &cursor, palette.accent.colorref());
    }
}

fn style_colors(style: &UiCellStyle, palette: &ThemePalette) -> (COLORREF, COLORREF) {
    let mut foreground = terminal_color(style.foreground, palette, true);
    let mut background = terminal_color(style.background, palette, false);
    if style.inverse {
        std::mem::swap(&mut foreground, &mut background);
    }
    (foreground, background)
}

fn terminal_color(color: UiColor, palette: &ThemePalette, foreground: bool) -> COLORREF {
    match color {
        UiColor::Default if foreground => palette.terminal_foreground.colorref(),
        UiColor::Default => palette.terminal_background.colorref(),
        UiColor::Indexed { index } => {
            if index < 16 {
                palette.ansi[usize::from(index)].colorref()
            } else {
                indexed_rgb(index).colorref()
            }
        }
        UiColor::Rgb { red, green, blue } => Rgb::new(red, green, blue).colorref(),
    }
}

fn indexed_rgb(index: u8) -> Rgb {
    if index >= 232 {
        let value = 8_u8.saturating_add(index.saturating_sub(232).saturating_mul(10));
        return Rgb::new(value, value, value);
    }
    let cube = index.saturating_sub(16);
    let channel = |value: u8| {
        let value = value % 6;
        if value == 0 { 0 } else { 55 + value * 40 }
    };
    Rgb::new(channel(cube / 36), channel(cube / 6), channel(cube))
}

fn tab_depth(tabs: &[UiTabBootstrap], tab: &UiTabBootstrap) -> usize {
    let mut depth = 0;
    let mut parent = tab.parent_id.as_deref();
    while let Some(parent_id) = parent {
        depth += 1;
        parent = tabs
            .iter()
            .find(|candidate| candidate.id == parent_id)
            .and_then(|candidate| candidate.parent_id.as_deref());
        if depth > tabs.len() {
            break;
        }
    }
    depth
}

fn fill(device: HDC, rect: &RECT, color: COLORREF) {
    let brush = unsafe { CreateSolidBrush(color) };
    unsafe {
        FillRect(device, rect, brush);
        DeleteObject(brush as HGDIOBJ);
    }
}

fn frame(device: HDC, rect: &RECT, color: COLORREF) {
    let brush = unsafe { CreateSolidBrush(color) };
    unsafe {
        FrameRect(device, rect, brush);
        DeleteObject(brush as HGDIOBJ);
    }
}

fn draw_text(device: HDC, mut rect: RECT, text: &str, color: COLORREF) {
    unsafe { SetTextColor(device, color) };
    let text = wide_without_nul(text);
    unsafe {
        DrawTextW(
            device,
            text.as_ptr(),
            i32::try_from(text.len()).unwrap_or(i32::MAX),
            &mut rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
    }
}

fn win_rect(rect: PixelRect) -> RECT {
    RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

fn window_text(window: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0_u16; usize::try_from(length).unwrap_or(0) + 1];
    let copied = unsafe {
        GetWindowTextW(
            window,
            buffer.as_mut_ptr(),
            i32::try_from(buffer.len()).unwrap_or(i32::MAX),
        )
    };
    String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap_or(0)])
}

fn show_without_activation(window: HWND) {
    unsafe {
        let foreground = GetForegroundWindow();
        if !foreground.is_null() && foreground != window {
            SetWindowPos(
                window,
                foreground,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        } else {
            SetWindowPos(
                window,
                ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        ShowWindow(window, SW_SHOWNOACTIVATE);
    }
}

fn state_mut(window: HWND) -> Option<&'static mut RemoteWindowState> {
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) };
    (!pointer.eq(&0)).then(|| unsafe { &mut *(pointer as *mut RemoteWindowState) })
}

fn state_ref(window: HWND) -> Option<&'static RemoteWindowState> {
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) };
    (!pointer.eq(&0)).then(|| unsafe { &*(pointer as *const RemoteWindowState) })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn wide_without_nul(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_bridge::{UI_SCREEN_SCHEMA_VERSION, UiCellRun, UiCursorSnapshot};

    fn plain_style() -> UiCellStyle {
        UiCellStyle {
            foreground: UiColor::Default,
            background: UiColor::Default,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }

    #[test]
    fn selection_text_preserves_wide_cells_and_multiline_bounds() {
        let screen = UiScreenSnapshot {
            schema_version: UI_SCREEN_SCHEMA_VERSION,
            tab_id: "@1".to_owned(),
            generation: 7,
            rows: 2,
            columns: 8,
            scrollback_offset: 0,
            max_scrollback: 0,
            cursor: UiCursorSnapshot {
                row: 1,
                column: 4,
                visible: true,
            },
            runs: vec![
                UiCellRun {
                    row: 0,
                    column: 0,
                    columns: 4,
                    text: "A界B".to_owned(),
                    style: plain_style(),
                },
                UiCellRun {
                    row: 1,
                    column: 0,
                    columns: 4,
                    text: "tail".to_owned(),
                    style: plain_style(),
                },
            ],
            complete: true,
            truncated: false,
        };
        let selection = RemoteTerminalSelection {
            tab_id: "@1".to_owned(),
            generation: 7,
            anchor: RemoteTerminalPoint { row: 0, column: 1 },
            active: RemoteTerminalPoint { row: 1, column: 1 },
            dragging: false,
        };
        assert_eq!(screen_selection_text(&screen, &selection), "界B\r\nta");
    }

    #[test]
    fn terminal_paste_normalizes_lines_and_filters_controls() {
        assert_eq!(
            normalize_terminal_paste("one\r\ntwo\nthree\rfour\t\u{1b}[31m\0"),
            "one\rtwo\rthree\rfour\t[31m"
        );
    }

    #[test]
    fn surface_navigation_is_directional_and_modifier_exact() {
        assert_eq!(
            remote_surface_navigation(RemoteFocusSurface::Terminal, true, false, false, 0x28),
            Some(RemoteFocusSurface::Composer)
        );
        assert_eq!(
            remote_surface_navigation(RemoteFocusSurface::Composer, true, false, false, 0x26),
            Some(RemoteFocusSurface::Terminal)
        );
        assert_eq!(
            remote_surface_navigation(RemoteFocusSurface::Terminal, true, false, false, 0x25),
            Some(RemoteFocusSurface::Tabs)
        );
        assert_eq!(
            remote_surface_navigation(RemoteFocusSurface::Tabs, true, false, false, 0x27),
            Some(RemoteFocusSurface::Terminal)
        );
        assert_eq!(
            remote_surface_navigation(RemoteFocusSurface::Composer, true, false, false, 0x25),
            None
        );
        assert_eq!(
            remote_surface_navigation(RemoteFocusSurface::Terminal, true, true, false, 0x28),
            None
        );
        assert_eq!(
            remote_surface_navigation(RemoteFocusSurface::Terminal, true, false, true, 0x28),
            None
        );
    }
}
