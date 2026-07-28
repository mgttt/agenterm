use std::{
    ffi::c_void,
    mem,
    process::Command,
    ptr,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use windows_sys::Win32::{
    Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
        DEFAULT_CHARSET, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteObject,
        DrawTextW, EndPaint, FF_MODERN, FIXED_PITCH, FW_NORMAL, FillRect, FrameRect, GetDC,
        GetTextMetricsW, HDC, HFONT, HGDIOBJ, LOGPIXELSY, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
        ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT,
        UpdateWindow,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{
            GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_DOWN,
            VK_END, VK_ESCAPE, VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9,
            VK_F10, VK_F11, VK_F12, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_UP,
        },
        WindowsAndMessaging::{
            CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
            DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN, GWLP_USERDATA,
            GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
            GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, IDC_SIZEWE, LoadCursorW, LoadIconW,
            MSG, MoveWindow, PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetCursor,
            SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
            ShowWindow, TranslateMessage, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_COMMAND,
            WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_SETFOCUS,
            WM_SIZE, WM_TIMER, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN,
            WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
};

use crate::{
    client::{ipc_address, ipc_socket_addr},
    commands::tmux_key_bytes,
    settings::{AppConfig, clamp_tabs_width, load_config, save_config},
    theme::{Rgb, ThemePalette},
    ui_bridge::{
        UI_TAB_NOTE_MAX_BYTES, UI_TAB_TITLE_MAX_BYTES, UiCellStyle, UiColor, UiScreenSnapshot,
        UiTabBootstrap,
    },
    ui_client::{UiClientModel, tab_by_id},
    ui_geometry::{
        PixelRect, WorkspaceLayout, WorkspaceLayoutInput, tabs_width_from_drag, workspace_layout,
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
    let tab_title_edit = create_hidden_edit(window, instance, TAB_TITLE_EDIT_ID);
    let tab_note_edit = create_hidden_edit(window, instance, TAB_NOTE_EDIT_ID);
    let tab_save = create_hidden_button(window, instance, TAB_SAVE_ID, "Save");
    let tab_cancel = create_hidden_button(window, instance, TAB_CANCEL_ID, "Cancel");
    let close_keep = create_hidden_button(window, instance, CLOSE_KEEP_ID, "Keep Server Running");
    let close_stop = create_hidden_button(window, instance, CLOSE_STOP_ID, "Stop Server && Exit");
    let close_cancel = create_hidden_button(window, instance, CLOSE_CANCEL_ID, "Cancel");
    if edit.is_null()
        || send.is_null()
        || new_tab.is_null()
        || tabs.is_null()
        || tab_title_edit.is_null()
        || tab_note_edit.is_null()
        || tab_save.is_null()
        || tab_cancel.is_null()
        || close_keep.is_null()
        || close_stop.is_null()
        || close_cancel.is_null()
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
            tab_title_edit,
            tab_note_edit,
            tab_save,
            tab_cancel,
            close_keep,
            close_stop,
            close_cancel,
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
    tab_title_edit: HWND,
    tab_note_edit: HWND,
    tab_save: HWND,
    tab_cancel: HWND,
    close_keep: HWND,
    close_stop: HWND,
    close_cancel: HWND,
}

#[derive(Clone, Copy)]
enum RemoteCloseChoice {
    KeepServerRunning,
    StopServerAndExit,
    Cancel,
}

struct RemoteWindowState {
    window: HWND,
    edit: HWND,
    send: HWND,
    new_tab: HWND,
    tabs_button: HWND,
    tab_title_edit: HWND,
    tab_note_edit: HWND,
    tab_save: HWND,
    tab_cancel: HWND,
    close_keep: HWND,
    close_stop: HWND,
    close_cancel: HWND,
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
            tab_title_edit,
            tab_note_edit,
            tab_save,
            tab_cancel,
            close_keep,
            close_stop,
            close_cancel,
        } = controls;
        let config = load_config();
        let (font, cell_width, cell_height) = create_terminal_font(window, &config)?;
        let last_active_id = client.snapshot().active_tab_id.clone();
        Ok(Self {
            window,
            edit,
            send,
            new_tab,
            tabs_button,
            tab_title_edit,
            tab_note_edit,
            tab_save,
            tab_cancel,
            close_keep,
            close_stop,
            close_cancel,
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
                if self.tabs_visible && !self.window_close_pending {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
        }
        self.layout_tab_editor();
        self.layout_close_controls();
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
        let columns = ((terminal.right - terminal.left) / self.cell_width)
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
        unsafe { SetFocus(self.window) };
    }

    fn request_window_close(&mut self) {
        if self.window_close_pending {
            return;
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
                unsafe { SetFocus(self.window) };
            }
        }
    }

    fn terminal_input(&mut self, bytes: &[u8]) {
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

    fn resize_grip_contains(&self, x: i32, y: i32) -> bool {
        self.workspace_geometry()
            .resize_grip
            .is_some_and(|grip| grip.contains(x, y))
    }

    fn begin_tabs_resize(&mut self, x: i32, y: i32) -> bool {
        if !self.resize_grip_contains(x, y) {
            return false;
        }
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
        if unsafe { GetKeyState(VK_CONTROL as i32) } < 0
            && (u16::from(b'A')..=u16::from(b'Z')).contains(&key)
        {
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
        let palette = self.config.color_theme.palette();
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
        if self.window_close_pending {
            self.paint_window_close(device, palette);
        }
        unsafe { EndPaint(self.window, &paint) };
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
                if state.window_close_pending {
                    return 0;
                }
                let x = (lparam as u32 & 0xffff) as i16 as i32;
                let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
                if state.begin_tabs_resize(x, y) {
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
                    unsafe { SetFocus(window) };
                }
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                };
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some(state) = state_mut(window)
                && state.tabs_resize_dragging
            {
                let x = (lparam as u32 & 0xffff) as i16 as i32;
                state.drag_tabs_resize(x);
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                };
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_mut(window) {
                state.finish_tabs_resize();
            }
            0
        }
        WM_CAPTURECHANGED => {
            if let Some(state) = state_mut(window) {
                state.tabs_resize_capture_lost();
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
                if unsafe { GetFocus() } == window && state.terminal_key(wparam as u32) {
                    return 0;
                }
            }
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_CHAR => {
            if let Some(state) = state_mut(window) {
                if state.window_close_pending {
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
