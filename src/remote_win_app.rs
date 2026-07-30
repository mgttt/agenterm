use std::{
    ffi::c_void,
    mem,
    process::Command,
    ptr,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use unicode_width::UnicodeWidthChar;
use windows_sys::Win32::{
    Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
        DEFAULT_CHARSET, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
        DeleteObject, DrawTextW, EndPaint, FF_MODERN, FIXED_PITCH, FW_NORMAL, FillRect, FrameRect,
        GetDC, GetTextMetricsW, HDC, HFONT, HGDIOBJ, LOGPIXELSY, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
        ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT,
        UpdateWindow,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::{
            EnableWindow, GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL,
            VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8,
            VK_F9, VK_F10, VK_F11, VK_F12, VK_HOME, VK_LEFT, VK_MENU, VK_NEXT, VK_PRIOR, VK_RIGHT,
            VK_UP,
        },
        WindowsAndMessaging::{
            CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CheckMenuItem, CreateWindowExW,
            DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE,
            ES_WANTRETURN, EnableMenuItem, GWLP_USERDATA, GetClientRect, GetCursorPos, GetMessageW,
            GetSystemMenu, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
            IDC_ARROW, IDC_SIZEWE, InsertMenuW, IsIconic, IsWindowVisible, IsZoomed, LoadCursorW,
            LoadIconW, MF_BYCOMMAND, MF_CHECKED, MF_ENABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
            MF_UNCHECKED, MSG, ModifyMenuW, MoveWindow, PostMessageW, PostQuitMessage,
            RegisterClassW, SC_CLOSE, SIZE_MINIMIZED, SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE,
            SW_RESTORE, SW_SHOW, SendMessageW, SetCursor, SetTimer, SetWindowLongPtrW,
            SetWindowTextW, ShowWindow, TranslateMessage, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE,
            WM_COMMAND, WM_COPY, WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_INITMENUPOPUP,
            WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
            WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_PASTE, WM_SETCURSOR, WM_SETFOCUS, WM_SIZE,
            WM_SYSCOMMAND, WM_TIMER, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN,
            WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        },
    },
};

use crate::{
    client::{ipc_address, ipc_socket_addr},
    commands::{option_value, positional_values, screenshot_output_path, tmux_key_bytes},
    instances::intentional_shutdown_matches,
    locale::UiText,
    platform::{
        CapabilityStatus, KeyClassification, action,
        windows::{
            activation, clipboard,
            input::{Utf16TextDecoder, primary_shortcut, windows_modifiers},
            screenshot::{self, CaptureArea},
            toolbar::WindowsToolbarHit,
        },
    },
    protocol::IpcResponse,
    settings::{
        AppConfig, EffectiveTerminalAppearance, MAX_TERMINAL_FONT_SIZE, MIN_TERMINAL_FONT_SIZE,
        TerminalAppearanceOverride, clamp_tabs_width, config_path, load_config, save_config,
    },
    tab_tree::{TabTreeNode, tree_rows},
    theme::{Rgb, ThemeId, ThemePalette},
    ui_bridge::{
        UI_TAB_NOTE_MAX_BYTES, UI_TAB_TITLE_MAX_BYTES, UiCellStyle, UiColor, UiScreenSnapshot,
        UiTabBootstrap,
    },
    ui_client::{UiClientModel, tab_by_id},
    ui_clipboard::{TERMINAL_PASTE_LIMIT_BYTES, normalize_terminal_paste},
    ui_command::UiClientCommand,
    ui_geometry::{
        PixelRect, TAB_HEIGHT, TAB_TOP, TERMINAL_SCROLLBAR_WIDTH, TerminalScrollbarGeometry,
        TreeRowActionDensity, TreeRowGeometry, TreeRowMode, WorkspaceLayout, WorkspaceLayoutInput,
        pixel_rect_json, reset_tabs_width, scrollback_for_thumb_top, sidebar_scrollbar_track,
        sidebar_tree_row_geometry, tabs_width_from_drag, terminal_scrollbar_geometry,
        tree_connector_segments, tree_row_at_y, workspace_layout,
    },
    working_context::parse_proxy_url,
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
const TAB_CLOSE_CONFIRM_ID: usize = 2119;
const TAB_CLOSE_CANCEL_ID: usize = 2120;
const NEW_DEFAULT_SHELL_ID: usize = 2121;
const NEW_CMD_SHELL_ID: usize = 2122;
const NEW_POWERSHELL_ID: usize = 2123;
const NEW_INITIAL_COMMAND_ID: usize = 2124;
const NEW_HTTP_PROXY_ID: usize = 2125;
const NEW_HTTPS_PROXY_ID: usize = 2126;
const NEW_CREATE_ID: usize = 2127;
const NEW_CANCEL_ID: usize = 2128;
const LOCALE_ID: usize = 2129;
const FONT_DECREASE_ID: usize = 2130;
const FONT_INCREASE_ID: usize = 2131;
const SETTINGS_DEFAULT_SCOPE_ID: usize = 2132;
const SETTINGS_CURRENT_SCOPE_ID: usize = 2133;
const SETTINGS_FONT_INHERIT_ID: usize = 2134;
const SETTINGS_SIZE_INHERIT_ID: usize = 2135;
const SETTINGS_THEME_INHERIT_ID: usize = 2136;
const SETTINGS_RESET_OVERRIDES_ID: usize = 2137;
const SYSTEM_MENU_COPY_ID: usize = 0x1f00;
const SYSTEM_MENU_PASTE_ID: usize = 0x1f10;
const SYSTEM_MENU_TOGGLE_TABS_ID: usize = 0x1f20;
const WM_APP_AUTOMATION_SHORTCUT: u32 = 0x8000 + 2;
const WM_APP_FOCUS_QUERY: u32 = 0x8000 + 3;
const WM_APP_DESTROY_WINDOW: u32 = 0x8000 + 4;

const fn windows_toolbar_hit(control_id: usize) -> Option<WindowsToolbarHit> {
    match control_id {
        TABS_ID => Some(WindowsToolbarHit::ToggleTabs),
        NEW_ID => Some(WindowsToolbarHit::NewTab),
        SETTINGS_ID => Some(WindowsToolbarHit::Settings),
        LOCALE_ID => Some(WindowsToolbarHit::ToggleLocale),
        FONT_DECREASE_ID => Some(WindowsToolbarHit::FontDecrease),
        FONT_INCREASE_ID => Some(WindowsToolbarHit::FontIncrease),
        _ => None,
    }
}

fn platform_capability_error(status: CapabilityStatus) -> anyhow::Error {
    match status {
        CapabilityStatus::Failed { code, message } => anyhow::anyhow!("{code}: {message}"),
        CapabilityStatus::Unsupported { reason } => {
            anyhow::anyhow!("platform capability is unsupported: {reason}")
        }
        CapabilityStatus::Available => {
            anyhow::anyhow!("platform capability failed without a typed diagnostic")
        }
    }
}
const STATUS_HEIGHT: i32 = 26;
const COMPOSER_HEIGHT: i32 = 104;
const MARGIN: i32 = 6;
const RECONNECT_INTERVAL: Duration = Duration::from_millis(500);
const SERVER_RESTART_INTERVAL: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(8);
const WINDOW_CLOSE_BUTTON_TEXT_FORMAT: u32 = DT_CENTER | DT_SINGLELINE | DT_VCENTER;

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
    window_class.style = CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW;
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
    let locale = create_button(window, instance, LOCALE_ID, "En|繁");
    let font_decrease = create_button(window, instance, FONT_DECREASE_ID, "z");
    let font_increase = create_button(window, instance, FONT_INCREASE_ID, "Z");
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
    let settings_default_scope = create_hidden_button(
        window,
        instance,
        SETTINGS_DEFAULT_SCOPE_ID,
        "Default values",
    );
    let settings_current_scope = create_hidden_button(
        window,
        instance,
        SETTINGS_CURRENT_SCOPE_ID,
        "Current terminal",
    );
    let settings_font_inherit = create_hidden_button(
        window,
        instance,
        SETTINGS_FONT_INHERIT_ID,
        "Inherit default",
    );
    let settings_size_inherit = create_hidden_button(
        window,
        instance,
        SETTINGS_SIZE_INHERIT_ID,
        "Inherit default",
    );
    let settings_theme_inherit = create_hidden_button(
        window,
        instance,
        SETTINGS_THEME_INHERIT_ID,
        "Inherit default",
    );
    let settings_reset_overrides = create_hidden_button(
        window,
        instance,
        SETTINGS_RESET_OVERRIDES_ID,
        "Reset overrides",
    );
    let tab_close_confirm =
        create_hidden_button(window, instance, TAB_CLOSE_CONFIRM_ID, "Terminate && Close");
    let tab_close_cancel = create_hidden_button(window, instance, TAB_CLOSE_CANCEL_ID, "Cancel");
    let new_default_shell = create_hidden_button(window, instance, NEW_DEFAULT_SHELL_ID, "Default");
    let new_cmd_shell = create_hidden_button(window, instance, NEW_CMD_SHELL_ID, "Command Prompt");
    let new_powershell = create_hidden_button(window, instance, NEW_POWERSHELL_ID, "PowerShell");
    let new_initial_command = create_hidden_edit(window, instance, NEW_INITIAL_COMMAND_ID);
    let new_http_proxy = create_hidden_edit(window, instance, NEW_HTTP_PROXY_ID);
    let new_https_proxy = create_hidden_edit(window, instance, NEW_HTTPS_PROXY_ID);
    let new_create = create_hidden_button(window, instance, NEW_CREATE_ID, "Create");
    let new_cancel = create_hidden_button(window, instance, NEW_CANCEL_ID, "Cancel");
    if edit.is_null()
        || send.is_null()
        || new_tab.is_null()
        || tabs.is_null()
        || settings.is_null()
        || locale.is_null()
        || font_decrease.is_null()
        || font_increase.is_null()
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
        || settings_default_scope.is_null()
        || settings_current_scope.is_null()
        || settings_font_inherit.is_null()
        || settings_size_inherit.is_null()
        || settings_theme_inherit.is_null()
        || settings_reset_overrides.is_null()
        || tab_close_confirm.is_null()
        || tab_close_cancel.is_null()
        || new_default_shell.is_null()
        || new_cmd_shell.is_null()
        || new_powershell.is_null()
        || new_initial_command.is_null()
        || new_http_proxy.is_null()
        || new_https_proxy.is_null()
        || new_create.is_null()
        || new_cancel.is_null()
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
            locale,
            font_decrease,
            font_increase,
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
            settings_default_scope,
            settings_current_scope,
            settings_font_inherit,
            settings_size_inherit,
            settings_theme_inherit,
            settings_reset_overrides,
            tab_close_confirm,
            tab_close_cancel,
            new_default_shell,
            new_cmd_shell,
            new_powershell,
            new_initial_command,
            new_http_proxy,
            new_https_proxy,
            new_create,
            new_cancel,
        },
        client_id,
        client,
    )?);
    unsafe {
        SetWindowLongPtrW(window, GWLP_USERDATA, Box::into_raw(state) as isize);
        SetTimer(window, TIMER_ID, 100, None);
    }
    if let Some(state) = state_mut(window) {
        state.apply_locale();
        state.layout();
        state.load_composer();
        state.resize_active_terminal();
    }
    if no_activate {
        activation::show_without_activation(window)
            .map_err(|error| platform_capability_error(error.to_capability_status()))?;
    } else {
        activation::show_new_and_request_activation(window);
    }
    unsafe { UpdateWindow(window) };

    let mut message: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        if message.message == WM_KEYDOWN
            && let Some(state) = state_mut(window)
            && (state.handle_cwd_editor_keydown(message.wParam as u32)
                || state.handle_keyboard_navigation(message.wParam as u32))
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
            if server_endpoint_is_listening() {
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

fn server_endpoint_is_listening() -> bool {
    ipc_socket_addr().is_ok_and(|address| {
        std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok()
    })
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
    locale: HWND,
    font_decrease: HWND,
    font_increase: HWND,
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
    settings_default_scope: HWND,
    settings_current_scope: HWND,
    settings_font_inherit: HWND,
    settings_size_inherit: HWND,
    settings_theme_inherit: HWND,
    settings_reset_overrides: HWND,
    tab_close_confirm: HWND,
    tab_close_cancel: HWND,
    new_default_shell: HWND,
    new_cmd_shell: HWND,
    new_powershell: HWND,
    new_initial_command: HWND,
    new_http_proxy: HWND,
    new_https_proxy: HWND,
    new_create: HWND,
    new_cancel: HWND,
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

#[derive(Clone, Copy)]
enum RemoteTabAction {
    AddChild,
    Edit,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsScope {
    Defaults,
    CurrentTerminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppearanceField {
    FontFamily,
    FontSize,
    Theme,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NewShellChoice {
    Default,
    CommandPrompt,
    PowerShell,
}

impl NewShellChoice {
    const fn id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::CommandPrompt => "cmd",
            Self::PowerShell => "powershell",
        }
    }
}

#[derive(Clone)]
struct RemoteTreeRow {
    tab_index: usize,
    depth: usize,
    is_last: bool,
    guides: Vec<bool>,
    has_children: bool,
    collapsed: bool,
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

#[derive(Clone, Copy)]
struct RemoteSidebarScrollDrag {
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

fn remote_composer_identity(
    client: &UiClientModel,
) -> Option<(String, Option<String>, bool, usize)> {
    let tab_id = client.snapshot().active_tab_id.clone()?;
    let composer = &tab_by_id(client.snapshot(), &tab_id)?.composer;
    Some((
        tab_id,
        composer.text.clone(),
        composer.sensitive,
        composer.byte_length,
    ))
}

struct RemoteWindowState {
    window: HWND,
    edit: HWND,
    send: HWND,
    new_tab: HWND,
    tabs_button: HWND,
    settings: HWND,
    locale: HWND,
    font_decrease: HWND,
    font_increase: HWND,
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
    settings_default_scope: HWND,
    settings_current_scope: HWND,
    settings_font_inherit: HWND,
    settings_size_inherit: HWND,
    settings_theme_inherit: HWND,
    settings_reset_overrides: HWND,
    tab_close_confirm: HWND,
    tab_close_cancel: HWND,
    new_default_shell: HWND,
    new_cmd_shell: HWND,
    new_powershell: HWND,
    new_initial_command: HWND,
    new_http_proxy: HWND,
    new_https_proxy: HWND,
    new_create: HWND,
    new_cancel: HWND,
    client_id: String,
    client: Option<UiClientModel>,
    reconnect_after: Instant,
    server_restart_after: Instant,
    server_restart_suppressed: bool,
    last_message: Option<String>,
    last_error: Option<String>,
    tabs_visible: bool,
    config: AppConfig,
    font: HFONT,
    cell_width: i32,
    cell_height: i32,
    terminal_text_decoder: Utf16TextDecoder,
    last_active_id: Option<String>,
    last_composer_identity: Option<(String, Option<String>, bool, usize)>,
    tabs_resize_dragging: bool,
    editing_tab_id: Option<String>,
    window_close_pending: bool,
    settings_open: bool,
    new_terminal_open: bool,
    new_shell_choice: NewShellChoice,
    settings_theme_draft: ThemeId,
    settings_scope: SettingsScope,
    settings_default_draft: EffectiveTerminalAppearance,
    settings_override_draft: TerminalAppearanceOverride,
    settings_target_tab_id: Option<String>,
    terminal_selection: Option<RemoteTerminalSelection>,
    scroll_drag: Option<RemoteScrollDrag>,
    sidebar_scroll_offset: usize,
    sidebar_scroll_drag: Option<RemoteSidebarScrollDrag>,
    focus_surface: RemoteFocusSurface,
    pending_close_tab_id: Option<String>,
    cwd_edit_tab_id: Option<String>,
    last_published_snapshot: Option<String>,
    relay_close_after_completion: Option<RemoteCloseChoice>,
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
            locale,
            font_decrease,
            font_increase,
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
            settings_default_scope,
            settings_current_scope,
            settings_font_inherit,
            settings_size_inherit,
            settings_theme_inherit,
            settings_reset_overrides,
            tab_close_confirm,
            tab_close_cancel,
            new_default_shell,
            new_cmd_shell,
            new_powershell,
            new_initial_command,
            new_http_proxy,
            new_https_proxy,
            new_create,
            new_cancel,
        } = controls;
        let config = load_config();
        let settings_theme_draft = config.color_theme;
        let settings_default_draft = config.effective_terminal_appearance(&ipc_address(), None);
        let last_active_id = client.snapshot().active_tab_id.clone();
        let appearance =
            config.effective_terminal_appearance(&ipc_address(), last_active_id.as_deref());
        let (font, cell_width, cell_height) = create_terminal_font(
            window,
            &appearance.terminal_font_family,
            appearance.terminal_font_size,
        )?;
        let last_composer_identity = remote_composer_identity(&client);
        Ok(Self {
            window,
            edit,
            send,
            new_tab,
            tabs_button,
            settings,
            locale,
            font_decrease,
            font_increase,
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
            settings_default_scope,
            settings_current_scope,
            settings_font_inherit,
            settings_size_inherit,
            settings_theme_inherit,
            settings_reset_overrides,
            tab_close_confirm,
            tab_close_cancel,
            new_default_shell,
            new_cmd_shell,
            new_powershell,
            new_initial_command,
            new_http_proxy,
            new_https_proxy,
            new_create,
            new_cancel,
            client_id,
            client: Some(client),
            reconnect_after: Instant::now(),
            server_restart_after: Instant::now(),
            server_restart_suppressed: false,
            last_message: None,
            last_error: None,
            tabs_visible: config.tabs_visible,
            config,
            font,
            cell_width,
            cell_height,
            terminal_text_decoder: Utf16TextDecoder::default(),
            last_active_id,
            last_composer_identity,
            tabs_resize_dragging: false,
            editing_tab_id: None,
            window_close_pending: false,
            settings_open: false,
            new_terminal_open: false,
            new_shell_choice: NewShellChoice::Default,
            settings_theme_draft,
            settings_scope: SettingsScope::Defaults,
            settings_default_draft,
            settings_override_draft: TerminalAppearanceOverride::default(),
            settings_target_tab_id: None,
            terminal_selection: None,
            scroll_drag: None,
            sidebar_scroll_offset: 0,
            sidebar_scroll_drag: None,
            focus_surface: RemoteFocusSurface::Terminal,
            pending_close_tab_id: None,
            cwd_edit_tab_id: None,
            last_published_snapshot: None,
            relay_close_after_completion: None,
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
                self.reconcile_tab_close();
                self.reconcile_cwd_editor();
                let active = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                let composer = self.client.as_ref().and_then(remote_composer_identity);
                if active != self.last_active_id || composer != self.last_composer_identity {
                    self.last_active_id = active;
                    self.last_composer_identity = composer;
                    if let Err(error) = self.apply_effective_terminal_font() {
                        self.last_error = Some(format!("Terminal font update failed: {error:#}"));
                    }
                    self.load_composer();
                }
                self.last_error = None;
                let command_changed = match self.process_client_command() {
                    Ok(changed) => changed,
                    Err(error) => {
                        self.last_error =
                            Some(format!("UI client command relay failed: {error:#}"));
                        true
                    }
                };
                match self.publish_ui_snapshot() {
                    Ok(published) => changed || command_changed || published,
                    Err(error) => {
                        self.last_error =
                            Some(format!("UI snapshot publication failed: {error:#}"));
                        true
                    }
                }
            }
            Err(error) => {
                let disconnected_server_pid = self.client.as_ref().map(UiClientModel::server_pid);
                // A failed poll invalidates this causal client projection. Do not
                // keep rendering it as connected or accept input against stale
                // server-owned state while recovery is in progress.
                self.client = None;
                if disconnected_server_pid
                    .is_some_and(|pid| intentional_shutdown_matches(&ipc_address(), pid))
                {
                    self.server_restart_suppressed = true;
                }
                self.show_workspace_controls(false);
                self.show_tab_editor(false);
                self.show_tab_close_controls(false);
                self.last_error = Some(format!("{error:#}"));
                if Instant::now() >= self.reconnect_after {
                    self.reconnect_after = Instant::now() + RECONNECT_INTERVAL;
                    match UiClientModel::connect(self.client_id.clone()) {
                        Ok(client) => {
                            self.client = Some(client);
                            self.server_restart_after = Instant::now();
                            self.server_restart_suppressed = false;
                            self.last_published_snapshot = None;
                            self.editing_tab_id = None;
                            self.terminal_selection = None;
                            self.pending_close_tab_id = None;
                            self.cwd_edit_tab_id = None;
                            self.show_tab_editor(false);
                            self.show_tab_close_controls(false);
                            self.apply_locale();
                            self.show_workspace_controls(true);
                            self.last_active_id = self
                                .client
                                .as_ref()
                                .and_then(|client| client.snapshot().active_tab_id.clone());
                            self.last_composer_identity =
                                self.client.as_ref().and_then(remote_composer_identity);
                            self.last_error = None;
                            if let Err(error) = self.apply_effective_terminal_font() {
                                self.last_error =
                                    Some(format!("Terminal font update failed: {error:#}"));
                            }
                            self.load_composer();
                            self.resize_active_terminal();
                            return true;
                        }
                        Err(reconnect_error) => {
                            let now = Instant::now();
                            let recovery = if now >= self.server_restart_after
                                && !self.server_restart_suppressed
                                && !server_endpoint_is_listening()
                            {
                                self.server_restart_after = now + SERVER_RESTART_INTERVAL;
                                match start_server_process() {
                                    Ok(()) => Some(
                                        "Server disappeared; recovery server started".to_owned(),
                                    ),
                                    Err(error) => {
                                        Some(format!("Server recovery failed: {error:#}"))
                                    }
                                }
                            } else {
                                None
                            };
                            self.last_error =
                                Some(recovery.unwrap_or_else(|| {
                                    format!("Disconnected: {reconnect_error:#}")
                                }));
                        }
                    }
                }
                true
            }
        }
    }

    fn process_client_command(&mut self) -> Result<bool> {
        let Some(command) = self
            .client
            .as_mut()
            .context("replaceable UI is disconnected")?
            .poll_client_command()?
        else {
            return Ok(false);
        };
        let outcome = self.execute_client_command(&command);
        let close_after_completion = outcome
            .as_ref()
            .ok()
            .and_then(|_| self.relay_close_after_completion.take());
        let response = match outcome {
            Ok(Some(output)) => IpcResponse::success(output),
            Ok(None) if close_after_completion.is_some() => {
                IpcResponse::success(self.detached_ui_snapshot_json()?)
            }
            Ok(None) => IpcResponse::success(self.ui_snapshot_json()?),
            Err(error) => {
                let error = format!("{error:#}");
                self.last_error = Some(error.clone());
                IpcResponse::typed_failure(error, "ui_client_command_failed", "command", false)
            }
        };
        self.client
            .as_mut()
            .context("replaceable UI is disconnected")?
            .complete_client_command(
                &command.command_id,
                &response,
                close_after_completion.is_some(),
                matches!(
                    close_after_completion,
                    Some(RemoteCloseChoice::StopServerAndExit)
                ),
            )?;
        if let Some(choice) = close_after_completion {
            self.window_close_pending = false;
            match choice {
                RemoteCloseChoice::KeepServerRunning | RemoteCloseChoice::StopServerAndExit => {
                    request_window_destroy(self.window);
                }
                RemoteCloseChoice::Cancel => {}
            }
            return Ok(true);
        }
        if response.ok
            && serde_json::from_str::<serde_json::Value>(&response.output)
                .ok()
                .is_some_and(|value| value["projection"].as_str() == Some("replaceable_ui_client"))
        {
            self.last_published_snapshot = Some(response.output);
        }
        Ok(true)
    }

    fn execute_client_command(&mut self, command: &UiClientCommand) -> Result<Option<String>> {
        if command.args.first().map(String::as_str) != Some("ui-action") {
            return self.execute_client_local_command(command);
        }
        let action = command
            .args
            .get(1)
            .map(String::as_str)
            .context("ui-action requires an action")?;
        match action {
            "new-tab" => {
                self.sync_composer()?;
                self.apply_client_command(&command.command_id)?;
                self.last_active_id = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                self.load_composer();
                self.resize_active_terminal();
            }
            "new-child" => {
                self.sync_composer()?;
                self.apply_client_command(&command.command_id)?;
                self.last_active_id = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                self.load_composer();
                self.resize_active_terminal();
                if let Some(tab) = self.active_tab().cloned() {
                    self.begin_tab_edit(tab);
                }
            }
            "select-tab" => {
                self.sync_composer()?;
                self.apply_client_command(&command.command_id)?;
                self.focus_surface = RemoteFocusSurface::Terminal;
                self.last_active_id = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                self.load_composer();
                self.resize_active_terminal();
                unsafe { SetFocus(self.window) };
            }
            "toggle-tree" => {
                self.apply_client_command(&command.command_id)?;
                self.reconcile_tab_editor();
            }
            "composer-send" => {
                self.sync_composer()?;
                self.apply_client_command(&command.command_id)?;
                unsafe { SetWindowTextW(self.edit, wide("").as_ptr()) };
            }
            "tabs-show" => self.set_tabs_visible(true),
            "tabs-hide" => self.set_tabs_visible(false),
            "tabs-toggle" | "toggle-tabs" => self.toggle_tabs(),
            "tabs-set-width" => {
                let width = option_value(&command.args, "--width")
                    .and_then(|value| value.parse::<i32>().ok())
                    .context("tabs-set-width requires numeric --width")?;
                self.config.tabs_width = clamp_tabs_width(width);
                save_config(&self.config).context("could not save Tabs width")?;
                self.layout();
                self.resize_active_terminal();
            }
            "edit-tab" => {
                let tab = self.command_target_tab(&command.args)?;
                self.begin_tab_edit(tab);
            }
            "tab-editor-save" => {
                if self.editing_tab_id.is_none() {
                    anyhow::bail!("no tab editor is open");
                }
                self.finish_tab_edit(true);
                if self.editing_tab_id.is_some() {
                    anyhow::bail!(
                        "{}",
                        self.last_error
                            .clone()
                            .unwrap_or_else(|| "tab edit could not be saved".to_owned())
                    );
                }
            }
            "tab-editor-cancel" => {
                if self.editing_tab_id.is_none() {
                    anyhow::bail!("no tab editor is open");
                }
                self.finish_tab_edit(false);
            }
            "open-settings" => {
                self.open_settings();
                if !self.settings_open {
                    anyhow::bail!("Settings could not be opened");
                }
            }
            "toggle-locale" => self.toggle_locale(),
            "font-decrease" => self.adjust_active_terminal_font(-1),
            "font-increase" => self.adjust_active_terminal_font(1),
            "settings-defaults" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.switch_settings_scope(SettingsScope::Defaults);
            }
            "settings-current" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.switch_settings_scope(SettingsScope::CurrentTerminal);
            }
            "settings-font-toggle" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.toggle_settings_inheritance(AppearanceField::FontFamily);
            }
            "settings-size-toggle" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.toggle_settings_inheritance(AppearanceField::FontSize);
            }
            "settings-theme-toggle" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.toggle_settings_inheritance(AppearanceField::Theme);
            }
            "settings-reset-overrides" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.reset_settings_overrides();
            }
            "settings-theme-dark" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.preview_settings_theme(ThemeId::Dark);
            }
            "settings-theme-light" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.preview_settings_theme(ThemeId::Light);
            }
            "settings-apply" => {
                if !self.settings_open {
                    anyhow::bail!("Settings is not open");
                }
                self.finish_settings(true);
                if self.settings_open {
                    anyhow::bail!(
                        "{}",
                        self.last_error
                            .clone()
                            .unwrap_or_else(|| "Settings could not be applied".to_owned())
                    );
                }
            }
            "open-cwd-editor" => {
                if let Some(target) = option_value(&command.args, "-t")
                    && self.active_tab().is_none_or(|tab| tab.id != target)
                {
                    self.client
                        .as_mut()
                        .context("UI is disconnected")?
                        .select_tab(target)?;
                    self.client
                        .as_mut()
                        .context("UI is disconnected")?
                        .poll_deltas()?;
                }
                self.open_cwd_editor();
                if self.cwd_edit_tab_id.is_none() {
                    anyhow::bail!("CWD editor could not be opened");
                }
            }
            "cwd-prepare" | "cwd-prepare-append" | "cwd-prepare-replace" | "cwd-send-now" => {
                let mut direct = vec![action.to_owned()];
                direct.extend(command.args.iter().skip(2).cloned());
                self.client
                    .as_mut()
                    .context("UI is disconnected")?
                    .run_control(direct)?;
                self.client
                    .as_mut()
                    .context("UI is disconnected")?
                    .poll_deltas()?;
            }
            "close-tab" => {
                let tab = self.command_target_tab(&command.args)?;
                if tab.dead {
                    if !self.close_tab_now(tab.id) {
                        anyhow::bail!("dead tab could not be closed");
                    }
                } else {
                    self.sync_composer()?;
                    self.finish_tab_edit(false);
                    self.pending_close_tab_id = Some(tab.id);
                    self.show_workspace_controls(false);
                    self.layout_tab_close_controls();
                    unsafe { SetFocus(self.window) };
                }
            }
            "confirm" => {
                if self.pending_close_tab_id.is_none() {
                    anyhow::bail!("no confirmation is pending");
                }
                self.finish_close_tab(true);
                if self.pending_close_tab_id.is_some() {
                    anyhow::bail!("tab close could not be confirmed");
                }
            }
            "cancel" => {
                if self.window_close_pending {
                    self.finish_window_close(RemoteCloseChoice::Cancel);
                } else if self.new_terminal_open {
                    self.finish_new_terminal(false);
                } else if self.settings_open {
                    self.finish_settings(false);
                } else if self.cwd_edit_tab_id.is_some() {
                    self.finish_cwd_editor(false);
                } else if self.pending_close_tab_id.is_some() {
                    self.finish_close_tab(false);
                } else if self.editing_tab_id.is_some() {
                    self.finish_tab_edit(false);
                } else {
                    anyhow::bail!("no modal is pending");
                }
            }
            "copy-selection" => self.copy_terminal_selection()?,
            "close-window" => {
                self.request_window_close();
                if !self.window_close_pending {
                    anyhow::bail!("window-close confirmation could not be opened");
                }
            }
            "window-minimize" => unsafe {
                ShowWindow(self.window, SW_MINIMIZE);
            },
            "window-maximize" => unsafe {
                ShowWindow(self.window, SW_MAXIMIZE);
            },
            "window-restore" => unsafe {
                ShowWindow(self.window, SW_RESTORE);
            },
            "window-resize" => {
                let width = option_value(&command.args, "--width")
                    .and_then(|value| value.parse::<i32>().ok())
                    .filter(|value| *value >= 320)
                    .context("window-resize requires --width of at least 320")?;
                let height = option_value(&command.args, "--height")
                    .and_then(|value| value.parse::<i32>().ok())
                    .filter(|value| *value >= 240)
                    .context("window-resize requires --height of at least 240")?;
                let mut current_client: RECT = unsafe { mem::zeroed() };
                let mut outer: RECT = unsafe { mem::zeroed() };
                unsafe {
                    GetClientRect(self.window, &mut current_client);
                    GetWindowRect(self.window, &mut outer);
                    MoveWindow(
                        self.window,
                        outer.left,
                        outer.top,
                        width + (outer.right - outer.left) - current_client.right,
                        height + (outer.bottom - outer.top) - current_client.bottom,
                        1,
                    );
                }
            }
            "keep-server-running" => {
                if !self.window_close_pending {
                    anyhow::bail!("no window-close confirmation is pending");
                }
                self.relay_close_after_completion = Some(RemoteCloseChoice::KeepServerRunning);
            }
            "stop-server-and-exit" => {
                if !self.window_close_pending {
                    anyhow::bail!("no window-close confirmation is pending");
                }
                self.relay_close_after_completion = Some(RemoteCloseChoice::StopServerAndExit);
            }
            action if action.starts_with("proxy-") || action == "open-proxy-editor" => {
                anyhow::bail!("proxy workbench controls are archived")
            }
            other => anyhow::bail!("unknown UI action: {other}"),
        }
        unsafe { windows_sys::Win32::Graphics::Gdi::InvalidateRect(self.window, ptr::null(), 0) };
        Ok(None)
    }

    fn execute_client_local_command(
        &mut self,
        command: &UiClientCommand,
    ) -> Result<Option<String>> {
        let name = command
            .args
            .first()
            .map(String::as_str)
            .context("relayed UI command is empty")?;
        let mut output = None;
        match name {
            "set-composer" => {
                let target = option_value(&command.args, "-t")
                    .or_else(|| self.active_tab().map(|tab| tab.id.as_str()))
                    .context("set-composer requires an active tab")?;
                if self.editing_tab_id.as_deref() != Some(target) {
                    anyhow::bail!("set-composer target is not open in the inline tab editor");
                }
                let text = positional_values(&command.args, &["-t"], &[]).join(" ");
                let normalized = text.replace("\r\n", "\n");
                let (title, note) = normalized
                    .split_once('\n')
                    .unwrap_or((normalized.as_str(), ""));
                unsafe {
                    SetWindowTextW(self.tab_title_edit, wide(title).as_ptr());
                    SetWindowTextW(self.tab_note_edit, wide(note).as_ptr());
                    SetFocus(self.tab_title_edit);
                }
            }
            "focus" => {
                self.apply_client_command(&command.command_id)?;
                let surface = command
                    .args
                    .get(1)
                    .map(String::as_str)
                    .unwrap_or("terminal");
                let target = match surface {
                    "terminal" => RemoteFocusSurface::Terminal,
                    "composer" => RemoteFocusSurface::Composer,
                    "tabs" | "sidebar" => RemoteFocusSurface::Tabs,
                    other => anyhow::bail!("unknown focus surface: {other}"),
                };
                if !self.set_focus_surface(target) {
                    anyhow::bail!("focus surface is unavailable: {surface}");
                }
            }
            "get-settings" => {
                let active_tab_id = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.as_deref());
                let terminal_override = active_tab_id.and_then(|tab_id| {
                    self.config
                        .terminal_override_entry(&ipc_address(), tab_id)
                        .cloned()
                });
                let effective = self
                    .config
                    .effective_terminal_appearance(&ipc_address(), active_tab_id);
                output = Some(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "terminal_font_family": self.config.terminal_font_family,
                        "terminal_font_size": self.config.terminal_font_size,
                        "color_theme": self.config.color_theme.as_str(),
                        "locale": self.config.locale.as_str(),
                        "active_tab_id": active_tab_id,
                        "current_terminal_override": terminal_override,
                        "effective": {
                            "terminal_font_family": effective.terminal_font_family,
                            "terminal_font_size": effective.terminal_font_size,
                            "color_theme": effective.color_theme.as_str(),
                        },
                        "resolved_font_family": effective.terminal_font_family,
                        "config_path": config_path(),
                        "recommended_cjk_font": "Sarasa Fixed SC",
                        "recommended_font_license": "SIL Open Font License 1.1",
                    }))
                    .context("could not encode Settings")?,
                );
            }
            "set-setting" => {
                let key = command
                    .args
                    .get(1)
                    .map(String::as_str)
                    .context("set-setting requires a key")?;
                let value = command.args.get(2..).unwrap_or_default().join(" ");
                let mut next = self.config.clone();
                match key {
                    "terminal.font-family" if !value.trim().is_empty() => {
                        next.terminal_font_family = value;
                    }
                    "terminal.font-family" => anyhow::bail!("font family cannot be empty"),
                    "terminal.font-size" => {
                        let size = value
                            .parse::<u16>()
                            .context("font size must be a number from 8 to 36")?;
                        if !(8..=36).contains(&size) {
                            anyhow::bail!("font size must be from 8 to 36");
                        }
                        next.terminal_font_size = size;
                    }
                    other => anyhow::bail!("unknown setting: {other}"),
                }
                let appearance = next.effective_terminal_appearance(
                    &ipc_address(),
                    self.client
                        .as_ref()
                        .and_then(|client| client.snapshot().active_tab_id.as_deref()),
                );
                let (font, cell_width, cell_height) = create_terminal_font(
                    self.window,
                    &appearance.terminal_font_family,
                    appearance.terminal_font_size,
                )?;
                if let Err(error) = save_config(&next) {
                    unsafe { DeleteObject(font as HGDIOBJ) };
                    return Err(error).context("could not save settings");
                }
                unsafe { DeleteObject(self.font as HGDIOBJ) };
                self.font = font;
                self.cell_width = cell_width;
                self.cell_height = cell_height;
                self.config = next;
                self.layout();
                self.resize_active_terminal();
            }
            "screenshot" => {
                self.paint();
                let path = screenshot_output_path(&command.args, "agenterm-window");
                screenshot::save_png(self.window, &path, CaptureArea::Window)
                    .map_err(|error| platform_capability_error(error.to_capability_status()))?;
                output = Some(path.display().to_string());
            }
            "screenshot-pane" | "screenshot-tab" => {
                if let Some(target) = option_value(&command.args, "-t") {
                    let stable = self.resolve_stable_tab_target(target)?;
                    if self.active_tab().is_none_or(|tab| tab.id != stable) {
                        self.client
                            .as_mut()
                            .context("UI is disconnected")?
                            .select_tab(&stable)?;
                        self.client
                            .as_mut()
                            .context("UI is disconnected")?
                            .poll_deltas()?;
                        self.load_composer();
                        self.resize_active_terminal();
                    }
                }
                self.paint();
                let path = screenshot_output_path(&command.args, "agenterm-pane");
                let terminal = self.workspace_geometry().terminal;
                screenshot::save_png(
                    self.window,
                    &path,
                    CaptureArea::Client {
                        left: terminal.left,
                        top: terminal.top,
                        width: terminal.width(),
                        height: terminal.height(),
                    },
                )
                .map_err(|error| platform_capability_error(error.to_capability_status()))?;
                output = Some(path.display().to_string());
            }
            "__focus" => {
                activation::restore_and_activate(self.window)
                    .map_err(|error| platform_capability_error(error.to_capability_status()))?;
                unsafe { SetFocus(self.window) };
            }
            "__show-no-activate" => activation::show_without_activation(self.window)
                .map_err(|error| platform_capability_error(error.to_capability_status()))?,
            other => anyhow::bail!("unsupported relayed UI command: {other}"),
        }
        unsafe { windows_sys::Win32::Graphics::Gdi::InvalidateRect(self.window, ptr::null(), 0) };
        Ok(output)
    }

    fn resolve_stable_tab_target(&mut self, target: &str) -> Result<String> {
        if target.starts_with('@') {
            return Ok(target.to_owned());
        }
        let response = self
            .client
            .as_mut()
            .context("UI is disconnected")?
            .run_control(vec![
                "display-message".to_owned(),
                "-p".to_owned(),
                "-t".to_owned(),
                target.to_owned(),
                "#{window_id}".to_owned(),
            ])?;
        let stable = response.output.trim();
        if !stable.starts_with('@') {
            anyhow::bail!("can't resolve tab target: {target}");
        }
        Ok(stable.to_owned())
    }

    fn apply_client_command(&mut self, command_id: &str) -> Result<()> {
        self.client
            .as_mut()
            .context("UI is disconnected")?
            .apply_client_command(command_id)?;
        self.client
            .as_mut()
            .context("UI is disconnected")?
            .poll_deltas()?;
        Ok(())
    }

    fn detached_ui_snapshot_json(&self) -> Result<String> {
        let mut value: serde_json::Value = serde_json::from_str(&self.ui_snapshot_json()?)
            .context("could not decode replaceable UI snapshot")?;
        value["window"]["visible"] = serde_json::Value::Bool(false);
        value["window"]["detached"] = serde_json::Value::Bool(true);
        value["layout"]["composer"]["visible"] = serde_json::Value::Bool(false);
        value["layout"]["composer"]["input_visible"] = serde_json::Value::Bool(false);
        value["layout"]["composer"]["send_visible"] = serde_json::Value::Bool(false);
        value["modal"] = serde_json::Value::Null;
        value["tab_editor"] = serde_json::Value::Null;
        value["focus"]["surface"] = serde_json::Value::String("terminal".to_owned());
        serde_json::to_string_pretty(&value)
            .context("could not encode detached replaceable UI snapshot")
    }

    fn command_target_tab(&self, args: &[String]) -> Result<UiTabBootstrap> {
        let client = self
            .client
            .as_ref()
            .context("replaceable UI is disconnected")?;
        let target = option_value(args, "-t")
            .or(client.snapshot().active_tab_id.as_deref())
            .context("no tab is active")?;
        client
            .snapshot()
            .tabs
            .iter()
            .find(|tab| tab.id == target)
            .cloned()
            .with_context(|| format!("can't find stable tab: {target}"))
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

    fn reconcile_tab_close(&mut self) {
        let still_exists = self.pending_close_tab_id.as_ref().is_some_and(|id| {
            self.client
                .as_ref()
                .is_some_and(|client| client.snapshot().tabs.iter().any(|tab| &tab.id == id))
        });
        if self.pending_close_tab_id.is_some() && !still_exists {
            self.pending_close_tab_id = None;
            self.show_tab_close_controls(false);
            self.show_workspace_controls(true);
        }
    }

    fn reconcile_cwd_editor(&mut self) {
        let still_active = self.cwd_edit_tab_id.as_ref().is_some_and(|id| {
            self.client
                .as_ref()
                .is_some_and(|client| client.snapshot().active_tab_id.as_ref() == Some(id))
        });
        if self.cwd_edit_tab_id.is_some() && !still_active {
            self.cwd_edit_tab_id = None;
            unsafe { SetWindowTextW(self.send, wide("Send").as_ptr()) };
            self.load_composer();
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

    fn sidebar_row_capacity(&self) -> usize {
        let height = self.workspace_geometry().sidebar_tree.height();
        usize::try_from((height - TAB_TOP).max(0) / TAB_HEIGHT)
            .unwrap_or_default()
            .max(1)
    }

    fn sidebar_row_count(&self) -> usize {
        self.client
            .as_ref()
            .map(|client| remote_tree_rows(&client.snapshot().tabs).len())
            .unwrap_or_default()
    }

    fn sidebar_max_offset(&self) -> usize {
        self.sidebar_row_count()
            .saturating_sub(self.sidebar_row_capacity())
    }

    fn sidebar_offset(&self) -> usize {
        self.sidebar_scroll_offset.min(self.sidebar_max_offset())
    }

    fn sidebar_row_geometry(
        &self,
        visual_position: usize,
        depth: usize,
        mode: TreeRowMode,
    ) -> TreeRowGeometry {
        sidebar_tree_row_geometry(
            self.workspace_geometry().sidebar_tree,
            visual_position,
            depth,
            mode,
        )
    }

    fn sidebar_scrollbar_state(&self) -> Option<(TerminalScrollbarGeometry, usize, usize)> {
        if !self.tabs_visible {
            return None;
        }
        let layout = self.workspace_geometry();
        let track = sidebar_scrollbar_track(layout.sidebar_tree);
        let maximum = self.sidebar_max_offset();
        let offset = self.sidebar_offset();
        let track_height = track.height().max(0);
        let total = self.sidebar_row_count().max(1);
        let visible = self.sidebar_row_capacity().min(total).max(1);
        let proportional = (i64::from(track_height) * visible as i64 / total as i64) as i32;
        let thumb_height = if maximum == 0 {
            track_height
        } else {
            proportional.max(24).min(track_height)
        };
        let travel = (track_height - thumb_height).max(0);
        let thumb_top = if maximum == 0 {
            track.top
        } else {
            track.top + (offset as i64 * i64::from(travel) / maximum as i64) as i32
        };
        Some((
            TerminalScrollbarGeometry {
                track,
                thumb: PixelRect {
                    left: track.left + 2,
                    top: thumb_top,
                    right: (track.right - 2).max(track.left + 2),
                    bottom: thumb_top + thumb_height,
                },
            },
            offset,
            maximum,
        ))
    }

    fn sidebar_row_index_at_y(&self, y: i32) -> Option<usize> {
        tree_row_at_y(y).map(|position| self.sidebar_offset().saturating_add(position))
    }

    fn ui_snapshot_json(&self) -> Result<String> {
        let client = self
            .client
            .as_ref()
            .context("replaceable UI is disconnected")?;
        let source = client.snapshot();
        let active_override = source.active_tab_id.as_deref().and_then(|tab_id| {
            self.config
                .terminal_override_entry(&ipc_address(), tab_id)
                .cloned()
        });
        let effective = self
            .config
            .effective_terminal_appearance(&ipc_address(), source.active_tab_id.as_deref());
        let mut client_rect: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client_rect) };
        let layout = self.workspace_geometry();
        let visible_rows = remote_tree_rows(&source.tabs);
        let tabs = source
            .tabs
            .iter()
            .map(|tab| {
                let visible_position = self
                    .tabs_visible
                    .then(|| {
                        let source_position = visible_rows
                            .iter()
                            .position(|row| source.tabs[row.tab_index].id == tab.id)?;
                        source_position
                            .checked_sub(self.sidebar_offset())
                            .filter(|position| *position < self.sidebar_row_capacity())
                    })
                    .flatten();
                let depth = remote_tab_depth(&source.tabs, tab);
                let mode = if self.editing_tab_id.as_ref() == Some(&tab.id) {
                    TreeRowMode::Editing
                } else {
                    TreeRowMode::Normal
                };
                let geometry = visible_position
                    .map(|position| self.sidebar_row_geometry(position, depth, mode));
                let selection = self
                    .terminal_selection
                    .as_ref()
                    .filter(|selection| selection.tab_id == tab.id)
                    .map(|selection| {
                        let (start, end) = selection.bounds();
                        serde_json::json!({
                            "start": {"row": start.row, "col": start.column},
                            "end": {"row": end.row, "col": end.column},
                            "dragging": selection.dragging,
                        })
                    });
                let actions = visible_position
                    .filter(|_| source.active_tab_id.as_ref() == Some(&tab.id))
                    .map(|position| {
                        let geometry = self.sidebar_row_geometry(position, depth, mode);
                        let action = |id: &str, label: &str, bounds: PixelRect| {
                            serde_json::json!({
                                "id": id,
                                "label": label,
                                "bounds": pixel_rect_json(bounds),
                                "x": bounds.left,
                                "y": bounds.top,
                                "width": bounds.width(),
                                "height": bounds.height(),
                            })
                        };
                        match mode {
                            TreeRowMode::Normal => serde_json::json!({
                                "mode": "normal",
                                "density": tree_action_density_name(geometry.actions.density),
                                "new_child": action(
                                    "new-child",
                                    self.config.locale.text(UiText::Add),
                                    geometry.actions.add_child.expect("normal row has Add"),
                                ),
                                "edit": action(
                                    "edit-tab",
                                    self.config.locale.text(UiText::Edit),
                                    geometry.actions.primary,
                                ),
                                "close": action(
                                    "close-tab",
                                    self.config.locale.text(UiText::Close),
                                    geometry.actions.secondary,
                                ),
                            }),
                            TreeRowMode::Editing => serde_json::json!({
                                "mode": "editing",
                                "density": tree_action_density_name(geometry.actions.density),
                                "save": action(
                                    "tab-editor-save",
                                    self.config.locale.text(UiText::Save),
                                    geometry.actions.primary,
                                ),
                                "cancel": action(
                                    "tab-editor-cancel",
                                    self.config.locale.text(UiText::Cancel),
                                    geometry.actions.secondary,
                                ),
                            }),
                        }
                    });
                serde_json::json!({
                    "id": tab.id,
                    "index": tab.index,
                    "parent_id": tab.parent_id,
                    "depth": depth,
                    "has_children": source.tabs.iter().any(
                        |candidate| candidate.parent_id.as_ref() == Some(&tab.id)
                    ),
                    "collapsed": tab.collapsed,
                    "visible": visible_position.is_some(),
                    "name": tab.title,
                    "terminal_title": tab.screen.terminal_title,
                    "note": tab.note,
                    "active": source.active_tab_id.as_ref() == Some(&tab.id),
                    "pid": tab.process_id,
                    "state": if tab.dead { "dead" } else { "running" },
                    "exit_code": tab.exit_code,
                    "working_context": {
                        "cwd": {
                            "path": tab.working_context.cwd,
                            "confirmed_path": tab.working_context.cwd_confirmed_path,
                            "confirmed": tab.working_context.cwd_confirmed,
                            "source": tab.working_context.cwd_source,
                            "pending": tab.working_context.cwd_request_pending,
                        },
                        "shell": tab.working_context.shell,
                        "proxy": {
                            "configured": tab.working_context.proxy_configured,
                            "source": tab.working_context.proxy_source,
                            "application_state": tab.working_context.proxy_application_state,
                            "request_pending": tab.working_context.proxy_request_pending,
                            "endpoint_visible": false,
                            "credential_revealed": false,
                        },
                    },
                    "scrollback_offset": tab.screen.scrollback_offset,
                    "selection": selection,
                    "draft": tab.composer.byte_length > 0,
                    "bounds": geometry.map(|value| pixel_rect_json(value.row)),
                    "render": geometry.map(|value| serde_json::json!({
                        "mode": match value.mode {
                            TreeRowMode::Normal => "normal",
                            TreeRowMode::Editing => "editing",
                        },
                        "row": pixel_rect_json(value.row),
                        "selection": pixel_rect_json(value.selection),
                        "node": {"x": value.node_x, "y": value.node_y},
                        "expander": pixel_rect_json(value.expander),
                        "status": pixel_rect_json(value.status),
                        "disclosure_hit": pixel_rect_json(value.disclosure_hit),
                        "text": pixel_rect_json(value.text),
                        "name": pixel_rect_json(value.name),
                        "note": pixel_rect_json(value.note),
                        "editors": value.editors.map(|editors| serde_json::json!({
                            "name": pixel_rect_json(editors.name),
                            "note": pixel_rect_json(editors.note),
                        })),
                    })),
                    "actions": actions,
                })
            })
            .collect::<Vec<_>>();
        let scrollbar = self.scrollbar_state().map(|(geometry, offset, maximum)| {
            serde_json::json!({
                "visible": true,
                "track": pixel_rect_json(geometry.track),
                "thumb": pixel_rect_json(geometry.thumb),
                "offset": offset,
                "max_offset": maximum,
            })
        });
        let sidebar_scrollbar =
            self.sidebar_scrollbar_state()
                .map(|(geometry, offset, maximum)| {
                    serde_json::json!({
                        "visible": true,
                        "track": pixel_rect_json(geometry.track),
                        "thumb": pixel_rect_json(geometry.thumb),
                        "offset": offset,
                        "max_offset": maximum,
                    })
                });
        let (rows, columns) = self
            .active_tab()
            .map(|tab| (tab.screen.rows, tab.screen.columns))
            .unwrap_or_default();
        let tab_editor = self.editing_tab_id.as_ref().map(|id| {
            let focused = unsafe { GetFocus() };
            serde_json::json!({
                "target": id,
                "name_length": window_text(self.tab_title_edit).chars().count(),
                "note_length": window_text(self.tab_note_edit).chars().count(),
                "focus": if focused == self.tab_title_edit {
                    Some("name")
                } else if focused == self.tab_note_edit {
                    Some("note")
                } else {
                    None
                },
            })
        });
        let modal = if self.window_close_pending {
            Some(serde_json::json!({
                "kind": "confirm-window-close",
                "default_action": "keep-server-running",
                "actions": ["keep-server-running", "stop-server-and-exit", "cancel"],
                "buttons": [
                    close_button_snapshot("keep-server-running", "Keep Server Running"),
                    close_button_snapshot("stop-server-and-exit", "Stop Server & Exit"),
                    close_button_snapshot("cancel", "Cancel"),
                ],
            }))
        } else if self.new_terminal_open {
            Some(serde_json::json!({
                "kind": "new-terminal",
                "shell": self.new_shell_choice.id(),
                "initial_command_configured":
                    !window_text(self.new_initial_command).trim().is_empty(),
                "http_proxy_configured":
                    !window_text(self.new_http_proxy).trim().is_empty(),
                "https_proxy_configured":
                    !window_text(self.new_https_proxy).trim().is_empty(),
                "proxy_values_exposed": false,
                "default_action": "create",
                "actions": ["create", "cancel"],
            }))
        } else if self.settings_open {
            Some(serde_json::json!({"kind": "settings"}))
        } else if let Some(id) = &self.cwd_edit_tab_id {
            Some(serde_json::json!({
                "kind": "cwd-editor",
                "window_id": id,
                "default_action": "cwd-prepare",
                "actions": [
                    "cwd-prepare",
                    "cwd-prepare-append",
                    "cwd-prepare-replace",
                    "cwd-send-now",
                    "cancel"
                ],
            }))
        } else {
            self.pending_close_tab_id.as_ref().map(|id| {
                serde_json::json!({
                    "kind": "confirm-close-live",
                    "window_id": id,
                })
            })
        };
        let (copy_enabled, paste_enabled) = self.system_menu_state();
        let composer_input = PixelRect {
            left: layout.composer.left + MARGIN,
            top: layout.composer.top + 26,
            right: (layout.composer.right - 76 - MARGIN * 2)
                .max(layout.composer.left + MARGIN + 80),
            bottom: (layout.composer.bottom - 8).max(layout.composer.top + 56),
        };
        let focus = if self.window_close_pending {
            "window-close"
        } else if self.new_terminal_open {
            "new-terminal"
        } else if self.settings_open {
            "settings"
        } else if self.cwd_edit_tab_id.is_some() {
            "cwd-editor"
        } else if self.pending_close_tab_id.is_some() {
            "tab-close"
        } else if self.editing_tab_id.is_some() {
            "tab-editor"
        } else {
            match self.current_focus_surface() {
                RemoteFocusSurface::Terminal => "terminal",
                RemoteFocusSurface::Composer => "composer",
                RemoteFocusSurface::Tabs => "tabs",
            }
        };
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": crate::ui_bridge::UI_CLIENT_STATE_SCHEMA_VERSION,
            "protocol_version": 1,
            "projection": "replaceable_ui_client",
            "client_pid": std::process::id(),
            "server_pid": source.server_pid,
            "event_position": {
                "epoch": source.server_epoch,
                "sequence": source.position.sequence,
            },
            "window": {
                "title": window_text(self.window),
                "client_width": client_rect.right,
                "client_height": client_rect.bottom,
                "visible": unsafe { IsWindowVisible(self.window) } != 0,
                "detached": false,
                "minimized": unsafe { IsIconic(self.window) } != 0,
                "state": if unsafe { IsIconic(self.window) } != 0 {
                    "minimized"
                } else if unsafe { IsZoomed(self.window) } != 0 {
                    "maximized"
                } else {
                    "restored"
                },
            },
            "layout": {
                "sidebar": {
                    "x": layout.sidebar.left,
                    "y": layout.sidebar.top,
                    "visible": self.tabs_visible,
                    "configured_width": self.config.tabs_width,
                    "effective_width": layout.effective_tabs_width,
                    "width": layout.sidebar.width(),
                    "height": layout.sidebar.height(),
                    "bounds": pixel_rect_json(layout.sidebar),
                    "resize_grip": layout.resize_grip.map(pixel_rect_json),
                    "resizing": self.tabs_resize_dragging,
                    "scrollbar": sidebar_scrollbar,
                },
                "toolbar": layout.workspace_toolbar.map(|toolbar| serde_json::json!({
                    "bounds": pixel_rect_json(toolbar.bounds),
                    "new": pixel_rect_json(toolbar.new_tab),
                    "tabs": pixel_rect_json(toolbar.tabs),
                    "settings": pixel_rect_json(toolbar.settings),
                    "locale": pixel_rect_json(toolbar.locale),
                    "font_decrease": pixel_rect_json(toolbar.font_decrease),
                    "font_increase": pixel_rect_json(toolbar.font_increase),
                })),
                "terminal": {
                    "x": layout.terminal.left,
                    "y": layout.terminal.top,
                    "width": layout.terminal.width(),
                    "viewport_width": (
                        layout.terminal.width() - TERMINAL_SCROLLBAR_WIDTH
                    ).max(0),
                    "height": layout.terminal.height(),
                    "bounds": pixel_rect_json(layout.terminal),
                    "rows": rows,
                    "cols": columns,
                    "scrollbar": scrollbar,
                },
                "composer": {
                    "visible": unsafe { IsWindowVisible(self.edit) } != 0
                        && unsafe { IsWindowVisible(self.send) } != 0,
                    "input_visible": unsafe { IsWindowVisible(self.edit) } != 0,
                    "send_visible": unsafe { IsWindowVisible(self.send) } != 0,
                    "x": layout.composer.left,
                    "y": layout.composer.top,
                    "width": layout.composer.width(),
                    "height": layout.composer.height(),
                    "bounds": pixel_rect_json(layout.composer),
                    "input": {
                        "bounds": pixel_rect_json(composer_input),
                        "target_rows": 3,
                        "vertical_scrollbar": true,
                    },
                },
                "status_bar": {
                    "x": layout.status.left,
                    "y": layout.status.top,
                    "width": layout.status.width(),
                    "height": layout.status.height(),
                    "bounds": pixel_rect_json(layout.status),
                    "tabs_recovery": layout.status_segments.tabs_recovery.map(pixel_rect_json),
                    "cwd": {
                        "bounds": pixel_rect_json(layout.status_segments.cwd),
                        "action": "open-cwd-editor",
                    },
                    "proxy": {
                        "bounds": pixel_rect_json(layout.status_segments.proxy),
                        "available": false,
                        "archived": true,
                        "action": serde_json::Value::Null,
                        "eye_action": serde_json::Value::Null,
                    },
                    "provider": "placeholder",
                },
            },
            "focus": {
                "surface": focus,
                "window_id": source.active_tab_id,
            },
            "system_menu": {
                "toggle_tabs": {
                    "id": SYSTEM_MENU_TOGGLE_TABS_ID,
                    "label": self.config.locale.text(UiText::ToggleTabs),
                    "checked": self.tabs_visible,
                },
                "copy": {
                    "id": SYSTEM_MENU_COPY_ID,
                    "label": self.config.locale.text(UiText::Copy),
                    "enabled": copy_enabled,
                },
                "paste": {
                    "id": SYSTEM_MENU_PASTE_ID,
                    "label": self.config.locale.text(UiText::Paste),
                    "enabled": paste_enabled,
                },
            },
            "terminal_interaction": {
                "selection": self.terminal_selection.as_ref().map(|selection| {
                    let (start, end) = selection.bounds();
                    serde_json::json!({
                        "phase": if selection.dragging { "dragging" } else { "complete" },
                        "tab_id": selection.tab_id,
                        "selection": {
                            "start": {"row": start.row, "col": start.column},
                            "end": {"row": end.row, "col": end.column},
                        },
                        "autoscroll": {"active": false},
                    })
                }),
                "raw_mouse_arbitration": false,
                "rectangular_selection": false,
            },
            "tabs": tabs,
            "tab_editor": tab_editor,
            "modal": modal,
            "settings": {
                "terminal_font_family": self.config.terminal_font_family,
                "terminal_font_size": self.config.terminal_font_size,
                "color_theme": self.config.color_theme.as_str(),
                "scope": match self.settings_scope {
                    SettingsScope::Defaults => "defaults",
                    SettingsScope::CurrentTerminal => "current-terminal",
                },
                "target_tab_id": self.settings_target_tab_id,
                "current_terminal_override": active_override,
                "effective": {
                    "terminal_font_family": effective.terminal_font_family,
                    "terminal_font_size": effective.terminal_font_size,
                    "color_theme": effective.color_theme.as_str(),
                },
                "theme_draft": self.settings_open.then(
                    || self.settings_theme_draft.as_str()
                ),
                "theme_options": ThemeId::ALL.map(|theme| serde_json::json!({
                    "id": theme.as_str(),
                    "label": theme.label(),
                })),
                "tabs_visible": self.tabs_visible,
                "tabs_width": self.config.tabs_width,
            },
            "locale": {
                "id": self.config.locale.as_str(),
                "controls": {
                    "send": self.config.locale.text(UiText::Send),
                    "settings": self.config.locale.text(UiText::Settings),
                    "new": self.config.locale.text(UiText::New),
                    "apply": self.config.locale.text(UiText::Apply),
                    "save": self.config.locale.text(UiText::Save),
                },
            },
            "feedback": {
                "message": self.last_message,
                "error": self.last_error,
            },
        }))
        .context("could not encode replaceable UI snapshot")
    }

    fn publish_ui_snapshot(&mut self) -> Result<bool> {
        let json = self.ui_snapshot_json()?;
        if self.last_published_snapshot.as_deref() == Some(json.as_str()) {
            return Ok(false);
        }
        self.client
            .as_mut()
            .context("replaceable UI is disconnected")?
            .publish_snapshot(&json)?;
        self.last_published_snapshot = Some(json);
        Ok(true)
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

    fn cwd_status_rect(&self) -> RECT {
        win_rect(self.workspace_geometry().status_segments.cwd)
    }

    fn tabs_recovery_rect(&self) -> Option<RECT> {
        self.workspace_geometry()
            .status_segments
            .tabs_recovery
            .map(win_rect)
    }

    fn set_tabs_visible(&mut self, visible: bool) {
        self.finish_cwd_editor(false);
        self.finish_tab_edit(false);
        self.tabs_visible = visible;
        self.config.tabs_visible = visible;
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Tabs visibility save failed: {error:#}"));
        }
        if !visible && self.focus_surface == RemoteFocusSurface::Tabs {
            self.focus_surface = RemoteFocusSurface::Terminal;
            unsafe { SetFocus(self.window) };
        }
        self.layout();
        self.resize_active_terminal();
    }

    fn toggle_tabs(&mut self) {
        self.set_tabs_visible(!self.tabs_visible);
    }

    fn active_terminal_appearance(&self) -> EffectiveTerminalAppearance {
        let active = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.as_deref());
        self.config
            .effective_terminal_appearance(&ipc_address(), active)
    }

    fn apply_effective_terminal_font(&mut self) -> Result<()> {
        let appearance = self.active_terminal_appearance();
        let (font, cell_width, cell_height) = create_terminal_font(
            self.window,
            &appearance.terminal_font_family,
            appearance.terminal_font_size,
        )?;
        unsafe { DeleteObject(self.font as HGDIOBJ) };
        self.font = font;
        self.cell_width = cell_width;
        self.cell_height = cell_height;
        self.layout();
        self.resize_active_terminal();
        Ok(())
    }

    fn adjust_active_terminal_font(&mut self, delta: i16) {
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return;
        };
        let current = self.active_terminal_appearance().terminal_font_size;
        let next = i32::from(current).saturating_add(i32::from(delta)).clamp(
            i32::from(MIN_TERMINAL_FONT_SIZE),
            i32::from(MAX_TERMINAL_FONT_SIZE),
        ) as u16;
        if next == current {
            return;
        }
        let mut terminal_override = self.config.terminal_override(&ipc_address(), &tab_id);
        terminal_override.terminal_font_size = Some(next);
        self.config
            .set_terminal_override(&ipc_address(), &tab_id, terminal_override);
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Terminal font override save failed: {error:#}"));
            return;
        }
        if let Err(error) = self.apply_effective_terminal_font() {
            self.last_error = Some(format!("Terminal font update failed: {error:#}"));
        }
    }

    fn toggle_locale(&mut self) {
        self.config.locale = self.config.locale.toggled();
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Locale save failed: {error:#}"));
            return;
        }
        self.apply_locale();
        self.layout();
    }

    fn apply_locale(&self) {
        let locale = self.config.locale;
        let set = |control: HWND, text: &str| unsafe {
            SetWindowTextW(control, wide(text).as_ptr());
        };
        set(self.send, locale.text(UiText::Send));
        set(self.new_tab, locale.text(UiText::New));
        set(self.settings, locale.text(UiText::Settings));
        set(self.locale, locale.toolbar_label());
        set(self.tab_save, locale.text(UiText::Save));
        set(self.tab_cancel, locale.text(UiText::Cancel));
        set(self.close_keep, locale.text(UiText::KeepServerRunning));
        set(self.close_stop, locale.text(UiText::StopServerAndExit));
        set(self.close_cancel, locale.text(UiText::Cancel));
        set(self.settings_apply, locale.text(UiText::Apply));
        set(self.settings_cancel, locale.text(UiText::Cancel));
        set(
            self.settings_default_scope,
            locale.text(UiText::DefaultValues),
        );
        set(
            self.settings_current_scope,
            locale.text(UiText::CurrentTerminal),
        );
        set(
            self.settings_reset_overrides,
            locale.text(UiText::ResetOverrides),
        );
        set(
            self.tab_close_confirm,
            locale.text(UiText::TerminateAndClose),
        );
        set(self.tab_close_cancel, locale.text(UiText::Cancel));
        set(self.new_create, locale.text(UiText::Create));
        set(self.new_cancel, locale.text(UiText::Cancel));
        set(self.new_default_shell, locale.text(UiText::Default));
        let menu = unsafe { GetSystemMenu(self.window, 0) };
        if !menu.is_null() {
            for (id, label) in [
                (SYSTEM_MENU_COPY_ID, locale.text(UiText::Copy)),
                (SYSTEM_MENU_PASTE_ID, locale.text(UiText::Paste)),
                (SYSTEM_MENU_TOGGLE_TABS_ID, locale.text(UiText::ToggleTabs)),
            ] {
                unsafe {
                    ModifyMenuW(
                        menu,
                        id as u32,
                        MF_BYCOMMAND | MF_STRING,
                        id,
                        wide(label).as_ptr(),
                    );
                }
            }
        }
    }

    fn layout(&mut self) {
        let geometry = self.workspace_geometry();
        let composer = win_rect(geometry.composer);
        unsafe {
            SetWindowTextW(
                self.tabs_button,
                wide(&format!(
                    "{}{}",
                    if self.tabs_visible { '<' } else { '>' },
                    self.config.locale.text(UiText::Tabs)
                ))
                .as_ptr(),
            );
            if let Some(toolbar) = geometry.workspace_toolbar {
                for (window, bounds) in [
                    (self.new_tab, toolbar.new_tab),
                    (self.tabs_button, toolbar.tabs),
                    (self.settings, toolbar.settings),
                    (self.locale, toolbar.locale),
                    (self.font_decrease, toolbar.font_decrease),
                    (self.font_increase, toolbar.font_increase),
                ] {
                    MoveWindow(
                        window,
                        bounds.left,
                        bounds.top,
                        bounds.width(),
                        bounds.height(),
                        1,
                    );
                }
            }
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
                if geometry.workspace_toolbar.is_some()
                    && !self.window_close_pending
                    && !self.settings_open
                    && !self.new_terminal_open
                    && self.pending_close_tab_id.is_none()
                {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
            let workspace_command = if geometry.workspace_toolbar.is_some()
                && !self.window_close_pending
                && !self.settings_open
                && !self.new_terminal_open
            {
                SW_SHOW
            } else {
                SW_HIDE
            };
            ShowWindow(self.tabs_button, workspace_command);
            ShowWindow(self.settings, workspace_command);
            ShowWindow(self.locale, workspace_command);
            ShowWindow(self.font_decrease, workspace_command);
            ShowWindow(self.font_increase, workspace_command);
        }
        self.layout_tab_editor();
        self.layout_close_controls();
        self.layout_settings_controls();
        self.layout_tab_close_controls();
        self.layout_new_terminal_controls();
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
        let rows = remote_tree_rows(&client.snapshot().tabs);
        let Some((position, row)) = rows
            .iter()
            .enumerate()
            .find(|(_, row)| client.snapshot().tabs[row.tab_index].id == tab_id)
        else {
            self.show_tab_editor(false);
            return;
        };
        let Some(viewport_position) = position
            .checked_sub(self.sidebar_offset())
            .filter(|position| *position < self.sidebar_row_capacity())
        else {
            self.show_tab_editor(false);
            return;
        };
        let geometry =
            self.sidebar_row_geometry(viewport_position, row.depth, TreeRowMode::Editing);
        let Some(editors) = geometry.editors else {
            self.show_tab_editor(false);
            return;
        };
        unsafe {
            let name = win_rect(editors.name);
            let note = win_rect(editors.note);
            let save = win_rect(geometry.actions.primary);
            let cancel = win_rect(geometry.actions.secondary);
            MoveWindow(
                self.tab_title_edit,
                name.left,
                name.top,
                name.right - name.left,
                name.bottom - name.top,
                1,
            );
            MoveWindow(
                self.tab_note_edit,
                note.left,
                note.top,
                note.right - note.left,
                note.bottom - note.top,
                1,
            );
            MoveWindow(
                self.tab_save,
                save.left,
                save.top,
                save.right - save.left,
                save.bottom - save.top,
                1,
            );
            MoveWindow(
                self.tab_cancel,
                cancel.left,
                cancel.top,
                cancel.right - cancel.left,
                cancel.bottom - cancel.top,
                1,
            );
        }
        self.show_tab_editor(
            self.tabs_visible && !self.window_close_pending && self.pending_close_tab_id.is_none(),
        );
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
            let toolbar_command = if visible
                && self.workspace_geometry().workspace_toolbar.is_some()
                && self.pending_close_tab_id.is_none()
            {
                SW_SHOW
            } else {
                SW_HIDE
            };
            ShowWindow(self.tabs_button, toolbar_command);
            ShowWindow(self.settings, toolbar_command);
            ShowWindow(self.locale, toolbar_command);
            ShowWindow(self.font_decrease, toolbar_command);
            ShowWindow(self.font_increase, toolbar_command);
            ShowWindow(self.new_tab, toolbar_command);
        }
        self.show_tab_editor(visible && self.tabs_visible && self.editing_tab_id.is_some());
    }

    fn settings_modal_geometry(&self) -> (RECT, [RECT; 12]) {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let width = (client.right - 32).clamp(520, 680);
        let height = (client.bottom - 32).clamp(390, 430);
        let left = ((client.right - width) / 2).max(0);
        let top = ((client.bottom - height) / 2).max(0);
        let modal = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let default_scope = RECT {
            left: left + 32,
            top: top + 54,
            right: left + 210,
            bottom: top + 88,
        };
        let current_scope = RECT {
            left: default_scope.right + 8,
            top: default_scope.top,
            right: (default_scope.right + 228).min(left + width - 32),
            bottom: default_scope.bottom,
        };
        let font = RECT {
            left: left + 32,
            top: top + 132,
            right: left + width - 174,
            bottom: top + 164,
        };
        let font_inherit = RECT {
            left: left + width - 158,
            top: font.top,
            right: left + width - 32,
            bottom: font.bottom,
        };
        let size = RECT {
            left: left + 32,
            top: top + 204,
            right: left + 164,
            bottom: top + 236,
        };
        let size_inherit = RECT {
            left: left + width - 158,
            top: size.top,
            right: left + width - 32,
            bottom: size.bottom,
        };
        let dark = RECT {
            left: left + 32,
            top: top + 276,
            right: left + 178,
            bottom: top + 310,
        };
        let light = RECT {
            left: left + 190,
            top: dark.top,
            right: left + 336,
            bottom: dark.bottom,
        };
        let theme_inherit = RECT {
            left: left + width - 158,
            top: dark.top,
            right: left + width - 32,
            bottom: dark.bottom,
        };
        let reset = RECT {
            left: left + 32,
            top: top + height - 54,
            right: left + 174,
            bottom: top + height - 18,
        };
        let apply = RECT {
            left: left + width - 126,
            top: top + height - 54,
            right: left + width - 32,
            bottom: top + height - 18,
        };
        let cancel = RECT {
            left: apply.left - 106,
            top: apply.top,
            right: apply.left - 12,
            bottom: apply.bottom,
        };
        (
            modal,
            [
                default_scope,
                current_scope,
                font,
                font_inherit,
                size,
                size_inherit,
                dark,
                light,
                theme_inherit,
                reset,
                apply,
                cancel,
            ],
        )
    }

    fn layout_settings_controls(&self) {
        let (_, controls) = self.settings_modal_geometry();
        for (control, rect) in [
            (self.settings_default_scope, controls[0]),
            (self.settings_current_scope, controls[1]),
            (self.settings_font, controls[2]),
            (self.settings_font_inherit, controls[3]),
            (self.settings_size, controls[4]),
            (self.settings_size_inherit, controls[5]),
            (self.settings_dark, controls[6]),
            (self.settings_light, controls[7]),
            (self.settings_theme_inherit, controls[8]),
            (self.settings_reset_overrides, controls[9]),
            (self.settings_apply, controls[10]),
            (self.settings_cancel, controls[11]),
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
            ShowWindow(self.settings_default_scope, command);
            ShowWindow(self.settings_current_scope, command);
            let override_command =
                if visible && self.settings_scope == SettingsScope::CurrentTerminal {
                    SW_SHOW
                } else {
                    SW_HIDE
                };
            ShowWindow(self.settings_font_inherit, override_command);
            ShowWindow(self.settings_size_inherit, override_command);
            ShowWindow(self.settings_theme_inherit, override_command);
            ShowWindow(self.settings_reset_overrides, override_command);
        }
    }

    fn new_terminal_modal_geometry(&self) -> (RECT, [RECT; 8]) {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let width = (client.right - 32).clamp(480, 620);
        let height = (client.bottom - 32).clamp(390, 450);
        let left = ((client.right - width) / 2).max(0);
        let top = ((client.bottom - height) / 2).max(0);
        let modal = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let inner_left = left + 28;
        let inner_right = left + width - 28;
        let gap = 8;
        let shell_width = ((inner_right - inner_left - gap * 2) / 3).max(90);
        let shell_top = top + 74;
        let shell = |index: i32| RECT {
            left: inner_left + index * (shell_width + gap),
            top: shell_top,
            right: inner_left + index * (shell_width + gap) + shell_width,
            bottom: shell_top + 34,
        };
        let field = |field_top: i32| RECT {
            left: inner_left,
            top: field_top,
            right: inner_right,
            bottom: field_top + 30,
        };
        let create = RECT {
            left: inner_right - 96,
            top: top + height - 52,
            right: inner_right,
            bottom: top + height - 18,
        };
        let cancel = RECT {
            left: create.left - 106,
            top: create.top,
            right: create.left - 10,
            bottom: create.bottom,
        };
        (
            modal,
            [
                shell(0),
                shell(1),
                shell(2),
                field(top + 142),
                field(top + 218),
                field(top + 294),
                create,
                cancel,
            ],
        )
    }

    fn layout_new_terminal_controls(&self) {
        let (_, bounds) = self.new_terminal_modal_geometry();
        for (control, rect) in [
            (self.new_default_shell, bounds[0]),
            (self.new_cmd_shell, bounds[1]),
            (self.new_powershell, bounds[2]),
            (self.new_initial_command, bounds[3]),
            (self.new_http_proxy, bounds[4]),
            (self.new_https_proxy, bounds[5]),
            (self.new_create, bounds[6]),
            (self.new_cancel, bounds[7]),
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
        self.show_new_terminal_controls(self.new_terminal_open);
    }

    fn show_new_terminal_controls(&self, visible: bool) {
        let command = if visible { SW_SHOW } else { SW_HIDE };
        for control in [
            self.new_default_shell,
            self.new_cmd_shell,
            self.new_powershell,
            self.new_initial_command,
            self.new_http_proxy,
            self.new_https_proxy,
            self.new_create,
            self.new_cancel,
        ] {
            unsafe { ShowWindow(control, command) };
        }
    }

    fn tab_close_modal_geometry(&self) -> (RECT, [RECT; 2]) {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let width = (client.right - 32).clamp(380, 500);
        let height = 210;
        let left = ((client.right - width) / 2).max(0);
        let top = ((client.bottom - height) / 2).max(0);
        let modal = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let confirm = RECT {
            left: left + width - 250,
            top: top + 148,
            right: left + width - 116,
            bottom: top + 184,
        };
        let cancel = RECT {
            left: left + width - 104,
            top: top + 148,
            right: left + width - 24,
            bottom: top + 184,
        };
        (modal, [confirm, cancel])
    }

    fn layout_tab_close_controls(&self) {
        let (_, buttons) = self.tab_close_modal_geometry();
        for (control, rect) in [
            (self.tab_close_confirm, buttons[0]),
            (self.tab_close_cancel, buttons[1]),
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
        self.show_tab_close_controls(self.pending_close_tab_id.is_some());
    }

    fn show_tab_close_controls(&self, visible: bool) {
        let command = if visible { SW_SHOW } else { SW_HIDE };
        unsafe {
            ShowWindow(self.tab_close_confirm, command);
            ShowWindow(self.tab_close_cancel, command);
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
        if self.cwd_edit_tab_id.is_some() {
            return;
        }
        let text = self
            .active_tab()
            .and_then(|tab| tab.composer.text.as_deref())
            .unwrap_or_default();
        unsafe { SetWindowTextW(self.edit, wide(text).as_ptr()) };
    }

    fn sync_composer(&mut self) -> Result<()> {
        if self.cwd_edit_tab_id.is_some() {
            return Ok(());
        }
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
        if self.cwd_edit_tab_id.is_some() {
            self.finish_cwd_editor(true);
            return;
        }
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

    fn open_cwd_editor(&mut self) {
        if self.cwd_edit_tab_id.is_some()
            || self.window_close_pending
            || self.settings_open
            || self.new_terminal_open
            || self.pending_close_tab_id.is_some()
            || self.editing_tab_id.is_some()
        {
            return;
        }
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.cancel_terminal_selection();
        self.cwd_edit_tab_id = Some(tab.id);
        unsafe {
            SetWindowTextW(
                self.edit,
                wide(tab.working_context.cwd.as_deref().unwrap_or_default()).as_ptr(),
            );
            SetWindowTextW(self.send, wide("Prepare").as_ptr());
            SetFocus(self.edit);
        }
        self.focus_surface = RemoteFocusSurface::Composer;
        self.last_error = None;
    }

    fn finish_cwd_editor(&mut self, prepare: bool) {
        let Some(tab_id) = self.cwd_edit_tab_id.clone() else {
            return;
        };
        if prepare {
            let path = window_text(self.edit).trim().to_owned();
            if path.is_empty() {
                self.last_error = Some("CWD path cannot be empty".to_owned());
                return;
            }
            let result = self
                .client
                .as_mut()
                .context("UI is disconnected")
                .and_then(|client| {
                    client.run_control(vec![
                        "cwd-prepare-replace".to_owned(),
                        "-t".to_owned(),
                        tab_id,
                        "--path".to_owned(),
                        path,
                    ])?;
                    client.poll_deltas()?;
                    Ok(())
                });
            if let Err(error) = result {
                self.last_error = Some(format!("CWD prepare failed: {error:#}"));
                return;
            }
        }
        self.cwd_edit_tab_id = None;
        unsafe { SetWindowTextW(self.send, wide("Send").as_ptr()) };
        self.load_composer();
        self.focus_surface = RemoteFocusSurface::Composer;
        unsafe { SetFocus(self.edit) };
        self.last_error = None;
    }

    fn handle_cwd_editor_keydown(&mut self, key: u32) -> bool {
        if self.cwd_edit_tab_id.is_none() || unsafe { GetFocus() } != self.edit {
            return false;
        }
        if key == u32::from(VK_ESCAPE) {
            self.finish_cwd_editor(false);
            return true;
        }
        if key == 0x0d && unsafe { GetKeyState(VK_CONTROL as i32) } < 0 {
            self.finish_cwd_editor(true);
            return true;
        }
        false
    }

    fn open_new_terminal(&mut self) {
        if self.new_terminal_open
            || self.settings_open
            || self.window_close_pending
            || self.pending_close_tab_id.is_some()
        {
            return;
        }
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.finish_tab_edit(false);
        self.finish_cwd_editor(false);
        self.new_terminal_open = true;
        self.new_shell_choice = NewShellChoice::Default;
        self.last_error = None;
        unsafe {
            SetWindowTextW(self.new_initial_command, wide("").as_ptr());
            SetWindowTextW(self.new_http_proxy, wide("").as_ptr());
            SetWindowTextW(self.new_https_proxy, wide("").as_ptr());
        }
        self.refresh_new_shell_controls();
        self.show_workspace_controls(false);
        self.layout_new_terminal_controls();
        unsafe { SetFocus(self.new_initial_command) };
    }

    fn choose_new_shell(&mut self, choice: NewShellChoice) {
        if !self.new_terminal_open {
            return;
        }
        self.new_shell_choice = choice;
        self.refresh_new_shell_controls();
    }

    fn refresh_new_shell_controls(&self) {
        for (control, choice, label) in [
            (self.new_default_shell, NewShellChoice::Default, "Default"),
            (
                self.new_cmd_shell,
                NewShellChoice::CommandPrompt,
                "Command Prompt",
            ),
            (
                self.new_powershell,
                NewShellChoice::PowerShell,
                "PowerShell",
            ),
        ] {
            let selected = self.new_shell_choice == choice;
            unsafe {
                SetWindowTextW(
                    control,
                    wide(&format!("{} {label}", if selected { "●" } else { "○" })).as_ptr(),
                );
            }
        }
    }

    fn finish_new_terminal(&mut self, create: bool) {
        if !self.new_terminal_open {
            return;
        }
        if create {
            let initial = window_text(self.new_initial_command).trim().to_owned();
            let http_proxy = window_text(self.new_http_proxy).trim().to_owned();
            let https_proxy = window_text(self.new_https_proxy).trim().to_owned();
            for (label, value) in [
                ("HTTP proxy", http_proxy.as_str()),
                ("HTTPS proxy", https_proxy.as_str()),
            ] {
                if !value.is_empty() && parse_proxy_url(value).is_none() {
                    self.last_error =
                        Some(format!("{label} must be a valid http:// or https:// URL"));
                    return;
                }
            }

            let mut args = vec![
                "new-window".to_owned(),
                "-P".to_owned(),
                "-F".to_owned(),
                "#{window_id}".to_owned(),
            ];
            for (name, value) in [
                ("HTTP_PROXY", http_proxy.as_str()),
                ("HTTPS_PROXY", https_proxy.as_str()),
            ] {
                if !value.is_empty() {
                    args.push("-e".to_owned());
                    args.push(format!("{name}={value}"));
                }
            }
            let mut child = match self.new_shell_choice {
                NewShellChoice::Default if initial.is_empty() => Vec::new(),
                NewShellChoice::Default | NewShellChoice::CommandPrompt => {
                    let mut child = vec!["cmd.exe".to_owned(), "/d".to_owned()];
                    if !initial.is_empty() {
                        child.extend(["/k".to_owned(), initial]);
                    }
                    child
                }
                NewShellChoice::PowerShell => {
                    let mut child = vec!["powershell.exe".to_owned(), "-NoLogo".to_owned()];
                    if !initial.is_empty() {
                        child.extend(["-NoExit".to_owned(), "-Command".to_owned(), initial]);
                    }
                    child
                }
            };
            if !child.is_empty() {
                args.push("--".to_owned());
                args.append(&mut child);
            }
            let result = self.client.as_mut().context("UI is disconnected").and_then(
                |client| -> Result<()> {
                    client.run_control(args)?;
                    client.poll_deltas()?;
                    Ok(())
                },
            );
            if result.is_err() {
                self.last_error =
                    Some("New terminal could not be created; check its configuration".to_owned());
                return;
            }
        }
        self.new_terminal_open = false;
        self.show_new_terminal_controls(false);
        self.show_workspace_controls(true);
        self.layout();
        self.last_active_id = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone());
        self.load_composer();
        self.resize_active_terminal();
        self.focus_surface = RemoteFocusSurface::Terminal;
        unsafe { SetFocus(self.window) };
    }

    fn select_tab_at(&mut self, y: i32) {
        if !self.tabs_visible {
            return;
        }
        let Some(row_index) = self.sidebar_row_index_at_y(y) else {
            return;
        };
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| {
                remote_tree_rows(&client.snapshot().tabs)
                    .get(row_index)
                    .map(|row| &client.snapshot().tabs[row.tab_index])
            })
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
        let row_index = self.sidebar_row_index_at_y(y)?;
        let client = self.client.as_ref()?;
        let row = remote_tree_rows(&client.snapshot().tabs)
            .get(row_index)
            .cloned()?;
        client.snapshot().tabs.get(row.tab_index)
    }

    fn tab_action_at(&self, x: i32, y: i32) -> Option<RemoteTabAction> {
        let row_index = self.sidebar_row_index_at_y(y)?;
        let client = self.client.as_ref()?;
        let row = remote_tree_rows(&client.snapshot().tabs)
            .get(row_index)
            .cloned()?;
        let tab = client.snapshot().tabs.get(row.tab_index)?;
        if client.snapshot().active_tab_id.as_deref() != Some(tab.id.as_str()) {
            return None;
        }
        let viewport_position = row_index.checked_sub(self.sidebar_offset())?;
        let geometry = self.sidebar_row_geometry(viewport_position, row.depth, TreeRowMode::Normal);
        if geometry
            .actions
            .add_child
            .is_some_and(|bounds| bounds.contains(x, y))
        {
            Some(RemoteTabAction::AddChild)
        } else if geometry.actions.primary.contains(x, y) {
            Some(RemoteTabAction::Edit)
        } else if geometry.actions.secondary.contains(x, y) {
            Some(RemoteTabAction::Close)
        } else {
            None
        }
    }

    fn tab_disclosure_at(&self, x: i32, y: i32) -> Option<String> {
        let row_index = self.sidebar_row_index_at_y(y)?;
        let client = self.client.as_ref()?;
        let row = remote_tree_rows(&client.snapshot().tabs)
            .get(row_index)
            .cloned()?;
        if !row.has_children {
            return None;
        }
        let viewport_position = row_index.checked_sub(self.sidebar_offset())?;
        let geometry = self.sidebar_row_geometry(viewport_position, row.depth, TreeRowMode::Normal);
        geometry
            .disclosure_hit
            .contains(x, y)
            .then(|| client.snapshot().tabs[row.tab_index].id.clone())
    }

    fn toggle_tree_at(&mut self, x: i32, y: i32) -> bool {
        let Some(tab_id) = self.tab_disclosure_at(x, y) else {
            return false;
        };
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.invoke_client_action(vec![
                    "ui-action".to_owned(),
                    "toggle-tree".to_owned(),
                    "-t".to_owned(),
                    tab_id,
                ])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Toggle tree failed: {error:#}"));
        } else {
            self.last_error = None;
            self.reconcile_tab_editor();
        }
        true
    }

    fn new_child_at(&mut self, y: i32) {
        let Some(parent_id) = self.tab_at_y(y).map(|tab| tab.id.clone()) else {
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
                client.invoke_client_action(vec![
                    "ui-action".to_owned(),
                    "new-child".to_owned(),
                    "-t".to_owned(),
                    parent_id,
                ])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Add child failed: {error:#}"));
        } else {
            self.last_active_id = self
                .client
                .as_ref()
                .and_then(|client| client.snapshot().active_tab_id.clone());
            self.load_composer();
            self.resize_active_terminal();
            if let Some(child) = self.active_tab().cloned() {
                self.begin_tab_edit(child);
            }
        }
    }

    fn request_close_tab_at(&mut self, y: i32) {
        let Some(tab) = self.tab_at_y(y).cloned() else {
            return;
        };
        self.cancel_terminal_selection();
        if tab.dead {
            let _ = self.close_tab_now(tab.id);
            return;
        }
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.finish_tab_edit(false);
        self.pending_close_tab_id = Some(tab.id);
        self.show_workspace_controls(false);
        self.layout_tab_close_controls();
        unsafe { SetFocus(self.window) };
    }

    fn close_tab_now(&mut self, tab_id: String) -> bool {
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.run_control(vec!["kill-window".to_owned(), "-t".to_owned(), tab_id])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Close tab failed: {error:#}"));
            false
        } else {
            self.last_error = None;
            self.last_active_id = self
                .client
                .as_ref()
                .and_then(|client| client.snapshot().active_tab_id.clone());
            self.load_composer();
            self.resize_active_terminal();
            true
        }
    }

    fn finish_close_tab(&mut self, confirm: bool) {
        let pending = self.pending_close_tab_id.clone();
        if confirm
            && let Some(tab_id) = pending
            && !self.close_tab_now(tab_id)
        {
            return;
        }
        self.pending_close_tab_id = None;
        self.show_tab_close_controls(false);
        self.show_workspace_controls(true);
        self.focus_surface = RemoteFocusSurface::Tabs;
        unsafe { SetFocus(self.window) };
    }

    fn begin_tab_edit_at(&mut self, y: i32) {
        let Some(tab) = self.tab_at_y(y).cloned() else {
            return;
        };
        self.begin_tab_edit(tab);
    }

    fn begin_tab_edit(&mut self, tab: UiTabBootstrap) {
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
        if self.settings_open || self.new_terminal_open || self.window_close_pending {
            return;
        }
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.finish_tab_edit(false);
        self.settings_open = true;
        self.settings_scope = SettingsScope::Defaults;
        self.settings_default_draft = self
            .config
            .effective_terminal_appearance(&ipc_address(), None);
        self.settings_target_tab_id = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone());
        self.settings_override_draft = self
            .settings_target_tab_id
            .as_deref()
            .map(|tab_id| self.config.terminal_override(&ipc_address(), tab_id))
            .unwrap_or_default();
        self.load_settings_scope_controls();
        self.show_workspace_controls(false);
        self.layout_settings_controls();
        unsafe { SetFocus(self.settings_font) };
    }

    fn capture_settings_scope(&mut self) -> Result<()> {
        let family = window_text(self.settings_font).trim().to_owned();
        let size = window_text(self.settings_size)
            .trim()
            .parse::<u16>()
            .context("font size must be a number from 8 to 36")?;
        if family.is_empty()
            || family.len() > 256
            || !(MIN_TERMINAL_FONT_SIZE..=MAX_TERMINAL_FONT_SIZE).contains(&size)
        {
            anyhow::bail!("font family is required (maximum 256 bytes) and size must be 8 to 36");
        }
        match self.settings_scope {
            SettingsScope::Defaults => {
                self.settings_default_draft.terminal_font_family = family;
                self.settings_default_draft.terminal_font_size = size;
                self.settings_default_draft.color_theme = self.settings_theme_draft;
            }
            SettingsScope::CurrentTerminal => {
                if self.settings_override_draft.terminal_font_family.is_some() {
                    self.settings_override_draft.terminal_font_family = Some(family);
                }
                if self.settings_override_draft.terminal_font_size.is_some() {
                    self.settings_override_draft.terminal_font_size = Some(size);
                }
                if self.settings_override_draft.color_theme.is_some() {
                    self.settings_override_draft.color_theme = Some(self.settings_theme_draft);
                }
            }
        }
        Ok(())
    }

    fn switch_settings_scope(&mut self, scope: SettingsScope) {
        if !self.settings_open
            || scope == self.settings_scope
            || (scope == SettingsScope::CurrentTerminal && self.settings_target_tab_id.is_none())
        {
            return;
        }
        if let Err(error) = self.capture_settings_scope() {
            self.last_error = Some(format!("Settings draft invalid: {error:#}"));
            return;
        }
        self.settings_scope = scope;
        self.load_settings_scope_controls();
        self.layout_settings_controls();
    }

    fn load_settings_scope_controls(&mut self) {
        let locale = self.config.locale;
        let (family, size, theme) = match self.settings_scope {
            SettingsScope::Defaults => (
                self.settings_default_draft.terminal_font_family.clone(),
                self.settings_default_draft.terminal_font_size,
                self.settings_default_draft.color_theme,
            ),
            SettingsScope::CurrentTerminal => (
                self.settings_override_draft
                    .terminal_font_family
                    .clone()
                    .unwrap_or_else(|| self.settings_default_draft.terminal_font_family.clone()),
                self.settings_override_draft
                    .terminal_font_size
                    .unwrap_or(self.settings_default_draft.terminal_font_size),
                self.settings_override_draft
                    .color_theme
                    .unwrap_or(self.settings_default_draft.color_theme),
            ),
        };
        self.settings_theme_draft = theme;
        unsafe {
            SetWindowTextW(self.settings_font, wide(&family).as_ptr());
            SetWindowTextW(self.settings_size, wide(&size.to_string()).as_ptr());
            EnableWindow(
                self.settings_current_scope,
                self.settings_target_tab_id.is_some().into(),
            );
            let current = self.settings_scope == SettingsScope::CurrentTerminal;
            EnableWindow(
                self.settings_font,
                (!current || self.settings_override_draft.terminal_font_family.is_some()).into(),
            );
            EnableWindow(
                self.settings_size,
                (!current || self.settings_override_draft.terminal_font_size.is_some()).into(),
            );
            EnableWindow(
                self.settings_dark,
                (!current || self.settings_override_draft.color_theme.is_some()).into(),
            );
            EnableWindow(
                self.settings_light,
                (!current || self.settings_override_draft.color_theme.is_some()).into(),
            );
        }
        for (control, overridden) in [
            (
                self.settings_font_inherit,
                self.settings_override_draft.terminal_font_family.is_some(),
            ),
            (
                self.settings_size_inherit,
                self.settings_override_draft.terminal_font_size.is_some(),
            ),
            (
                self.settings_theme_inherit,
                self.settings_override_draft.color_theme.is_some(),
            ),
        ] {
            unsafe {
                SetWindowTextW(
                    control,
                    wide(locale.text(if overridden {
                        UiText::InheritDefault
                    } else {
                        UiText::Override
                    }))
                    .as_ptr(),
                );
            }
        }
        unsafe {
            SetWindowTextW(
                self.settings_default_scope,
                wide(&format!(
                    "{}{}",
                    locale.text(UiText::DefaultValues),
                    if self.settings_scope == SettingsScope::Defaults {
                        " · ✓"
                    } else {
                        ""
                    }
                ))
                .as_ptr(),
            );
            SetWindowTextW(
                self.settings_current_scope,
                wide(&format!(
                    "{}{}",
                    locale.text(UiText::CurrentTerminal),
                    if self.settings_scope == SettingsScope::CurrentTerminal {
                        " · ✓"
                    } else {
                        ""
                    }
                ))
                .as_ptr(),
            );
        }
        self.refresh_settings_theme_controls();
    }

    fn toggle_settings_inheritance(&mut self, field: AppearanceField) {
        if self.settings_scope != SettingsScope::CurrentTerminal {
            return;
        }
        if let Err(error) = self.capture_settings_scope() {
            self.last_error = Some(format!("Settings draft invalid: {error:#}"));
            return;
        }
        match field {
            AppearanceField::FontFamily => {
                self.settings_override_draft.terminal_font_family = self
                    .settings_override_draft
                    .terminal_font_family
                    .take()
                    .is_none()
                    .then(|| self.settings_default_draft.terminal_font_family.clone());
            }
            AppearanceField::FontSize => {
                self.settings_override_draft.terminal_font_size = self
                    .settings_override_draft
                    .terminal_font_size
                    .take()
                    .is_none()
                    .then_some(self.settings_default_draft.terminal_font_size);
            }
            AppearanceField::Theme => {
                self.settings_override_draft.color_theme = self
                    .settings_override_draft
                    .color_theme
                    .take()
                    .is_none()
                    .then_some(self.settings_default_draft.color_theme);
            }
        }
        self.load_settings_scope_controls();
    }

    fn reset_settings_overrides(&mut self) {
        if self.settings_scope != SettingsScope::CurrentTerminal {
            return;
        }
        self.settings_override_draft = TerminalAppearanceOverride::default();
        self.load_settings_scope_controls();
    }

    fn preview_settings_theme(&mut self, theme: ThemeId) {
        if !self.settings_open {
            return;
        }
        if self.settings_scope == SettingsScope::CurrentTerminal
            && self.settings_override_draft.color_theme.is_none()
        {
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
                self.config.locale.text(UiText::Selected)
            } else {
                self.config.locale.text(UiText::Preview)
            };
            unsafe {
                SetWindowTextW(
                    control,
                    wide(&format!(
                        "{} · {state}",
                        self.config.locale.text(match theme {
                            ThemeId::Dark => UiText::ThemeDark,
                            ThemeId::Light => UiText::Light,
                        })
                    ))
                    .as_ptr(),
                );
            }
        }
    }

    fn apply_settings(&mut self) -> Result<()> {
        self.capture_settings_scope()?;
        let mut next = self.config.clone();
        next.terminal_font_family = self.settings_default_draft.terminal_font_family.clone();
        next.terminal_font_size = self.settings_default_draft.terminal_font_size;
        next.color_theme = self.settings_default_draft.color_theme;
        if let Some(tab_id) = self.settings_target_tab_id.as_deref() {
            next.set_terminal_override(
                &ipc_address(),
                tab_id,
                self.settings_override_draft.clone(),
            );
        }
        let active = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.as_deref());
        let appearance = next.effective_terminal_appearance(&ipc_address(), active);
        let (font, cell_width, cell_height) = create_terminal_font(
            self.window,
            &appearance.terminal_font_family,
            appearance.terminal_font_size,
        )?;
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
        self.settings_theme_draft = self.active_terminal_appearance().color_theme;
        self.settings_target_tab_id = None;
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
        if self.cwd_edit_tab_id.is_some() {
            self.finish_cwd_editor(false);
        }
        if self.pending_close_tab_id.is_some() {
            self.finish_close_tab(false);
            return;
        }
        self.cancel_terminal_selection();
        if self.settings_open {
            self.finish_settings(false);
        }
        if self.new_terminal_open {
            self.finish_new_terminal(false);
        }
        if self.client.is_some()
            && let Err(error) = self.sync_composer()
        {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
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
                request_window_destroy(self.window);
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
                request_window_destroy(self.window);
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
        clipboard::set_text(self.window, &text)
            .map_err(|error| platform_capability_error(error.to_capability_status()))?;
        self.last_error = None;
        Ok(())
    }

    fn paste_terminal_clipboard(&mut self) -> Result<()> {
        let text = normalize_terminal_paste(
            &clipboard::get_text(TERMINAL_PASTE_LIMIT_BYTES)
                .map_err(|error| platform_capability_error(error.to_capability_status()))?,
        );
        if text.is_empty() {
            anyhow::bail!("clipboard text contains no pasteable characters");
        }
        if text.len() > TERMINAL_PASTE_LIMIT_BYTES {
            anyhow::bail!(
                "normalized clipboard text exceeds the {TERMINAL_PASTE_LIMIT_BYTES}-byte limit"
            );
        }
        let tab_id = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
            .context("no active terminal is available for paste")?;
        let characters = text.chars().count();
        self.last_error = None;
        self.terminal_input(text.as_bytes());
        if let Some(error) = self.last_error.take() {
            anyhow::bail!("{error}");
        }
        self.last_message = Some(format!("Pasted {characters} characters into {tab_id}"));
        Ok(())
    }

    fn is_edit_control(&self, window: HWND) -> bool {
        [
            self.edit,
            self.tab_title_edit,
            self.tab_note_edit,
            self.settings_font,
            self.settings_size,
            self.new_initial_command,
            self.new_http_proxy,
            self.new_https_proxy,
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
        if self.window_close_pending
            || self.settings_open
            || self.new_terminal_open
            || self.editing_tab_id.is_some()
            || self.pending_close_tab_id.is_some()
            || self.cwd_edit_tab_id.is_some()
        {
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
                    self.set_tabs_visible(true);
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
            return (true, clipboard::has_unicode_text());
        }
        let terminal_ready = focused == self.window
            && !self.window_close_pending
            && !self.settings_open
            && !self.new_terminal_open
            && self.active_tab().is_some_and(|tab| !tab.dead);
        (
            terminal_ready
                && self
                    .terminal_selection
                    .as_ref()
                    .is_some_and(|selection| !selection.is_empty()),
            terminal_ready && clipboard::has_unicode_text(),
        )
    }

    fn refresh_system_menu(&self) {
        let menu = unsafe { GetSystemMenu(self.window, 0) };
        if menu.is_null() {
            return;
        }
        let (copy, paste) = self.system_menu_state();
        unsafe {
            CheckMenuItem(
                menu,
                SYSTEM_MENU_TOGGLE_TABS_ID as u32,
                MF_BYCOMMAND
                    | if self.tabs_visible {
                        MF_CHECKED
                    } else {
                        MF_UNCHECKED
                    },
            );
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

    fn scroll_sidebar(&mut self, wheel_delta: i32) {
        let steps = (wheel_delta.unsigned_abs() as usize / 120).max(1) * 3;
        let maximum = self.sidebar_max_offset();
        self.sidebar_scroll_offset = if wheel_delta > 0 {
            self.sidebar_offset().saturating_sub(steps)
        } else {
            self.sidebar_offset().saturating_add(steps).min(maximum)
        };
        self.layout_tab_editor();
    }

    fn click_sidebar_scrollbar(&mut self, x: i32, y: i32) -> bool {
        let Some((geometry, current, maximum)) = self.sidebar_scrollbar_state() else {
            return false;
        };
        if !geometry.track.contains(x, y) {
            return false;
        }
        if maximum == 0 {
            return true;
        }
        if geometry.thumb.contains(x, y) {
            self.sidebar_scroll_drag = Some(RemoteSidebarScrollDrag {
                thumb_grab_offset: y - geometry.thumb.top,
            });
            unsafe { SetCapture(self.window) };
        } else {
            let page = self.sidebar_row_capacity().max(1);
            self.sidebar_scroll_offset = if y < geometry.thumb.top {
                current.saturating_sub(page)
            } else {
                current.saturating_add(page).min(maximum)
            };
            self.layout_tab_editor();
        }
        true
    }

    fn drag_sidebar_scrollbar(&mut self, y: i32) -> bool {
        let Some(drag) = self.sidebar_scroll_drag else {
            return false;
        };
        let Some((geometry, _, maximum)) = self.sidebar_scrollbar_state() else {
            self.end_sidebar_scroll_drag();
            return false;
        };
        let travel = geometry.track.height() - geometry.thumb.height();
        if maximum == 0 || travel <= 0 {
            self.sidebar_scroll_offset = 0;
        } else {
            let top = (y - drag.thumb_grab_offset).clamp(
                geometry.track.top,
                geometry.track.bottom - geometry.thumb.height(),
            );
            self.sidebar_scroll_offset = ((i64::from(top - geometry.track.top) * maximum as i64
                + i64::from(travel) / 2)
                / i64::from(travel)) as usize;
        }
        self.layout_tab_editor();
        true
    }

    fn end_sidebar_scroll_drag(&mut self) {
        if self.sidebar_scroll_drag.take().is_some() {
            unsafe { ReleaseCapture() };
        }
    }

    fn sidebar_scrollbar_capture_lost(&mut self) {
        self.sidebar_scroll_drag = None;
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

    fn reset_tabs_width(&mut self) {
        self.tabs_resize_dragging = false;
        unsafe { ReleaseCapture() };
        self.config.tabs_width = clamp_tabs_width(reset_tabs_width());
        self.layout();
        self.resize_active_terminal();
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Tabs width reset failed: {error:#}"));
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
        if let KeyClassification::TextCommit(text) = self.terminal_text_decoder.push(value) {
            match text.as_str() {
                "\u{8}" => self.terminal_input(b"\x7f"),
                "\r" => self.terminal_input(b"\r"),
                _ => self.terminal_input(text.as_bytes()),
            }
        }
    }

    fn terminal_key(&mut self, key: u32) -> bool {
        let key = u16::try_from(key).unwrap_or_default();
        let control = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
        let alt = unsafe { GetKeyState(VK_MENU as i32) } < 0;
        let primary = primary_shortcut(windows_modifiers(control, false, alt, false));
        if primary && key == u16::from(b'C') && self.terminal_selection.is_some() {
            if let Err(error) = self.copy_terminal_selection() {
                self.last_error = Some(format!("Copy failed: {error:#}"));
            }
            return true;
        }
        if primary && key == u16::from(b'V') {
            if let Err(error) = self.paste_terminal_clipboard() {
                self.last_error = Some(format!("Paste failed: {error:#}"));
            }
            return true;
        }
        if primary && (u16::from(b'A')..=u16::from(b'Z')).contains(&key) {
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

    fn dispatch_windows_toolbar_action(&mut self, action_id: &str) {
        if !action::is_toolbar_action_id(action_id) {
            return;
        }
        match action_id {
            action::TOGGLE_TABS => self.toggle_tabs(),
            action::NEW_TAB => self.open_new_terminal(),
            action::OPEN_SETTINGS => {
                self.finish_cwd_editor(false);
                self.open_settings();
            }
            action::TOGGLE_LOCALE => self.toggle_locale(),
            action::FONT_DECREASE => self.adjust_active_terminal_font(-1),
            action::FONT_INCREASE => self.adjust_active_terminal_font(1),
            _ => {}
        }
    }

    fn paint(&self) {
        let palette = if self.settings_open {
            self.settings_theme_draft.palette()
        } else {
            self.active_terminal_appearance().color_theme.palette()
        };
        let mut paint: PAINTSTRUCT = unsafe { mem::zeroed() };
        let device = unsafe { BeginPaint(self.window, &mut paint) };
        if device.is_null() {
            return;
        }
        let (sidebar, terminal, composer, status) = self.layout_rects();
        fill(device, &sidebar, palette.sidebar.colorref());
        if let Some(toolbar) = self.workspace_geometry().workspace_toolbar {
            let toolbar = win_rect(toolbar.bounds);
            fill(device, &toolbar, palette.composer.colorref());
        }
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
                &if self.cwd_edit_tab_id.as_deref() == Some(tab.id.as_str()) {
                    format!("CWD → {}  Ctrl+Enter prepares · Esc cancels", tab.id)
                } else {
                    format!(
                        "{} → {}  {}",
                        self.config.locale.text(UiText::Input),
                        tab.id,
                        tab.title
                    )
                },
                palette.muted_text.colorref(),
            );
        }
        let status_text = if let Some(error) = &self.last_error {
            error.clone()
        } else if let Some(client) = &self.client {
            format!(
                "Connected · server PID {} · {}",
                client.server_pid(),
                client.client_id()
            )
        } else {
            "Disconnected · reconnecting".to_owned()
        };
        let cwd_status = self.cwd_status_rect();
        let tabs_recovery = self.tabs_recovery_rect();
        if let Some(recovery) = tabs_recovery {
            frame(device, &recovery, palette.active_border.colorref());
            draw_text(
                device,
                RECT {
                    left: recovery.left + MARGIN,
                    top: recovery.top,
                    right: recovery.right - MARGIN,
                    bottom: recovery.bottom,
                },
                "Tabs",
                palette.muted_text.colorref(),
            );
        }
        draw_text(
            device,
            RECT {
                left: tabs_recovery.map_or(status.left + MARGIN, |rect| rect.right + MARGIN),
                top: status.top,
                right: cwd_status.left - MARGIN,
                bottom: status.bottom,
            },
            &status_text,
            if self.last_error.is_some() {
                palette.danger.colorref()
            } else {
                palette.muted_text.colorref()
            },
        );
        frame(device, &cwd_status, palette.active_border.colorref());
        let cwd = self
            .active_tab()
            .and_then(|tab| tab.working_context.cwd.as_deref())
            .unwrap_or("-");
        draw_text(
            device,
            RECT {
                left: cwd_status.left + MARGIN,
                top: cwd_status.top,
                right: cwd_status.right - MARGIN,
                bottom: cwd_status.bottom,
            },
            &format!("CWD: {cwd}"),
            palette.muted_text.colorref(),
        );
        if !self.window_close_pending
            && !self.settings_open
            && !self.new_terminal_open
            && self.pending_close_tab_id.is_none()
        {
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
        } else if self.new_terminal_open {
            self.paint_new_terminal(device, palette);
        } else if self.pending_close_tab_id.is_some() {
            self.paint_tab_close(device, palette);
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
        let locale = self.config.locale;
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
            locale.text(UiText::Settings),
            palette.text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 32,
                top: modal.top + 96,
                right: modal.right - 32,
                bottom: modal.top + 126,
            },
            locale.text(UiText::FontFamily),
            palette.muted_text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 32,
                top: modal.top + 168,
                right: modal.right - 32,
                bottom: modal.top + 198,
            },
            locale.text(UiText::Size),
            palette.muted_text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 32,
                top: modal.top + 240,
                right: modal.right - 32,
                bottom: modal.top + 270,
            },
            locale.text(UiText::ColorTheme),
            palette.muted_text.colorref(),
        );
        if self.settings_scope == SettingsScope::CurrentTerminal {
            let target = self.settings_target_tab_id.as_deref().unwrap_or("-");
            draw_text(
                device,
                RECT {
                    left: modal.left + 454,
                    top: modal.top + 18,
                    right: modal.right - 28,
                    bottom: modal.top + 50,
                },
                target,
                palette.muted_text.colorref(),
            );
        }
    }

    fn paint_new_terminal(&self, device: HDC, palette: &ThemePalette) {
        let (modal, _) = self.new_terminal_modal_geometry();
        fill(device, &modal, palette.modal.colorref());
        frame(device, &modal, palette.accent.colorref());
        for (top, text, color) in [
            (
                18,
                self.config.locale.text(UiText::NewTerminal),
                palette.text.colorref(),
            ),
            (
                48,
                self.config.locale.text(UiText::ShellProfile),
                palette.muted_text.colorref(),
            ),
            (
                116,
                "Initial command · optional; leaves the selected shell open",
                palette.muted_text.colorref(),
            ),
            (
                192,
                "HTTP proxy · optional, applied only to this terminal",
                palette.muted_text.colorref(),
            ),
            (
                268,
                "HTTPS proxy · optional, values are never exposed in snapshots",
                palette.muted_text.colorref(),
            ),
        ] {
            draw_text(
                device,
                RECT {
                    left: modal.left + 28,
                    top: modal.top + top,
                    right: modal.right - 28,
                    bottom: modal.top + top + 28,
                },
                text,
                color,
            );
        }
    }

    fn paint_tab_close(&self, device: HDC, palette: &ThemePalette) {
        let (modal, _) = self.tab_close_modal_geometry();
        fill(device, &modal, palette.modal.colorref());
        frame(device, &modal, palette.warning.colorref());
        draw_text(
            device,
            RECT {
                left: modal.left + 24,
                top: modal.top + 18,
                right: modal.right - 24,
                bottom: modal.top + 50,
            },
            "Close live tab?",
            palette.text.colorref(),
        );
        let target = self.pending_close_tab_id.as_deref().unwrap_or("-");
        draw_text(
            device,
            RECT {
                left: modal.left + 24,
                top: modal.top + 60,
                right: modal.right - 24,
                bottom: modal.top + 90,
            },
            &format!("{target} is still running."),
            palette.muted_text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 24,
                top: modal.top + 88,
                right: modal.right - 24,
                bottom: modal.top + 116,
            },
            "Closing it will terminate the PTY process.",
            palette.muted_text.colorref(),
        );
        draw_text(
            device,
            RECT {
                left: modal.left + 24,
                top: modal.top + 116,
                right: modal.right - 24,
                bottom: modal.top + 144,
            },
            "Cancel returns without changing the tab tree.",
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
        for (viewport_position, tree_row) in remote_tree_rows(&client.snapshot().tabs)
            .into_iter()
            .skip(self.sidebar_offset())
            .take(self.sidebar_row_capacity())
            .enumerate()
        {
            let tab = &client.snapshot().tabs[tree_row.tab_index];
            let geometry =
                self.sidebar_row_geometry(viewport_position, tree_row.depth, TreeRowMode::Normal);
            let row = win_rect(geometry.selection);
            if row.bottom > sidebar.bottom {
                break;
            }
            let active = client.snapshot().active_tab_id.as_deref() == Some(tab.id.as_str());
            if active {
                fill(device, &row, palette.active.colorref());
                frame(device, &row, palette.active_border.colorref());
            }
            for segment in tree_connector_segments(
                self.workspace_geometry().sidebar_tree,
                &geometry,
                tree_row.depth,
                &tree_row.guides,
                tree_row.is_last,
                TreeRowMode::Normal,
            ) {
                fill(device, &win_rect(segment), palette.divider.colorref());
            }
            if tree_row.has_children {
                let expander = win_rect(geometry.expander);
                frame(device, &expander, palette.active_border.colorref());
                draw_text(
                    device,
                    expander,
                    if tree_row.collapsed { "+" } else { "-" },
                    palette.muted_text.colorref(),
                );
            }
            if self.editing_tab_id.as_deref() == Some(tab.id.as_str()) {
                continue;
            }
            if active {
                let compact = geometry.actions.density == TreeRowActionDensity::Compact;
                let locale = self.config.locale;
                for (bounds, label) in [
                    (
                        geometry
                            .actions
                            .add_child
                            .expect("normal tab rows expose Add"),
                        "+",
                    ),
                    (
                        geometry.actions.primary,
                        if compact {
                            "E"
                        } else {
                            locale.text(UiText::Edit)
                        },
                    ),
                    (
                        geometry.actions.secondary,
                        if compact {
                            "X"
                        } else {
                            locale.text(UiText::Close)
                        },
                    ),
                ] {
                    let bounds = win_rect(bounds);
                    frame(device, &bounds, palette.active_border.colorref());
                    draw_text(device, bounds, label, palette.muted_text.colorref());
                }
            }
            draw_text(
                device,
                win_rect(geometry.name),
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
            if !tab.note.is_empty() {
                draw_text(
                    device,
                    win_rect(geometry.note),
                    &tab.note,
                    palette.muted_text.colorref(),
                );
            }
        }
        if let Some((geometry, _, _)) = self.sidebar_scrollbar_state() {
            fill(
                device,
                &win_rect(geometry.track),
                palette.scrollbar_track.colorref(),
            );
            fill(
                device,
                &win_rect(geometry.thumb),
                if self.sidebar_scroll_drag.is_some() {
                    palette.scrollbar_thumb_active.colorref()
                } else {
                    palette.scrollbar_thumb.colorref()
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
                if wparam != SIZE_MINIMIZED as usize {
                    state.resize_active_terminal();
                }
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
        WM_LBUTTONDBLCLK => {
            if let Some(state) = state_mut(window) {
                if state.window_close_pending
                    || state.settings_open
                    || state.new_terminal_open
                    || state.pending_close_tab_id.is_some()
                {
                    return 0;
                }
                let x = (lparam as u32 & 0xffff) as i16 as i32;
                let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
                if state.resize_grip_contains(x, y) {
                    state.reset_tabs_width();
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                }
            }
            0
        }
        WM_LBUTTONDOWN => {
            if let Some(state) = state_mut(window) {
                if state.window_close_pending
                    || state.settings_open
                    || state.new_terminal_open
                    || state.pending_close_tab_id.is_some()
                {
                    return 0;
                }
                let x = (lparam as u32 & 0xffff) as i16 as i32;
                let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
                if state.tabs_recovery_rect().is_some_and(|rect| {
                    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
                }) {
                    state.set_tabs_visible(true);
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                    return 0;
                }
                let cwd_status = state.cwd_status_rect();
                let in_cwd_status = x >= cwd_status.left
                    && x < cwd_status.right
                    && y >= cwd_status.top
                    && y < cwd_status.bottom;
                if state.cwd_edit_tab_id.is_some() {
                    state.finish_cwd_editor(false);
                    if in_cwd_status {
                        unsafe {
                            windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                                window,
                                ptr::null(),
                                0,
                            )
                        };
                        return 0;
                    }
                } else if in_cwd_status {
                    state.open_cwd_editor();
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                    return 0;
                }
                if state.begin_tabs_resize(x, y) {
                    return 0;
                }
                if state.click_sidebar_scrollbar(x, y) {
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
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
                    if state.toggle_tree_at(x, y) {
                        // Disclosure changes only the server-owned tree view;
                        // it does not implicitly select the row.
                    } else {
                        match state.tab_action_at(x, y) {
                            Some(RemoteTabAction::AddChild) => state.new_child_at(y),
                            Some(RemoteTabAction::Edit) => state.begin_tab_edit_at(y),
                            Some(RemoteTabAction::Close) => state.request_close_tab_at(y),
                            None => {
                                state.finish_tab_edit(false);
                                state.select_tab_at(y);
                            }
                        }
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
                if state.sidebar_scroll_drag.is_some() {
                    if state.drag_sidebar_scrollbar(y) {
                        unsafe {
                            windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                                window,
                                ptr::null(),
                                0,
                            )
                        };
                    }
                } else if state.scroll_drag.is_some() {
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
                if state.sidebar_scroll_drag.is_some() {
                    state.end_sidebar_scroll_drag();
                } else if state.scroll_drag.is_some() {
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
                state.sidebar_scrollbar_capture_lost();
                state.scrollbar_capture_lost();
                state.terminal_selection_capture_lost();
            }
            0
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = state_mut(window) {
                if state.window_close_pending
                    || state.settings_open
                    || state.new_terminal_open
                    || state.pending_close_tab_id.is_some()
                {
                    return 0;
                }
                let delta = ((wparam >> 16) & 0xffff) as u16 as i16 as i32;
                let mut point = POINT {
                    x: (lparam as u32 & 0xffff) as i16 as i32,
                    y: ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
                };
                unsafe { ScreenToClient(window, &mut point) };
                if state
                    .workspace_geometry()
                    .sidebar_tree
                    .contains(point.x, point.y)
                {
                    state.scroll_sidebar(delta);
                } else {
                    state.scroll_terminal(delta);
                }
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
            SYSTEM_MENU_TOGGLE_TABS_ID => {
                if let Some(state) = state_mut(window) {
                    state.toggle_tabs();
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::InvalidateRect(window, ptr::null(), 0)
                    };
                }
                0
            }
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
                if state.pending_close_tab_id.is_some() {
                    match wparam as u32 {
                        0x0d => state.finish_close_tab(true),
                        key if key == u32::from(VK_ESCAPE) => state.finish_close_tab(false),
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
                if state.new_terminal_open {
                    if wparam as u32 == u32::from(VK_ESCAPE) {
                        state.finish_new_terminal(false);
                    } else if wparam as u32 == 0x0d {
                        state.finish_new_terminal(true);
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
                if state.window_close_pending
                    || state.settings_open
                    || state.new_terminal_open
                    || state.pending_close_tab_id.is_some()
                {
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
                let control_id = wparam & 0xffff;
                if let Some(hit) = windows_toolbar_hit(control_id) {
                    state.dispatch_windows_toolbar_action(hit.action_id());
                } else {
                    match control_id {
                        SEND_ID => state.send_composer(),
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
                        SETTINGS_DEFAULT_SCOPE_ID => {
                            state.switch_settings_scope(SettingsScope::Defaults)
                        }
                        SETTINGS_CURRENT_SCOPE_ID => {
                            state.switch_settings_scope(SettingsScope::CurrentTerminal)
                        }
                        SETTINGS_FONT_INHERIT_ID => {
                            state.toggle_settings_inheritance(AppearanceField::FontFamily)
                        }
                        SETTINGS_SIZE_INHERIT_ID => {
                            state.toggle_settings_inheritance(AppearanceField::FontSize)
                        }
                        SETTINGS_THEME_INHERIT_ID => {
                            state.toggle_settings_inheritance(AppearanceField::Theme)
                        }
                        SETTINGS_RESET_OVERRIDES_ID => state.reset_settings_overrides(),
                        SETTINGS_APPLY_ID => state.finish_settings(true),
                        SETTINGS_CANCEL_ID => state.finish_settings(false),
                        TAB_CLOSE_CONFIRM_ID => state.finish_close_tab(true),
                        TAB_CLOSE_CANCEL_ID => state.finish_close_tab(false),
                        NEW_DEFAULT_SHELL_ID => state.choose_new_shell(NewShellChoice::Default),
                        NEW_CMD_SHELL_ID => state.choose_new_shell(NewShellChoice::CommandPrompt),
                        NEW_POWERSHELL_ID => state.choose_new_shell(NewShellChoice::PowerShell),
                        NEW_CREATE_ID => state.finish_new_terminal(true),
                        NEW_CANCEL_ID => state.finish_new_terminal(false),
                        _ => {}
                    }
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
        WM_APP_DESTROY_WINDOW => {
            unsafe { DestroyWindow(window) };
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

fn request_window_destroy(window: HWND) {
    // DestroyWindow synchronously sends WM_DESTROY/WM_NCDESTROY. Calling it
    // from a method on RemoteWindowState would therefore drop that state while
    // the method (and, for relayed commands, the timer tick) still has `self`
    // borrowed. Queue destruction so the current window-procedure invocation
    // and every state access return before WM_NCDESTROY releases the state.
    unsafe {
        PostMessageW(window, WM_APP_DESTROY_WINDOW, 0, 0);
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
            SYSTEM_MENU_TOGGLE_TABS_ID,
            wide("Toggle Tabs").as_ptr(),
        )
    } == 0
        || unsafe { InsertMenuW(menu, SC_CLOSE, MF_BYCOMMAND | MF_SEPARATOR, 0, ptr::null()) } == 0
        || unsafe {
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
        anyhow::bail!("could not add Toggle Tabs, Copy, and Paste to the window system menu");
    }
    Ok(())
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

fn create_terminal_font(window: HWND, family: &str, size: u16) -> Result<(HFONT, i32, i32)> {
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
    let height = -((i32::from(size) * dpi) / 72).max(1);
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
            wide(family).as_ptr(),
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

fn stable_tab_number(id: &str) -> Option<u64> {
    id.strip_prefix('@')?.parse().ok()
}

fn remote_tree_rows(tabs: &[UiTabBootstrap]) -> Vec<RemoteTreeRow> {
    let nodes = tabs
        .iter()
        .filter_map(|tab| {
            Some(TabTreeNode {
                id: stable_tab_number(&tab.id)?,
                parent_id: tab.parent_id.as_deref().and_then(stable_tab_number),
                sort_key: tab.index,
            })
        })
        .collect::<Vec<_>>();
    tree_rows(&nodes)
        .into_iter()
        .filter(|row| {
            !row.ancestors.iter().any(|ancestor| {
                tabs.iter()
                    .any(|tab| stable_tab_number(&tab.id) == Some(*ancestor) && tab.collapsed)
            })
        })
        .filter_map(|row| {
            let tab_index = tabs
                .iter()
                .position(|tab| stable_tab_number(&tab.id) == Some(row.id))?;
            Some(RemoteTreeRow {
                tab_index,
                depth: row.depth,
                is_last: row.is_last,
                guides: row.guides,
                has_children: nodes.iter().any(|node| node.parent_id == Some(row.id)),
                collapsed: tabs[tab_index].collapsed,
            })
        })
        .collect()
}

fn remote_tab_depth(tabs: &[UiTabBootstrap], tab: &UiTabBootstrap) -> usize {
    let mut depth = 0_usize;
    let mut parent = tab.parent_id.as_deref();
    while let Some(parent_id) = parent {
        if depth >= tabs.len() {
            break;
        }
        let Some(parent_tab) = tabs.iter().find(|candidate| candidate.id == parent_id) else {
            break;
        };
        depth += 1;
        parent = parent_tab.parent_id.as_deref();
    }
    depth
}

fn tree_action_density_name(density: TreeRowActionDensity) -> &'static str {
    match density {
        TreeRowActionDensity::Full => "full",
        TreeRowActionDensity::Compact => "compact",
    }
}

fn close_button_snapshot(action: &str, label: &str) -> serde_json::Value {
    serde_json::json!({
        "action": action,
        "label": label,
        "text_alignment": {
            "horizontal": "center",
            "vertical": "center",
            "win32_draw_text_format": WINDOW_CLOSE_BUTTON_TEXT_FORMAT,
        },
    })
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
            terminal_title: "terminal".to_owned(),
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
    fn native_toolbar_ids_resolve_to_stable_product_actions() {
        let cases = [
            (TABS_ID, action::TOGGLE_TABS),
            (NEW_ID, action::NEW_TAB),
            (SETTINGS_ID, action::OPEN_SETTINGS),
            (LOCALE_ID, action::TOGGLE_LOCALE),
            (FONT_DECREASE_ID, action::FONT_DECREASE),
            (FONT_INCREASE_ID, action::FONT_INCREASE),
        ];
        for (control_id, action_id) in cases {
            assert_eq!(
                windows_toolbar_hit(control_id).map(WindowsToolbarHit::action_id),
                Some(action_id)
            );
        }
        assert_eq!(windows_toolbar_hit(SEND_ID), None);
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

    #[test]
    fn sidebar_rows_are_positioned_to_the_right_of_the_left_scrollbar() {
        let sidebar = PixelRect {
            left: 0,
            top: 0,
            right: 244,
            bottom: 700,
        };
        let geometry = sidebar_tree_row_geometry(sidebar, 0, 2, TreeRowMode::Editing);
        let track = sidebar_scrollbar_track(sidebar);

        assert_eq!(track.left, sidebar.left);
        assert_eq!(track.right, TERMINAL_SCROLLBAR_WIDTH);
        assert_eq!(geometry.row.left, TERMINAL_SCROLLBAR_WIDTH + 2);
        assert!(geometry.disclosure_hit.left >= track.right);
        assert!(geometry.actions.bounds.right <= sidebar.right);
        let editors = geometry.editors.expect("editing geometry has editors");
        assert!(editors.name.left >= track.right);
    }
}
