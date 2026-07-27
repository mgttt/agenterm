use std::{
    cell::RefCell,
    collections::HashSet,
    env,
    ffi::c_void,
    fs::OpenOptions,
    io::{Read, Write},
    mem, ptr,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context as _, Result};
use rmux_pty::{
    ChildCommand, ProcessId, PtyChild, PtyMaster, TerminalSize, write_windows_console_mouse_drag,
};
use windows_sys::Win32::{
    Foundation::{
        COLORREF, GlobalFree, HINSTANCE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, POINT, RECT,
        SIZE, WPARAM,
    },
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CLEARTYPE_QUALITY,
        CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
        CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DIB_RGB_COLORS, DT_END_ELLIPSIS,
        DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint,
        ExtTextOutW, FF_MODERN, FIXED_PITCH, FW_NORMAL, FillRect, FrameRect, GetDC, GetDIBits,
        GetDeviceCaps, GetStockObject, GetTextExtentPoint32W, GetTextFaceW, GetTextMetricsW,
        GetWindowDC, HDC, HFONT, HGDIOBJ, InvalidateRect, LOGPIXELSY, OUT_DEFAULT_PRECIS,
        PAINTSTRUCT, ReleaseDC, SRCCOPY, SYSTEM_FIXED_FONT, ScreenToClient, SelectObject,
        SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT, UpdateWindow,
    },
    System::{
        Console::{
            ATTACH_PARENT_PROCESS, AttachConsole, FreeConsole, GetStdHandle, STD_ERROR_HANDLE,
        },
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        LibraryLoader::GetModuleHandleW,
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
    UI::Controls::{EM_GETSEL, EM_SETSEL},
    UI::Input::KeyboardAndMouse::{GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus},
    UI::Shell::ShellExecuteW,
    UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
        DestroyWindow, DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN,
        EnableMenuItem, GWLP_USERDATA, GetClientRect, GetMessageW, GetSystemMenu,
        GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IDC_ARROW,
        InsertMenuW, IsIconic, IsWindowVisible, IsZoomed, LoadCursorW, LoadIconW, MB_ICONERROR,
        MB_OK, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, MessageBoxW,
        MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW, SC_CLOSE, SW_HIDE, SW_MAXIMIZE,
        SW_MINIMIZE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWNORMAL, SendMessageW, SetForegroundWindow,
        SetTimer, SetWindowLongPtrW, SetWindowTextW, ShowWindow, TranslateMessage, WM_APP, WM_CHAR,
        WM_CLOSE, WM_COMMAND, WM_COPY, WM_CREATE, WM_CUT, WM_DESTROY, WM_ENDSESSION, WM_ERASEBKGND,
        WM_INITMENUPOPUP, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        WM_NCDESTROY, WM_PAINT, WM_PASTE, WM_QUERYENDSESSION, WM_RBUTTONDOWN, WM_SETFOCUS, WM_SIZE,
        WM_SYSCOMMAND, WM_TIMER, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN,
        WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
    },
};

mod commands;
mod event_journal;
mod instances;
mod protocol;
mod rmux_status;
pub mod script_protocol;
mod settings;
mod tab_tree;
mod theme;
mod ui_geometry;
mod workspace;

use commands::{
    BACKSPACE_INPUT, MUX_COMMANDS, MuxStatus, SUPPORTED_COMMANDS, canonical_control_command,
    control_command_requests_help, control_command_usage, has_option, last_positional, mux_command,
    option_value, parse_new_command, parse_tab_environment, positional_values,
    screenshot_output_path, tmux_key_bytes, validate_control_command,
};
use event_journal::EventJournal;
use instances::{InstanceRegistration, discover_instances, prune_instance, register_instance};
use protocol::{IpcRequest, IpcResponse};
use rmux_status::parse_status_windows;
use script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, ScriptBudgets, ScriptExitClass, ScriptInvocation,
    ScriptOperation, ScriptProfile, ScriptResult,
};
use settings::{AppConfig, config_path, load_config, save_config};
use tab_tree::{TabTreeNode, TabTreeRow, tree_rows, would_create_cycle};
use theme::{ThemeId, ThemePalette};
use ui_geometry::{
    PixelRect, TAB_HEIGHT, TERMINAL_SCROLLBAR_WIDTH, TerminalScrollbarGeometry,
    scrollback_for_thumb_top, terminal_scrollbar_geometry, tree_anchor_x, tree_row_at_y,
    tree_row_geometry,
};
use workspace::{SavedTab, SavedWorkspace, load_workspace, save_workspace, workspace_path};

const APP_NAME: &str = "AgenTerm";
const INITIAL_ROWS: u16 = 30;
const INITIAL_COLS: u16 = 100;
const SCROLLBACK_LINES: usize = 10_000;
const SIDEBAR_WIDTH: i32 = 250;
const COMPOSER_HEIGHT: i32 = 78;
const STATUS_BAR_HEIGHT: i32 = 26;
const TAB_ADD_LEFT: i32 = SIDEBAR_WIDTH - 72;
const TAB_EDIT_LEFT: i32 = SIDEBAR_WIDTH - 48;
const TAB_CLOSE_LEFT: i32 = SIDEBAR_WIDTH - 24;
const BUTTON_ID: usize = 1001;
const EDIT_ID: usize = 1002;
const SETTINGS_BUTTON_ID: usize = 1003;
const SETTINGS_FONT_ID: usize = 1004;
const SETTINGS_SIZE_ID: usize = 1005;
const SETTINGS_APPLY_ID: usize = 1006;
const NEW_BUTTON_ID: usize = 1007;
const SETTINGS_DARK_ID: usize = 1008;
const SETTINGS_LIGHT_ID: usize = 1009;
const SETTINGS_CANCEL_ID: usize = 1010;
const TIMER_ID: usize = 1;
const WM_APP_WAKE: u32 = WM_APP + 1;
const SYSTEM_MENU_COPY_ID: usize = 0x1f00;
const SYSTEM_MENU_PASTE_ID: usize = 0x1f10;
const CLIPBOARD_UNICODE_TEXT: u32 = 13;
const TERMINAL_PASTE_LIMIT_BYTES: usize = 256 * 1024;
const IPC_TIMEOUT: Duration = Duration::from_secs(5);
const IPC_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(500);
const IPC_AUTOSTART_TIMEOUT: Duration = Duration::from_secs(15);
const IPC_AUTOSTART_POLL: Duration = Duration::from_millis(100);
const COMPOSER_SUBMIT_DELAY: Duration = Duration::from_millis(500);
const RAW_OUTPUT_LIMIT: usize = 1024 * 1024;
const WHEEL_DELTA: i32 = 120;
const WHEEL_ROWS_PER_NOTCH: usize = 3;
const UI_LOCALE: &str = "en-US";
const LABEL_SEND: &str = "Send";
const LABEL_SETTINGS: &str = "Settings";
const LABEL_NEW: &str = "New";
const LABEL_APPLY: &str = "Apply";
const LABEL_SAVE: &str = "Save";

thread_local! {
    static IPC_ADDRESS_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

#[derive(Clone, Copy)]
struct PaintColors {
    sidebar: COLORREF,
    terminal: COLORREF,
    terminal_text: COLORREF,
    composer: COLORREF,
    text: COLORREF,
    muted: COLORREF,
    tree: COLORREF,
    active: COLORREF,
    active_border: COLORREF,
    green: COLORREF,
    orange: COLORREF,
    red: COLORREF,
    blue: COLORREF,
    modal: COLORREF,
    status: COLORREF,
    control: COLORREF,
    control_hover: COLORREF,
    control_pressed: COLORREF,
    focus_ring: COLORREF,
    selection_foreground: COLORREF,
    selection_background: COLORREF,
    scrollbar_track: COLORREF,
    scrollbar_thumb: COLORREF,
    scrollbar_thumb_active: COLORREF,
}

impl From<&ThemePalette> for PaintColors {
    fn from(palette: &ThemePalette) -> Self {
        Self {
            sidebar: palette.sidebar.colorref(),
            terminal: palette.terminal_background.colorref(),
            terminal_text: palette.terminal_foreground.colorref(),
            composer: palette.composer.colorref(),
            text: palette.text.colorref(),
            muted: palette.muted_text.colorref(),
            tree: palette.divider.colorref(),
            active: palette.active.colorref(),
            active_border: palette.active_border.colorref(),
            green: palette.success.colorref(),
            orange: palette.warning.colorref(),
            red: palette.danger.colorref(),
            blue: palette.accent.colorref(),
            modal: palette.modal.colorref(),
            status: palette.status.colorref(),
            control: palette.control.colorref(),
            control_hover: palette.control_hover.colorref(),
            control_pressed: palette.control_pressed.colorref(),
            focus_ring: palette.focus_ring.colorref(),
            selection_foreground: palette.selection_foreground.colorref(),
            selection_background: palette.selection_background.colorref(),
            scrollbar_track: palette.scrollbar_track.colorref(),
            scrollbar_thumb: palette.scrollbar_thumb.colorref(),
            scrollbar_thumb_active: palette.scrollbar_thumb_active.colorref(),
        }
    }
}

fn effective_theme(configured: ThemeId, draft: ThemeId, settings_open: bool) -> ThemeId {
    if settings_open { draft } else { configured }
}

struct IpcEnvelope {
    request: IpcRequest,
    respond_to: Sender<IpcResponse>,
}

pub fn run_gui_entry() -> i32 {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| argument != "--address")
    {
        write_best_effort_stderr(&gui_cli_guidance(&arguments));
        return 2;
    }
    if let Err(error) = configure_gui_address(&arguments) {
        write_best_effort_stderr(&format!(
            "AgenTerm GUI argument error: {error:#}\n\
             No GUI server was started by this invocation.\n\
             More CLI commands: agenterm-cli.exe -h"
        ));
        return 2;
    }
    write_best_effort_stderr(&gui_console_summary(&ipc_address()));
    if env::var_os("AGENTERM_SERVER").is_none()
        && send_ipc_request(vec!["__focus".to_owned()]).is_ok()
    {
        return 0;
    }

    if let Err(error) = run_gui() {
        show_startup_error(&error);
        return 1;
    }
    0
}

fn configure_gui_address(arguments: &[String]) -> Result<()> {
    let Some(argument) = arguments.first() else {
        return Ok(());
    };
    if argument != "--address" {
        anyhow::bail!("unsupported AgenTerm GUI argument: {argument}");
    }
    let address = arguments
        .get(1)
        .context("agenterm.exe --address requires HOST:PORT")?;
    if arguments.len() != 2 {
        anyhow::bail!("agenterm.exe accepts only --address HOST:PORT");
    }
    parse_loopback_ipc_address(address)?;
    IPC_ADDRESS_OVERRIDE.with(|override_address| {
        *override_address.borrow_mut() = Some(address.clone());
    });
    Ok(())
}

fn quote_argument_for_display(argument: &str) -> String {
    if argument.is_empty() || argument.chars().any(char::is_whitespace) {
        format!("\"{}\"", argument.replace('"', "\\\""))
    } else {
        argument.to_owned()
    }
}

fn gui_cli_guidance(arguments: &[String]) -> String {
    let forwarded = arguments
        .iter()
        .map(|argument| quote_argument_for_display(argument))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "AgenTerm GUI entry point\n\n\
         No CLI command was executed and no GUI server was started by this invocation.\n\n\
         Use instead:\nagenterm-cli.exe {forwarded}\n\n\
         Launcher PID: {}\nConfigured server address: {}\n\n\
         List running server PID and port: agenterm-cli.exe server-list\n\
         More CLI commands: agenterm-cli.exe -h",
        std::process::id(),
        ipc_address()
    )
}

fn gui_console_summary(address: &str) -> String {
    format!(
        "Launcher PID: {}\n\
         Configured server address: {address}\n\n\
         List running server PID and port: agenterm-cli.exe server-list\n\
         More CLI commands: agenterm-cli.exe -h",
        std::process::id()
    )
}

fn write_best_effort_stderr(message: &str) {
    let payload = format!("{message}\n");
    let stderr_handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if !stderr_handle.is_null() && stderr_handle != INVALID_HANDLE_VALUE {
        let mut stderr = std::io::stderr().lock();
        if stderr.write_all(payload.as_bytes()).is_ok() && stderr.flush().is_ok() {
            return;
        }
    }

    // A /SUBSYSTEM:WINDOWS process normally has no standard handles even when
    // launched from PowerShell or cmd. Briefly attach to the parent's console
    // and write directly to its output buffer without allocating a console,
    // rebinding process-wide stdio, or reading stdin.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    if let Ok(mut console) = OpenOptions::new().write(true).open("CONOUT$") {
        let _ = console.write_all(payload.as_bytes());
        let _ = console.flush();
    }
    unsafe {
        FreeConsole();
    }
}

fn show_startup_error(error: &anyhow::Error) {
    let text = format!("AgenTerm failed to start:\n\n{error:#}");
    let message = wide(&text);
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            wide(APP_NAME).as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
    write_best_effort_stderr(&text);
}

pub fn run_cli_entry() -> i32 {
    let mut arguments: Vec<String> = env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|argument| argument == "--address")
    {
        if arguments.len() < 2 {
            eprintln!("agenterm-cli --address requires HOST:PORT");
            return 2;
        }
        arguments.remove(0);
        let address = arguments.remove(0);
        if let Err(error) = parse_loopback_ipc_address(&address) {
            eprintln!("{error:#}");
            return 2;
        }
        IPC_ADDRESS_OVERRIDE.with(|override_address| {
            *override_address.borrow_mut() = Some(address);
        });
    }
    if arguments
        .first()
        .is_some_and(|arg| arg == "-V" || arg == "--version")
    {
        println!("agenterm-cli {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if arguments.is_empty()
        || arguments
            .first()
            .is_some_and(|arg| arg == "-h" || arg == "--help")
    {
        print_help();
        return 0;
    }
    if arguments
        .first()
        .is_some_and(|argument| argument.starts_with('-'))
    {
        eprintln!(
            "unknown global option '{}'. To target an AgenTerm instance, use \
             `agenterm-cli --address HOST:PORT COMMAND` or set AGENTERM_IPC_ADDRESS.",
            arguments[0]
        );
        return 2;
    }
    run_cli(arguments)
}

pub fn run_mux_entry() -> i32 {
    let mut arguments: Vec<String> = env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|arg| arg == "-V" || arg == "--version")
    {
        println!(
            "agenterm-mux {} (AgenTerm compatibility frontend)",
            env!("CARGO_PKG_VERSION")
        );
        return 0;
    }
    if arguments.is_empty()
        || arguments
            .first()
            .is_some_and(|arg| arg == "-h" || arg == "--help")
    {
        print_mux_help();
        return 0;
    }

    let mut address = None;
    let mut session = None;
    loop {
        match arguments.first().map(String::as_str) {
            Some("--address") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-mux --address requires HOST:PORT");
                    return 2;
                }
                arguments.remove(0);
                let candidate = arguments.remove(0);
                if let Err(error) = parse_loopback_ipc_address(&candidate) {
                    eprintln!("{error:#}");
                    return 2;
                }
                address = Some(candidate);
            }
            Some("--session") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-mux --session requires a session name");
                    return 2;
                }
                arguments.remove(0);
                session = Some(arguments.remove(0));
            }
            Some("-L" | "-S") => {
                eprintln!(
                    "agenterm-mux does not support tmux socket selection; use --address HOST:PORT"
                );
                return 2;
            }
            _ => break,
        }
    }
    let Some(command) = arguments.first().cloned() else {
        eprintln!("agenterm-mux requires a command");
        return 2;
    };

    if command == "compatibility" {
        print_mux_compatibility(arguments.iter().any(|argument| argument == "--json"));
        return 0;
    }
    if command == "agenterm" {
        arguments.remove(0);
        if arguments.is_empty() {
            eprintln!("agenterm-mux agenterm requires a native AgenTerm command");
            return 2;
        }
    } else {
        let Some(specification) = mux_command(&command) else {
            eprintln!(
                "{command} is not in the agenterm-mux compatibility surface; \
                 use `agenterm-mux agenterm {command} ...` for native AgenTerm extensions"
            );
            return 2;
        };
        if let MuxStatus::Unsupported(reason) = specification.status {
            eprintln!("{command} is unsupported: {reason}");
            return 1;
        }
        if matches!(command.as_str(), "list-commands" | "lscm") {
            print_mux_commands();
            return 0;
        }
    }

    if let Some(session) = session
        && matches!(
            arguments.first().map(String::as_str),
            Some("attach" | "attach-session" | "has" | "has-session" | "kill-session")
        )
        && !has_option(&arguments, "-t")
    {
        arguments.extend(["-t".to_owned(), session]);
    }
    IPC_ADDRESS_OVERRIDE.with(|override_address| {
        *override_address.borrow_mut() = address;
    });
    run_cli(arguments)
}

fn run_gui() -> Result<()> {
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    if instance.is_null() {
        anyhow::bail!("GetModuleHandleW failed");
    }

    let class_name = wide("AgenTermWindowClass");
    let address = ipc_address();
    let socket = parse_loopback_ipc_address(&address)?;
    let title = wide(&format!(
        "AgenTerm-{}:{}",
        env!("CARGO_PKG_VERSION"),
        socket.port()
    ));
    let mut window_class: WNDCLASSW = unsafe { mem::zeroed() };
    window_class.style = CS_HREDRAW | CS_VREDRAW;
    window_class.lpfnWndProc = Some(window_proc);
    window_class.hInstance = instance as HINSTANCE;
    window_class.hCursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
    // Win32's MAKEINTRESOURCEW convention encodes the numeric resource ID in
    // the pointer value; no memory is dereferenced.
    window_class.hIcon =
        unsafe { LoadIconW(instance as HINSTANCE, ptr::without_provenance::<u16>(1)) };
    window_class.lpszClassName = class_name.as_ptr();
    if unsafe { RegisterClassW(&window_class) } == 0 {
        anyhow::bail!("RegisterClassW failed");
    }

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
        anyhow::bail!("CreateWindowExW failed");
    }
    install_system_clipboard_menu(window)?;

    let edit = unsafe {
        CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_TABSTOP
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_WANTRETURN as u32,
            0,
            0,
            100,
            50,
            window,
            EDIT_ID as *mut c_void,
            instance,
            ptr::null(),
        )
    };
    let send_button = unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(LABEL_SEND).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            74,
            32,
            window,
            BUTTON_ID as *mut c_void,
            instance,
            ptr::null(),
        )
    };
    let settings_button = unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(LABEL_SETTINGS).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            86,
            30,
            window,
            SETTINGS_BUTTON_ID as *mut c_void,
            instance,
            ptr::null(),
        )
    };
    let new_button = unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(LABEL_NEW).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            82,
            30,
            window,
            NEW_BUTTON_ID as *mut c_void,
            instance,
            ptr::null(),
        )
    };
    let settings_font = create_hidden_edit(window, instance, SETTINGS_FONT_ID);
    let settings_size = create_hidden_edit(window, instance, SETTINGS_SIZE_ID);
    let settings_dark = create_hidden_button(window, instance, SETTINGS_DARK_ID, "Dark");
    let settings_light = create_hidden_button(window, instance, SETTINGS_LIGHT_ID, "Light");
    let settings_cancel = create_hidden_button(window, instance, SETTINGS_CANCEL_ID, "Cancel");
    let settings_apply = unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(LABEL_APPLY).as_ptr(),
            WS_CHILD | WS_TABSTOP,
            0,
            0,
            86,
            32,
            window,
            SETTINGS_APPLY_ID as *mut c_void,
            instance,
            ptr::null(),
        )
    };
    if edit.is_null()
        || send_button.is_null()
        || settings_button.is_null()
        || new_button.is_null()
        || settings_font.is_null()
        || settings_size.is_null()
        || settings_dark.is_null()
        || settings_light.is_null()
        || settings_cancel.is_null()
        || settings_apply.is_null()
    {
        unsafe { DestroyWindow(window) };
        anyhow::bail!("failed to create native controls");
    }

    let state = Box::new(AppState::new(
        window,
        NativeControls {
            edit,
            send_button,
            settings_button,
            new_button,
            settings_font,
            settings_size,
            settings_dark,
            settings_light,
            settings_cancel,
            settings_apply,
        },
    )?);
    unsafe {
        SetWindowLongPtrW(window, GWLP_USERDATA, Box::into_raw(state) as isize);
        SetTimer(window, TIMER_ID, 100, None);
    }
    if let Some(state) = state_mut(window) {
        state.layout();
        state.load_active_composer();
    }
    unsafe {
        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);
    }

    let mut message: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        if message.message == WM_KEYDOWN
            && let Some(state) = state_mut(window)
            && state.handle_shortcut(message.wParam as u32)
        {
            continue;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

fn install_system_clipboard_menu(window: HWND) -> Result<()> {
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
            28,
            window,
            id as *mut c_void,
            instance,
            ptr::null(),
        )
    }
}

fn create_hidden_button(window: HWND, instance: HINSTANCE, id: usize, label: &str) -> HWND {
    unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(label).as_ptr(),
            WS_CHILD | WS_TABSTOP,
            0,
            0,
            100,
            30,
            window,
            id as *mut c_void,
            instance,
            ptr::null(),
        )
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let _ = lparam as *const CREATESTRUCTW;
            0
        }
        WM_TIMER => {
            if let Some(state) = state_mut(window)
                && state.tick()
            {
                unsafe { InvalidateRect(window, ptr::null(), 0) };
            }
            0
        }
        WM_APP_WAKE => {
            if let Some(state) = state_mut(window)
                && state.tick()
            {
                unsafe { InvalidateRect(window, ptr::null(), 0) };
            }
            0
        }
        WM_SIZE => {
            if let Some(state) = state_mut(window) {
                state.layout();
                unsafe { InvalidateRect(window, ptr::null(), 0) };
            }
            0
        }
        WM_PAINT => {
            if let Some(state) = state_mut(window) {
                state.paint();
                0
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        WM_LBUTTONDOWN => {
            let x = (lparam as u32 & 0xffff) as i16 as i32;
            let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
            if let Some(state) = state_mut(window) {
                state.left_button_down(x, y);
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_mut(window) {
                state.left_button_up();
            }
            0
        }
        WM_MOUSEMOVE => {
            let x = (lparam as u32 & 0xffff) as i16 as i32;
            let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
            if let Some(state) = state_mut(window) {
                state.mouse_move(x, y);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let x = (lparam as u32 & 0xffff) as i16 as i32;
            let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
            let delta = ((wparam >> 16) & 0xffff) as u16 as i16 as i32;
            if let Some(state) = state_mut(window) {
                state.mouse_wheel(x, y, delta);
            }
            0
        }
        WM_RBUTTONDOWN => {
            let x = (lparam as u32 & 0xffff) as i16 as i32;
            let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
            if let Some(state) = state_mut(window) {
                state.right_click(x, y);
            }
            0
        }
        WM_COMMAND => {
            if (wparam & 0xffff) == BUTTON_ID {
                if let Some(state) = state_mut(window) {
                    state.send_composer();
                }
                0
            } else if (wparam & 0xffff) == SETTINGS_BUTTON_ID {
                if let Some(state) = state_mut(window) {
                    state.open_settings();
                }
                0
            } else if (wparam & 0xffff) == NEW_BUTTON_ID {
                if let Some(state) = state_mut(window) {
                    if let Err(error) = state.create_tab(None, Vec::new(), true) {
                        state.last_error = Some(format!("{error:#}"));
                    }
                    unsafe { InvalidateRect(window, ptr::null(), 0) };
                }
                0
            } else if (wparam & 0xffff) == SETTINGS_APPLY_ID {
                if let Some(state) = state_mut(window) {
                    state.apply_settings_from_controls();
                }
                0
            } else if (wparam & 0xffff) == SETTINGS_DARK_ID {
                if let Some(state) = state_mut(window) {
                    state.preview_theme(ThemeId::Dark);
                }
                0
            } else if (wparam & 0xffff) == SETTINGS_LIGHT_ID {
                if let Some(state) = state_mut(window) {
                    state.preview_theme(ThemeId::Light);
                }
                0
            } else if (wparam & 0xffff) == SETTINGS_CANCEL_ID {
                if let Some(state) = state_mut(window) {
                    state.close_settings();
                }
                0
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
        }
        WM_CHAR => {
            if let Some(state) = state_mut(window) {
                state.character(wparam as u32);
            }
            0
        }
        WM_KEYDOWN => {
            if let Some(state) = state_mut(window) {
                state.key_down(wparam as u32);
            }
            0
        }
        WM_INITMENUPOPUP => {
            if let Some(state) = state_mut(window) {
                state.refresh_system_clipboard_menu();
            }
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
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
        WM_SETFOCUS => 0,
        WM_ERASEBKGND => 1,
        WM_CLOSE => {
            if let Some(state) = state_mut(window) {
                if state.close_requested {
                    unsafe { DestroyWindow(window) };
                } else {
                    state.request_window_close();
                }
            } else {
                unsafe { DestroyWindow(window) };
            }
            0
        }
        WM_QUERYENDSESSION => {
            if let Some(state) = state_mut(window)
                && let Err(error) = state.persist_workspace()
            {
                state.last_error = Some(format!("workspace save failed: {error:#}"));
                return 0;
            }
            1
        }
        WM_ENDSESSION => {
            if wparam != 0 {
                if let Some(state) = state_mut(window) {
                    state.close_requested = true;
                }
                unsafe { DestroyWindow(window) };
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_NCDESTROY => {
            let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *mut AppState;
            if !pointer.is_null() {
                unsafe {
                    SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                    drop(Box::from_raw(pointer));
                }
            }
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn state_mut(window: HWND) -> Option<&'static mut AppState> {
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *mut AppState;
    unsafe { pointer.as_mut() }
}

enum PtyEvent {
    Output(Vec<u8>),
    Exited(u32),
    Error(String),
}

#[derive(Default)]
struct TerminalCallbacks {
    title: String,
}

impl vt100::Callbacks for TerminalCallbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).trim().to_owned();
    }
}

struct TerminalTab {
    id: u64,
    index: u32,
    parent_id: Option<u64>,
    title: String,
    note: String,
    command_name: String,
    command_line: Vec<String>,
    environment_names: Vec<String>,
    process_id: Option<u32>,
    composer: String,
    parser: vt100::Parser<TerminalCallbacks>,
    receiver: Receiver<PtyEvent>,
    master: PtyMaster,
    child: PtyChild,
    exited: Option<u32>,
    error: Option<String>,
    last_size: (u16, u16),
    input_bytes: usize,
    input_writes: usize,
    pending_submit_at: Option<Instant>,
    output_bytes: usize,
    raw_output: Vec<u8>,
}

struct TerminalLaunch {
    id: u64,
    index: u32,
    parent_id: Option<u64>,
    title: Option<String>,
    command_line: Vec<String>,
    tab_environment: Vec<(String, String)>,
    session_name: String,
    window: HWND,
    initial_size: TerminalSize,
}

impl TerminalTab {
    fn spawn(launch: TerminalLaunch) -> Result<Self> {
        let TerminalLaunch {
            id,
            index,
            parent_id,
            title,
            command_line,
            tab_environment,
            session_name,
            window,
            initial_size,
        } = launch;
        let program = command_line.first().cloned().unwrap_or_else(|| {
            env::var("COMSPEC").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_owned())
        });
        let persisted_command_line = if command_line.is_empty() {
            vec![program.clone()]
        } else {
            command_line.clone()
        };
        let mut command = ChildCommand::new(&program)
            .size(initial_size)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("TERM_PROGRAM", "AgenTerm")
            .env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"))
            .env("AGENTERM_IPC_ADDRESS", ipc_address())
            .env("AGENTERM_TAB_ID", format!("@{id}"))
            .env("AGENTERM_SESSION", session_name)
            .env("AGENTERM_WORKSPACE_PATH", workspace_path());
        let environment_names = tab_environment
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        for (name, value) in tab_environment {
            command = command.env(name, value);
        }
        for argument in command_line.iter().skip(1) {
            command = command.arg(argument);
        }
        if let Ok(directory) = env::current_dir() {
            command = command.current_dir(directory);
        }

        let spawned = command
            .spawn()
            .context("failed to start terminal command")?;
        let (mut master, child) = spawned.into_parts();
        let process_id = Some(child.pid().as_u32());
        let reader = master
            .try_clone_for_startup_reader()
            .context("failed to clone ConPTY reader")?;
        let mut wait_child = child
            .try_clone_for_wait()
            .context("failed to clone ConPTY child wait handle")?;
        let (sender, receiver) = mpsc::channel();
        let wake_window = window as isize;

        let output_sender = sender.clone();
        thread::spawn(move || {
            let mut buffer = [0_u8; 16 * 1024];
            loop {
                match reader.io().read(&mut buffer) {
                    Ok(0) => break,
                    Ok(size) => {
                        if output_sender
                            .send(PtyEvent::Output(buffer[..size].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                        unsafe {
                            PostMessageW(wake_window as HWND, WM_APP_WAKE, 0, 0);
                        }
                    }
                    Err(error) => {
                        let _ = output_sender.send(PtyEvent::Error(error.to_string()));
                        unsafe {
                            PostMessageW(wake_window as HWND, WM_APP_WAKE, 0, 0);
                        }
                        break;
                    }
                }
            }
        });

        thread::spawn(move || {
            let event = match wait_child.wait() {
                Ok(status) => {
                    wait_child.close_pseudoconsole();
                    PtyEvent::Exited(status.code().unwrap_or(1) as u32)
                }
                Err(error) => PtyEvent::Error(format!("process wait failed: {error}")),
            };
            let _ = sender.send(event);
            unsafe {
                PostMessageW(wake_window as HWND, WM_APP_WAKE, 0, 0);
            }
        });

        let command_name = std::path::Path::new(&program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&program)
            .to_owned();
        Ok(Self {
            id,
            index,
            parent_id,
            title: title.unwrap_or_else(|| command_name.clone()),
            command_name,
            command_line: persisted_command_line,
            environment_names,
            process_id,
            composer: String::new(),
            note: String::new(),
            parser: vt100::Parser::new_with_callbacks(
                initial_size.rows,
                initial_size.cols,
                SCROLLBACK_LINES,
                TerminalCallbacks::default(),
            ),
            receiver,
            master,
            child,
            exited: None,
            error: None,
            last_size: (initial_size.rows, initial_size.cols),
            input_bytes: 0,
            input_writes: 0,
            pending_submit_at: None,
            output_bytes: 0,
            raw_output: Vec::new(),
        })
    }

    fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.receiver.try_recv() {
            changed = true;
            match event {
                PtyEvent::Output(bytes) => {
                    self.output_bytes += bytes.len();
                    self.parser.process(&bytes);
                    self.raw_output.extend_from_slice(&bytes);
                    if self.raw_output.len() > RAW_OUTPUT_LIMIT {
                        let excess = self.raw_output.len() - RAW_OUTPUT_LIMIT;
                        self.raw_output.drain(..excess);
                    }
                }
                PtyEvent::Exited(code) => self.exited = Some(code),
                PtyEvent::Error(error) => self.error = Some(error),
            }
        }
        if self
            .pending_submit_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.pending_submit_at = None;
            self.send(b"\r");
            changed = true;
        }
        changed
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if self.last_size == (rows, cols) {
            return;
        }
        self.last_size = (rows, cols);
        self.parser.screen_mut().set_size(rows, cols);
        if let Err(error) = self.master.resize(TerminalSize { rows, cols }) {
            self.error = Some(format!("resize failed: {error}"));
        }
    }

    fn scroll_viewport(&mut self, action: &str, count: Option<usize>) -> Result<usize> {
        let page = usize::from(self.last_size.0.saturating_sub(1)).max(1);
        let screen = self.parser.screen_mut();
        let current = screen.scrollback();
        let requested = match action {
            "up" => current.saturating_add(count.unwrap_or(1)),
            "down" => current.saturating_sub(count.unwrap_or(1)),
            "page-up" => current.saturating_add(count.unwrap_or(page)),
            "page-down" => current.saturating_sub(count.unwrap_or(page)),
            "top" if count.is_none() => usize::MAX,
            "bottom" if count.is_none() => 0,
            "top" | "bottom" => anyhow::bail!("{action} does not accept a row count"),
            _ => anyhow::bail!(
                "usage: scroll-pane [-t target] up|down|page-up|page-down|top|bottom [rows]"
            ),
        };
        screen.set_scrollback(requested);
        Ok(screen.scrollback())
    }

    fn scrollback_bounds(&mut self) -> (usize, usize) {
        let screen = self.parser.screen_mut();
        let current = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let maximum = screen.scrollback();
        screen.set_scrollback(current);
        (screen.scrollback(), maximum)
    }

    fn set_scrollback(&mut self, offset: usize) -> usize {
        let screen = self.parser.screen_mut();
        screen.set_scrollback(offset);
        screen.scrollback()
    }

    fn send(&mut self, bytes: &[u8]) -> bool {
        if self.exited.is_some() {
            return false;
        }
        if let Err(error) = self.master.write_all(bytes) {
            self.error = Some(format!("input failed: {error}"));
            false
        } else {
            self.input_bytes += bytes.len();
            self.input_writes += 1;
            true
        }
    }

    fn submit(&mut self, text: &str) -> bool {
        if self.pending_submit_at.is_some() || !self.send(text.as_bytes()) {
            return false;
        }
        // Interactive TUIs classify rapid character streams as paste and
        // temporarily reinterpret Enter as a pasted newline. Schedule Enter
        // after that suppression window without blocking the GUI thread.
        self.pending_submit_at = Some(Instant::now() + COMPOSER_SUBMIT_DELAY);
        true
    }

    fn send_native_mouse_click(&mut self, x: u16, y: u16) -> Result<()> {
        if self.exited.is_some() {
            anyhow::bail!("process has exited");
        }
        let raw_pid = self.process_id.context("process id is unavailable")?;
        let process_id = ProcessId::new(raw_pid).context("invalid process id")?;
        let x = i16::try_from(x).context("mouse x coordinate is too large")?;
        let y = i16::try_from(y).context("mouse y coordinate is too large")?;
        write_windows_console_mouse_drag(process_id, x, y, x, y)
            .context("WriteConsoleInputW click failed")?;
        self.input_bytes += 3;
        Ok(())
    }

    fn send_rmux_status_click(&mut self, x: u16, y: u16) -> bool {
        let (rows, cols) = self.last_size;
        if y >= rows {
            return false;
        }
        let (target, active, mut windows) = {
            let screen = self.parser.screen();
            let mut target = None;
            let mut active = None;
            let mut windows = Vec::new();
            for row in 0..rows {
                let mut line = String::with_capacity(cols as usize);
                for col in 0..cols {
                    let text = screen
                        .cell(row, col)
                        .map(|cell| cell.contents())
                        .unwrap_or(" ");
                    line.push(if text.is_empty() {
                        ' '
                    } else {
                        text.chars().next().unwrap_or(' ')
                    });
                }
                for status_window in parse_status_windows(&line) {
                    if status_window.active {
                        active = Some(status_window.index);
                    }
                    if row == y
                        && usize::from(x) >= status_window.start
                        && usize::from(x) < status_window.end
                    {
                        target = Some(status_window.index);
                    }
                    if !windows.contains(&status_window.index) {
                        windows.push(status_window.index);
                    }
                }
            }
            (target, active, windows)
        };
        let (Some(target), Some(active)) = (target, active) else {
            return false;
        };
        if target == active {
            return true;
        }
        windows.sort_unstable();
        let Some(active_position) = windows.iter().position(|index| *index == active) else {
            return false;
        };
        let Some(target_position) = windows.iter().position(|index| *index == target) else {
            return false;
        };
        let forward = (target_position + windows.len() - active_position) % windows.len();
        let backward = (active_position + windows.len() - target_position) % windows.len();
        let (key, repeats) = if forward <= backward {
            (b"\x1bOS".as_slice(), forward)
        } else {
            (b"\x1bOR".as_slice(), backward)
        };
        for _ in 0..repeats {
            self.send(key);
        }
        true
    }

    fn close_process(&mut self) {
        if self.exited.is_none() {
            let _ = self.child.terminate_forcefully();
        }
    }
}

impl Drop for TerminalTab {
    fn drop(&mut self) {
        self.close_process();
    }
}

struct NativeControls {
    edit: HWND,
    send_button: HWND,
    settings_button: HWND,
    new_button: HWND,
    settings_font: HWND,
    settings_size: HWND,
    settings_dark: HWND,
    settings_light: HWND,
    settings_cancel: HWND,
    settings_apply: HWND,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditShortcut {
    SelectAll,
    Copy,
    Cut,
    Paste,
}

fn edit_shortcut(control: bool, virtual_key: u32) -> Option<EditShortcut> {
    control.then_some(())?;
    match virtual_key {
        key if key == b'A' as u32 => Some(EditShortcut::SelectAll),
        key if key == b'C' as u32 => Some(EditShortcut::Copy),
        key if key == b'X' as u32 => Some(EditShortcut::Cut),
        key if key == b'V' as u32 => Some(EditShortcut::Paste),
        _ => None,
    }
}

fn terminal_copy_shortcut(
    control: bool,
    virtual_key: u32,
    terminal_focused: bool,
    has_active_selection: bool,
) -> bool {
    control && virtual_key == b'C' as u32 && terminal_focused && has_active_selection
}

#[derive(Clone, Copy, Debug)]
struct ScrollDrag {
    thumb_grab_offset: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TerminalPoint {
    row: u16,
    col: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalSelection {
    tab_id: u64,
    anchor: TerminalPoint,
    focus: TerminalPoint,
    dragging: bool,
    moved: bool,
}

impl TerminalSelection {
    fn bounds(self) -> (TerminalPoint, TerminalPoint) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    fn contains(self, row: u16, col: u16) -> bool {
        let (start, end) = self.bounds();
        TerminalPoint { row, col } >= start && TerminalPoint { row, col } <= end
    }
}

fn terminal_selection_text(screen: &vt100::Screen, selection: TerminalSelection) -> String {
    let (rows, cols) = screen.size();
    if rows == 0 || cols == 0 {
        return String::new();
    }
    let (mut start, mut end) = selection.bounds();
    start.row = start.row.min(rows - 1);
    start.col = start.col.min(cols - 1);
    end.row = end.row.min(rows - 1);
    end.col = end.col.min(cols - 1);

    let mut selected = String::new();
    for row in start.row..=end.row {
        let first_col = if row == start.row { start.col } else { 0 };
        let last_col = if row == end.row { end.col } else { cols - 1 };
        let mut line = String::new();
        for col in first_col..=last_col {
            match screen.cell(row, col) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if !cell.contents().is_empty() => line.push_str(cell.contents()),
                _ => line.push(' '),
            }
        }
        selected.push_str(line.trim_end_matches(' '));
        if row != end.row {
            selected.push_str("\r\n");
        }
    }
    selected
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
                "clipboard text exceeds the {TERMINAL_PASTE_LIMIT_BYTES} byte terminal paste limit"
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

struct ClipboardPaste {
    tab_id: u64,
    result: std::result::Result<String, String>,
}

#[derive(Clone, Copy)]
enum WindowCloseChoice {
    KeepServerRunning,
    StopServerAndExit,
    Cancel,
}

struct AppState {
    window: HWND,
    edit: HWND,
    send_button: HWND,
    settings_button: HWND,
    new_button: HWND,
    settings_font: HWND,
    settings_size: HWND,
    settings_dark: HWND,
    settings_light: HWND,
    settings_cancel: HWND,
    settings_apply: HWND,
    tabs: Vec<TerminalTab>,
    collapsed_tabs: HashSet<u64>,
    active: Option<u64>,
    next_id: u64,
    session_name: String,
    started_at: SystemTime,
    event_journal: EventJournal,
    ipc_receiver: Receiver<IpcEnvelope>,
    clipboard_sender: Sender<ClipboardPaste>,
    clipboard_receiver: Receiver<ClipboardPaste>,
    startup_tab_receiver: Receiver<std::result::Result<TerminalTab, String>>,
    startup_tab_pending: bool,
    startup_tabs_remaining: usize,
    last_error: Option<String>,
    close_requested: bool,
    window_close_pending: bool,
    window_detached: bool,
    detached_was_maximized: bool,
    window_close_previous_focus: HWND,
    window_close_restore_settings: bool,
    pending_close: Option<u64>,
    feedback: Option<String>,
    note_edit_target: Option<u64>,
    settings_open: bool,
    settings_theme_draft: ThemeId,
    config: AppConfig,
    terminal_font: HFONT,
    terminal_font_owned: bool,
    resolved_font_family: String,
    wheel_remainder: i32,
    scroll_drag: Option<ScrollDrag>,
    terminal_selection: Option<TerminalSelection>,
    _instance_registration: InstanceRegistration,
}

impl AppState {
    fn new(window: HWND, controls: NativeControls) -> Result<Self> {
        let ipc_receiver = start_ipc_server(window)?;
        let (startup_tab_sender, startup_tab_receiver) = mpsc::channel();
        let (clipboard_sender, clipboard_receiver) = mpsc::channel();
        let config = load_config();
        let restored = load_workspace();
        let (session_name, active_id, collapsed_tabs, saved_tabs) =
            if let Some(workspace) = restored {
                (
                    if workspace.session_name.is_empty() {
                        "agenterm".to_owned()
                    } else {
                        workspace.session_name
                    },
                    workspace.active_id,
                    workspace.collapsed_ids.into_iter().collect(),
                    workspace.tabs,
                )
            } else {
                (
                    "agenterm".to_owned(),
                    Some(1),
                    HashSet::new(),
                    vec![SavedTab {
                        id: 1,
                        index: 0,
                        ..SavedTab::default()
                    }],
                )
            };
        let next_id = saved_tabs.iter().map(|tab| tab.id).max().unwrap_or(0) + 1;
        let startup_tabs_remaining = saved_tabs.len();
        let startup_session_name = session_name.clone();
        let instance_registration =
            register_instance(&ipc_address(), &workspace_path(), &session_name)?;
        let (terminal_font, terminal_font_owned, resolved_font_family) =
            create_terminal_font(window, &config);
        let state = Self {
            window,
            edit: controls.edit,
            send_button: controls.send_button,
            settings_button: controls.settings_button,
            new_button: controls.new_button,
            settings_font: controls.settings_font,
            settings_size: controls.settings_size,
            settings_dark: controls.settings_dark,
            settings_light: controls.settings_light,
            settings_cancel: controls.settings_cancel,
            settings_apply: controls.settings_apply,
            tabs: Vec::new(),
            collapsed_tabs,
            active: active_id,
            next_id,
            session_name,
            started_at: SystemTime::now(),
            event_journal: EventJournal::new(),
            ipc_receiver,
            clipboard_sender,
            clipboard_receiver,
            startup_tab_receiver,
            startup_tab_pending: startup_tabs_remaining > 0,
            startup_tabs_remaining,
            last_error: None,
            close_requested: false,
            window_close_pending: false,
            window_detached: false,
            detached_was_maximized: false,
            window_close_previous_focus: ptr::null_mut(),
            window_close_restore_settings: false,
            pending_close: None,
            feedback: None,
            note_edit_target: None,
            settings_open: false,
            settings_theme_draft: config.color_theme,
            config,
            terminal_font,
            terminal_font_owned,
            resolved_font_family,
            wheel_remainder: 0,
            scroll_drag: None,
            terminal_selection: None,
            _instance_registration: instance_registration,
        };

        let wake_window = window as isize;
        thread::spawn(move || {
            for saved in saved_tabs {
                let title = (!saved.title.is_empty()).then(|| saved.title.clone());
                let tab = TerminalTab::spawn(TerminalLaunch {
                    id: saved.id,
                    index: saved.index,
                    parent_id: saved.parent_id,
                    title,
                    command_line: saved.command_line,
                    tab_environment: Vec::new(),
                    session_name: startup_session_name.clone(),
                    window: wake_window as HWND,
                    initial_size: TerminalSize {
                        rows: INITIAL_ROWS,
                        cols: INITIAL_COLS,
                    },
                })
                .map(|mut tab| {
                    tab.note = saved.note;
                    tab.composer = saved.composer;
                    tab
                })
                .map_err(|error| format!("@{}: {error:#}", saved.id));
                if startup_tab_sender.send(tab).is_err() {
                    break;
                }
                unsafe {
                    PostMessageW(wake_window as HWND, WM_APP_WAKE, 0, 0);
                }
            }
        });
        Ok(state)
    }

    fn create_tab(
        &mut self,
        title: Option<String>,
        command: Vec<String>,
        select: bool,
    ) -> Result<u32> {
        self.create_tab_with_parent(title, command, Vec::new(), select, None)
    }

    fn saved_workspace(&self) -> SavedWorkspace {
        SavedWorkspace {
            version: 1,
            session_name: self.session_name.clone(),
            active_id: self.active,
            collapsed_ids: self.collapsed_tabs.iter().copied().collect(),
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    id: tab.id,
                    index: tab.index,
                    parent_id: tab.parent_id,
                    title: tab.title.clone(),
                    note: tab.note.clone(),
                    composer: tab.composer.clone(),
                    command_line: tab.command_line.clone(),
                })
                .collect(),
        }
    }

    fn persist_workspace(&mut self) -> Result<()> {
        self.save_active_composer();
        save_workspace(&self.saved_workspace())?;
        self.event_journal
            .commit("workspace.saved", None, serde_json::json!({}));
        Ok(())
    }

    fn create_tab_with_parent(
        &mut self,
        title: Option<String>,
        command: Vec<String>,
        tab_environment: Vec<(String, String)>,
        select: bool,
        parent_id: Option<u64>,
    ) -> Result<u32> {
        if let Some(parent_id) = parent_id
            && !self.tabs.iter().any(|tab| tab.id == parent_id)
        {
            anyhow::bail!("can't find parent tab: @{parent_id}");
        }
        self.save_active_composer();
        let id = self.next_id;
        self.next_id += 1;
        let index = (0..)
            .find(|candidate| {
                !(self.startup_tab_pending && *candidate == 0)
                    && !self.tabs.iter().any(|tab| tab.index == *candidate)
            })
            .unwrap_or(self.tabs.len() as u32);
        let (rows, cols) = self
            .active_position()
            .and_then(|position| self.tabs.get(position))
            .or_else(|| self.tabs.first())
            .map(|tab| tab.last_size)
            .unwrap_or((INITIAL_ROWS, INITIAL_COLS));
        let tab = TerminalTab::spawn(TerminalLaunch {
            id,
            index,
            parent_id,
            title,
            command_line: command,
            tab_environment,
            session_name: self.session_name.clone(),
            window: self.window,
            initial_size: TerminalSize { rows, cols },
        })?;
        self.tabs.push(tab);
        self.tabs.sort_by_key(|tab| tab.index);
        self.event_journal.commit(
            "tab.created",
            Some(id),
            serde_json::json!({
                "index": index,
                "parent_id": parent_id,
                "selected": select,
            }),
        );
        if select {
            self.active = Some(id);
            self.load_active_composer();
            self.event_journal
                .commit("tab.selected", Some(id), serde_json::json!({}));
        }
        Ok(index)
    }

    fn tree_nodes(&self) -> Vec<TabTreeNode> {
        self.tabs
            .iter()
            .map(|tab| TabTreeNode {
                id: tab.id,
                parent_id: tab.parent_id,
                sort_key: tab.index,
            })
            .collect()
    }

    fn all_tree_rows(&self) -> Vec<TabTreeRow> {
        tree_rows(&self.tree_nodes())
    }

    fn tree_rows(&self) -> Vec<TabTreeRow> {
        self.all_tree_rows()
            .into_iter()
            .filter(|row| {
                !row.ancestors
                    .iter()
                    .any(|id| self.collapsed_tabs.contains(id))
            })
            .collect()
    }

    fn tree_row_position(&self, row: usize) -> Option<usize> {
        let id = self.tree_rows().get(row)?.id;
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn tab_depth(&self, id: u64) -> usize {
        self.all_tree_rows()
            .iter()
            .find(|row| row.id == id)
            .map(|row| row.depth)
            .unwrap_or(0)
    }

    fn set_tab_parent(&mut self, child_id: u64, parent_id: Option<u64>) -> Result<()> {
        let Some(child_position) = self.tabs.iter().position(|tab| tab.id == child_id) else {
            anyhow::bail!("can't find child tab: @{child_id}");
        };
        if let Some(parent_id) = parent_id {
            if !self.tabs.iter().any(|tab| tab.id == parent_id) {
                anyhow::bail!("can't find parent tab: @{parent_id}");
            }
            if would_create_cycle(&self.tree_nodes(), child_id, parent_id) {
                anyhow::bail!("moving @{child_id} under @{parent_id} would create a cycle");
            }
        }
        let previous_parent_id = self.tabs[child_position].parent_id;
        self.tabs[child_position].parent_id = parent_id;
        self.event_journal.commit(
            "tab.parent",
            Some(child_id),
            serde_json::json!({
                "previous_parent_id": previous_parent_id,
                "parent_id": parent_id,
            }),
        );
        Ok(())
    }

    fn active_position(&self) -> Option<usize> {
        let active = self.active?;
        self.tabs.iter().position(|tab| tab.id == active)
    }

    fn target_position(&self, target: Option<&str>) -> Option<usize> {
        let Some(target) = target else {
            return self.active_position();
        };
        let target = target
            .rsplit(':')
            .next()
            .unwrap_or(target)
            .trim_start_matches(['=', '%']);
        if let Some(id) = target
            .strip_prefix('@')
            .and_then(|value| value.parse::<u64>().ok())
        {
            return self.tabs.iter().position(|tab| tab.id == id);
        }
        if let Ok(index) = target.parse::<u32>() {
            return self.tabs.iter().position(|tab| tab.index == index);
        }
        self.tabs.iter().position(|tab| tab.title == target)
    }

    fn parent_id_from_target(&self, target: &str) -> Result<Option<u64>> {
        if matches!(target, "root" | "none" | "-") {
            return Ok(None);
        }
        let Some(position) = self.target_position(Some(target)) else {
            anyhow::bail!("can't find parent tab: {target}");
        };
        Ok(Some(self.tabs[position].id))
    }

    fn close_tab(&mut self, id: u64) {
        self.save_active_composer();
        let Some(position) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let parent_id = self.tabs[position].parent_id;
        let index = self.tabs[position].index;
        let exit_code = self.tabs[position].exited;
        let promoted_children = self
            .tabs
            .iter()
            .filter(|tab| tab.parent_id == Some(id))
            .map(|tab| tab.id)
            .collect::<Vec<_>>();
        for tab in &mut self.tabs {
            if tab.parent_id == Some(id) {
                tab.parent_id = parent_id;
            }
        }
        self.collapsed_tabs.remove(&id);
        self.tabs[position].close_process();
        self.tabs.remove(position);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(position)
                .or_else(|| position.checked_sub(1).and_then(|i| self.tabs.get(i)))
                .map(|tab| tab.id);
        }
        self.event_journal.commit(
            "tab.closed",
            Some(id),
            serde_json::json!({
                "index": index,
                "parent_id": parent_id,
                "exit_code": exit_code,
                "promoted_children": promoted_children,
                "active_id": self.active,
            }),
        );
        self.load_active_composer();
    }

    fn request_close_tab(&mut self, id: u64) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.exited.is_some() {
            self.close_tab(id);
            return;
        }
        self.pending_close = Some(id);
        unsafe {
            ShowWindow(self.edit, SW_HIDE);
            ShowWindow(self.send_button, SW_HIDE);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn finish_close_confirmation(&mut self, confirm: bool) {
        let pending = self.pending_close.take();
        if confirm && let Some(id) = pending {
            self.close_tab(id);
        }
        unsafe {
            ShowWindow(self.edit, SW_SHOW);
            ShowWindow(self.send_button, SW_SHOW);
            SetFocus(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn request_window_close(&mut self) {
        if self.window_close_pending {
            return;
        }
        let previous_focus = unsafe { GetFocus() };
        self.window_close_restore_settings = self.settings_open;
        if self.settings_open {
            self.settings_open = false;
            unsafe {
                ShowWindow(self.settings_font, SW_HIDE);
                ShowWindow(self.settings_size, SW_HIDE);
                ShowWindow(self.settings_dark, SW_HIDE);
                ShowWindow(self.settings_light, SW_HIDE);
                ShowWindow(self.settings_cancel, SW_HIDE);
                ShowWindow(self.settings_apply, SW_HIDE);
            }
        }
        if self.pending_close.is_some() {
            self.finish_close_confirmation(false);
        }
        self.save_active_composer();
        self.window_close_previous_focus = previous_focus;
        self.window_close_pending = true;
        unsafe {
            ShowWindow(self.edit, SW_HIDE);
            ShowWindow(self.send_button, SW_HIDE);
            SetFocus(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn finish_window_close(&mut self, choice: WindowCloseChoice) {
        if !self.window_close_pending {
            return;
        }
        self.window_close_pending = false;
        match choice {
            WindowCloseChoice::KeepServerRunning => {
                if let Err(error) = self.persist_workspace() {
                    self.last_error = Some(format!("workspace save failed: {error:#}"));
                    self.restore_after_window_close_modal();
                    return;
                }
                self.detached_was_maximized = unsafe { IsZoomed(self.window) } != 0;
                self.window_detached = true;
                self.event_journal.commit(
                    "window.visibility",
                    None,
                    serde_json::json!({"visible": false, "reason": "detach"}),
                );
                unsafe { ShowWindow(self.window, SW_HIDE) };
            }
            WindowCloseChoice::StopServerAndExit => {
                if let Err(error) = self.persist_workspace() {
                    self.last_error = Some(format!("workspace save failed: {error:#}"));
                    self.restore_after_window_close_modal();
                    return;
                }
                self.event_journal.commit(
                    "workspace.shutdown",
                    None,
                    serde_json::json!({"saved": true, "source": "window-close"}),
                );
                self.close_requested = true;
                unsafe { PostMessageW(self.window, WM_CLOSE, 0, 0) };
            }
            WindowCloseChoice::Cancel => self.restore_after_window_close_modal(),
        }
    }

    fn restore_after_window_close_modal(&mut self) {
        unsafe {
            if self.window_close_restore_settings {
                self.settings_open = true;
                ShowWindow(self.settings_font, SW_SHOW);
                ShowWindow(self.settings_size, SW_SHOW);
                ShowWindow(self.settings_dark, SW_SHOW);
                ShowWindow(self.settings_light, SW_SHOW);
                ShowWindow(self.settings_cancel, SW_SHOW);
                ShowWindow(self.settings_apply, SW_SHOW);
            } else {
                ShowWindow(self.edit, SW_SHOW);
                ShowWindow(self.send_button, SW_SHOW);
            }
            if self.window_close_previous_focus.is_null() {
                SetFocus(self.window);
            } else {
                SetFocus(self.window_close_previous_focus);
            }
            InvalidateRect(self.window, ptr::null(), 0);
        }
        if !self.window_close_restore_settings {
            self.load_active_composer();
        }
        self.window_close_restore_settings = false;
    }

    fn reattach_window(&mut self, reason: &str) {
        let was_detached = self.window_detached || unsafe { IsWindowVisible(self.window) } == 0;
        self.window_detached = false;
        unsafe {
            ShowWindow(
                self.window,
                if self.detached_was_maximized {
                    SW_SHOWMAXIMIZED
                } else {
                    SW_SHOWNORMAL
                },
            );
            SetForegroundWindow(self.window);
            SetFocus(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
        if was_detached {
            self.event_journal.commit(
                "window.visibility",
                None,
                serde_json::json!({"visible": true, "reason": reason}),
            );
        }
    }

    fn tick(&mut self) -> bool {
        let mut changed = false;
        if self.startup_tab_pending {
            loop {
                match self.startup_tab_receiver.try_recv() {
                    Ok(Ok(tab)) => {
                        let id = tab.id;
                        let index = tab.index;
                        self.tabs.push(tab);
                        self.tabs.sort_by_key(|tab| tab.index);
                        if self.active.is_none() {
                            self.active = Some(id);
                        }
                        if self.active == Some(id) {
                            self.load_active_composer();
                        }
                        self.event_journal.commit(
                            "tab.created",
                            Some(id),
                            serde_json::json!({
                                "index": index,
                                "restored": true,
                                "selected": self.active == Some(id),
                            }),
                        );
                        self.startup_tabs_remaining = self.startup_tabs_remaining.saturating_sub(1);
                        changed = true;
                    }
                    Ok(Err(error)) => {
                        self.startup_tabs_remaining = self.startup_tabs_remaining.saturating_sub(1);
                        self.last_error = Some(format!("restored terminal failed: {error}"));
                        changed = true;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if self.startup_tabs_remaining > 0 {
                            self.last_error =
                                Some("workspace restore worker stopped unexpectedly".to_owned());
                            self.startup_tabs_remaining = 0;
                            changed = true;
                        }
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
            self.startup_tab_pending = self.startup_tabs_remaining > 0;
            if !self.startup_tab_pending
                && self.active_position().is_none()
                && let Some(tab) = self.tabs.first()
            {
                self.active = Some(tab.id);
                self.load_active_composer();
            }
        }
        let clipboard_pastes: Vec<ClipboardPaste> = self.clipboard_receiver.try_iter().collect();
        for paste in clipboard_pastes {
            changed |= self.apply_terminal_paste(paste);
        }
        let mut polled_events = Vec::new();
        for tab in &mut self.tabs {
            let output_before = tab.output_bytes;
            let exit_before = tab.exited;
            let error_before = tab.error.clone();
            changed |= tab.poll();
            if tab.output_bytes != output_before {
                polled_events.push((
                    "terminal.output",
                    tab.id,
                    serde_json::json!({
                        "output_bytes": tab.output_bytes,
                        "advanced_by": tab.output_bytes - output_before,
                    }),
                ));
            }
            if tab.exited != exit_before || tab.error != error_before {
                polled_events.push((
                    "tab.state",
                    tab.id,
                    serde_json::json!({
                        "state": if tab.error.is_some() {
                            "error"
                        } else if tab.exited.is_some() {
                            "dead"
                        } else {
                            "running"
                        },
                        "exit_code": tab.exited,
                        "error": tab.error.clone(),
                    }),
                ));
            }
        }
        for (kind, tab_id, payload) in polled_events {
            self.event_journal.commit(kind, Some(tab_id), payload);
        }
        let envelopes: Vec<IpcEnvelope> = self.ipc_receiver.try_iter().collect();
        changed |= !envelopes.is_empty();
        for envelope in envelopes {
            let response = self.execute_command(&envelope.request.args);
            let _ = envelope.respond_to.send(response);
        }
        if self.close_requested {
            unsafe { PostMessageW(self.window, WM_CLOSE, 0, 0) };
        }
        changed
    }

    fn layout(&self) {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let content_bottom = (client.bottom - STATUS_BAR_HEIGHT).max(0);
        let content_width = (client.right - SIDEBAR_WIDTH).max(180);
        let edit_width = (content_width - 104).max(80);
        unsafe {
            MoveWindow(
                self.settings_button,
                12,
                (content_bottom - 42).max(0),
                108,
                30,
                1,
            );
            MoveWindow(
                self.new_button,
                130,
                (content_bottom - 42).max(0),
                108,
                30,
                1,
            );
            MoveWindow(
                self.edit,
                SIDEBAR_WIDTH + 10,
                (content_bottom - COMPOSER_HEIGHT + 30).max(0),
                edit_width,
                COMPOSER_HEIGHT - 40,
                1,
            );
            MoveWindow(
                self.send_button,
                SIDEBAR_WIDTH + 20 + edit_width,
                (content_bottom - COMPOSER_HEIGHT + 32).max(0),
                74,
                34,
                1,
            );
            let settings_left = SIDEBAR_WIDTH + (content_width - 520) / 2;
            let settings_top = (content_bottom - 320) / 2;
            MoveWindow(
                self.settings_font,
                settings_left + 34,
                settings_top + 92,
                330,
                30,
                1,
            );
            MoveWindow(
                self.settings_size,
                settings_left + 380,
                settings_top + 92,
                68,
                30,
                1,
            );
            MoveWindow(
                self.settings_dark,
                settings_left + 34,
                settings_top + 174,
                128,
                32,
                1,
            );
            MoveWindow(
                self.settings_light,
                settings_left + 174,
                settings_top + 174,
                128,
                32,
                1,
            );
            MoveWindow(
                self.settings_cancel,
                settings_left + 264,
                settings_top + 246,
                86,
                34,
                1,
            );
            MoveWindow(
                self.settings_apply,
                settings_left + 362,
                settings_top + 246,
                86,
                34,
                1,
            );
        }
    }

    fn open_settings(&mut self) {
        self.save_active_composer();
        self.settings_open = true;
        self.settings_theme_draft = self.config.color_theme;
        self.note_edit_target = None;
        self.refresh_theme_controls();
        unsafe {
            SetWindowTextW(
                self.settings_font,
                wide(&self.config.terminal_font_family).as_ptr(),
            );
            SetWindowTextW(
                self.settings_size,
                wide(&self.config.terminal_font_size.to_string()).as_ptr(),
            );
            ShowWindow(self.edit, SW_HIDE);
            ShowWindow(self.send_button, SW_HIDE);
            ShowWindow(self.settings_font, SW_SHOW);
            ShowWindow(self.settings_size, SW_SHOW);
            ShowWindow(self.settings_dark, SW_SHOW);
            ShowWindow(self.settings_light, SW_SHOW);
            ShowWindow(self.settings_cancel, SW_SHOW);
            ShowWindow(self.settings_apply, SW_SHOW);
            SetFocus(self.settings_font);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn close_settings(&mut self) {
        self.settings_open = false;
        self.settings_theme_draft = self.config.color_theme;
        unsafe {
            ShowWindow(self.settings_font, SW_HIDE);
            ShowWindow(self.settings_size, SW_HIDE);
            ShowWindow(self.settings_dark, SW_HIDE);
            ShowWindow(self.settings_light, SW_HIDE);
            ShowWindow(self.settings_cancel, SW_HIDE);
            ShowWindow(self.settings_apply, SW_HIDE);
            ShowWindow(self.edit, SW_SHOW);
            ShowWindow(self.send_button, SW_SHOW);
            SetFocus(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
        self.load_active_composer();
    }

    fn preview_theme(&mut self, theme: ThemeId) {
        if !self.settings_open {
            return;
        }
        self.settings_theme_draft = theme;
        self.refresh_theme_controls();
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn refresh_theme_controls(&self) {
        for theme in ThemeId::ALL {
            let control = match theme {
                ThemeId::Dark => self.settings_dark,
                ThemeId::Light => self.settings_light,
            };
            let marker = if theme == self.settings_theme_draft {
                "Selected"
            } else {
                "Preview"
            };
            unsafe {
                SetWindowTextW(
                    control,
                    wide(&format!("{} · {marker}", theme.label())).as_ptr(),
                );
            }
        }
    }

    fn palette(&self) -> &'static ThemePalette {
        effective_theme(
            self.config.color_theme,
            self.settings_theme_draft,
            self.settings_open,
        )
        .palette()
    }

    fn apply_settings_from_controls(&mut self) {
        let family = window_text(self.settings_font).trim().to_owned();
        let size = window_text(self.settings_size).trim().parse::<u16>();
        let Ok(size) = size else {
            self.last_error = Some("Font size must be a number from 8 to 36".to_owned());
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        };
        if family.is_empty() || !(8..=36).contains(&size) {
            self.last_error =
                Some("Font family is required and size must be from 8 to 36".to_owned());
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        let mut next_config = self.config.clone();
        next_config.terminal_font_family = family;
        next_config.terminal_font_size = size;
        next_config.color_theme = self.settings_theme_draft;
        if let Err(error) = save_config(&next_config) {
            self.last_error = Some(format!("Could not save settings: {error:#}"));
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        self.config = next_config;
        self.rebuild_terminal_font();
        self.last_error = None;
        self.feedback = Some(format!(
            "{} theme · Terminal font: {} {}pt (resolved: {})",
            self.config.color_theme.label(),
            self.config.terminal_font_family,
            self.config.terminal_font_size,
            self.resolved_font_family
        ));
        self.close_settings();
    }

    fn rebuild_terminal_font(&mut self) {
        let (font, owned, resolved) = create_terminal_font(self.window, &self.config);
        if self.terminal_font_owned {
            unsafe { DeleteObject(self.terminal_font as HGDIOBJ) };
        }
        self.terminal_font = font;
        self.terminal_font_owned = owned;
        self.resolved_font_family = resolved;
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn left_button_down(&mut self, x: i32, y: i32) {
        if self.window_close_pending
            || self.pending_close.is_some()
            || self.settings_open
            || x < SIDEBAR_WIDTH
        {
            self.terminal_selection = None;
            self.click(x, y);
            return;
        }
        unsafe { SetFocus(self.window) };
        if self.click_scrollbar(x, y) {
            self.terminal_selection = None;
            return;
        }
        let Some((column, row)) = self.terminal_cell_at(x, y) else {
            return;
        };
        let Some(position) = self.active_position() else {
            return;
        };
        let point = TerminalPoint { row, col: column };
        self.terminal_selection = Some(TerminalSelection {
            tab_id: self.tabs[position].id,
            anchor: point,
            focus: point,
            dragging: true,
            moved: false,
        });
        unsafe {
            SetCapture(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn left_button_up(&mut self) {
        if self.scroll_drag.is_some() {
            self.end_scroll_drag();
            return;
        }
        let Some(mut selection) = self.terminal_selection else {
            return;
        };
        if !selection.dragging {
            return;
        }
        selection.dragging = false;
        unsafe { ReleaseCapture() };
        if selection.moved {
            self.terminal_selection = Some(selection);
        } else {
            self.terminal_selection = None;
            self.send_terminal_click(selection.anchor.col, selection.anchor.row);
        }
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn mouse_move(&mut self, x: i32, y: i32) {
        if self.scroll_drag.is_some() {
            self.drag_scrollbar(x, y);
            return;
        }
        let Some(mut selection) = self.terminal_selection else {
            return;
        };
        if !selection.dragging {
            return;
        }
        let terminal = self.terminal_rect();
        let max_x = (terminal.right - TERMINAL_SCROLLBAR_WIDTH - 1).max(terminal.left);
        let max_y = terminal.bottom.saturating_sub(1).max(terminal.top);
        let clamped_x = x.clamp(terminal.left, max_x);
        let clamped_y = y.clamp(terminal.top, max_y);
        let Some((column, row)) = self.terminal_cell_at(clamped_x, clamped_y) else {
            return;
        };
        let focus = TerminalPoint { row, col: column };
        if focus != selection.focus {
            selection.focus = focus;
            selection.moved = selection.focus != selection.anchor;
            self.terminal_selection = Some(selection);
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        }
    }

    fn send_terminal_click(&mut self, column: u16, row: u16) {
        let Some(position) = self.active_position() else {
            return;
        };
        if !self.tabs[position].send_rmux_status_click(column, row)
            && let Err(error) = self.tabs[position].send_native_mouse_click(column, row)
        {
            self.last_error = Some(format!("native mouse input failed: {error}"));
        }
    }

    fn click(&mut self, x: i32, y: i32) {
        if self.window_close_pending {
            let mut client: RECT = unsafe { mem::zeroed() };
            unsafe { GetClientRect(self.window, &mut client) };
            let left = SIDEBAR_WIDTH + (client.right - SIDEBAR_WIDTH - 620) / 2;
            let top = (client.bottom - STATUS_BAR_HEIGHT - 230) / 2;
            if y >= top + 154 && y <= top + 194 {
                if x >= left + 24 && x <= left + 234 {
                    self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                } else if x >= left + 246 && x <= left + 456 {
                    self.finish_window_close(WindowCloseChoice::StopServerAndExit);
                } else if x >= left + 468 && x <= left + 596 {
                    self.finish_window_close(WindowCloseChoice::Cancel);
                }
            }
            return;
        }
        if self.pending_close.is_some() || self.settings_open {
            let mut client: RECT = unsafe { mem::zeroed() };
            unsafe { GetClientRect(self.window, &mut client) };
            let modal_left = SIDEBAR_WIDTH + (client.right - SIDEBAR_WIDTH - 460) / 2;
            let modal_top = (client.bottom - STATUS_BAR_HEIGHT - 190) / 2;
            if y >= modal_top + 125 && y <= modal_top + 163 {
                if x >= modal_left + 238 && x <= modal_left + 432 {
                    self.finish_close_confirmation(true);
                } else if x >= modal_left + 126 && x <= modal_left + 224 {
                    self.finish_close_confirmation(false);
                }
            }
            return;
        }
        if x >= SIDEBAR_WIDTH {
            unsafe { SetFocus(self.window) };
            if self.click_scrollbar(x, y) {
                return;
            }
            if let Some((column, row)) = self.terminal_cell_at(x, y) {
                self.send_terminal_click(column, row);
            }
            return;
        }
        let Some(row) = tree_row_at_y(y) else {
            return;
        };
        let Some(position) = self.tree_row_position(row) else {
            return;
        };
        let id = self.tabs[position].id;
        let tree_row = self.tree_rows().get(row).cloned();
        let has_children = self.tabs.iter().any(|tab| tab.parent_id == Some(id));
        let geometry = tree_row_geometry(
            row,
            tree_row.as_ref().map_or(0, |row| row.depth),
            SIDEBAR_WIDTH,
        );
        if has_children && geometry.disclosure_hit.contains_x(x) {
            if !self.collapsed_tabs.remove(&id) {
                self.collapsed_tabs.insert(id);
            }
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        let actions_visible = self.active == Some(id);
        if actions_visible && x >= TAB_CLOSE_LEFT {
            self.request_close_tab(id);
        } else if actions_visible && x >= TAB_EDIT_LEFT {
            self.open_tab_editor(id);
            return;
        } else if actions_visible && x >= TAB_ADD_LEFT {
            self.collapsed_tabs.remove(&id);
            match self.create_tab_with_parent(
                Some("New child".to_owned()),
                Vec::new(),
                Vec::new(),
                true,
                Some(id),
            ) {
                Ok(index) => {
                    if let Some(child_id) = self
                        .tabs
                        .iter()
                        .find(|tab| tab.index == index)
                        .map(|tab| tab.id)
                    {
                        self.open_tab_editor(child_id);
                        return;
                    }
                }
                Err(error) => self.last_error = Some(format!("{error:#}")),
            }
        } else {
            self.save_active_composer();
            self.active = Some(id);
            self.load_active_composer();
            self.event_journal
                .commit("tab.selected", Some(id), serde_json::json!({}));
        }
        unsafe {
            SetFocus(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn right_click(&mut self, x: i32, y: i32) {
        if x >= SIDEBAR_WIDTH || self.pending_close.is_some() || self.window_close_pending {
            return;
        }
        let Some(row) = tree_row_at_y(y) else {
            return;
        };
        let Some(position) = self.tree_row_position(row) else {
            return;
        };
        let id = self.tabs[position].id;
        self.open_tab_editor(id);
    }

    fn open_tab_editor(&mut self, id: u64) {
        self.save_active_composer();
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let content = format!("{}\r\n{}", tab.title, tab.note);
        self.active = Some(id);
        self.note_edit_target = Some(id);
        unsafe {
            SetWindowTextW(self.edit, wide(&content).as_ptr());
            SetWindowTextW(self.send_button, wide(LABEL_SAVE).as_ptr());
            SetFocus(self.edit);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn finish_note_edit(&mut self, save: bool) {
        let Some(id) = self.note_edit_target else {
            return;
        };
        if save {
            let content = window_text(self.edit).replace("\r\n", "\n");
            let (title, note) = content.split_once('\n').unwrap_or((&content, ""));
            let title = title.trim();
            if title.is_empty() {
                self.last_error = Some("Tab name cannot be empty".to_owned());
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                return;
            }
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                let previous_title = tab.title.clone();
                let previous_note = tab.note.clone();
                tab.title = title.to_owned();
                tab.note = note.trim_end().to_owned();
                if tab.title != previous_title {
                    self.event_journal.commit(
                        "tab.renamed",
                        Some(id),
                        serde_json::json!({
                            "previous_name": previous_title,
                            "name": tab.title,
                        }),
                    );
                }
                if tab.note != previous_note {
                    self.event_journal.commit(
                        "tab.note",
                        Some(id),
                        serde_json::json!({
                            "previous_note": previous_note,
                            "note": tab.note,
                        }),
                    );
                }
            }
        }
        self.note_edit_target = None;
        unsafe { SetWindowTextW(self.send_button, wide(LABEL_SEND).as_ptr()) };
        self.load_active_composer();
        unsafe {
            SetFocus(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn dispatch_edit_shortcut(&self, shortcut: EditShortcut, focused: HWND) {
        unsafe {
            match shortcut {
                EditShortcut::SelectAll => {
                    SendMessageW(focused, EM_SETSEL, 0, -1);
                }
                EditShortcut::Copy => {
                    SendMessageW(focused, WM_COPY, 0, 0);
                }
                EditShortcut::Cut => {
                    SendMessageW(focused, WM_CUT, 0, 0);
                }
                EditShortcut::Paste => {
                    SendMessageW(focused, WM_PASTE, 0, 0);
                }
            }
        }
    }

    fn is_edit_control(&self, window: HWND) -> bool {
        window == self.edit || window == self.settings_font || window == self.settings_size
    }

    fn edit_has_selection(&self, window: HWND) -> bool {
        let mut start = 0_u32;
        let mut end = 0_u32;
        unsafe {
            SendMessageW(
                window,
                EM_GETSEL,
                (&mut start as *mut u32) as usize,
                (&mut end as *mut u32) as isize,
            );
        }
        start != end
    }

    fn system_clipboard_menu_state(&self) -> (bool, bool) {
        let focused = unsafe { GetFocus() };
        if self.is_edit_control(focused) {
            return (
                self.edit_has_selection(focused),
                clipboard_has_unicode_text(),
            );
        }
        let terminal_active = focused == self.window
            && self
                .active_position()
                .is_some_and(|position| self.tabs[position].exited.is_none());
        let copy_enabled = terminal_active
            && self
                .terminal_selection
                .is_some_and(|selection| self.active == Some(selection.tab_id));
        let paste_enabled = terminal_active
            && !self.window_close_pending
            && self.pending_close.is_none()
            && !self.settings_open
            && clipboard_has_unicode_text();
        (copy_enabled, paste_enabled)
    }

    fn refresh_system_clipboard_menu(&self) {
        let menu = unsafe { GetSystemMenu(self.window, 0) };
        if menu.is_null() {
            return;
        }
        let (copy_enabled, paste_enabled) = self.system_clipboard_menu_state();
        unsafe {
            EnableMenuItem(
                menu,
                SYSTEM_MENU_COPY_ID as u32,
                MF_BYCOMMAND | if copy_enabled { MF_ENABLED } else { MF_GRAYED },
            );
            EnableMenuItem(
                menu,
                SYSTEM_MENU_PASTE_ID as u32,
                MF_BYCOMMAND | if paste_enabled { MF_ENABLED } else { MF_GRAYED },
            );
        }
    }

    fn system_menu_copy(&mut self) {
        let focused = unsafe { GetFocus() };
        if self.is_edit_control(focused) {
            if self.edit_has_selection(focused) {
                self.dispatch_edit_shortcut(EditShortcut::Copy, focused);
                self.feedback = Some("Copied selected editor text".to_owned());
                self.last_error = None;
            }
        } else if let Err(error) = self.copy_terminal_selection() {
            self.last_error = Some(format!("system menu copy failed: {error:#}"));
        }
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn system_menu_paste(&mut self) {
        let focused = unsafe { GetFocus() };
        if self.is_edit_control(focused) {
            self.dispatch_edit_shortcut(EditShortcut::Paste, focused);
            self.feedback = Some("Pasted clipboard text into editor".to_owned());
            self.last_error = None;
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        let Some(position) = self.active_position() else {
            self.last_error = Some("system menu paste failed: no active terminal".to_owned());
            return;
        };
        if focused != self.window || self.tabs[position].exited.is_some() {
            self.last_error =
                Some("system menu paste failed: focus is not in a running terminal".to_owned());
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        if self.tabs[position].pending_submit_at.is_some() {
            self.last_error =
                Some("system menu paste failed: composer submission is pending".to_owned());
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        let tab_id = self.tabs[position].id;
        let sender = self.clipboard_sender.clone();
        let wake_window = self.window as isize;
        self.feedback = Some(format!("Reading clipboard for @{tab_id}"));
        self.last_error = None;
        thread::spawn(move || {
            let result = read_clipboard_text()
                .map(|text| normalize_terminal_paste(&text))
                .and_then(|text| {
                    if text.is_empty() {
                        anyhow::bail!("clipboard text contains no pasteable characters");
                    }
                    if text.len() > TERMINAL_PASTE_LIMIT_BYTES {
                        anyhow::bail!(
                            "normalized clipboard text exceeds the \
                             {TERMINAL_PASTE_LIMIT_BYTES} byte terminal paste limit"
                        );
                    }
                    Ok(text)
                })
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(ClipboardPaste { tab_id, result });
            unsafe {
                PostMessageW(wake_window as HWND, WM_APP_WAKE, 0, 0);
            }
        });
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn apply_terminal_paste(&mut self, paste: ClipboardPaste) -> bool {
        let text = match paste.result {
            Ok(text) => text,
            Err(error) => {
                self.last_error = Some(format!("system menu paste failed: {error}"));
                return true;
            }
        };
        let Some(position) = self.tabs.iter().position(|tab| tab.id == paste.tab_id) else {
            self.last_error =
                Some("system menu paste canceled: target terminal was closed".to_owned());
            return true;
        };
        if self.active != Some(paste.tab_id) || unsafe { GetFocus() } != self.window {
            self.last_error =
                Some("system menu paste canceled because terminal focus changed".to_owned());
            return true;
        }
        let bracketed = self.tabs[position].parser.screen().bracketed_paste();
        let mut bytes = Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(text.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        if !self.tabs[position].send(&bytes) {
            self.last_error =
                Some("system menu paste failed: terminal input was rejected".to_owned());
            return true;
        }
        self.terminal_selection = None;
        let characters = text.chars().count();
        self.event_journal.commit(
            "terminal.pasted",
            Some(paste.tab_id),
            serde_json::json!({
                "characters": characters,
                "bytes": text.len(),
                "bracketed": bracketed,
                "source": "system-menu",
            }),
        );
        self.feedback = Some(format!(
            "Pasted {characters} characters into @{}",
            paste.tab_id
        ));
        self.last_error = None;
        true
    }

    fn copy_terminal_selection(&mut self) -> Result<usize> {
        let selection = self
            .terminal_selection
            .context("no terminal text is selected")?;
        let position = self
            .tabs
            .iter()
            .position(|tab| tab.id == selection.tab_id)
            .context("selected terminal is no longer available")?;
        if self.active != Some(selection.tab_id) {
            anyhow::bail!("selected terminal is not active");
        }
        let text = terminal_selection_text(self.tabs[position].parser.screen(), selection);
        set_clipboard_text(self.window, &text)?;
        let characters = text.chars().count();
        self.feedback = Some(format!(
            "Copied {characters} characters from @{}",
            selection.tab_id
        ));
        self.last_error = None;
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        Ok(characters)
    }

    fn handle_shortcut(&mut self, virtual_key: u32) -> bool {
        let control = unsafe { GetKeyState(0x11) } < 0;
        let shift = unsafe { GetKeyState(0x10) } < 0;
        let focused = unsafe { GetFocus() };

        if self.window_close_pending {
            if virtual_key == 0x0d {
                self.finish_window_close(WindowCloseChoice::KeepServerRunning);
            } else if virtual_key == 0x1b {
                self.finish_window_close(WindowCloseChoice::Cancel);
            }
            return true;
        }
        if self.pending_close.is_some() {
            if virtual_key == 0x0d {
                self.finish_close_confirmation(true);
                return true;
            }
            if virtual_key == 0x1b {
                self.finish_close_confirmation(false);
                return true;
            }
            return true;
        }
        if self.settings_open && virtual_key == 0x1b {
            self.close_settings();
            return true;
        }

        let has_active_selection = self
            .terminal_selection
            .is_some_and(|selection| self.active == Some(selection.tab_id));
        if terminal_copy_shortcut(
            control,
            virtual_key,
            focused == self.window,
            has_active_selection,
        ) {
            if let Err(error) = self.copy_terminal_selection() {
                self.last_error = Some(format!("terminal copy failed: {error:#}"));
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            }
            return true;
        }

        let focused_edit =
            focused == self.edit || focused == self.settings_font || focused == self.settings_size;
        if focused_edit && let Some(shortcut) = edit_shortcut(control, virtual_key) {
            self.dispatch_edit_shortcut(shortcut, focused);
            return true;
        }

        if focused == self.edit {
            if control && virtual_key == 0x0d {
                if self.note_edit_target.is_some() {
                    self.finish_note_edit(true);
                } else {
                    self.send_composer();
                    self.feedback = self.active.map(|id| format!("Sent to @{id}"));
                }
                unsafe { SetFocus(self.window) };
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                return true;
            }
            if virtual_key == 0x1b {
                if self.note_edit_target.is_some() {
                    self.finish_note_edit(false);
                } else {
                    self.save_active_composer();
                    unsafe { SetFocus(self.window) };
                }
                return true;
            }
        }

        if control && shift && virtual_key == b'T' as u32 {
            if let Err(error) = self.create_tab(None, Vec::new(), true) {
                self.last_error = Some(format!("{error:#}"));
            }
        } else if control && shift && virtual_key == b'W' as u32 {
            if let Some(position) = self.active_position() {
                self.request_close_tab(self.tabs[position].id);
            }
        } else if control && shift && virtual_key == b'I' as u32 {
            unsafe { SetFocus(self.edit) };
        } else if control && virtual_key == 0x09 {
            let response = self.select_adjacent(if shift { -1 } else { 1 });
            if !response.ok {
                self.last_error = Some(response.error);
            }
        } else {
            return false;
        }
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        true
    }

    fn terminal_rect(&self) -> PixelRect {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        PixelRect {
            left: SIDEBAR_WIDTH,
            top: 0,
            right: client.right.max(SIDEBAR_WIDTH),
            bottom: client
                .bottom
                .saturating_sub(STATUS_BAR_HEIGHT + COMPOSER_HEIGHT)
                .max(0),
        }
    }

    fn scrollbar_state(&mut self) -> Option<(TerminalScrollbarGeometry, usize)> {
        let terminal = self.terminal_rect();
        let position = self.active_position()?;
        let visible_rows = usize::from(self.tabs[position].last_size.0);
        let (offset, maximum) = self.tabs[position].scrollback_bounds();
        Some((
            terminal_scrollbar_geometry(terminal, visible_rows, offset, maximum),
            maximum,
        ))
    }

    fn click_scrollbar(&mut self, x: i32, y: i32) -> bool {
        let Some((geometry, maximum)) = self.scrollbar_state() else {
            return false;
        };
        if !geometry.track.contains(x, y) {
            return false;
        }
        if maximum == 0 {
            return true;
        }
        if geometry.thumb.contains(x, y) {
            self.scroll_drag = Some(ScrollDrag {
                thumb_grab_offset: y - geometry.thumb.top,
            });
            unsafe { SetCapture(self.window) };
        } else if let Some(position) = self.active_position() {
            let action = if y < geometry.thumb.top {
                "page-up"
            } else {
                "page-down"
            };
            if let Ok(offset) = self.tabs[position].scroll_viewport(action, None) {
                self.commit_viewport_event(position, offset, "scrollbar-track");
            }
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        }
        true
    }

    fn drag_scrollbar(&mut self, _x: i32, y: i32) {
        let Some(drag) = self.scroll_drag else {
            return;
        };
        let Some((geometry, maximum)) = self.scrollbar_state() else {
            self.end_scroll_drag();
            return;
        };
        let offset = scrollback_for_thumb_top(geometry, y - drag.thumb_grab_offset, maximum);
        if let Some(position) = self.active_position() {
            let offset = self.tabs[position].set_scrollback(offset);
            self.commit_viewport_event(position, offset, "scrollbar-drag");
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        }
    }

    fn end_scroll_drag(&mut self) {
        if self.scroll_drag.take().is_some() {
            unsafe { ReleaseCapture() };
        }
    }

    fn mouse_wheel(&mut self, screen_x: i32, screen_y: i32, delta: i32) {
        if self.window_close_pending || self.pending_close.is_some() || self.settings_open {
            return;
        }
        let mut point = POINT {
            x: screen_x,
            y: screen_y,
        };
        unsafe { ScreenToClient(self.window, &mut point) };
        if !self.terminal_rect().contains(point.x, point.y) {
            return;
        }
        self.wheel_remainder += delta;
        let notches = self.wheel_remainder / WHEEL_DELTA;
        self.wheel_remainder %= WHEEL_DELTA;
        if notches == 0 {
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        let before = self.tabs[position].parser.screen().scrollback();
        let rows = notches.unsigned_abs() as usize * WHEEL_ROWS_PER_NOTCH;
        let action = if notches > 0 { "up" } else { "down" };
        let after = self.tabs[position]
            .scroll_viewport(action, Some(rows))
            .unwrap_or(before);
        if after != before {
            self.terminal_selection = None;
            self.commit_viewport_event(position, after, "mouse-wheel");
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        }
    }

    fn commit_viewport_event(&mut self, position: usize, offset: usize, source: &str) {
        let id = self.tabs[position].id;
        self.event_journal.commit(
            "terminal.viewport",
            Some(id),
            serde_json::json!({
                "scrollback_offset": offset,
                "source": source,
            }),
        );
    }

    fn terminal_cell_at(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        if x < SIDEBAR_WIDTH
            || x >= client.right.saturating_sub(TERMINAL_SCROLLBAR_WIDTH)
            || y < 0
            || y >= client
                .bottom
                .saturating_sub(STATUS_BAR_HEIGHT + COMPOSER_HEIGHT)
        {
            return None;
        }

        let device = unsafe { GetDC(self.window) };
        if device.is_null() {
            return None;
        }
        let font = self.terminal_font;
        let previous_font = unsafe { SelectObject(device, font as HGDIOBJ) };
        let mut metrics: TEXTMETRICW = unsafe { mem::zeroed() };
        let mut extent: SIZE = unsafe { mem::zeroed() };
        let sample = ['W' as u16];
        unsafe {
            GetTextMetricsW(device, &mut metrics);
            GetTextExtentPoint32W(device, sample.as_ptr(), 1, &mut extent);
            SelectObject(device, previous_font);
            ReleaseDC(self.window, device);
        }
        let cell_width = extent.cx.max(7);
        let cell_height = (metrics.tmHeight + metrics.tmExternalLeading).max(14);
        let (rows, cols) = self
            .active_position()
            .and_then(|position| self.tabs.get(position))
            .map(|tab| tab.last_size)
            .unwrap_or((1, 1));
        let column =
            ((x - SIDEBAR_WIDTH) / cell_width).clamp(0, i32::from(cols.saturating_sub(1))) as u16;
        let row = (y / cell_height).clamp(0, i32::from(rows.saturating_sub(1))) as u16;
        Some((column, row))
    }

    fn character(&mut self, codepoint: u32) {
        let Some(position) = self.active_position() else {
            return;
        };
        if self.tabs[position].pending_submit_at.is_some() {
            self.feedback = Some("Composer submission pending; terminal input paused".to_owned());
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        if self.terminal_selection.take().is_some() {
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        }
        match codepoint {
            8 => {
                self.tabs[position].send(BACKSPACE_INPUT);
            }
            9 => {
                self.tabs[position].send(b"\t");
            }
            13 => {
                self.tabs[position].send(b"\r");
            }
            27 => {
                self.tabs[position].send(b"\x1b");
            }
            value => {
                if let Some(character) = char::from_u32(value) {
                    let mut buffer = [0_u8; 4];
                    self.tabs[position].send(character.encode_utf8(&mut buffer).as_bytes());
                }
            }
        }
    }

    fn key_down(&mut self, virtual_key: u32) {
        let Some(position) = self.active_position() else {
            return;
        };
        if self.tabs[position].pending_submit_at.is_some() {
            self.feedback = Some("Composer submission pending; terminal input paused".to_owned());
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        let bytes = match virtual_key {
            0x21 => Some(b"\x1b[5~".as_slice()),
            0x22 => Some(b"\x1b[6~".as_slice()),
            0x23 => Some(b"\x1b[F".as_slice()),
            0x24 => Some(b"\x1b[H".as_slice()),
            0x25 => Some(b"\x1b[D".as_slice()),
            0x26 => Some(b"\x1b[A".as_slice()),
            0x27 => Some(b"\x1b[C".as_slice()),
            0x28 => Some(b"\x1b[B".as_slice()),
            0x2e => Some(b"\x1b[3~".as_slice()),
            0x70 => Some(b"\x1bOP".as_slice()),
            0x71 => Some(b"\x1bOQ".as_slice()),
            0x72 => Some(b"\x1bOR".as_slice()),
            0x73 => Some(b"\x1bOS".as_slice()),
            0x74 => Some(b"\x1b[15~".as_slice()),
            0x75 => Some(b"\x1b[17~".as_slice()),
            0x76 => Some(b"\x1b[18~".as_slice()),
            0x77 => Some(b"\x1b[19~".as_slice()),
            0x78 => Some(b"\x1b[20~".as_slice()),
            0x79 => Some(b"\x1b[21~".as_slice()),
            0x7a => Some(b"\x1b[23~".as_slice()),
            0x7b => Some(b"\x1b[24~".as_slice()),
            _ => None,
        };
        if let Some(bytes) = bytes {
            if self.terminal_selection.take().is_some() {
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            }
            self.tabs[position].send(bytes);
        }
    }

    fn save_active_composer(&mut self) {
        if self.note_edit_target.is_some() || self.settings_open {
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        let composer = window_text(self.edit);
        if self.tabs[position].composer != composer {
            self.tabs[position].composer = composer.clone();
            let id = self.tabs[position].id;
            self.event_journal.commit(
                "composer.draft",
                Some(id),
                serde_json::json!({
                    "length": composer.chars().count(),
                }),
            );
        }
    }

    fn load_active_composer(&self) {
        let text = self
            .active_position()
            .and_then(|position| self.tabs.get(position))
            .map(|tab| tab.composer.as_str())
            .unwrap_or("");
        let text = wide(text);
        unsafe { SetWindowTextW(self.edit, text.as_ptr()) };
    }

    fn send_composer(&mut self) {
        if self.note_edit_target.is_some() {
            self.finish_note_edit(true);
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        let text = window_text(self.edit);
        self.tabs[position].composer = text.clone();
        if text.is_empty() || self.tabs[position].exited.is_some() {
            return;
        }
        if self.tabs[position].submit(&text) {
            let id = self.tabs[position].id;
            self.tabs[position].composer.clear();
            self.event_journal.commit(
                "composer.submitted",
                Some(id),
                serde_json::json!({
                    "length": text.chars().count(),
                }),
            );
            self.feedback = Some(format!("Sending to @{}", self.tabs[position].id));
            unsafe { SetWindowTextW(self.edit, wide("").as_ptr()) };
        } else {
            self.feedback = Some(format!(
                "A submission is already pending for @{}",
                self.tabs[position].id
            ));
        }
    }

    fn paint(&mut self) {
        let colors = PaintColors::from(self.palette());
        let mut paint: PAINTSTRUCT = unsafe { mem::zeroed() };
        let paint_device = unsafe { BeginPaint(self.window, &mut paint) };
        if paint_device.is_null() {
            return;
        }
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let width = client.right.max(1);
        let height = client.bottom.max(1);
        let buffer_device = unsafe { CreateCompatibleDC(paint_device) };
        let buffer_bitmap = unsafe { CreateCompatibleBitmap(paint_device, width, height) };
        let buffered = !buffer_device.is_null() && !buffer_bitmap.is_null();
        let previous_bitmap = if buffered {
            unsafe { SelectObject(buffer_device, buffer_bitmap as HGDIOBJ) }
        } else {
            ptr::null_mut()
        };
        let device = if buffered {
            buffer_device
        } else {
            paint_device
        };
        let content_bottom = (client.bottom - STATUS_BAR_HEIGHT).max(0);

        fill(device, &client, colors.terminal);
        fill(
            device,
            &RECT {
                left: 0,
                top: 0,
                right: SIDEBAR_WIDTH,
                bottom: content_bottom,
            },
            colors.sidebar,
        );
        fill(
            device,
            &RECT {
                left: SIDEBAR_WIDTH,
                top: (content_bottom - COMPOSER_HEIGHT).max(0),
                right: client.right,
                bottom: content_bottom,
            },
            colors.composer,
        );
        fill(
            device,
            &RECT {
                left: 0,
                top: content_bottom,
                right: client.right,
                bottom: client.bottom,
            },
            colors.status,
        );

        let ui_font = unsafe { GetStockObject(DEFAULT_GUI_FONT) as HFONT };
        let terminal_font = self.terminal_font;
        unsafe {
            SelectObject(device, ui_font as HGDIOBJ);
            SetBkMode(device, TRANSPARENT as i32);
        }
        let tree_rows = self.tree_rows();
        for (visual_position, row) in tree_rows.iter().enumerate() {
            let Some(tab) = self.tabs.iter().find(|tab| tab.id == row.id) else {
                continue;
            };
            let geometry = tree_row_geometry(visual_position, row.depth, SIDEBAR_WIDTH);
            let top = geometry.row.top;
            let node_x = geometry.node_x;
            let node_y = geometry.node_y;
            let rect = win_rect(geometry.selection);
            if self.active == Some(tab.id) {
                fill(device, &rect, colors.active);
                frame(device, &rect, colors.active_border);
            }
            let has_children = self
                .tabs
                .iter()
                .any(|child| child.parent_id == Some(tab.id));
            for (level, continues) in row.guides.iter().enumerate() {
                if *continues {
                    let x = tree_anchor_x(level);
                    fill(
                        device,
                        &RECT {
                            left: x,
                            top,
                            right: x + 1,
                            bottom: top + TAB_HEIGHT,
                        },
                        colors.tree,
                    );
                }
            }
            if row.depth > 0 {
                let branch_x = tree_anchor_x(row.depth.saturating_sub(1));
                fill(
                    device,
                    &RECT {
                        left: branch_x,
                        top,
                        right: branch_x + 1,
                        bottom: if row.is_last {
                            node_y + 1
                        } else {
                            top + TAB_HEIGHT
                        },
                    },
                    colors.tree,
                );
                fill(
                    device,
                    &RECT {
                        left: branch_x,
                        top: node_y,
                        right: node_x + 1,
                        bottom: node_y + 1,
                    },
                    colors.tree,
                );
            }
            if has_children && !self.collapsed_tabs.contains(&tab.id) {
                fill(
                    device,
                    &RECT {
                        left: node_x,
                        top: node_y,
                        right: node_x + 1,
                        bottom: top + TAB_HEIGHT,
                    },
                    colors.tree,
                );
            }
            if has_children {
                let expander = win_rect(geometry.expander);
                fill(
                    device,
                    &expander,
                    if self.active == Some(tab.id) {
                        colors.active
                    } else {
                        colors.sidebar
                    },
                );
                frame(device, &expander, colors.tree);
                fill(
                    device,
                    &RECT {
                        left: node_x - 3,
                        top: node_y,
                        right: node_x + 4,
                        bottom: node_y + 1,
                    },
                    colors.text,
                );
                if self.collapsed_tabs.contains(&tab.id) {
                    fill(
                        device,
                        &RECT {
                            left: node_x,
                            top: node_y - 3,
                            right: node_x + 1,
                            bottom: node_y + 4,
                        },
                        colors.text,
                    );
                }
            }
            let status_rect = win_rect(geometry.status);
            frame(device, &status_rect, colors.tree);
            fill(
                device,
                &RECT {
                    left: status_rect.left + 2,
                    top: status_rect.top + 2,
                    right: status_rect.right - 2,
                    bottom: status_rect.bottom - 2,
                },
                if tab.exited.is_some() {
                    colors.orange
                } else {
                    colors.green
                },
            );
            let actions_visible = self.active == Some(tab.id);
            let text_right = if actions_visible {
                TAB_ADD_LEFT - 4
            } else {
                SIDEBAR_WIDTH - 10
            };
            draw_text(
                device,
                &tab.title,
                RECT {
                    left: node_x + 24,
                    top: top + 1,
                    right: text_right,
                    bottom: top + 22,
                },
                colors.text,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
            let secondary = if !tab.note.is_empty() {
                tab.note.clone()
            } else {
                match tab.exited {
                    Some(code) => format!("{} · {} · exit {code}", tab.index, tab.command_name),
                    None if !tab.composer.is_empty() => {
                        format!("{} · {} · draft", tab.index, tab.command_name)
                    }
                    None => format!("{} · {}", tab.index, tab.command_name),
                }
            };
            draw_text(
                device,
                &secondary,
                RECT {
                    left: node_x + 24,
                    top: top + 21,
                    right: text_right,
                    bottom: top + TAB_HEIGHT - 1,
                },
                if tab.note.is_empty() {
                    colors.muted
                } else {
                    colors.blue
                },
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
            if actions_visible {
                draw_text(
                    device,
                    "T+",
                    RECT {
                        left: TAB_ADD_LEFT,
                        top,
                        right: TAB_EDIT_LEFT,
                        bottom: top + TAB_HEIGHT - 1,
                    },
                    colors.green,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                );
                draw_text(
                    device,
                    "✎",
                    RECT {
                        left: TAB_EDIT_LEFT,
                        top,
                        right: TAB_CLOSE_LEFT,
                        bottom: top + TAB_HEIGHT - 1,
                    },
                    colors.blue,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                );
                draw_text(
                    device,
                    "×",
                    RECT {
                        left: TAB_CLOSE_LEFT,
                        top,
                        right: SIDEBAR_WIDTH - 4,
                        bottom: top + TAB_HEIGHT - 1,
                    },
                    colors.muted,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                );
            }
        }

        draw_text(
            device,
            "Status",
            RECT {
                left: 14,
                top: content_bottom,
                right: SIDEBAR_WIDTH - 10,
                bottom: client.bottom,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            "metrics · agent context · extensible providers",
            RECT {
                left: SIDEBAR_WIDTH + 10,
                top: content_bottom,
                right: client.right - 10,
                bottom: client.bottom,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );

        if let Some(position) = self.active_position() {
            unsafe { SelectObject(device, terminal_font as HGDIOBJ) };
            self.paint_terminal(device, position, &client);
        } else {
            draw_text(
                device,
                if self.startup_tab_pending {
                    "Starting cmd.exe…"
                } else {
                    "Click New to create a cmd.exe tab"
                },
                RECT {
                    left: SIDEBAR_WIDTH + 24,
                    top: 24,
                    right: client.right - 24,
                    bottom: 64,
                },
                colors.muted,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
        }

        if let Some(position) = self.active_position() {
            let tab = &self.tabs[position];
            draw_text(
                device,
                &if let Some(id) = self.note_edit_target {
                    format!("Edit tab @{id} · line 1 name, remaining lines note")
                } else {
                    format!("Compose input for {}:{}", tab.index, tab.title)
                },
                RECT {
                    left: SIDEBAR_WIDTH + 10,
                    top: content_bottom - COMPOSER_HEIGHT + 4,
                    right: client.right - 270,
                    bottom: content_bottom - COMPOSER_HEIGHT + 28,
                },
                if tab.exited.is_some() {
                    colors.orange
                } else {
                    colors.muted
                },
                DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            draw_text(
                device,
                if self.note_edit_target.is_some() {
                    "Ctrl+Enter save · Esc cancel"
                } else if tab.exited.is_some() {
                    "Process exited · draft preserved"
                } else {
                    "Ctrl+Shift+I focus · Ctrl+Enter send · Esc terminal"
                },
                RECT {
                    left: client.right - 260,
                    top: content_bottom - COMPOSER_HEIGHT + 4,
                    right: client.right - 10,
                    bottom: content_bottom - COMPOSER_HEIGHT + 28,
                },
                colors.muted,
                DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }

        if let Some(position) = self.active_position()
            && let Some(code) = self.tabs[position].exited
        {
            fill(
                device,
                &RECT {
                    left: SIDEBAR_WIDTH,
                    top: content_bottom - COMPOSER_HEIGHT - 30,
                    right: client.right,
                    bottom: content_bottom - COMPOSER_HEIGHT,
                },
                colors.modal,
            );
            draw_text(
                device,
                &format!(
                    "Process exited with code {code}. Output and draft remain until you close this tab."
                ),
                RECT {
                    left: SIDEBAR_WIDTH + 12,
                    top: content_bottom - COMPOSER_HEIGHT - 30,
                    right: client.right - 12,
                    bottom: content_bottom - COMPOSER_HEIGHT,
                },
                colors.orange,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
        }

        if let Some(feedback) = &self.feedback {
            draw_text(
                device,
                feedback,
                RECT {
                    left: SIDEBAR_WIDTH + 10,
                    top: content_bottom - 25,
                    right: client.right - 100,
                    bottom: content_bottom - 3,
                },
                colors.green,
                DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }

        if let Some(error) = &self.last_error {
            draw_text(
                device,
                error,
                RECT {
                    left: SIDEBAR_WIDTH + 10,
                    top: (content_bottom - COMPOSER_HEIGHT - 28).max(0),
                    right: client.right - 10,
                    bottom: (content_bottom - COMPOSER_HEIGHT).max(0),
                },
                colors.red,
                DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
        if let Some(id) = self.pending_close {
            self.paint_close_confirmation(device, &client, id);
        }
        if self.window_close_pending {
            self.paint_window_close_confirmation(device, &client);
        }
        if self.settings_open {
            self.paint_settings(device, &client);
        }
        unsafe {
            if buffered {
                BitBlt(
                    paint_device,
                    0,
                    0,
                    width,
                    height,
                    buffer_device,
                    0,
                    0,
                    SRCCOPY,
                );
                SelectObject(buffer_device, previous_bitmap);
            }
            if !buffer_bitmap.is_null() {
                DeleteObject(buffer_bitmap as HGDIOBJ);
            }
            if !buffer_device.is_null() {
                DeleteDC(buffer_device);
            }
            EndPaint(self.window, &paint);
        };
    }

    fn paint_window_close_confirmation(&self, device: HDC, client: &RECT) {
        let colors = PaintColors::from(self.palette());
        let left = SIDEBAR_WIDTH + (client.right - SIDEBAR_WIDTH - 620) / 2;
        let top = (client.bottom - STATUS_BAR_HEIGHT - 230) / 2;
        let rect = RECT {
            left,
            top,
            right: left + 620,
            bottom: top + 230,
        };
        fill(device, &rect, colors.modal);
        frame(device, &rect, colors.blue);
        draw_text(
            device,
            "Close AgenTerm window?",
            RECT {
                left: left + 24,
                top: top + 18,
                right: left + 596,
                bottom: top + 52,
            },
            colors.text,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            "Keep the server running to preserve live tabs, processes, and terminal history.",
            RECT {
                left: left + 24,
                top: top + 58,
                right: left + 596,
                bottom: top + 88,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        draw_text(
            device,
            "Run agenterm.exe again to reopen this window.",
            RECT {
                left: left + 24,
                top: top + 90,
                right: left + 596,
                bottom: top + 120,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        let keep = RECT {
            left: left + 24,
            top: top + 154,
            right: left + 234,
            bottom: top + 194,
        };
        let stop = RECT {
            left: left + 246,
            top: top + 154,
            right: left + 456,
            bottom: top + 194,
        };
        let cancel = RECT {
            left: left + 468,
            top: top + 154,
            right: left + 596,
            bottom: top + 194,
        };
        fill(device, &keep, colors.active);
        frame(device, &keep, colors.green);
        frame(device, &stop, colors.red);
        frame(device, &cancel, colors.blue);
        draw_text(
            device,
            "Keep Server Running",
            keep,
            colors.green,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            "Stop Server & Exit",
            stop,
            colors.red,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            "Cancel",
            cancel,
            colors.blue,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
    }

    fn paint_close_confirmation(&self, device: HDC, client: &RECT, id: u64) {
        let colors = PaintColors::from(self.palette());
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let left = SIDEBAR_WIDTH + (client.right - SIDEBAR_WIDTH - 460) / 2;
        let top = (client.bottom - STATUS_BAR_HEIGHT - 190) / 2;
        let rect = RECT {
            left,
            top,
            right: left + 460,
            bottom: top + 190,
        };
        fill(device, &rect, colors.modal);
        let border = unsafe { CreateSolidBrush(colors.orange) };
        unsafe {
            FrameRect(device, &rect, border);
            DeleteObject(border as HGDIOBJ);
        }
        draw_text(
            device,
            "Terminate live process?",
            RECT {
                left: left + 26,
                top: top + 18,
                right: left + 430,
                bottom: top + 52,
            },
            colors.text,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            &format!(
                "Close “{}” and terminate PID {} and its child processes?",
                tab.title,
                tab.process_id
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".to_owned())
            ),
            RECT {
                left: left + 26,
                top: top + 58,
                right: left + 430,
                bottom: top + 108,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        fill(
            device,
            &RECT {
                left: left + 238,
                top: top + 125,
                right: left + 432,
                bottom: top + 163,
            },
            colors.red,
        );
        draw_text(
            device,
            "Terminate and close",
            RECT {
                left: left + 250,
                top: top + 125,
                right: left + 424,
                bottom: top + 163,
            },
            colors.text,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            "Cancel",
            RECT {
                left: left + 142,
                top: top + 125,
                right: left + 212,
                bottom: top + 163,
            },
            colors.blue,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
    }

    fn paint_settings(&self, device: HDC, client: &RECT) {
        let colors = PaintColors::from(self.palette());
        let left = SIDEBAR_WIDTH + (client.right - SIDEBAR_WIDTH - 520) / 2;
        let top = (client.bottom - STATUS_BAR_HEIGHT - 320) / 2;
        let rect = RECT {
            left,
            top,
            right: left + 520,
            bottom: top + 320,
        };
        fill(device, &rect, colors.modal);
        let border = unsafe { CreateSolidBrush(colors.blue) };
        unsafe {
            FrameRect(device, &rect, border);
            DeleteObject(border as HGDIOBJ);
        }
        draw_text(
            device,
            "Terminal settings",
            RECT {
                left: left + 28,
                top: top + 18,
                right: left + 490,
                bottom: top + 52,
            },
            colors.text,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            "Font family",
            RECT {
                left: left + 34,
                top: top + 62,
                right: left + 350,
                bottom: top + 88,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            "Size (pt)",
            RECT {
                left: left + 380,
                top: top + 62,
                right: left + 472,
                bottom: top + 88,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        draw_text(
            device,
            &format!(
                "Resolved by Windows: {} · Recommended CJK: Sarasa Fixed SC (SIL OFL 1.1)",
                self.resolved_font_family
            ),
            RECT {
                left: left + 34,
                top: top + 124,
                right: left + 480,
                bottom: top + 146,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        draw_text(
            device,
            "Esc cancels · Changing font size resizes the ConPTY grid",
            RECT {
                left: left + 34,
                top: top + 232,
                right: left + 330,
                bottom: top + 276,
            },
            colors.orange,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        draw_text(
            device,
            "Color theme · selection previews immediately",
            RECT {
                left: left + 34,
                top: top + 148,
                right: left + 448,
                bottom: top + 170,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
        let sample = RECT {
            left: left + 318,
            top: top + 174,
            right: left + 448,
            bottom: top + 206,
        };
        fill(device, &sample, colors.control);
        frame(device, &sample, colors.focus_ring);
        fill(
            device,
            &RECT {
                left: sample.left + 8,
                top: sample.top + 8,
                right: sample.left + 38,
                bottom: sample.bottom - 8,
            },
            colors.control_hover,
        );
        fill(
            device,
            &RECT {
                left: sample.left + 46,
                top: sample.top + 8,
                right: sample.left + 76,
                bottom: sample.bottom - 8,
            },
            colors.control_pressed,
        );
    }

    fn paint_terminal(&mut self, device: HDC, position: usize, client: &RECT) {
        let colors = PaintColors::from(self.palette());
        if unsafe { IsIconic(self.window) } != 0 {
            return;
        }
        let mut metrics: TEXTMETRICW = unsafe { mem::zeroed() };
        unsafe { GetTextMetricsW(device, &mut metrics) };
        let mut extent: SIZE = unsafe { mem::zeroed() };
        let sample = ['W' as u16];
        unsafe { GetTextExtentPoint32W(device, sample.as_ptr(), 1, &mut extent) };
        let cell_width = extent.cx.max(7);
        let cell_height = (metrics.tmHeight + metrics.tmExternalLeading).max(14);
        let width = (client.right - SIDEBAR_WIDTH - TERMINAL_SCROLLBAR_WIDTH).max(cell_width * 10);
        let height = (client.bottom - STATUS_BAR_HEIGHT - COMPOSER_HEIGHT).max(cell_height * 2);
        let cols = (width / cell_width).clamp(10, u16::MAX as i32) as u16;
        let rows = (height / cell_height).clamp(2, u16::MAX as i32) as u16;
        self.tabs[position].resize(rows, cols);
        let (scrollback_offset, max_scrollback) = self.tabs[position].scrollback_bounds();
        let scrollbar = terminal_scrollbar_geometry(
            PixelRect {
                left: SIDEBAR_WIDTH,
                top: 0,
                right: client.right,
                bottom: height,
            },
            usize::from(rows),
            scrollback_offset,
            max_scrollback,
        );
        let selection = self
            .terminal_selection
            .filter(|selection| selection.tab_id == self.tabs[position].id);

        let screen = self.tabs[position].parser.screen();
        for row in 0..rows {
            let mut backgrounds: Vec<(u16, u16, COLORREF)> = Vec::new();
            for col in 0..cols {
                let cell = screen.cell(row, col);
                let mut foreground = cell
                    .map(|cell| terminal_color(cell.fgcolor(), false, self.palette()))
                    .unwrap_or(colors.terminal_text);
                let mut background = cell
                    .map(|cell| terminal_color(cell.bgcolor(), true, self.palette()))
                    .unwrap_or(colors.terminal);
                if cell.is_some_and(|value| value.inverse()) {
                    mem::swap(&mut foreground, &mut background);
                }
                if selection.is_some_and(|selection| selection.contains(row, col)) {
                    background = colors.selection_background;
                }
                if let Some((_, end, run_background)) = backgrounds.last_mut()
                    && *run_background == background
                {
                    *end = col + 1;
                } else {
                    backgrounds.push((col, col + 1, background));
                }
            }
            for (start_col, end_col, background) in backgrounds {
                fill(
                    device,
                    &RECT {
                        left: SIDEBAR_WIDTH + start_col as i32 * cell_width,
                        top: row as i32 * cell_height,
                        right: SIDEBAR_WIDTH + end_col as i32 * cell_width,
                        bottom: (row as i32 + 1) * cell_height,
                    },
                    background,
                );
            }
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() || cell.contents().is_empty() {
                    continue;
                }
                let foreground = if selection.is_some_and(|selection| selection.contains(row, col))
                {
                    colors.selection_foreground
                } else if cell.inverse() {
                    terminal_color(cell.bgcolor(), true, self.palette())
                } else {
                    terminal_color(cell.fgcolor(), false, self.palette())
                };
                let encoded: Vec<u16> = cell.contents().encode_utf16().collect();
                let left = SIDEBAR_WIDTH + col as i32 * cell_width;
                let top = row as i32 * cell_height;
                let cell_span = if col + 1 < cols
                    && screen
                        .cell(row, col + 1)
                        .is_some_and(|next| next.is_wide_continuation())
                {
                    2
                } else {
                    1
                };
                let rect = RECT {
                    left,
                    top,
                    right: left + cell_span * cell_width,
                    bottom: top + cell_height,
                };
                unsafe {
                    SetTextColor(device, foreground);
                    SetBkMode(device, TRANSPARENT as i32);
                    ExtTextOutW(
                        device,
                        left,
                        top,
                        0,
                        &rect,
                        encoded.as_ptr(),
                        encoded.len() as u32,
                        ptr::null(),
                    );
                }
            }
        }

        if scrollback_offset == 0 && self.tabs[position].exited.is_none() && !screen.hide_cursor() {
            let (row, col) = screen.cursor_position();
            let cursor = RECT {
                left: SIDEBAR_WIDTH + col as i32 * cell_width,
                top: row as i32 * cell_height,
                right: SIDEBAR_WIDTH + (col as i32 + 1) * cell_width,
                bottom: (row as i32 + 1) * cell_height,
            };
            let brush = unsafe { CreateSolidBrush(rgb(235, 235, 235)) };
            unsafe {
                FrameRect(device, &cursor, brush);
                DeleteObject(brush as HGDIOBJ);
            }
        }

        let track = win_rect(scrollbar.track);
        fill(device, &track, colors.scrollbar_track);
        fill(
            device,
            &RECT {
                left: track.left,
                top: track.top,
                right: track.left + 1,
                bottom: track.bottom,
            },
            colors.tree,
        );
        let thumb = win_rect(scrollbar.thumb);
        fill(
            device,
            &thumb,
            if max_scrollback == 0 {
                colors.scrollbar_thumb_active
            } else {
                colors.scrollbar_thumb
            },
        );
        if max_scrollback > 0 {
            frame(device, &thumb, colors.active_border);
        }

        if let Some(code) = self.tabs[position].exited {
            draw_text(
                device,
                &format!("Process exited with code {code}. This tab remains until you close it."),
                RECT {
                    left: SIDEBAR_WIDTH + 8,
                    top: height - cell_height,
                    right: client.right - TERMINAL_SCROLLBAR_WIDTH - 8,
                    bottom: height,
                },
                colors.orange,
                DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
    }

    fn focus_surface(&self) -> &'static str {
        let focused = unsafe { GetFocus() };
        if focused == self.settings_font
            || focused == self.settings_size
            || focused == self.settings_dark
            || focused == self.settings_light
            || focused == self.settings_cancel
            || focused == self.settings_apply
        {
            "settings"
        } else if focused == self.edit && self.note_edit_target.is_some() {
            "note-editor"
        } else if focused == self.edit {
            "composer"
        } else {
            "terminal"
        }
    }

    fn ui_snapshot(&mut self) -> String {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let scrollbar = self.scrollbar_state().map(|(geometry, maximum)| {
            serde_json::json!({
                "visible": true,
                "track": pixel_rect_json(geometry.track),
                "thumb": pixel_rect_json(geometry.thumb),
                "max_offset": maximum,
            })
        });
        let active_draft = window_text(self.edit);
        let visible_rows = self.tree_rows();
        let all_rows = self.all_tree_rows();
        let tabs = all_rows
            .iter()
            .filter_map(|row| {
                let tab = self.tabs.iter().find(|tab| tab.id == row.id)?;
                let visible_position = visible_rows.iter().position(|visible| visible.id == row.id);
                let geometry = visible_position
                    .map(|position| tree_row_geometry(position, row.depth, SIDEBAR_WIDTH));
                let draft = if self.active == Some(tab.id) {
                    !active_draft.is_empty()
                } else {
                    !tab.composer.is_empty()
                };
                Some(serde_json::json!({
                    "id": format!("@{}", tab.id),
                    "index": tab.index,
                    "parent_id": tab.parent_id.map(|id| format!("@{id}")),
                    "depth": row.depth,
                    "has_children": self.tabs.iter().any(|child| child.parent_id == Some(tab.id)),
                    "collapsed": self.collapsed_tabs.contains(&tab.id),
                    "visible": visible_position.is_some(),
                    "name": tab.title,
                    "terminal_title": tab.parser.callbacks().title,
                    "note": tab.note,
                    "active": self.active == Some(tab.id),
                    "state": if tab.error.is_some() {
                        "error"
                    } else if tab.exited.is_some() {
                        "dead"
                    } else {
                        "running"
                    },
                    "exit_code": tab.exited,
                    "environment_names": tab.environment_names,
                    "scrollback_offset": tab.parser.screen().scrollback(),
                    "selection": self.terminal_selection
                        .filter(|selection| selection.tab_id == tab.id)
                        .map(|selection| {
                            let (start, end) = selection.bounds();
                            serde_json::json!({
                                "start": {"row": start.row, "col": start.col},
                                "end": {"row": end.row, "col": end.col},
                                "dragging": selection.dragging,
                            })
                        }),
                    "draft": draft,
                    "bounds": geometry.map(|geometry| pixel_rect_json(geometry.row)),
                    "render": geometry.map(|geometry| serde_json::json!({
                        "row": pixel_rect_json(geometry.row),
                        "selection": pixel_rect_json(geometry.selection),
                        "node": {
                            "x": geometry.node_x,
                            "y": geometry.node_y,
                        },
                        "expander": pixel_rect_json(geometry.expander),
                        "status": pixel_rect_json(geometry.status),
                        "disclosure_hit": pixel_rect_json(geometry.disclosure_hit),
                    })),
                    "actions": visible_position
                        .filter(|_| self.active == Some(tab.id))
                        .map(|position| {
                            let geometry =
                                tree_row_geometry(position, row.depth, SIDEBAR_WIDTH);
                            serde_json::json!({
                        "new_child": {
                            "x": TAB_ADD_LEFT,
                            "y": geometry.row.top,
                            "width": TAB_EDIT_LEFT - TAB_ADD_LEFT,
                            "height": geometry.row.height(),
                        },
                        "edit": {
                            "x": TAB_EDIT_LEFT,
                            "y": geometry.row.top,
                            "width": TAB_CLOSE_LEFT - TAB_EDIT_LEFT,
                            "height": geometry.row.height(),
                        },
                        "close": {
                            "x": TAB_CLOSE_LEFT,
                            "y": geometry.row.top,
                            "width": SIDEBAR_WIDTH - TAB_CLOSE_LEFT,
                            "height": geometry.row.height(),
                        }
                    })
                        })
                }))
            })
            .collect::<Vec<_>>();
        let (rows, cols) = self
            .active_position()
            .and_then(|position| self.tabs.get(position))
            .map(|tab| tab.last_size)
            .unwrap_or((0, 0));
        let (system_copy_enabled, system_paste_enabled) = self.system_clipboard_menu_state();
        let event_position = self.event_journal.position();
        serde_json::to_string_pretty(&serde_json::json!({
            "protocol_version": 1,
            "event_position": event_position,
            "startup": {
                "initial_tab_pending": self.startup_tab_pending,
                "tabs_remaining": self.startup_tabs_remaining,
                "workspace_file_exists": workspace_path().exists(),
            },
            "workspace": {
                "persistent": true,
                "path": workspace_path(),
                "restore_behavior": "restart-processes",
            },
            "window": {
                "title": window_text(self.window),
                "client_width": client.right,
                "client_height": client.bottom,
                "visible": unsafe { IsWindowVisible(self.window) } != 0,
                "detached": self.window_detached,
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
                    "x": 0, "y": 0, "width": SIDEBAR_WIDTH,
                    "height": (client.bottom - STATUS_BAR_HEIGHT).max(0)
                },
                "terminal": {
                    "x": SIDEBAR_WIDTH, "y": 0,
                    "width": (client.right - SIDEBAR_WIDTH).max(0),
                    "viewport_width": (
                        client.right - SIDEBAR_WIDTH - TERMINAL_SCROLLBAR_WIDTH
                    ).max(0),
                    "height": (client.bottom - STATUS_BAR_HEIGHT - COMPOSER_HEIGHT).max(0),
                    "rows": rows, "cols": cols,
                    "scrollbar": scrollbar,
                },
                "composer": {
                    "visible": !self.window_close_pending
                        && self.pending_close.is_none()
                        && !self.settings_open,
                    "height": COMPOSER_HEIGHT,
                },
                "status_bar": {
                    "x": 0,
                    "y": (client.bottom - STATUS_BAR_HEIGHT).max(0),
                    "width": client.right,
                    "height": STATUS_BAR_HEIGHT,
                    "provider": "placeholder",
                }
            },
            "focus": {
                "surface": self.focus_surface(),
                "window_id": self.active.map(|id| format!("@{id}")),
            },
            "system_menu": {
                "copy": {
                    "id": SYSTEM_MENU_COPY_ID,
                    "label": "Copy",
                    "enabled": system_copy_enabled,
                },
                "paste": {
                    "id": SYSTEM_MENU_PASTE_ID,
                    "label": "Paste",
                    "enabled": system_paste_enabled,
                },
            },
            "tabs": tabs,
            "modal": if self.window_close_pending {
                Some(serde_json::json!({
                    "kind": "confirm-window-close",
                    "default_action": "keep-server-running",
                    "actions": [
                        "keep-server-running",
                        "stop-server-and-exit",
                        "cancel"
                    ],
                }))
            } else if self.settings_open {
                Some(serde_json::json!({"kind": "settings"}))
            } else {
                self.pending_close.map(|id| serde_json::json!({
                    "kind": "confirm-close-live",
                    "window_id": format!("@{id}"),
                }))
            },
            "settings": {
                "terminal_font_family": self.config.terminal_font_family,
                "terminal_font_size": self.config.terminal_font_size,
                "resolved_font_family": self.resolved_font_family,
                "color_theme": self.config.color_theme.as_str(),
                "theme_draft": self.settings_open.then(|| self.settings_theme_draft.as_str()),
                "theme_options": ThemeId::ALL.map(|theme| serde_json::json!({
                    "id": theme.as_str(),
                    "label": theme.label(),
                })),
            },
            "locale": {
                "id": UI_LOCALE,
                "controls": {
                    "send": LABEL_SEND,
                    "settings": LABEL_SETTINGS,
                    "new": LABEL_NEW,
                    "apply": LABEL_APPLY,
                    "save": LABEL_SAVE,
                }
            },
            "feedback": {
                "message": self.feedback.as_deref(),
                "error": self.last_error.as_deref(),
            }
        }))
        .unwrap_or_else(|error| format!(r#"{{"error":"{error}"}}"#))
    }

    fn execute_command(&mut self, args: &[String]) -> IpcResponse {
        let Some(command) = args.first().map(String::as_str) else {
            return IpcResponse::failure("no command specified");
        };
        match command {
            "__focus" | "start-server" => {
                self.reattach_window(if command == "__focus" {
                    "launcher"
                } else {
                    "start-server"
                });
                IpcResponse::success("")
            }
            "attach" | "attach-session" => {
                if let Some(requested) = option_value(args, "-t")
                    && requested != self.session_name
                {
                    return IpcResponse::failure(format!("can't find session: {requested}"));
                }
                self.reattach_window("attach-session");
                IpcResponse::success("")
            }
            "new" | "new-session" => {
                if let Some(name) = option_value(args, "-s") {
                    self.session_name = name.to_owned();
                }
                if self.tabs.is_empty() {
                    let (_, _, child_command) = parse_new_command(args);
                    let tab_environment = match parse_tab_environment(args) {
                        Ok(environment) => environment,
                        Err(error) => return IpcResponse::failure(error),
                    };
                    if let Err(error) = self.create_tab_with_parent(
                        None,
                        child_command,
                        tab_environment,
                        true,
                        None,
                    ) {
                        return IpcResponse::failure(format!("{error:#}"));
                    }
                }
                self.reattach_window("new-session");
                IpcResponse::success("")
            }
            "neww" | "new-window" => {
                let (title, detached, child_command) = parse_new_command(args);
                let tab_environment = match parse_tab_environment(args) {
                    Ok(environment) => environment,
                    Err(error) => return IpcResponse::failure(error),
                };
                let parent_id = match option_value(args, "--parent") {
                    Some(target) => match self.parent_id_from_target(target) {
                        Ok(parent_id) => parent_id,
                        Err(error) => return IpcResponse::failure(format!("{error:#}")),
                    },
                    None => None,
                };
                match self.create_tab_with_parent(
                    title,
                    child_command,
                    tab_environment,
                    !detached,
                    parent_id,
                ) {
                    Ok(index) => {
                        let format = option_value(args, "-F").unwrap_or("#{window_index}");
                        let tab = self
                            .tabs
                            .iter()
                            .find(|tab| tab.index == index)
                            .expect("newly created tab must remain present");
                        IpcResponse::success(render_format(
                            format,
                            tab,
                            &self.session_name,
                            self.active == Some(tab.id),
                        ))
                    }
                    Err(error) => IpcResponse::failure(format!("{error:#}")),
                }
            }
            "new-agent" => {
                let (title, detached, agent_arguments) = parse_new_command(args);
                let tab_environment = match parse_tab_environment(args) {
                    Ok(environment) => environment,
                    Err(error) => return IpcResponse::failure(error),
                };
                let parent_id = match option_value(args, "--parent") {
                    Some(target) => match self.parent_id_from_target(target) {
                        Ok(parent_id) => parent_id,
                        Err(error) => return IpcResponse::failure(format!("{error:#}")),
                    },
                    None => None,
                };
                let mut child_command = if let Some(program) = option_value(args, "--program") {
                    vec![program.to_owned()]
                } else {
                    vec![
                        env::var("COMSPEC")
                            .unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_owned()),
                        "/d".to_owned(),
                        "/c".to_owned(),
                        "codex".to_owned(),
                    ]
                };
                if has_option(args, "--yolo") {
                    child_command.push("--dangerously-bypass-approvals-and-sandbox".to_owned());
                }
                child_command.extend(agent_arguments);
                match self.create_tab_with_parent(
                    title.or_else(|| Some("Codex".to_owned())),
                    child_command,
                    tab_environment,
                    !detached,
                    parent_id,
                ) {
                    Ok(index) => {
                        let format = option_value(args, "-F").unwrap_or("#{window_index}");
                        let tab = self
                            .tabs
                            .iter()
                            .find(|tab| tab.index == index)
                            .expect("newly created agent tab must remain present");
                        IpcResponse::success(render_format(
                            format,
                            tab,
                            &self.session_name,
                            self.active == Some(tab.id),
                        ))
                    }
                    Err(error) => {
                        IpcResponse::failure(format!("failed to start Codex agent tab: {error:#}"))
                    }
                }
            }
            "ls" | "list-sessions" => IpcResponse::success(format!(
                "{}: {} windows (created {}) (attached)",
                self.session_name,
                self.tabs.len(),
                self.started_at
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or_default()
            )),
            "has" | "has-session" => {
                let requested = option_value(args, "-t");
                if requested.is_none_or(|name| name == self.session_name) {
                    IpcResponse::success("")
                } else {
                    IpcResponse::failure(format!(
                        "can't find session: {}",
                        requested.unwrap_or_default()
                    ))
                }
            }
            "lsw" | "list-windows" => {
                let format = option_value(args, "-F").unwrap_or("#I:#W#{?window_active,*,}");
                IpcResponse::success(
                    self.tabs
                        .iter()
                        .map(|tab| {
                            render_format(
                                format,
                                tab,
                                &self.session_name,
                                self.active == Some(tab.id),
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
            "lsp" | "list-panes" => {
                let format = option_value(args, "-F").unwrap_or(
                    "#{pane_id}: [#{pane_width}x#{pane_height}] #{pane_current_command}",
                );
                let all = args.iter().any(|arg| arg == "-a");
                let tabs: Vec<&TerminalTab> = if all {
                    self.tabs.iter().collect()
                } else {
                    self.target_position(option_value(args, "-t"))
                        .and_then(|position| self.tabs.get(position))
                        .into_iter()
                        .collect()
                };
                IpcResponse::success(
                    tabs.into_iter()
                        .map(|tab| {
                            render_format(
                                format,
                                tab,
                                &self.session_name,
                                self.active == Some(tab.id),
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
            "selectw" | "select-window" => {
                let target = if args.iter().any(|arg| arg == "-n") {
                    self.adjacent_position(1)
                } else if args.iter().any(|arg| arg == "-p") {
                    self.adjacent_position(-1)
                } else {
                    self.target_position(option_value(args, "-t"))
                };
                let Some(position) = target else {
                    return IpcResponse::failure("can't find window");
                };
                self.save_active_composer();
                let id = self.tabs[position].id;
                self.active = Some(id);
                self.load_active_composer();
                self.event_journal
                    .commit("tab.selected", Some(id), serde_json::json!({}));
                self.reattach_window("select-window");
                IpcResponse::success("")
            }
            "next" | "next-window" => self.select_adjacent(1),
            "prev" | "previous-window" => self.select_adjacent(-1),
            "renamew" | "rename-window" => {
                let Some(name) = last_positional(args, &["-t"]) else {
                    return IpcResponse::failure("usage: rename-window [-t target] new-name");
                };
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find window");
                };
                let id = self.tabs[position].id;
                let previous_name = self.tabs[position].title.clone();
                self.tabs[position].title = name.to_owned();
                self.event_journal.commit(
                    "tab.renamed",
                    Some(id),
                    serde_json::json!({
                        "previous_name": previous_name,
                        "name": name,
                    }),
                );
                IpcResponse::success("")
            }
            "set-tab-note" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find tab");
                };
                let note = positional_values(args, &["-t"], &[]).join(" ");
                let id = self.tabs[position].id;
                let previous_note = mem::replace(&mut self.tabs[position].note, note.clone());
                self.event_journal.commit(
                    "tab.note",
                    Some(id),
                    serde_json::json!({
                        "previous_note": previous_note,
                        "note": note,
                    }),
                );
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                IpcResponse::success("")
            }
            "show-tab-note" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find tab");
                };
                IpcResponse::success(self.tabs[position].note.clone())
            }
            "set-tab-parent" => {
                let Some(child_position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find child tab");
                };
                let Some(parent_target) = option_value(args, "--parent") else {
                    return IpcResponse::failure(
                        "usage: set-tab-parent -t child --parent parent|root",
                    );
                };
                let parent_id = match self.parent_id_from_target(parent_target) {
                    Ok(parent_id) => parent_id,
                    Err(error) => return IpcResponse::failure(format!("{error:#}")),
                };
                let child_id = self.tabs[child_position].id;
                match self.set_tab_parent(child_id, parent_id) {
                    Ok(()) => {
                        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                        IpcResponse::success("")
                    }
                    Err(error) => IpcResponse::failure(format!("{error:#}")),
                }
            }
            "show-tab-parent" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find tab");
                };
                IpcResponse::success(
                    self.tabs[position]
                        .parent_id
                        .map(|id| format!("@{id}"))
                        .unwrap_or_else(|| "root".to_owned()),
                )
            }
            "list-tab-tree" => {
                let format = option_value(args, "-F").unwrap_or("#{window_id}:#{window_name}");
                let rows = self.all_tree_rows();
                IpcResponse::success(
                    rows.iter()
                        .filter_map(|row| {
                            let tab = self.tabs.iter().find(|tab| tab.id == row.id)?;
                            let branch = if row.depth == 0 {
                                String::new()
                            } else {
                                let mut branch = row
                                    .guides
                                    .iter()
                                    .map(|continues| if *continues { "│ " } else { "  " })
                                    .collect::<String>();
                                branch.push_str(if row.is_last { "└─ " } else { "├─ " });
                                branch
                            };
                            let rendered = render_format(
                                format,
                                tab,
                                &self.session_name,
                                self.active == Some(tab.id),
                            )
                            .replace("#{tab_depth}", &row.depth.to_string())
                            .replace(
                                "#{tab_has_children}",
                                if self
                                    .tabs
                                    .iter()
                                    .any(|child| child.parent_id == Some(tab.id))
                                {
                                    "1"
                                } else {
                                    "0"
                                },
                            );
                            Some(format!("{branch}{rendered}"))
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
            "get-settings" => IpcResponse::success(
                serde_json::to_string_pretty(&serde_json::json!({
                    "terminal_font_family": self.config.terminal_font_family,
                    "terminal_font_size": self.config.terminal_font_size,
                    "resolved_font_family": self.resolved_font_family,
                    "config_path": config_path(),
                    "recommended_cjk_font": "Sarasa Fixed SC",
                    "recommended_font_license": "SIL Open Font License 1.1",
                }))
                .unwrap_or_default(),
            ),
            "set-setting" => {
                let Some(key) = args.get(1).map(String::as_str) else {
                    return IpcResponse::failure("set-setting requires a key and value");
                };
                let value = args.get(2..).unwrap_or_default().join(" ");
                match key {
                    "terminal.font-family" if !value.trim().is_empty() => {
                        self.config.terminal_font_family = value;
                    }
                    "terminal.font-size" => {
                        let Ok(size) = value.parse::<u16>() else {
                            return IpcResponse::failure("font size must be a number from 8 to 36");
                        };
                        if !(8..=36).contains(&size) {
                            return IpcResponse::failure("font size must be from 8 to 36");
                        }
                        self.config.terminal_font_size = size;
                    }
                    "terminal.font-family" => {
                        return IpcResponse::failure("font family cannot be empty");
                    }
                    other => return IpcResponse::failure(format!("unknown setting: {other}")),
                }
                self.rebuild_terminal_font();
                if let Err(error) = save_config(&self.config) {
                    return IpcResponse::failure(format!("{error:#}"));
                }
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                IpcResponse::success(self.ui_snapshot())
            }
            "rename" | "rename-session" => {
                let Some(name) = last_positional(args, &["-t"]) else {
                    return IpcResponse::failure("usage: rename-session new-name");
                };
                self.session_name = name.to_owned();
                IpcResponse::success("")
            }
            "killw" | "kill-window" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find window");
                };
                let id = self.tabs[position].id;
                self.close_tab(id);
                IpcResponse::success("")
            }
            "send" | "send-keys" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find pane");
                };
                if self.tabs[position].pending_submit_at.is_some() {
                    return IpcResponse::failure(
                        "composer submission is pending; wait with \
                         `wait-pane --submit-complete` before sending keys",
                    );
                }
                let literal = args.iter().any(|arg| arg == "-l");
                for key in positional_values(args, &["-t"], &["-l", "-R", "-X"]) {
                    if literal {
                        self.tabs[position].send(key.as_bytes());
                    } else if let Some(bytes) = tmux_key_bytes(key) {
                        self.tabs[position].send(&bytes);
                    } else {
                        self.tabs[position].send(key.as_bytes());
                    }
                }
                IpcResponse::success("")
            }
            "scroll-pane" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find pane");
                };
                let values = positional_values(args, &["-t"], &[]);
                let Some(action) = values.first().copied() else {
                    return IpcResponse::failure(
                        "usage: scroll-pane [-t target] \
                         up|down|page-up|page-down|top|bottom [rows]",
                    );
                };
                if values.len() > 2 {
                    return IpcResponse::failure("scroll-pane accepts at most one row count");
                }
                let count = match values.get(1) {
                    Some(value) => match value.parse::<usize>() {
                        Ok(count) if count > 0 => Some(count),
                        _ => {
                            return IpcResponse::failure(
                                "scroll-pane row count must be a positive integer",
                            );
                        }
                    },
                    None => None,
                };
                match self.tabs[position].scroll_viewport(action, count) {
                    Ok(offset) => {
                        self.commit_viewport_event(position, offset, "control");
                        if self
                            .terminal_selection
                            .is_some_and(|selection| selection.tab_id == self.tabs[position].id)
                        {
                            self.terminal_selection = None;
                        }
                        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                        IpcResponse::success(offset.to_string())
                    }
                    Err(error) => IpcResponse::failure(format!("{error:#}")),
                }
            }
            "capturep" | "capture-pane" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find pane");
                };
                if args.iter().any(|argument| argument == "--raw-escaped") {
                    return IpcResponse::success(
                        String::from_utf8_lossy(&self.tabs[position].raw_output)
                            .escape_debug()
                            .to_string(),
                    );
                }
                IpcResponse::success(self.tabs[position].parser.screen().contents())
            }
            "dump-cells" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find pane");
                };
                let requested_row =
                    option_value(args, "-r").and_then(|value| value.parse::<u16>().ok());
                let tab = &self.tabs[position];
                let screen = tab.parser.screen();
                let mut cells = Vec::new();
                for row in 0..tab.last_size.0 {
                    if requested_row.is_some_and(|requested| requested != row) {
                        continue;
                    }
                    for col in 0..tab.last_size.1 {
                        let Some(cell) = screen.cell(row, col) else {
                            continue;
                        };
                        let foreground = format!("{:?}", cell.fgcolor());
                        let background = format!("{:?}", cell.bgcolor());
                        if cell.contents().is_empty()
                            && foreground == "Default"
                            && background == "Default"
                            && !cell.inverse()
                        {
                            continue;
                        }
                        cells.push(serde_json::json!({
                            "row": row,
                            "col": col,
                            "text": cell.contents(),
                            "fg": foreground,
                            "bg": background,
                            "inverse": cell.inverse(),
                            "wide_continuation": cell.is_wide_continuation(),
                        }));
                    }
                }
                match serde_json::to_string_pretty(&serde_json::json!({
                    "window_id": format!("@{}", tab.id),
                    "rows": tab.last_size.0,
                    "cols": tab.last_size.1,
                    "cells": cells,
                })) {
                    Ok(json) => IpcResponse::success(json),
                    Err(error) => IpcResponse::failure(error.to_string()),
                }
            }
            "send-mouse" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find pane");
                };
                let Some(x) = option_value(args, "-x").and_then(|value| value.parse::<u16>().ok())
                else {
                    return IpcResponse::failure("send-mouse requires numeric -x");
                };
                let Some(y) = option_value(args, "-y").and_then(|value| value.parse::<u16>().ok())
                else {
                    return IpcResponse::failure("send-mouse requires numeric -y");
                };
                let button_name = option_value(args, "--button").unwrap_or("left");
                let button = match button_name {
                    "left" => 0,
                    "middle" => 1,
                    "right" => 2,
                    "wheel-up" => 64,
                    "wheel-down" => 65,
                    other => return IpcResponse::failure(format!("unknown mouse button: {other}")),
                };
                let suffix = if option_value(args, "--action") == Some("release") {
                    'm'
                } else {
                    'M'
                };
                let protocol = option_value(args, "--protocol").unwrap_or("auto");
                if protocol == "auto" && self.tabs[position].send_rmux_status_click(x, y) {
                    return IpcResponse::success("");
                }
                if protocol != "sgr" && button_name == "left" {
                    return match self.tabs[position].send_native_mouse_click(x, y) {
                        Ok(()) => IpcResponse::success(""),
                        Err(error) if protocol == "native" => {
                            IpcResponse::failure(format!("{error:#}"))
                        }
                        Err(_) => {
                            self.tabs[position].send(
                                format!("\x1b[<{button};{};{}{suffix}", x + 1, y + 1).as_bytes(),
                            );
                            IpcResponse::success("")
                        }
                    };
                }
                if protocol != "auto" && protocol != "sgr" && protocol != "native" {
                    return IpcResponse::failure(format!("unknown mouse protocol: {protocol}"));
                }
                self.tabs[position]
                    .send(format!("\x1b[<{button};{};{}{suffix}", x + 1, y + 1).as_bytes());
                IpcResponse::success("")
            }
            "active-window" | "active-tab" => {
                let Some(position) = self.active_position() else {
                    return IpcResponse::failure("no active window");
                };
                let format = option_value(args, "-F").unwrap_or("#{window_id}:#{window_name}");
                IpcResponse::success(render_format(
                    format,
                    &self.tabs[position],
                    &self.session_name,
                    true,
                ))
            }
            "ui-snapshot" => IpcResponse::success(self.ui_snapshot()),
            "read-events" => {
                let Some(epoch) = option_value(args, "--epoch") else {
                    return IpcResponse::failure("read-events requires --epoch");
                };
                let Some(after) =
                    option_value(args, "--after").and_then(|value| value.parse::<u64>().ok())
                else {
                    return IpcResponse::failure("read-events requires a numeric --after sequence");
                };
                let limit = match option_value(args, "--limit") {
                    Some(value) => match value.parse::<usize>() {
                        Ok(limit) if (1..=1_024).contains(&limit) => limit,
                        _ => {
                            return IpcResponse::failure(
                                "read-events --limit must be from 1 to 1024",
                            );
                        }
                    },
                    None => 256,
                };
                match self.event_journal.read_after(epoch, after, limit) {
                    Ok(events) => IpcResponse::success(
                        serde_json::to_string_pretty(&serde_json::json!({
                            "position": self.event_journal.position(),
                            "events": events,
                        }))
                        .unwrap_or_default(),
                    ),
                    Err(error) => IpcResponse::failure(error.to_json().to_string()),
                }
            }
            "protocol-info" => IpcResponse::success(protocol_info_json()),
            "focus" => {
                let surface = args.get(1).map(String::as_str).unwrap_or("terminal");
                if let Some(position) = self.target_position(option_value(args, "-t")) {
                    self.save_active_composer();
                    let id = self.tabs[position].id;
                    self.active = Some(id);
                    self.load_active_composer();
                    self.event_journal
                        .commit("tab.selected", Some(id), serde_json::json!({}));
                }
                match surface {
                    "terminal" => unsafe { SetFocus(self.window) },
                    "composer" => unsafe { SetFocus(self.edit) },
                    "sidebar" => unsafe { SetFocus(self.window) },
                    other => {
                        return IpcResponse::failure(format!("unknown focus surface: {other}"));
                    }
                };
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                IpcResponse::success(self.ui_snapshot())
            }
            "ui-action" => {
                let Some(action) = args.get(1).map(String::as_str) else {
                    return IpcResponse::failure("ui-action requires an action");
                };
                let response = match action {
                    "new-tab" => match self.create_tab(None, Vec::new(), true) {
                        Ok(_) => None,
                        Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                    },
                    "new-child" => {
                        let parent_position = self
                            .target_position(option_value(args, "-t"))
                            .or_else(|| self.active_position());
                        let Some(parent_position) = parent_position else {
                            return IpcResponse::failure("can't find parent tab");
                        };
                        let parent_id = self.tabs[parent_position].id;
                        match self.create_tab_with_parent(
                            Some("New child".to_owned()),
                            Vec::new(),
                            Vec::new(),
                            true,
                            Some(parent_id),
                        ) {
                            Ok(index) => {
                                if let Some(id) = self
                                    .tabs
                                    .iter()
                                    .find(|tab| tab.index == index)
                                    .map(|tab| tab.id)
                                {
                                    self.open_tab_editor(id);
                                }
                                None
                            }
                            Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                        }
                    }
                    "edit-tab" => {
                        let Some(position) = self.target_position(option_value(args, "-t")) else {
                            return IpcResponse::failure("can't find tab");
                        };
                        self.open_tab_editor(self.tabs[position].id);
                        None
                    }
                    "toggle-tree" => {
                        let Some(position) = self.target_position(option_value(args, "-t")) else {
                            return IpcResponse::failure("can't find tab");
                        };
                        let id = self.tabs[position].id;
                        if !self.tabs.iter().any(|tab| tab.parent_id == Some(id)) {
                            return IpcResponse::failure("tab has no child nodes");
                        }
                        if !self.collapsed_tabs.remove(&id) {
                            self.collapsed_tabs.insert(id);
                        }
                        None
                    }
                    "select-tab" => {
                        let Some(position) = self.target_position(option_value(args, "-t")) else {
                            return IpcResponse::failure("can't find tab");
                        };
                        self.save_active_composer();
                        let id = self.tabs[position].id;
                        self.active = Some(id);
                        self.load_active_composer();
                        self.event_journal
                            .commit("tab.selected", Some(id), serde_json::json!({}));
                        unsafe { SetFocus(self.window) };
                        None
                    }
                    "close-tab" => {
                        let Some(position) = self.target_position(option_value(args, "-t")) else {
                            return IpcResponse::failure("can't find tab");
                        };
                        self.request_close_tab(self.tabs[position].id);
                        None
                    }
                    "confirm" => {
                        if self.window_close_pending {
                            self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                        } else if self.pending_close.is_some() {
                            self.finish_close_confirmation(true);
                        } else {
                            return IpcResponse::failure("no confirmation is pending");
                        }
                        None
                    }
                    "cancel" => {
                        if self.window_close_pending {
                            self.finish_window_close(WindowCloseChoice::Cancel);
                        } else if self.settings_open {
                            self.close_settings();
                        } else if self.note_edit_target.is_some() {
                            self.finish_note_edit(false);
                        } else {
                            if self.pending_close.is_none() {
                                return IpcResponse::failure("no modal is pending");
                            }
                            self.finish_close_confirmation(false);
                        }
                        None
                    }
                    "close-window" => {
                        self.request_window_close();
                        None
                    }
                    "keep-server-running" => {
                        if !self.window_close_pending {
                            return IpcResponse::failure("no window-close confirmation is pending");
                        }
                        self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                        None
                    }
                    "stop-server-and-exit" => {
                        if !self.window_close_pending {
                            return IpcResponse::failure("no window-close confirmation is pending");
                        }
                        self.finish_window_close(WindowCloseChoice::StopServerAndExit);
                        None
                    }
                    "composer-send" => {
                        self.send_composer();
                        None
                    }
                    "copy-selection" => match self.copy_terminal_selection() {
                        Ok(_) => None,
                        Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                    },
                    "open-settings" => {
                        self.open_settings();
                        None
                    }
                    "window-minimize" => {
                        unsafe { ShowWindow(self.window, SW_MINIMIZE) };
                        None
                    }
                    "window-maximize" => {
                        unsafe { ShowWindow(self.window, SW_MAXIMIZE) };
                        None
                    }
                    "window-restore" => {
                        self.reattach_window("ui-action");
                        None
                    }
                    "window-resize" => {
                        let Some(width) = option_value(args, "--width")
                            .and_then(|value| value.parse::<i32>().ok())
                            .filter(|value| *value >= 320)
                        else {
                            return IpcResponse::failure(
                                "window-resize requires --width of at least 320",
                            );
                        };
                        let Some(height) = option_value(args, "--height")
                            .and_then(|value| value.parse::<i32>().ok())
                            .filter(|value| *value >= 240)
                        else {
                            return IpcResponse::failure(
                                "window-resize requires --height of at least 240",
                            );
                        };
                        self.reattach_window("window-resize");
                        let mut client = RECT::default();
                        let mut outer = RECT::default();
                        unsafe {
                            GetClientRect(self.window, &mut client);
                            GetWindowRect(self.window, &mut outer);
                            MoveWindow(
                                self.window,
                                outer.left,
                                outer.top,
                                width + (outer.right - outer.left) - client.right,
                                height + (outer.bottom - outer.top) - client.bottom,
                                1,
                            );
                        }
                        None
                    }
                    other => {
                        return IpcResponse::failure(format!("unknown UI action: {other}"));
                    }
                };
                if let Some(response) = response {
                    response
                } else {
                    unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                    IpcResponse::success(self.ui_snapshot())
                }
            }
            "show-composer" => {
                self.save_active_composer();
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find window");
                };
                IpcResponse::success(self.tabs[position].composer.clone())
            }
            "set-composer" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find window");
                };
                let text = positional_values(args, &["-t"], &[]).join(" ");
                let id = self.tabs[position].id;
                if self.note_edit_target != Some(id) {
                    self.tabs[position].composer = text.clone();
                    self.event_journal.commit(
                        "composer.draft",
                        Some(id),
                        serde_json::json!({
                            "length": text.chars().count(),
                        }),
                    );
                }
                if self.active == Some(id) {
                    unsafe { SetWindowTextW(self.edit, wide(&text).as_ptr()) };
                }
                IpcResponse::success("")
            }
            "send-composer" => {
                self.save_active_composer();
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find window");
                };
                let text = mem::take(&mut self.tabs[position].composer);
                if !text.is_empty() && !self.tabs[position].submit(&text) {
                    self.tabs[position].composer = text;
                    return IpcResponse::failure("a composer submission is already pending");
                }
                if !text.is_empty() {
                    let id = self.tabs[position].id;
                    self.event_journal.commit(
                        "composer.submitted",
                        Some(id),
                        serde_json::json!({
                            "length": text.chars().count(),
                        }),
                    );
                }
                if self.active == Some(self.tabs[position].id) {
                    unsafe { SetWindowTextW(self.edit, wide("").as_ptr()) };
                }
                IpcResponse::success("")
            }
            "inspect" | "pane-snapshot" => {
                self.save_active_composer();
                let selected: Vec<&TerminalTab> = match option_value(args, "-t") {
                    Some(target) => {
                        let Some(position) = self.target_position(Some(target)) else {
                            return IpcResponse::failure("can't find window");
                        };
                        vec![&self.tabs[position]]
                    }
                    None => self.tabs.iter().collect(),
                };
                let windows = selected
                    .into_iter()
                    .map(|tab| {
                        serde_json::json!({
                            "id": format!("@{}", tab.id),
                            "index": tab.index,
                            "parent_id": tab.parent_id.map(|id| format!("@{id}")),
                            "depth": self.tab_depth(tab.id),
                            "name": tab.title,
                            "terminal_title": tab.parser.callbacks().title,
                            "note": tab.note,
                            "active": self.active == Some(tab.id),
                            "dead": tab.exited.is_some(),
                            "exit_code": tab.exited,
                            "pid": tab.process_id,
                            "command": tab.command_name,
                            "environment_names": tab.environment_names,
                            "rows": tab.last_size.0,
                            "cols": tab.last_size.1,
                            "input_bytes": tab.input_bytes,
                            "input_writes": tab.input_writes,
                            "submit_pending": tab.pending_submit_at.is_some(),
                            "scrollback_offset": tab.parser.screen().scrollback(),
                            "output_bytes": tab.output_bytes,
                            "composer": tab.composer,
                            "error": tab.error,
                            "text": tab.parser.screen().contents(),
                        })
                    })
                    .collect::<Vec<_>>();
                match serde_json::to_string_pretty(&serde_json::json!({
                    "session": self.session_name,
                    "active_window_id": self.active.map(|id| format!("@{id}")),
                    "windows": windows,
                })) {
                    Ok(json) => IpcResponse::success(json),
                    Err(error) => IpcResponse::failure(error.to_string()),
                }
            }
            "screenshot" => {
                unsafe {
                    InvalidateRect(self.window, ptr::null(), 0);
                }
                self.paint();
                let path = screenshot_output_path(args, "agenterm-window");
                match save_window_png(self.window, &path, false) {
                    Ok(()) => IpcResponse::success(path.display().to_string()),
                    Err(error) => IpcResponse::failure(format!("{error:#}")),
                }
            }
            "screenshot-pane" | "screenshot-tab" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find window");
                };
                self.save_active_composer();
                let previous = self.active;
                self.active = Some(self.tabs[position].id);
                self.load_active_composer();
                unsafe {
                    InvalidateRect(self.window, ptr::null(), 0);
                }
                self.paint();
                let path = screenshot_output_path(args, "agenterm-pane");
                let result = save_window_png(self.window, &path, true);
                self.save_active_composer();
                self.active = previous;
                self.load_active_composer();
                unsafe {
                    InvalidateRect(self.window, ptr::null(), 0);
                }
                match result {
                    Ok(()) => IpcResponse::success(path.display().to_string()),
                    Err(error) => IpcResponse::failure(format!("{error:#}")),
                }
            }
            "display" | "display-message" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find pane");
                };
                let format = last_positional(args, &["-t"])
                    .unwrap_or("#{session_name}:#{window_index}.#{pane_index}");
                IpcResponse::success(render_format(
                    format,
                    &self.tabs[position],
                    &self.session_name,
                    self.active == Some(self.tabs[position].id),
                ))
            }
            "show" | "show-options" => IpcResponse::success(
                [
                    format!("default-shell {}", env::var("COMSPEC").unwrap_or_default()),
                    format!("terminal-font-family {}", self.config.terminal_font_family),
                    format!("terminal-font-size {}", self.config.terminal_font_size),
                    "remain-on-exit on".to_owned(),
                    "status on".to_owned(),
                ]
                .join("\n"),
            ),
            "save-workspace" => match self.persist_workspace() {
                Ok(()) => IpcResponse::success(workspace_path().display().to_string()),
                Err(error) => IpcResponse::failure(format!("{error:#}")),
            },
            "workspace-info" => IpcResponse::success(
                serde_json::to_string_pretty(&serde_json::json!({
                    "path": workspace_path(),
                    "version": 1,
                    "tab_count": self.tabs.len(),
                    "active_id": self.active.map(|id| format!("@{id}")),
                    "restore_behavior": "restart-processes",
                }))
                .unwrap_or_default(),
            ),
            "shutdown" => {
                if let Err(error) = self.persist_workspace() {
                    return IpcResponse::failure(format!("{error:#}"));
                }
                self.event_journal.commit(
                    "workspace.shutdown",
                    None,
                    serde_json::json!({"saved": true}),
                );
                self.close_requested = true;
                IpcResponse::success("")
            }
            "lscm" | "list-commands" => IpcResponse::success(SUPPORTED_COMMANDS),
            "splitw" | "split-window" => IpcResponse::failure(
                "split-window is not implemented yet; AgenTerm currently maps one ConPTY pane per tab",
            ),
            "kill-session" | "kill-server" => {
                if let Some(requested) = option_value(args, "-t")
                    && requested != self.session_name
                {
                    return IpcResponse::failure(format!("can't find session: {requested}"));
                }
                for tab in &mut self.tabs {
                    tab.close_process();
                }
                self.tabs.clear();
                self.active = None;
                self.event_journal.commit(
                    "workspace.shutdown",
                    None,
                    serde_json::json!({"saved": false, "destroyed": true}),
                );
                self.close_requested = true;
                IpcResponse::success("")
            }
            _ => IpcResponse::failure(format!("unknown command: {command}")),
        }
    }

    fn adjacent_position(&self, direction: i32) -> Option<usize> {
        if self.tabs.is_empty() {
            return None;
        }
        let current = self.active_position().unwrap_or(0) as i32;
        Some((current + direction).rem_euclid(self.tabs.len() as i32) as usize)
    }

    fn select_adjacent(&mut self, direction: i32) -> IpcResponse {
        let Some(position) = self.adjacent_position(direction) else {
            return IpcResponse::failure("no windows");
        };
        self.save_active_composer();
        let id = self.tabs[position].id;
        self.active = Some(id);
        self.load_active_composer();
        self.event_journal
            .commit("tab.selected", Some(id), serde_json::json!({}));
        IpcResponse::success("")
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        let _ = save_workspace(&self.saved_workspace());
        if self.terminal_font_owned {
            unsafe { DeleteObject(self.terminal_font as HGDIOBJ) };
        }
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

fn pixel_rect_json(rect: PixelRect) -> serde_json::Value {
    serde_json::json!({
        "x": rect.left,
        "y": rect.top,
        "width": rect.width(),
        "height": rect.height(),
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

fn draw_text(device: HDC, text: &str, mut rect: RECT, color: COLORREF, format: u32) {
    let encoded = wide(text);
    unsafe {
        SetTextColor(device, color);
        SetBkMode(device, TRANSPARENT as i32);
        DrawTextW(
            device,
            encoded.as_ptr(),
            text.encode_utf16().count() as i32,
            &mut rect,
            format,
        );
    }
}

fn terminal_color(color: vt100::Color, background: bool, palette: &ThemePalette) -> COLORREF {
    match color {
        vt100::Color::Default if background => palette.terminal_background.colorref(),
        vt100::Color::Default => palette.terminal_foreground.colorref(),
        vt100::Color::Rgb(red, green, blue) => rgb(red, green, blue),
        vt100::Color::Idx(index) => ansi_color(index, palette),
    }
}

fn ansi_color(index: u8, palette: &ThemePalette) -> COLORREF {
    if let Some(color) = palette.ansi.get(index as usize) {
        return color.colorref();
    }
    if (16..=231).contains(&index) {
        let value = index - 16;
        let component = |part: u8| if part == 0 { 0 } else { 55 + part * 40 };
        return rgb(
            component(value / 36),
            component((value / 6) % 6),
            component(value % 6),
        );
    }
    let gray = 8 + index.saturating_sub(232) * 10;
    rgb(gray, gray, gray)
}

fn window_text(window: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(window) }.max(0) as usize;
    let mut buffer = vec![0_u16; length + 1];
    let copied = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn create_terminal_font(window: HWND, config: &AppConfig) -> (HFONT, bool, String) {
    let device = unsafe { GetDC(window) };
    let dpi = if device.is_null() {
        96
    } else {
        unsafe { GetDeviceCaps(device, LOGPIXELSY as i32) }.max(72)
    };
    let height = -((i32::from(config.terminal_font_size) * dpi + 36) / 72);
    let family = wide(&config.terminal_font_family);
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
            DEFAULT_CHARSET.into(),
            OUT_DEFAULT_PRECIS.into(),
            CLIP_DEFAULT_PRECIS.into(),
            CLEARTYPE_QUALITY.into(),
            (FIXED_PITCH | FF_MODERN).into(),
            family.as_ptr(),
        )
    };
    let (font, owned) = if font.is_null() {
        (unsafe { GetStockObject(SYSTEM_FIXED_FONT) as HFONT }, false)
    } else {
        (font, true)
    };
    let resolved = if device.is_null() {
        config.terminal_font_family.clone()
    } else {
        let previous = unsafe { SelectObject(device, font as HGDIOBJ) };
        let mut buffer = [0_u16; 128];
        let copied = unsafe { GetTextFaceW(device, buffer.len() as i32, buffer.as_mut_ptr()) };
        unsafe {
            SelectObject(device, previous);
            ReleaseDC(window, device);
        }
        if copied > 0 {
            String::from_utf16_lossy(&buffer[..copied as usize])
                .trim_end_matches('\0')
                .to_owned()
        } else {
            config.terminal_font_family.clone()
        }
    };
    (font, owned, resolved)
}

fn ipc_address() -> String {
    if let Some(address) = IPC_ADDRESS_OVERRIDE.with(|value| value.borrow().clone()) {
        return address;
    }
    if let Ok(address) = env::var("AGENTERM_IPC_ADDRESS")
        && !address.trim().is_empty()
    {
        return address;
    }
    let user = env::var("USERNAME").unwrap_or_else(|_| "default".to_owned());
    let hash = user.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    format!("127.0.0.1:{}", 42_000 + hash % 10_000)
}

fn has_explicit_ipc_address() -> bool {
    IPC_ADDRESS_OVERRIDE.with(|value| value.borrow().is_some())
        || env::var("AGENTERM_IPC_ADDRESS").is_ok_and(|address| !address.trim().is_empty())
}

fn parse_loopback_ipc_address(address: &str) -> Result<std::net::SocketAddr> {
    if address.contains('\0') {
        anyhow::bail!("invalid AgenTerm IPC address: NUL is not allowed");
    }
    let socket: std::net::SocketAddr = address
        .parse()
        .with_context(|| format!("invalid AgenTerm IPC address: {address}"))?;
    if !socket.ip().is_loopback() {
        anyhow::bail!(
            "AgenTerm IPC address must use a loopback IP (127.0.0.0/8 or ::1): {address}"
        );
    }
    Ok(socket)
}

fn ipc_socket_addr() -> Result<std::net::SocketAddr> {
    parse_loopback_ipc_address(&ipc_address())
}

fn send_ipc_request(args: Vec<String>) -> Result<IpcResponse> {
    send_ipc_request_to(&ipc_address(), args)
}

fn send_ipc_request_to(address: &str, args: Vec<String>) -> Result<IpcResponse> {
    send_ipc_request_to_with_timeout(address, args, IPC_TIMEOUT)
}

fn send_ipc_request_to_with_timeout(
    address: &str,
    args: Vec<String>,
    timeout: Duration,
) -> Result<IpcResponse> {
    use std::io::BufRead as _;
    let socket = parse_loopback_ipc_address(address)?;
    let mut connection = std::net::TcpStream::connect_timeout(&socket, Duration::from_millis(100))
        .context("AgenTerm server is not running")?;
    connection.set_read_timeout(Some(timeout))?;
    connection.set_write_timeout(Some(timeout))?;
    connection.write_all(serde_json::to_string(&IpcRequest { args })?.as_bytes())?;
    connection.write_all(b"\n")?;
    connection.flush()?;
    let mut reader = std::io::BufReader::new(connection);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    if response.is_empty() {
        anyhow::bail!("AgenTerm server closed the IPC connection");
    }
    serde_json::from_str(&response).context("invalid response from AgenTerm server")
}

fn run_list_instances(arguments: &[String]) -> i32 {
    let json = arguments.iter().any(|argument| argument == "--json");
    let prune = arguments.iter().any(|argument| argument == "--prune");
    let instances = match discover_instances() {
        Ok(instances) => instances,
        Err(error) => {
            eprintln!("failed to discover AgenTerm instances: {error:#}");
            return 1;
        }
    };
    let mut views = Vec::new();
    for instance in instances {
        let response = send_ipc_request_to_with_timeout(
            &instance.record.address,
            vec!["ui-snapshot".to_owned()],
            IPC_DISCOVERY_TIMEOUT,
        );
        let (status, snapshot) = match response {
            Ok(response) if response.ok => (
                "running",
                serde_json::from_str::<serde_json::Value>(&response.output).ok(),
            ),
            Ok(_) => ("running", None),
            Err(_) => ("unreachable", None),
        };
        if prune && status == "unreachable" {
            if let Err(error) = prune_instance(&instance) {
                eprintln!("{error:#}");
                return 1;
            }
            continue;
        }
        let tab_count = snapshot
            .as_ref()
            .and_then(|value| value["tabs"].as_array())
            .map_or(0, Vec::len);
        let active_tab = snapshot
            .as_ref()
            .and_then(|value| value["focus"]["window_id"].as_str())
            .unwrap_or("-");
        let window_title = snapshot
            .as_ref()
            .and_then(|value| value["window"]["title"].as_str())
            .unwrap_or("");
        views.push(serde_json::json!({
            "pid": instance.record.pid,
            "address": instance.record.address,
            "version": instance.record.version,
            "status": status,
            "tab_count": tab_count,
            "active_tab": active_tab,
            "session": instance.record.session,
            "workspace_path": instance.record.workspace_path,
            "started_at_unix_ms": instance.record.started_at_unix_ms,
            "window_title": window_title,
        }));
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&views).unwrap_or_else(|_| "[]".to_owned())
        );
    } else if views.is_empty() {
        println!("No registered AgenTerm instances.");
    } else {
        println!("PID\tADDRESS\tVERSION\tSTATUS\tTABS\tACTIVE\tSESSION\tWORKSPACE");
        for view in views {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                view["pid"].as_u64().unwrap_or_default(),
                view["address"].as_str().unwrap_or_default(),
                view["version"].as_str().unwrap_or_default(),
                view["status"].as_str().unwrap_or_default(),
                view["tab_count"].as_u64().unwrap_or_default(),
                view["active_tab"].as_str().unwrap_or("-"),
                view["session"].as_str().unwrap_or_default(),
                view["workspace_path"].as_str().unwrap_or_default(),
            );
        }
    }
    0
}

fn start_ipc_server(window: HWND) -> Result<Receiver<IpcEnvelope>> {
    use std::io::BufRead as _;
    let listener = std::net::TcpListener::bind(ipc_socket_addr()?)
        .context("another AgenTerm server is already using the local IPC port")?;
    let (sender, receiver) = mpsc::channel();
    let wake_window = window as isize;
    thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(connection) = connection else {
                continue;
            };
            let mut reader = std::io::BufReader::new(connection);
            let mut line = String::new();
            let response = if reader.read_line(&mut line).is_err() {
                IpcResponse::failure("could not read IPC request")
            } else {
                match serde_json::from_str::<IpcRequest>(&line) {
                    Ok(request) => {
                        let (response_sender, response_receiver) = mpsc::channel();
                        if sender
                            .send(IpcEnvelope {
                                request,
                                respond_to: response_sender,
                            })
                            .is_err()
                        {
                            IpcResponse::failure("AgenTerm GUI is shutting down")
                        } else {
                            unsafe {
                                PostMessageW(wake_window as HWND, WM_APP_WAKE, 0, 0);
                            }
                            response_receiver
                                .recv_timeout(IPC_TIMEOUT)
                                .unwrap_or_else(|_| {
                                    IpcResponse::failure("AgenTerm GUI did not process the command")
                                })
                        }
                    }
                    Err(error) => IpcResponse::failure(format!("invalid IPC request: {error}")),
                }
            };
            if let Ok(serialized) = serde_json::to_string(&response) {
                let connection = reader.get_mut();
                let _ = connection.write_all(serialized.as_bytes());
                let _ = connection.write_all(b"\n");
                let _ = connection.flush();
            }
        }
    });
    Ok(receiver)
}

fn run_cli(arguments: Vec<String>) -> i32 {
    let mut arguments = arguments;
    if control_command_requests_help(&arguments) {
        let command = arguments.first().map(String::as_str).unwrap_or_default();
        if let Some(usage) = control_command_usage(command) {
            println!("Usage: {usage}");
            return 0;
        }
    }
    if let Err(error) = validate_control_command(&arguments) {
        eprintln!("{error}");
        return 2;
    }
    if arguments
        .first()
        .is_some_and(|command| command == "set-composer")
    {
        let content = if let Some(position) = arguments
            .iter()
            .take_while(|argument| argument.as_str() != "--")
            .position(|argument| argument == "--stdin")
        {
            arguments.remove(position);
            let mut content = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut content) {
                eprintln!("failed to read composer content from stdin: {error}");
                return 1;
            }
            Some(content)
        } else if let Some(position) = arguments
            .iter()
            .take_while(|argument| argument.as_str() != "--")
            .position(|argument| argument == "--file")
        {
            if position + 1 >= arguments.len() {
                eprintln!("set-composer --file requires a path");
                return 1;
            }
            let path = arguments.remove(position + 1);
            arguments.remove(position);
            match std::fs::read_to_string(&path) {
                Ok(content) => Some(content),
                Err(error) => {
                    eprintln!("failed to read composer file {path}: {error}");
                    return 1;
                }
            }
        } else {
            None
        };
        if let Some(content) = content {
            arguments.push("--".to_owned());
            arguments.push(content);
        }
    }
    if let Some(command) = arguments.first_mut() {
        let canonical = canonical_control_command(command);
        if canonical != command {
            *command = canonical.to_owned();
        }
    }
    let command = arguments.first().map(String::as_str).unwrap_or_default();
    if matches!(command, "list-commands" | "lscm") {
        print!("{SUPPORTED_COMMANDS}");
        return 0;
    }
    if command == "protocol-info" {
        println!("{}", protocol_info_json());
        return 0;
    }
    if matches!(command, "list-instances" | "server-list") {
        return run_list_instances(&arguments);
    }
    if command == "script" {
        return run_script_command(&arguments);
    }
    if !has_explicit_ipc_address() {
        match select_implicit_control_instance() {
            Ok(address) => IPC_ADDRESS_OVERRIDE.with(|value| {
                *value.borrow_mut() = Some(address);
            }),
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        }
    }
    if command == "wait-ui" {
        return run_wait_ui(&arguments);
    }
    if matches!(command, "wait-pane" | "expect-pane") {
        return run_wait_pane(&arguments);
    }
    if command == "wait-events" {
        return run_wait_events(&arguments);
    }
    let may_start_server = matches!(
        command,
        "new-session"
            | "new"
            | "new-agent"
            | "new-window"
            | "neww"
            | "attach-session"
            | "attach"
            | "start-server"
    );
    let mut response = send_ipc_request(arguments.clone());
    if response.is_err()
        && may_start_server
        && let Ok(executable) = gui_executable_path()
    {
        let executable = wide(&executable.to_string_lossy());
        let operation = wide("open");
        let parameters = wide(&format!("--address {}", ipc_address()));
        let launched = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                operation.as_ptr(),
                executable.as_ptr(),
                parameters.as_ptr(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if launched <= 32 {
            eprintln!("failed to launch AgenTerm GUI through Windows Shell ({launched})");
        }
        let deadline = Instant::now() + IPC_AUTOSTART_TIMEOUT;
        while Instant::now() < deadline {
            thread::sleep(
                IPC_AUTOSTART_POLL.min(deadline.saturating_duration_since(Instant::now())),
            );
            response = send_ipc_request(arguments.clone());
            if response.is_ok() {
                break;
            }
        }
    }
    match response {
        Ok(response) if response.ok => {
            if !response.output.is_empty() {
                print!("{}", response.output);
                if !response.output.ends_with('\n') {
                    println!();
                }
            }
            0
        }
        Ok(response) => {
            eprintln!("{}", response.error);
            1
        }
        Err(error) => {
            eprintln!("{error:#}");
            1
        }
    }
}

fn run_script_command(arguments: &[String]) -> i32 {
    let Some(operation_name) = arguments.get(1).map(String::as_str) else {
        eprintln!("script requires api, check, eval, or run");
        return 2;
    };
    let operation = match operation_name {
        "api" => ScriptOperation::Api,
        "check" => ScriptOperation::Check,
        "eval" => ScriptOperation::Eval,
        "run" => ScriptOperation::Run,
        other => {
            eprintln!("unknown script operation: {other}");
            return 2;
        }
    };
    let profile = match option_value(arguments, "--profile").unwrap_or("pure") {
        "pure" => ScriptProfile::Pure,
        "observe" => ScriptProfile::Observe,
        other => {
            eprintln!("unknown script profile: {other}");
            return 2;
        }
    };
    let mut budgets = ScriptBudgets::default();
    if let Some(value) = option_value(arguments, "--timeout-ms") {
        match value.parse::<u64>() {
            Ok(value) if (1..=10_000).contains(&value) => budgets.wall_time_ms = value,
            _ => {
                eprintln!("script --timeout-ms must be from 1 to 10000");
                return 2;
            }
        }
    }
    if let Some(value) = option_value(arguments, "--max-operations") {
        match value.parse::<u64>() {
            Ok(value) if (1..=10_000_000).contains(&value) => budgets.operations = value,
            _ => {
                eprintln!("script --max-operations must be from 1 to 10000000");
                return 2;
            }
        }
    }

    let operand = script_operand(arguments);
    let (source_label, source) = match operation {
        ScriptOperation::Api => ("api".to_owned(), String::new()),
        ScriptOperation::Eval => {
            let Some(expression) = operand else {
                eprintln!("script eval requires an expression");
                return 2;
            };
            ("eval".to_owned(), expression.to_owned())
        }
        ScriptOperation::Check | ScriptOperation::Run => {
            let Some(path) = operand else {
                eprintln!("script {operation_name} requires a file path or -");
                return 2;
            };
            if path == "-" {
                match read_script_source(std::io::stdin().lock(), budgets.source_bytes) {
                    Ok(source) => ("stdin".to_owned(), source),
                    Err((code, error)) => {
                        eprintln!("failed to read script from stdin: {error}");
                        return code;
                    }
                }
            } else {
                if std::fs::metadata(path)
                    .is_ok_and(|metadata| metadata.len() > budgets.source_bytes as u64)
                {
                    eprintln!(
                        "script source exceeds the {} byte limit",
                        budgets.source_bytes
                    );
                    return 3;
                }
                let file = match std::fs::File::open(path) {
                    Ok(file) => file,
                    Err(error) => {
                        eprintln!("failed to read script {path}: {error}");
                        return 1;
                    }
                };
                match read_script_source(file, budgets.source_bytes) {
                    Ok(source) => (path.to_owned(), source),
                    Err((code, error)) => {
                        eprintln!("failed to read script {path}: {error}");
                        return code;
                    }
                }
            }
        }
    };
    if source.len() > budgets.source_bytes {
        eprintln!(
            "script source exceeds the {} byte limit",
            budgets.source_bytes
        );
        return 3;
    }

    let needs_observation = profile == ScriptProfile::Observe
        && matches!(operation, ScriptOperation::Eval | ScriptOperation::Run);
    let observation = if needs_observation {
        if !has_explicit_ipc_address() {
            match select_implicit_control_instance() {
                Ok(address) => IPC_ADDRESS_OVERRIDE.with(|value| {
                    *value.borrow_mut() = Some(address);
                }),
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            }
        }
        match send_ipc_request(vec!["ui-snapshot".to_owned()]) {
            Ok(response) if response.ok => match serde_json::from_str(&response.output) {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    eprintln!("observe broker received an invalid snapshot: {error}");
                    return 1;
                }
            },
            Ok(response) => {
                eprintln!("{}", response.error);
                return 1;
            }
            Err(error) => {
                eprintln!("{error:#}");
                return 1;
            }
        }
    } else {
        None
    };
    let invocation_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let delimiter = arguments.iter().position(|argument| argument == "--");
    let script_arguments = delimiter
        .and_then(|position| arguments.get(position + 1..))
        .unwrap_or_default()
        .to_vec();
    let invocation = ScriptInvocation {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id,
        api_version: SCRIPT_API_VERSION,
        operation,
        profile,
        source_label,
        source,
        arguments: script_arguments,
        budgets,
        observation,
    };
    let executable = match std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("agenterm-script.exe"))
    }) {
        Some(path) if path.is_file() => path,
        _ => {
            eprintln!(
                "agenterm-script.exe is not installed next to agenterm-cli.exe; \
                 scripting is an optional component"
            );
            return 2;
        }
    };
    let mut worker = match std::process::Command::new(&executable)
        .arg("--worker")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(worker) => worker,
        Err(error) => {
            eprintln!("failed to start {}: {error}", executable.display());
            return 1;
        }
    };
    let serialized = match serde_json::to_vec(&invocation) {
        Ok(serialized) => serialized,
        Err(error) => {
            let _ = worker.kill();
            eprintln!("failed to encode script invocation: {error}");
            return 1;
        }
    };
    if worker
        .stdin
        .take()
        .is_none_or(|mut input| input.write_all(&serialized).is_err())
    {
        let _ = worker.kill();
        eprintln!("failed to send the invocation to agenterm-script.exe");
        return 1;
    }
    let output = match worker.wait_with_output() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("failed to wait for agenterm-script.exe: {error}");
            return 1;
        }
    };
    let result: ScriptResult = match serde_json::from_slice(&output.stdout) {
        Ok(result) => result,
        Err(error) => {
            eprintln!(
                "agenterm-script.exe returned an invalid result: {error}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return 1;
        }
    };
    if result.envelope_version != SCRIPT_ENVELOPE_VERSION
        || result.api_version != SCRIPT_API_VERSION
        || result.invocation_id != invocation.invocation_id
        || result.operation != Some(operation)
        || result.profile != Some(profile)
    {
        eprintln!(
            "agenterm-script.exe returned a mismatched protocol result \
             (envelope/API/invocation/operation/profile identity)"
        );
        return 1;
    }
    let result_consistent = if result.ok {
        output.status.success()
            && result.exit_class == ScriptExitClass::Success
            && result.failure.is_none()
    } else {
        !output.status.success()
            && result.exit_class != ScriptExitClass::Success
            && result.failure.is_some()
    };
    if !result_consistent {
        eprintln!(
            "agenterm-script.exe returned an inconsistent process status and result envelope"
        );
        return 1;
    }
    if arguments.iter().any(|argument| argument == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_owned())
        );
    } else if result.ok {
        if !result.stdout.is_empty() {
            print!("{}", result.stdout);
            if !result.stdout.ends_with('\n') {
                println!();
            }
        }
        if let Some(value) = result.value {
            if let Some(value) = value.as_str() {
                println!("{value}");
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
                );
            }
        } else if operation == ScriptOperation::Check {
            println!("OK");
        }
    } else if let Some(failure) = &result.failure {
        eprintln!(
            "{}",
            serde_json::json!({
                "code": failure.code,
                "message": failure.message,
                "invocation_id": result.invocation_id,
                "exit_class": result.exit_class,
            })
        );
    }
    if result.ok {
        0
    } else {
        match result.exit_class.as_str() {
            "configuration" => 2,
            "limit" => 3,
            _ => 1,
        }
    }
}

fn read_script_source(
    reader: impl Read,
    limit: usize,
) -> std::result::Result<String, (i32, String)> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024).saturating_add(1));
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| (1, error.to_string()))?;
    if bytes.len() > limit {
        return Err((3, format!("script source exceeds the {limit} byte limit")));
    }
    String::from_utf8(bytes).map_err(|error| (1, format!("script source is not UTF-8: {error}")))
}

fn script_operand(arguments: &[String]) -> Option<&str> {
    let mut position = 2;
    while position < arguments.len() {
        match arguments[position].as_str() {
            "--" => return None,
            "--profile" | "--timeout-ms" | "--max-operations" => position += 2,
            "--json" => position += 1,
            value => return Some(value),
        }
    }
    None
}

fn select_implicit_control_instance() -> std::result::Result<String, String> {
    let instances = discover_instances().map_err(|error| {
        instance_selection_error(
            "instance_discovery_failed",
            "AgenTerm instance discovery failed",
            &format!("{error:#}"),
            Vec::new(),
        )
    })?;
    let mut candidates = Vec::new();
    let mut healthy = Vec::new();
    for instance in instances {
        let running = send_ipc_request_to_with_timeout(
            &instance.record.address,
            vec!["protocol-info".to_owned()],
            IPC_DISCOVERY_TIMEOUT,
        )
        .is_ok_and(|response| response.ok);
        if running {
            healthy.push(instance.record.address.clone());
        }
        candidates.push(serde_json::json!({
            "pid": instance.record.pid,
            "address": instance.record.address,
            "session": instance.record.session,
            "workspace_path": instance.record.workspace_path,
            "status": if running { "running" } else { "unreachable" },
        }));
    }
    match healthy.as_slice() {
        [address] => Ok(address.clone()),
        [] => Err(instance_selection_error(
            "no_healthy_instance",
            "No healthy AgenTerm instance is available",
            "Start agenterm.exe, or use `agenterm-cli --address HOST:PORT COMMAND` to target and \
             autostart a specific local server. Inspect registrations with \
             `agenterm-cli list-instances --json`.",
            candidates,
        )),
        _ => Err(instance_selection_error(
            "ambiguous_instance",
            "More than one healthy AgenTerm instance is available",
            "Choose one with `agenterm-cli --address HOST:PORT COMMAND` or set \
             AGENTERM_IPC_ADDRESS. Inspect details with `agenterm-cli list-instances --json`.",
            candidates,
        )),
    }
}

fn instance_selection_error(
    code: &str,
    message: &str,
    hint: &str,
    candidates: Vec<serde_json::Value>,
) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "error": {
            "code": code,
            "message": message,
            "hint": hint,
            "candidates": candidates,
        }
    }))
    .unwrap_or_else(|_| format!("{message}: {hint}"))
}

fn run_wait_events(arguments: &[String]) -> i32 {
    let Some(epoch) = option_value(arguments, "--epoch") else {
        eprintln!("wait-events requires --epoch");
        return 2;
    };
    let Some(mut after) =
        option_value(arguments, "--after").and_then(|value| value.parse::<u64>().ok())
    else {
        eprintln!("wait-events requires a numeric --after sequence");
        return 2;
    };
    let Some(kind) = option_value(arguments, "--kind") else {
        eprintln!("wait-events requires --kind");
        return 2;
    };
    let tab_id = option_value(arguments, "--tab")
        .map(|value| value.trim_start_matches('@'))
        .and_then(|value| value.parse::<u64>().ok());
    if option_value(arguments, "--tab").is_some() && tab_id.is_none() {
        eprintln!("wait-events --tab must be a stable @ID");
        return 2;
    }
    let timeout_ms = option_value(arguments, "--timeout-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5_000)
        .min(60_000);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let response = send_ipc_request(vec![
            "read-events".to_owned(),
            "--epoch".to_owned(),
            epoch.to_owned(),
            "--after".to_owned(),
            after.to_string(),
            "--limit".to_owned(),
            "256".to_owned(),
        ]);
        let response = match response {
            Ok(response) if response.ok => response,
            Ok(response) => {
                eprintln!("{}", response.error);
                return 1;
            }
            Err(error) => {
                eprintln!("{error:#}");
                return 1;
            }
        };
        let Ok(batch) = serde_json::from_str::<serde_json::Value>(&response.output) else {
            eprintln!("wait-events received an invalid event batch");
            return 1;
        };
        if let Some(events) = batch["events"].as_array() {
            for event in events {
                if let Some(sequence) = event["sequence"].as_u64() {
                    after = after.max(sequence);
                }
                let kind_matches = event["kind"].as_str() == Some(kind);
                let tab_matches =
                    tab_id.is_none_or(|requested| event["tab_id"].as_u64() == Some(requested));
                if kind_matches && tab_matches {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(event).unwrap_or_else(|_| event.to_string())
                    );
                    return 0;
                }
            }
        }
        if Instant::now() >= deadline {
            eprintln!(
                "{}",
                serde_json::json!({
                    "code": "event_wait_timeout",
                    "epoch": epoch,
                    "after": after,
                    "kind": kind,
                    "tab_id": tab_id,
                    "timeout_ms": timeout_ms,
                })
            );
            return 1;
        }
        thread::sleep(
            Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn run_wait_ui(arguments: &[String]) -> i32 {
    let timeout_ms = option_value(arguments, "--timeout-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(10_000);
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    let expected_active = option_value(arguments, "--active");
    let expected_focus = option_value(arguments, "--focus");
    let expected_state = option_value(arguments, "--tab-state");
    let expected_window_state = option_value(arguments, "--window-state");
    let expected_client_width =
        option_value(arguments, "--client-width").and_then(|value| value.parse::<i64>().ok());
    let expected_client_height =
        option_value(arguments, "--client-height").and_then(|value| value.parse::<i64>().ok());
    let target = option_value(arguments, "-t");
    if expected_active.is_none()
        && expected_focus.is_none()
        && expected_state.is_none()
        && expected_window_state.is_none()
        && expected_client_width.is_none()
        && expected_client_height.is_none()
    {
        eprintln!(
            "wait-ui requires an active, focus, tab-state, window-state, or client-size condition"
        );
        return 1;
    }
    loop {
        match send_ipc_request(vec!["ui-snapshot".to_owned()]) {
            Ok(response) if response.ok => {
                if let Ok(snapshot) = serde_json::from_str::<serde_json::Value>(&response.output) {
                    let active_matches = expected_active.is_none_or(|expected| {
                        snapshot["focus"]["window_id"].as_str() == Some(expected)
                    });
                    let focus_matches = expected_focus.is_none_or(|expected| {
                        snapshot["focus"]["surface"].as_str() == Some(expected)
                    });
                    let state_matches = expected_state.is_none_or(|expected| {
                        snapshot["tabs"].as_array().is_some_and(|tabs| {
                            tabs.iter().any(|tab| {
                                let target_matches = target.is_none_or(|selector| {
                                    tab["id"].as_str() == Some(selector)
                                        || tab["name"].as_str() == Some(selector)
                                        || selector.parse::<u64>().ok().is_some_and(|index| {
                                            tab["index"].as_u64() == Some(index)
                                        })
                                });
                                target_matches && tab["state"].as_str() == Some(expected)
                            })
                        })
                    });
                    let window_state_matches = expected_window_state.is_none_or(|expected| {
                        snapshot["window"]["state"].as_str() == Some(expected)
                    });
                    let width_matches = expected_client_width.is_none_or(|expected| {
                        snapshot["window"]["client_width"].as_i64() == Some(expected)
                    });
                    let height_matches = expected_client_height.is_none_or(|expected| {
                        snapshot["window"]["client_height"].as_i64() == Some(expected)
                    });
                    if active_matches
                        && focus_matches
                        && state_matches
                        && window_state_matches
                        && width_matches
                        && height_matches
                    {
                        println!("{}", response.output);
                        return 0;
                    }
                    if std::time::Instant::now() >= deadline {
                        eprintln!(
                            "wait-ui timed out after {timeout_ms}ms; last state:\n{}",
                            response.output
                        );
                        return 1;
                    }
                }
            }
            Ok(response) => {
                eprintln!("{}", response.error);
                return 1;
            }
            Err(error) => {
                eprintln!("{error:#}");
                return 1;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_wait_pane(arguments: &[String]) -> i32 {
    let target = option_value(arguments, "-t").map(str::to_owned);
    let timeout_ms = option_value(arguments, "--timeout-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5_000);
    let contains = option_value(arguments, "--contains")
        .map(str::to_owned)
        .or_else(|| {
            (arguments
                .first()
                .is_some_and(|value| value == "expect-pane"))
            .then(|| last_positional(arguments, &["-t", "--timeout-ms"]))
            .flatten()
            .map(str::to_owned)
        });
    let wait_dead = arguments.iter().any(|argument| argument == "--dead");
    let wait_submit = arguments
        .iter()
        .any(|argument| argument == "--submit-complete");
    if contains.is_none() && !wait_dead && !wait_submit {
        eprintln!(
            "usage: wait-pane [-t target] (--contains text | --dead | --submit-complete) \
             [--timeout-ms ms]"
        );
        return 2;
    }
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let mut matched = true;
        if wait_dead {
            let mut request = vec![
                "display-message".to_owned(),
                "-p".to_owned(),
                "#{pane_dead}".to_owned(),
            ];
            if let Some(target) = &target {
                request.extend(["-t".to_owned(), target.clone()]);
            }
            matched &= send_ipc_request(request)
                .is_ok_and(|response| response.ok && response.output.trim() == "1");
        }
        if let Some(needle) = &contains {
            let mut request = vec!["capture-pane".to_owned(), "-p".to_owned()];
            if let Some(target) = &target {
                request.extend(["-t".to_owned(), target.clone()]);
            }
            matched &= send_ipc_request(request)
                .is_ok_and(|response| response.ok && response.output.contains(needle));
        }
        if wait_submit {
            let mut request = vec!["inspect".to_owned()];
            if let Some(target) = &target {
                request.extend(["-t".to_owned(), target.clone()]);
            }
            matched &= send_ipc_request(request).is_ok_and(|response| {
                response.ok
                    && serde_json::from_str::<serde_json::Value>(&response.output)
                        .ok()
                        .and_then(|snapshot| snapshot["windows"].as_array().cloned())
                        .is_some_and(|windows| {
                            !windows.is_empty()
                                && windows.iter().all(|window| {
                                    !window["submit_pending"].as_bool().unwrap_or(true)
                                })
                        })
            });
        }
        if matched {
            return 0;
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("wait-pane timed out after {timeout_ms} ms");
            return 1;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn gui_executable_path() -> Result<std::path::PathBuf> {
    let current =
        env::current_exe().context("could not locate the running agenterm-cli executable")?;
    let gui = current.with_file_name("agenterm.exe");
    if !gui.is_file() {
        anyhow::bail!(
            "AgenTerm GUI executable was not found beside agenterm-cli: {}",
            gui.display()
        );
    }
    Ok(gui)
}

fn print_help() {
    println!(
        "\
AgenTerm CLI - control the native tabbed terminal

Usage:
  agenterm-cli [--address HOST:PORT] command [args...]
  agenterm-cli list-instances [--json] [--prune]
  agenterm-cli server-list [--json] [--prune]
  agenterm-cli new-session [-s name]
  agenterm-cli new-window [-d] [-n name] [--parent target] [-F format] [-e NAME=VALUE] [command [args...]]
  agenterm-cli new-agent [-d] [-n name] [--parent target] [--proxy URL] [--yolo] [-- [codex args...]]
  agenterm-cli list-windows [-F format]
  agenterm-cli list-tab-tree [-F format]
  agenterm-cli select-window -t target
  agenterm-cli rename-window [-t target] name
  agenterm-cli kill-window -t target
  agenterm-cli send-keys [-t target] key...
  agenterm-cli scroll-pane [-t target] up|down|page-up|page-down|top|bottom [rows]
  agenterm-cli read-events --epoch EPOCH --after SEQUENCE [--limit COUNT]
  agenterm-cli wait-events --epoch EPOCH --after SEQUENCE --kind KIND [--tab @ID] [--timeout-ms MS]
  agenterm-cli capture-pane -p [-t target]
  agenterm-cli capture-pane --raw-escaped [-t target]
  agenterm-cli dump-cells [-t target] [-r row]
  agenterm-cli active-window [-F format]
  agenterm-cli inspect [-t target]
  agenterm-cli screenshot [-o file.png]
  agenterm-cli screenshot-pane [-t target] [-o file.png]
  agenterm-cli show-composer [-t target]
  agenterm-cli set-composer [-t target] text
  agenterm-cli set-composer [-t target] --stdin|--file path
  agenterm-cli send-composer [-t target]
  agenterm-cli set-tab-note [-t target] text
  agenterm-cli show-tab-note [-t target]
  agenterm-cli set-tab-parent -t child --parent parent|root
  agenterm-cli show-tab-parent [-t target]
  agenterm-cli save-workspace
  agenterm-cli workspace-info
  agenterm-cli shutdown
  agenterm-cli get-settings
  agenterm-cli set-setting terminal.font-family FAMILY
  agenterm-cli set-setting terminal.font-size 8..36
  agenterm-cli send-mouse [-t target] -x col -y row [--button left] [--action press]
  agenterm-cli ui-snapshot
  agenterm-cli ui-action new-tab|new-child|edit-tab|toggle-tree|select-tab|close-tab|close-window|keep-server-running|stop-server-and-exit|confirm|cancel|composer-send|copy-selection
  agenterm-cli focus terminal|composer|sidebar [-t target]
  agenterm-cli wait-pane [-t target] (--contains text|--dead|--submit-complete) [--timeout-ms ms]
  agenterm-cli wait-ui [--active @id] [--focus surface] [-t target --tab-state state]
  agenterm-cli protocol-info
  agenterm-cli list-panes [-F format]
  agenterm-cli list-sessions | has-session | kill-server | server-kill"
    );
}

fn print_mux_help() {
    println!(
        "\
agenterm-mux - tmux/RMUX compatibility frontend for AgenTerm

Usage:
  agenterm-mux [--address HOST:PORT] [--session NAME] COMMAND [ARGS...]
  agenterm-mux compatibility [--json]
  agenterm-mux agenterm COMMAND [ARGS...]

The GUI remains the only server and PTY owner. One AgenTerm tab maps to one
window and one pane. Unsupported tmux operations fail explicitly. Native
AgenTerm commands are available only through the `agenterm` namespace."
    );
}

fn print_mux_commands() {
    for command in MUX_COMMANDS {
        match command.status {
            MuxStatus::Supported => println!("{}", command.name),
            MuxStatus::Unsupported(reason) => {
                println!("{} (unsupported: {reason})", command.name);
            }
        }
    }
}

fn protocol_info_json() -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "protocol_version": 1,
        "agenterm_version": env!("CARGO_PKG_VERSION"),
        "compatibility": {
            "tmux_rmux": [
                "new-session", "list-sessions", "has-session",
                "new-window", "list-windows", "select-window",
                "next-window", "previous-window", "rename-window",
                "kill-window", "list-panes", "capture-pane",
                "send-keys", "display-message", "show-options"
            ],
            "partial": ["kill-session", "kill-server"],
            "planned": ["split-window", "layouts"]
        },
        "extensions": [
            "ui-snapshot", "ui-action", "focus", "protocol-info",
            "inspect", "screenshot", "screenshot-pane", "dump-cells",
            "wait-pane", "send-mouse", "show-composer",
            "set-composer", "send-composer", "get-settings",
            "set-setting", "set-tab-note", "show-tab-note",
            "list-tab-tree", "set-tab-parent", "show-tab-parent",
            "save-workspace", "workspace-info", "shutdown",
            "new-agent", "list-instances", "server-list", "server-kill", "scroll-pane", "read-events",
            "wait-events"
        ],
        "features": {
            "remain_on_exit": true,
            "live_close_confirmation": true,
            "rmux_status_click_bridge": true,
            "semantic_ui_automation": true,
            "hierarchical_tabs": true,
            "persistent_workspace": true,
            "tab_environment": true,
            "codex_launcher": true,
            "mux_frontend": true,
            "instance_discovery": true
        }
    }))
    .unwrap_or_default()
}

fn print_mux_compatibility(json: bool) {
    let supported = MUX_COMMANDS
        .iter()
        .filter(|command| command.status == MuxStatus::Supported)
        .map(|command| command.name)
        .collect::<Vec<_>>();
    let unsupported = MUX_COMMANDS
        .iter()
        .filter_map(|command| match command.status {
            MuxStatus::Supported => None,
            MuxStatus::Unsupported(reason) => Some(serde_json::json!({
                "name": command.name,
                "reason": reason,
            })),
        })
        .collect::<Vec<_>>();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "frontend": "agenterm-mux",
                "agenterm_version": env!("CARGO_PKG_VERSION"),
                "model": {
                    "server": "agenterm.exe",
                    "session": "workspace",
                    "window": "tab",
                    "pane": "single-pane-tab"
                },
                "differences": {
                    "workspace_persistence": "normal GUI shutdown saves tabs and restarts their commands on restore",
                    "process_ownership": "agenterm.exe owns every ConPTY child",
                    "live_close": "GUI close actions require confirmation; explicit CLI kill commands are authoritative",
                    "server_lifetime": "kill-server intentionally clears the saved workspace",
                    "split_panes": false
                },
                "supported": supported,
                "explicitly_unsupported": unsupported,
                "native_namespace": "agenterm",
            }))
            .unwrap_or_default()
        );
    } else {
        println!("agenterm-mux {}", env!("CARGO_PKG_VERSION"));
        println!("sessions=workspaces windows=tabs panes=single-pane-tabs");
        println!("supported: {}", supported.join(", "));
        for entry in unsupported {
            println!(
                "unsupported: {} ({})",
                entry["name"].as_str().unwrap_or_default(),
                entry["reason"].as_str().unwrap_or_default()
            );
        }
        println!("native AgenTerm extensions: agenterm-mux agenterm COMMAND ...");
    }
}

fn render_format(format: &str, tab: &TerminalTab, session_name: &str, active: bool) -> String {
    let dead = tab.exited.is_some();
    format
        .replace("#{?pane_dead,dead,}", if dead { "dead" } else { "" })
        .replace("#{?window_active,*,}", if active { "*" } else { "" })
        .replace("#{session_name}", session_name)
        .replace("#{window_index}", &tab.index.to_string())
        .replace("#{window_id}", &format!("@{}", tab.id))
        .replace(
            "#{tab_parent_id}",
            &tab.parent_id
                .map(|id| format!("@{id}"))
                .unwrap_or_else(|| "root".to_owned()),
        )
        .replace("#{window_name}", &tab.title)
        .replace("#{window_note}", &tab.note)
        .replace("#{terminal_title}", &tab.parser.callbacks().title)
        .replace("#{window_active}", if active { "1" } else { "0" })
        .replace("#{pane_index}", "0")
        .replace("#{pane_id}", &format!("%{}", tab.id))
        .replace("#{pane_dead}", if dead { "1" } else { "0" })
        .replace(
            "#{pane_pid}",
            &tab.process_id
                .map(|pid| pid.to_string())
                .unwrap_or_default(),
        )
        .replace("#{pane_current_command}", &tab.command_name)
        .replace("#{pane_start_command}", &tab.command_name)
        .replace("#{pane_input_bytes}", &tab.input_bytes.to_string())
        .replace("#{pane_output_bytes}", &tab.output_bytes.to_string())
        .replace("#{pane_error}", tab.error.as_deref().unwrap_or(""))
        .replace("#{pane_width}", &tab.last_size.1.to_string())
        .replace("#{pane_height}", &tab.last_size.0.to_string())
        .replace("#{history_limit}", &SCROLLBACK_LINES.to_string())
        .replace("#I", &tab.index.to_string())
        .replace("#W", &tab.title)
        .replace("#S", session_name)
        .replace("#P", "0")
}

fn save_window_png(window: HWND, path: &std::path::Path, pane_only: bool) -> Result<()> {
    let mut client: RECT = unsafe { mem::zeroed() };
    let mut outer: RECT = unsafe { mem::zeroed() };
    unsafe {
        GetClientRect(window, &mut client);
        GetWindowRect(window, &mut outer);
    }
    let (source, source_x, source_y, width, height) = if pane_only {
        (
            unsafe { GetDC(window) },
            SIDEBAR_WIDTH,
            0,
            (client.right - SIDEBAR_WIDTH).max(1),
            (client.bottom - STATUS_BAR_HEIGHT - COMPOSER_HEIGHT).max(1),
        )
    } else {
        (
            unsafe { GetWindowDC(window) },
            0,
            0,
            (outer.right - outer.left).max(1),
            (outer.bottom - outer.top).max(1),
        )
    };
    if source.is_null() {
        anyhow::bail!("failed to acquire window device context");
    }
    let memory_dc = unsafe { CreateCompatibleDC(source) };
    let bitmap = unsafe { CreateCompatibleBitmap(source, width, height) };
    if memory_dc.is_null() || bitmap.is_null() {
        if !memory_dc.is_null() {
            unsafe { DeleteDC(memory_dc) };
        }
        unsafe { ReleaseDC(window, source) };
        anyhow::bail!("failed to allocate screenshot bitmap");
    }

    let previous = unsafe { SelectObject(memory_dc, bitmap as HGDIOBJ) };
    let copied = unsafe {
        BitBlt(
            memory_dc, 0, 0, width, height, source, source_x, source_y, SRCCOPY,
        )
    };
    let mut info: BITMAPINFO = unsafe { mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..unsafe { mem::zeroed() }
    };
    let mut bgra = vec![0_u8; width as usize * height as usize * 4];
    let scanlines = if copied != 0 {
        unsafe {
            GetDIBits(
                memory_dc,
                bitmap,
                0,
                height as u32,
                bgra.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            )
        }
    } else {
        0
    };
    unsafe {
        SelectObject(memory_dc, previous);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteDC(memory_dc);
        ReleaseDC(window, source);
    }
    if copied == 0 || scanlines == 0 {
        anyhow::bail!("BitBlt/GetDIBits failed while capturing the window");
    }

    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let file = std::fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .context("failed to start PNG encoder")?;
    writer
        .write_image_data(&rgba)
        .context("failed to write PNG pixels")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        EditShortcut, TerminalPoint, TerminalSelection, ThemeId, edit_shortcut, effective_theme,
        gui_cli_guidance, normalize_terminal_paste, parse_loopback_ipc_address,
        terminal_copy_shortcut, terminal_selection_text,
    };

    #[test]
    fn settings_theme_draft_only_affects_an_open_settings_preview() {
        assert_eq!(
            effective_theme(ThemeId::Dark, ThemeId::Light, true),
            ThemeId::Light
        );
        assert_eq!(
            effective_theme(ThemeId::Dark, ThemeId::Light, false),
            ThemeId::Dark
        );
    }

    #[test]
    fn accepts_only_loopback_ipc_addresses() {
        assert!(parse_loopback_ipc_address("127.0.0.1:42000").is_ok());
        assert!(parse_loopback_ipc_address("[::1]:42000").is_ok());
        assert!(parse_loopback_ipc_address("0.0.0.0:42000").is_err());
        assert!(parse_loopback_ipc_address("192.0.2.1:42000").is_err());
        assert!(parse_loopback_ipc_address("127.0.0.1:42\0").is_err());
    }

    #[test]
    fn composer_edit_shortcuts_are_explicit_and_control_scoped() {
        assert_eq!(
            edit_shortcut(true, b'A' as u32),
            Some(EditShortcut::SelectAll)
        );
        assert_eq!(edit_shortcut(true, b'C' as u32), Some(EditShortcut::Copy));
        assert_eq!(edit_shortcut(true, b'X' as u32), Some(EditShortcut::Cut));
        assert_eq!(edit_shortcut(true, b'V' as u32), Some(EditShortcut::Paste));
        assert_eq!(edit_shortcut(false, b'A' as u32), None);
        assert_eq!(edit_shortcut(true, b'Z' as u32), None);
    }

    #[test]
    fn terminal_selection_extracts_forward_reverse_and_wide_text() {
        let mut parser = vt100::Parser::new(3, 8, 0);
        parser.process(b"alpha\r\nbeta");
        let forward = TerminalSelection {
            tab_id: 1,
            anchor: TerminalPoint { row: 0, col: 1 },
            focus: TerminalPoint { row: 1, col: 2 },
            dragging: false,
            moved: true,
        };
        let reverse = TerminalSelection {
            anchor: forward.focus,
            focus: forward.anchor,
            ..forward
        };
        assert_eq!(
            terminal_selection_text(parser.screen(), forward),
            "lpha\r\nbet"
        );
        assert_eq!(
            terminal_selection_text(parser.screen(), reverse),
            "lpha\r\nbet"
        );

        let mut wide_parser = vt100::Parser::new(1, 8, 0);
        wide_parser.process("你A".as_bytes());
        let wide = TerminalSelection {
            tab_id: 1,
            anchor: TerminalPoint { row: 0, col: 0 },
            focus: TerminalPoint { row: 0, col: 2 },
            dragging: false,
            moved: true,
        };
        assert_eq!(terminal_selection_text(wide_parser.screen(), wide), "你A");
    }

    #[test]
    fn ctrl_c_copies_only_an_active_terminal_selection() {
        assert!(terminal_copy_shortcut(true, b'C' as u32, true, true));
        assert!(!terminal_copy_shortcut(false, b'C' as u32, true, true));
        assert!(!terminal_copy_shortcut(true, b'C' as u32, true, false));
        assert!(!terminal_copy_shortcut(true, b'C' as u32, false, true));
        assert!(!terminal_copy_shortcut(true, b'V' as u32, true, true));
    }

    #[test]
    fn terminal_paste_normalizes_lines_and_filters_unsafe_controls() {
        assert_eq!(
            normalize_terminal_paste("one\r\ntwo\nthree\rfour\t\u{1b}[31m\0"),
            "one\rtwo\rthree\rfour\t[31m"
        );
    }

    #[test]
    fn gui_cli_guidance_preserves_arguments_and_names_the_real_cli() {
        let guidance = gui_cli_guidance(&[
            "list-windows".to_owned(),
            "-F".to_owned(),
            "#{window_id} #{window_name}".to_owned(),
        ]);
        assert!(guidance.contains("No CLI command was executed"));
        assert!(
            guidance.contains("agenterm-cli.exe list-windows -F \"#{window_id} #{window_name}\"")
        );
        assert!(guidance.contains("Launcher PID:"));
        assert!(guidance.contains("Configured server address:"));
        assert!(guidance.contains("agenterm-cli.exe server-list"));
        assert!(guidance.contains("agenterm-cli.exe -h"));
    }
}
