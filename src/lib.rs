use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env,
    ffi::c_void,
    fs::OpenOptions,
    io::{Read, Write},
    mem, ptr,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context as _, Result};
use rmux_pty::TerminalSize;
use windows_sys::Win32::{
    Foundation::{
        COLORREF, GlobalFree, HINSTANCE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, POINT, RECT,
        SIZE, WPARAM,
    },
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CLEARTYPE_QUALITY,
        CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
        CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_GUI_FONT, DIB_RGB_COLORS, DT_CENTER,
        DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW,
        Ellipse, EndPaint, ExtTextOutW, FF_MODERN, FIXED_PITCH, FW_NORMAL, FillRect, FrameRect,
        GetDC, GetDIBits, GetDeviceCaps, GetStockObject, GetTextExtentPoint32W, GetTextFaceW,
        GetTextMetricsW, GetWindowDC, HDC, HFONT, HGDIOBJ, InvalidateRect, LOGPIXELSY, LineTo,
        MoveToEx, NULL_BRUSH, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, ReleaseDC, SRCCOPY,
        SYSTEM_FIXED_FONT, ScreenToClient, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW,
        TRANSPARENT, UpdateWindow,
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
    UI::Input::KeyboardAndMouse::{
        GetCapture, GetDoubleClickTime, GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus,
    },
    UI::Shell::ShellExecuteW,
    UI::WindowsAndMessaging::{
        CREATESTRUCTW, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CheckMenuItem,
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOVSCROLL,
        ES_MULTILINE, ES_WANTRETURN, EnableMenuItem, GWLP_USERDATA, GetClientRect, GetCursorPos,
        GetForegroundWindow, GetMessageW, GetSystemMenu, GetWindowLongPtrW, GetWindowRect,
        GetWindowTextLengthW, GetWindowTextW, IDC_ARROW, IDC_SIZEWE, InsertMenuW, IsIconic,
        IsWindowVisible, IsZoomed, LoadCursorW, LoadIconW, MB_ICONERROR, MB_OK, MF_BYCOMMAND,
        MF_CHECKED, MF_ENABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, MessageBoxW,
        MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW, SC_CLOSE, SIZE_MINIMIZED,
        SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWNOACTIVATE,
        SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
        SendMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos,
        SetWindowTextW, ShowWindow, TranslateMessage, WM_ACTIVATEAPP, WM_APP, WM_CAPTURECHANGED,
        WM_CHAR, WM_CLOSE, WM_COMMAND, WM_COPY, WM_CREATE, WM_CUT, WM_DESTROY, WM_ENDSESSION,
        WM_ERASEBKGND, WM_INITMENUPOPUP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_PASTE,
        WM_QUERYENDSESSION, WM_RBUTTONDOWN, WM_SETCURSOR, WM_SETFOCUS, WM_SIZE, WM_SYSCOMMAND,
        WM_TIMER, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
        WS_VISIBLE,
    },
};

mod build_identity;
mod commands;
mod control_contract;
mod event_journal;
mod instances;
mod ipc_transport;
pub mod operations;
mod protocol;
mod rmux_status;
mod script_audit;
pub mod script_protocol;
mod settings;
mod tab_tree;
mod terminal_lifecycle;
mod terminal_observation;
mod terminal_runtime;
mod terminal_selection;
mod theme;
mod ui_geometry;
mod upgrade_identity;
mod wake_signal;
mod worker_supervisor;
mod working_context;
mod workspace;

use build_identity::BuildIdentity;
use commands::{
    BACKSPACE_INPUT, COMMAND_CATALOG, COMMAND_CATALOG_SCHEMA_VERSION, MUX_COMMANDS, MuxStatus,
    canonical_control_command, control_command_requests_help, control_command_usage, has_option,
    last_positional, mux_command, option_value, parse_new_command, parse_tab_environment,
    positional_values, screenshot_output_path, snapshot_modal_matches, supported_commands,
    tmux_key_bytes, validate_control_command,
};
use control_contract::{
    Admission, ControlError, ControlReceipt, ControlRequest, ErrorCategory,
    EventPosition as ControlEventPosition, OperationId, PayloadFingerprint, ReceiptOutcome,
    ReplayWindow, RequestId, RequestIntent, ResolvedTarget, WaitCondition, WaitDescriptor,
};
use event_journal::{EVENT_CATALOG, EVENT_CATALOG_SCHEMA_VERSION, EventJournal, EventKind};
use instances::{
    InstanceRegistration, discover_instances, instance_process_is_alive, prune_instance,
    register_instance,
};
use ipc_transport::{IpcEnvelope, read_bounded_ipc_line, start_ipc_server};
use operations::{
    OPERATION_CATALOG, OPERATION_CATALOG_SCHEMA_VERSION, OperationClass, OperationSpec,
    UI_TABS_HIDE, UI_TABS_SET_WIDTH, UI_TABS_SHOW, UI_TABS_TOGGLE, operation_for_args,
    validate_operation_args,
};
use protocol::{IpcRequest, IpcResponse};
use script_audit::{
    AuditBudgets, AuditInvocation, AuditOutcome, AuditSourceKind, ScriptAuditSink,
    source_fingerprint,
};
use script_protocol::{
    SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, ScriptBrokerError, ScriptBrokerRequest,
    ScriptBrokerResponse, ScriptBudgets, ScriptExitClass, ScriptInvocation, ScriptOperation,
    ScriptProfile,
};
use settings::{AppConfig, config_path, load_config, save_config};
use tab_tree::{TabTreeNode, TabTreeRow, tree_rows, would_create_cycle};
use terminal_observation::TerminalProcessState;
use terminal_runtime::{TerminalLaunch, TerminalTab};
use terminal_selection::{
    AutoScrollDirection, AutoScrollStep, SelectionGesture, TerminalPoint, TerminalSelection,
    autoscroll_step, terminal_selection_text, visible_row_selection, word_selection,
};
use theme::{ThemeId, ThemePalette};
use ui_geometry::{
    PixelRect, TAB_HEIGHT, TERMINAL_SCROLLBAR_WIDTH, TerminalScrollbarGeometry,
    TreeRowActionDensity, TreeRowMode, WorkspaceLayout, WorkspaceLayoutInput, reset_tabs_width,
    scrollback_for_thumb_top, tabs_width_from_drag, terminal_scrollbar_geometry, tree_connector_x,
    tree_row_at_y, tree_row_geometry_for_mode, workspace_layout,
};
use upgrade_identity::UpgradeIdentity;
use wake_signal::WakeSignal;
use worker_supervisor::{SupervisorError, WorkerSupervisor};
use working_context::{
    CwdSource, PROXY_MAX_BYTES, ProxyState, cwd_command, parse_proxy_editor, proxy_command,
    validate_path,
};
use workspace::{SavedTab, SavedWorkspace, load_workspace, save_workspace, workspace_path};

const APP_NAME: &str = "AgenTerm";
const INITIAL_ROWS: u16 = 30;
const INITIAL_COLS: u16 = 100;
const SCROLLBACK_LINES: usize = 10_000;
const COMPOSER_HEIGHT: i32 = 78;
const STATUS_BAR_HEIGHT: i32 = 26;
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
const TABS_BUTTON_ID: usize = 1011;
const TAB_NAME_EDIT_ID: usize = 1012;
const TAB_NOTE_EDIT_ID: usize = 1013;
const TIMER_ID: usize = 1;
const WM_APP_WAKE: u32 = WM_APP + 1;
const WM_APP_AUTOMATION_SHORTCUT: u32 = WM_APP + 2;
const AUTOMATION_MOD_CONTROL: u32 = 1;
const AUTOMATION_MOD_SHIFT: u32 = 1 << 1;
const AUTOMATION_MOD_ALT: u32 = 1 << 2;
const AUTOMATION_KEY_UP: u32 = 1 << 3;
const AUTOMATION_KEY_REPEAT: u32 = 1 << 4;
const SYSTEM_MENU_COPY_ID: usize = 0x1f00;
const SYSTEM_MENU_PASTE_ID: usize = 0x1f10;
const SYSTEM_MENU_TOGGLE_TABS_ID: usize = 0x1f20;
const WINDOW_CLOSE_BUTTON_TEXT_FORMAT: u32 = DT_CENTER | DT_SINGLELINE | DT_VCENTER;
const CLIPBOARD_UNICODE_TEXT: u32 = 13;
const TERMINAL_PASTE_LIMIT_BYTES: usize = 256 * 1024;
const CAPTURE_PUBLIC_MAX_BYTES: usize = 1024 * 1024;

fn bounded_utf8_prefix(value: &str, maximum: usize) -> &str {
    let mut take = maximum.min(value.len());
    while take > 0 && !value.is_char_boundary(take) {
        take -= 1;
    }
    &value[..take]
}
const IPC_TIMEOUT: Duration = Duration::from_secs(5);
const IPC_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(500);
const IPC_AUTOSTART_TIMEOUT: Duration = Duration::from_secs(15);
const IPC_AUTOSTART_POLL: Duration = Duration::from_millis(100);
const IPC_MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const IPC_REQUESTS_PER_TICK: usize = 16;
const CONTROL_REPLAY_CAPACITY: usize = 512;
const WHEEL_DELTA: i32 = 120;
const WHEEL_ROWS_PER_NOTCH: usize = 3;

fn request_gui_wake(wake_window: isize, wake_signal: &WakeSignal) {
    if wake_signal.request() {
        unsafe {
            PostMessageW(wake_window as HWND, WM_APP_WAKE, 0, 0);
        }
    }
}
const UI_LOCALE: &str = "en-US";
const LABEL_SEND: &str = "Send";
const LABEL_SETTINGS: &str = "Settings";
const LABEL_NEW: &str = "New";
const LABEL_TABS: &str = "Tabs";
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

struct PendingControlReceipt {
    request: ControlRequest,
    receipt: ControlReceipt,
}

pub fn run_gui_entry() -> i32 {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| !argument.starts_with("--"))
    {
        write_best_effort_stderr(&gui_cli_guidance(&arguments));
        return 2;
    }
    let launch_options = match configure_gui_launch(&arguments) {
        Ok(options) => options,
        Err(error) => {
            write_best_effort_stderr(&format!(
                "AgenTerm GUI argument error: {error:#}\n\
             No GUI server was started by this invocation.\n\
             More CLI commands: agenterm-cli.exe -h"
            ));
            return 2;
        }
    };
    let no_activate = launch_options.no_activate || no_activate_from_environment();
    write_best_effort_stderr(&gui_console_summary(&ipc_address()));
    if env::var_os("AGENTERM_SERVER").is_none() {
        let handoff = if no_activate {
            "__show-no-activate"
        } else {
            "__focus"
        };
        match send_ipc_request(vec![handoff.to_owned()]) {
            Ok(response) if gui_handoff_succeeded(&response) => return 0,
            Ok(response) => {
                write_best_effort_stderr(&format!(
                    "The running AgenTerm server rejected the launcher handoff: {}\n\
                     Restart that server to use this launcher capability.",
                    response.error
                ));
                return 1;
            }
            Err(_) => {}
        }
    }

    if let Err(error) = run_gui(no_activate) {
        show_startup_error(&error);
        return 1;
    }
    0
}

fn no_activate_from_environment() -> bool {
    no_activate_from_value(env::var_os("AGENTERM_NO_ACTIVATE").as_deref())
}

fn no_activate_from_value(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| {
        let value = value.to_string_lossy();
        !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GuiLaunchOptions {
    no_activate: bool,
}

fn configure_gui_launch(arguments: &[String]) -> Result<GuiLaunchOptions> {
    let (options, address) = parse_gui_launch(arguments)?;
    IPC_ADDRESS_OVERRIDE.with(|override_address| {
        *override_address.borrow_mut() = address;
    });
    Ok(options)
}

fn gui_handoff_succeeded(response: &IpcResponse) -> bool {
    response.ok
}

fn parse_gui_launch(arguments: &[String]) -> Result<(GuiLaunchOptions, Option<String>)> {
    let mut options = GuiLaunchOptions::default();
    let mut address = None;
    let mut position = 0;
    while position < arguments.len() {
        match arguments[position].as_str() {
            "--no-activate" | "--not-foreground" => {
                if options.no_activate {
                    anyhow::bail!(
                        "agenterm.exe --no-activate/--not-foreground may be specified only once"
                    );
                }
                options.no_activate = true;
                position += 1;
            }
            "--address" => {
                if address.is_some() {
                    anyhow::bail!("agenterm.exe --address may be specified only once");
                }
                let value = arguments
                    .get(position + 1)
                    .context("agenterm.exe --address requires HOST:PORT")?;
                if value.starts_with("--") {
                    anyhow::bail!("agenterm.exe --address requires HOST:PORT");
                }
                parse_loopback_ipc_address(value)?;
                address = Some(value.clone());
                position += 2;
            }
            argument => anyhow::bail!("unsupported AgenTerm GUI argument: {argument}"),
        }
    }
    Ok((options, address))
}

fn show_window_behind_foreground(window: HWND) {
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
    }
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

#[derive(Default)]
struct CliControlOptions {
    request_id: Option<String>,
    deadline_ms: Option<u64>,
    receipt_json: bool,
}

pub fn run_cli_entry() -> i32 {
    let mut arguments: Vec<String> = env::args().skip(1).collect();
    let mut control_options = CliControlOptions::default();
    loop {
        match arguments.first().map(String::as_str) {
            Some("--address") => {
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
            Some("--request-id") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-cli --request-id requires an ID");
                    return 2;
                }
                arguments.remove(0);
                let value = arguments.remove(0);
                if let Err(error) = RequestId::new(value.clone()) {
                    eprintln!("{error}");
                    return 2;
                }
                control_options.request_id = Some(value);
            }
            Some("--deadline-ms") => {
                if arguments.len() < 2 {
                    eprintln!("agenterm-cli --deadline-ms requires milliseconds");
                    return 2;
                }
                arguments.remove(0);
                let value = arguments.remove(0);
                match value.parse::<u64>() {
                    Ok(value) if (1..=60_000).contains(&value) => {
                        control_options.deadline_ms = Some(value);
                    }
                    _ => {
                        eprintln!("agenterm-cli --deadline-ms must be from 1 to 60000");
                        return 2;
                    }
                }
            }
            Some("--receipt-json") => {
                arguments.remove(0);
                control_options.receipt_json = true;
            }
            _ => break,
        }
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
    run_cli(arguments, control_options)
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
    run_cli(arguments, CliControlOptions::default())
}

fn run_gui(no_activate: bool) -> Result<()> {
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
    window_class.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
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
    let tabs_button = unsafe {
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(LABEL_TABS).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            54,
            30,
            window,
            TABS_BUTTON_ID as *mut c_void,
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
    let tab_name_edit = create_hidden_edit(window, instance, TAB_NAME_EDIT_ID);
    let tab_note_edit = create_hidden_edit(window, instance, TAB_NOTE_EDIT_ID);
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
        || tabs_button.is_null()
        || settings_button.is_null()
        || new_button.is_null()
        || settings_font.is_null()
        || settings_size.is_null()
        || settings_dark.is_null()
        || settings_light.is_null()
        || settings_cancel.is_null()
        || settings_apply.is_null()
        || tab_name_edit.is_null()
        || tab_note_edit.is_null()
    {
        unsafe { DestroyWindow(window) };
        anyhow::bail!("failed to create native controls");
    }

    let state = Box::new(AppState::new(
        window,
        NativeControls {
            edit,
            send_button,
            tabs_button,
            settings_button,
            new_button,
            settings_font,
            settings_size,
            settings_dark,
            settings_light,
            settings_cancel,
            settings_apply,
            tab_name_edit,
            tab_note_edit,
        },
    )?);
    unsafe {
        SetWindowLongPtrW(window, GWLP_USERDATA, Box::into_raw(state) as isize);
        SetTimer(window, TIMER_ID, 100, None);
    }
    if let Some(state) = state_mut(window) {
        state.layout();
        state.load_active_composer();
        state.refresh_system_menu();
    }
    unsafe {
        if no_activate {
            show_window_behind_foreground(window);
        } else {
            ShowWindow(window, SW_SHOW);
        }
        UpdateWindow(window);
    }

    let mut message: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) } > 0 {
        if let Some(state) = state_mut(window) {
            if message.message == WM_KEYDOWN
                && state.handle_shortcut(message.wParam as u32, message.lParam)
            {
                continue;
            }
            if message.message == WM_KEYUP {
                state.handle_shortcut_key_up(message.wParam as u32);
            }
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
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
            if let Some(state) = state_mut(window) {
                state.wake_signal.begin_drain();
                if state.tick() {
                    unsafe { InvalidateRect(window, ptr::null(), 0) };
                }
            }
            0
        }
        WM_SIZE => {
            if let Some(state) = state_mut(window) {
                if wparam == SIZE_MINIMIZED as usize {
                    state.remask_proxy_credentials();
                }
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
        WM_LBUTTONDBLCLK => {
            let x = (lparam as u32 & 0xffff) as i16 as i32;
            let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
            if let Some(state) = state_mut(window) {
                state.left_button_double_click(x, y);
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
        WM_CAPTURECHANGED => {
            if let Some(state) = state_mut(window) {
                state.terminal_selection_capture_lost();
            }
            0
        }
        WM_SETCURSOR => {
            if let Some(state) = state_mut(window)
                && state.set_resize_cursor_if_needed()
            {
                1
            } else {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
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
            let command_id = wparam & 0xffff;
            if let Some(state) = state_mut(window)
                && (state.cwd_edit_target.is_some() || state.proxy_edit_target.is_some())
                && command_id != BUTTON_ID
                && command_id != EDIT_ID
            {
                unsafe { SetFocus(state.edit) };
                return 0;
            }
            if command_id == BUTTON_ID {
                if let Some(state) = state_mut(window) {
                    if state.proxy_edit_target.is_some() {
                        if let Err(error) = state.prepare_proxy(None, None) {
                            state.last_error = Some(format!("{error:#}"));
                            unsafe { InvalidateRect(window, ptr::null(), 0) };
                        }
                    } else if state.cwd_edit_target.is_some() {
                        if let Err(error) =
                            state.prepare_cwd(None, None, ComposerWriteMode::EmptyOnly)
                        {
                            state.last_error = Some(format!("{error:#}"));
                            unsafe { InvalidateRect(window, ptr::null(), 0) };
                        }
                    } else {
                        state.send_composer();
                    }
                }
                0
            } else if command_id == TABS_BUTTON_ID {
                if let Some(state) = state_mut(window) {
                    state.set_tabs_visible(false, "button", UI_TABS_HIDE);
                }
                0
            } else if command_id == SETTINGS_BUTTON_ID {
                if let Some(state) = state_mut(window) {
                    state.open_settings();
                }
                0
            } else if command_id == NEW_BUTTON_ID {
                if let Some(state) = state_mut(window) {
                    if let Err(error) = state.create_tab(None, Vec::new(), true) {
                        state.last_error = Some(format!("{error:#}"));
                    }
                    unsafe { InvalidateRect(window, ptr::null(), 0) };
                }
                0
            } else if command_id == SETTINGS_APPLY_ID {
                if let Some(state) = state_mut(window) {
                    state.apply_settings_from_controls();
                }
                0
            } else if command_id == SETTINGS_DARK_ID {
                if let Some(state) = state_mut(window) {
                    state.preview_theme(ThemeId::Dark);
                }
                0
            } else if command_id == SETTINGS_LIGHT_ID {
                if let Some(state) = state_mut(window) {
                    state.preview_theme(ThemeId::Light);
                }
                0
            } else if command_id == SETTINGS_CANCEL_ID {
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
        WM_APP_AUTOMATION_SHORTCUT => {
            if !no_activate_from_environment() {
                return unsafe { DefWindowProcW(window, message, wparam, lparam) };
            }
            let flags = lparam as u32;
            let Some(state) = state_mut(window) else {
                return 0;
            };
            if flags & AUTOMATION_KEY_UP != 0 {
                state.handle_shortcut_key_up(wparam as u32);
                return 1;
            }
            let shortcut_lparam = if flags & AUTOMATION_KEY_REPEAT != 0 {
                1_isize << 30
            } else {
                0
            };
            state.handle_shortcut_with_modifiers(
                wparam as u32,
                shortcut_lparam,
                flags & AUTOMATION_MOD_CONTROL != 0,
                flags & AUTOMATION_MOD_SHIFT != 0,
                flags & AUTOMATION_MOD_ALT != 0,
            ) as LRESULT
        }
        WM_INITMENUPOPUP => {
            if let Some(state) = state_mut(window) {
                state.refresh_system_menu();
            }
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_SYSCOMMAND => match wparam & 0xfff0 {
            SYSTEM_MENU_TOGGLE_TABS_ID => {
                if let Some(state) = state_mut(window) {
                    if state.cwd_edit_target.is_some() || state.proxy_edit_target.is_some() {
                        unsafe { SetFocus(state.edit) };
                    } else {
                        state.set_tabs_visible(
                            !state.config.tabs_visible,
                            "system-menu",
                            UI_TABS_TOGGLE,
                        );
                    }
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
        WM_SETFOCUS => 0,
        WM_ACTIVATEAPP => {
            if wparam == 0
                && let Some(state) = state_mut(window)
            {
                state.remask_proxy_credentials();
            }
            0
        }
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

struct NativeControls {
    edit: HWND,
    send_button: HWND,
    tabs_button: HWND,
    settings_button: HWND,
    new_button: HWND,
    settings_font: HWND,
    settings_size: HWND,
    settings_dark: HWND,
    settings_light: HWND,
    settings_cancel: HWND,
    settings_apply: HWND,
    tab_name_edit: HWND,
    tab_note_edit: HWND,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditShortcut {
    SelectAll,
    Copy,
    Cut,
    Paste,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostFocusSurface {
    Terminal,
    Tabs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusSurface {
    Terminal,
    Composer,
    Tabs,
    Settings,
    NoteEditor,
    CwdEditor,
    ProxyEditor,
}

impl FocusSurface {
    fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Composer => "composer",
            Self::Tabs => "tabs",
            Self::Settings => "settings",
            Self::NoteEditor => "note-editor",
            Self::CwdEditor => "cwd-editor",
            Self::ProxyEditor => "proxy-editor",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComposerWriteMode {
    EmptyOnly,
    Append,
    Replace,
}

impl ComposerWriteMode {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("empty") {
            "empty" => Ok(Self::EmptyOnly),
            "append" => Ok(Self::Append),
            "replace" => Ok(Self::Replace),
            other => anyhow::bail!("unknown composer write mode: {other}"),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyOnly => "empty",
            Self::Append => "append",
            Self::Replace => "replace",
        }
    }
}

fn surface_navigation(
    source: FocusSurface,
    control: bool,
    shift: bool,
    alt: bool,
    virtual_key: u32,
) -> Option<FocusSurface> {
    if !control || shift || alt {
        return None;
    }
    match (source, virtual_key) {
        (FocusSurface::Terminal, 0x28) => Some(FocusSurface::Composer),
        (FocusSurface::Composer, 0x26) => Some(FocusSurface::Terminal),
        (FocusSurface::Terminal, 0x25) => Some(FocusSurface::Tabs),
        (FocusSurface::Tabs, 0x27) => Some(FocusSurface::Terminal),
        _ => None,
    }
}

fn is_latched_navigation_repeat(latch: Option<u32>, virtual_key: u32, lparam: LPARAM) -> bool {
    latch == Some(virtual_key) && (lparam as usize & (1usize << 30)) != 0
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

#[derive(Clone, Copy, Debug)]
struct TabsResizeDrag {
    original_width: u16,
}

#[derive(Clone, Copy, Debug)]
struct TerminalDoubleClick {
    tab_id: u64,
    point: TerminalPoint,
    expires_at: Instant,
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
    tabs_button: HWND,
    settings_button: HWND,
    new_button: HWND,
    settings_font: HWND,
    settings_size: HWND,
    settings_dark: HWND,
    settings_light: HWND,
    settings_cancel: HWND,
    settings_apply: HWND,
    tab_name_edit: HWND,
    tab_note_edit: HWND,
    tabs: Vec<TerminalTab>,
    collapsed_tabs: HashSet<u64>,
    active: Option<u64>,
    next_id: u64,
    session_name: String,
    started_at: SystemTime,
    event_journal: EventJournal,
    wake_signal: Arc<WakeSignal>,
    replay_window: ReplayWindow,
    pending_control_receipts: HashMap<u64, PendingControlReceipt>,
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
    cwd_edit_target: Option<u64>,
    proxy_edit_target: Option<u64>,
    proxy_credentials_revealed: bool,
    proxy_endpoint_visible: HashSet<u64>,
    settings_open: bool,
    settings_theme_draft: ThemeId,
    host_focus_surface: HostFocusSurface,
    navigation_latch: Option<u32>,
    config: AppConfig,
    terminal_font: HFONT,
    terminal_font_owned: bool,
    resolved_font_family: String,
    wheel_remainder: i32,
    scroll_drag: Option<ScrollDrag>,
    tabs_resize_drag: Option<TabsResizeDrag>,
    terminal_selection: Option<TerminalSelection>,
    terminal_selection_gesture: Option<SelectionGesture>,
    terminal_selection_pointer: Option<(i32, i32)>,
    terminal_selection_autoscroll: Option<AutoScrollStep>,
    terminal_double_click: Option<TerminalDoubleClick>,
    _instance_registration: InstanceRegistration,
}

impl AppState {
    fn new(window: HWND, controls: NativeControls) -> Result<Self> {
        let wake_signal = Arc::new(WakeSignal::new());
        let ipc_receiver = start_ipc_server(window, Arc::clone(&wake_signal))?;
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
            tabs_button: controls.tabs_button,
            settings_button: controls.settings_button,
            new_button: controls.new_button,
            settings_font: controls.settings_font,
            settings_size: controls.settings_size,
            settings_dark: controls.settings_dark,
            settings_light: controls.settings_light,
            settings_cancel: controls.settings_cancel,
            settings_apply: controls.settings_apply,
            tab_name_edit: controls.tab_name_edit,
            tab_note_edit: controls.tab_note_edit,
            tabs: Vec::new(),
            collapsed_tabs,
            active: active_id,
            next_id,
            session_name,
            started_at: SystemTime::now(),
            event_journal: EventJournal::new(),
            wake_signal: Arc::clone(&wake_signal),
            replay_window: ReplayWindow::new(CONTROL_REPLAY_CAPACITY),
            pending_control_receipts: HashMap::new(),
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
            cwd_edit_target: None,
            proxy_edit_target: None,
            proxy_credentials_revealed: false,
            proxy_endpoint_visible: HashSet::new(),
            settings_open: false,
            settings_theme_draft: config.color_theme,
            host_focus_surface: HostFocusSurface::Terminal,
            navigation_latch: None,
            config,
            terminal_font,
            terminal_font_owned,
            resolved_font_family,
            wheel_remainder: 0,
            scroll_drag: None,
            tabs_resize_drag: None,
            terminal_selection: None,
            terminal_selection_gesture: None,
            terminal_selection_pointer: None,
            terminal_selection_autoscroll: None,
            terminal_double_click: None,
            _instance_registration: instance_registration,
        };

        let wake_window = window as isize;
        let startup_wake_signal = wake_signal;
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
                    wake_signal: Arc::clone(&startup_wake_signal),
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
                request_gui_wake(wake_window, &startup_wake_signal);
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
            .commit(EventKind::WorkspaceSaved, None, serde_json::json!({}));
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
        if self.proxy_edit_target.is_some() {
            self.close_proxy_editor();
        }
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
            wake_signal: Arc::clone(&self.wake_signal),
            initial_size: TerminalSize { rows, cols },
        })?;
        self.tabs.push(tab);
        self.tabs.sort_by_key(|tab| tab.index);
        if select {
            self.finish_note_edit(false);
            self.cancel_terminal_selection(true);
        }
        self.event_journal.commit(
            EventKind::TabCreated,
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
                .commit(EventKind::TabSelected, Some(id), serde_json::json!({}));
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
            EventKind::TabParent,
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

    fn close_tab(&mut self, id: u64) -> bool {
        self.cancel_terminal_selection(true);
        self.finish_note_edit(false);
        if self.cwd_edit_target == Some(id) {
            self.close_cwd_editor();
        }
        if self.proxy_edit_target == Some(id) {
            self.close_proxy_editor();
        }
        self.save_active_composer();
        let Some(position) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
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
        let terminal_shutdown_complete = self.tabs[position].close_process();
        self.tabs.remove(position);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(position)
                .or_else(|| position.checked_sub(1).and_then(|i| self.tabs.get(i)))
                .map(|tab| tab.id);
        }
        self.event_journal.commit(
            EventKind::TabClosed,
            Some(id),
            serde_json::json!({
                "index": index,
                "parent_id": parent_id,
                "exit_code": exit_code,
                "promoted_children": promoted_children,
                "active_id": self.active,
                "terminal_shutdown_complete": terminal_shutdown_complete,
            }),
        );
        self.load_active_composer();
        terminal_shutdown_complete
    }

    fn request_close_tab(&mut self, id: u64) {
        self.cancel_terminal_selection(true);
        self.finish_note_edit(false);
        if self.proxy_edit_target.is_some() {
            self.close_proxy_editor();
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.exited.is_some() {
            self.close_tab(id);
            return;
        }
        self.pending_close = Some(id);
        self.navigation_latch = None;
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
        self.cancel_terminal_selection(true);
        self.finish_note_edit(false);
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
        if self.cwd_edit_target.is_some() {
            self.close_cwd_editor();
        }
        if self.proxy_edit_target.is_some() {
            self.close_proxy_editor();
        }
        self.save_active_composer();
        self.navigation_latch = None;
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
                // Closing the parent window hides the native composer/settings controls
                // while the confirmation surface is active. Restore the selected child
                // surface before detaching so a later launcher handoff does not re-show
                // a parent whose input controls are still individually hidden.
                self.restore_window_close_controls();
                self.detached_was_maximized = unsafe { IsZoomed(self.window) } != 0;
                self.window_detached = true;
                self.navigation_latch = None;
                self.event_journal.commit(
                    EventKind::WindowVisibility,
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
                    EventKind::WorkspaceShutdown,
                    None,
                    serde_json::json!({"saved": true, "source": "window-close"}),
                );
                self.close_requested = true;
                unsafe { PostMessageW(self.window, WM_CLOSE, 0, 0) };
            }
            WindowCloseChoice::Cancel => self.restore_after_window_close_modal(),
        }
    }

    fn restore_window_close_controls(&mut self) {
        if self.window_close_restore_settings {
            self.settings_open = true;
        }
        unsafe {
            if self.settings_open {
                ShowWindow(self.edit, SW_HIDE);
                ShowWindow(self.send_button, SW_HIDE);
                ShowWindow(self.settings_font, SW_SHOW);
                ShowWindow(self.settings_size, SW_SHOW);
                ShowWindow(self.settings_dark, SW_SHOW);
                ShowWindow(self.settings_light, SW_SHOW);
                ShowWindow(self.settings_cancel, SW_SHOW);
                ShowWindow(self.settings_apply, SW_SHOW);
            } else {
                ShowWindow(self.settings_font, SW_HIDE);
                ShowWindow(self.settings_size, SW_HIDE);
                ShowWindow(self.settings_dark, SW_HIDE);
                ShowWindow(self.settings_light, SW_HIDE);
                ShowWindow(self.settings_cancel, SW_HIDE);
                ShowWindow(self.settings_apply, SW_HIDE);
                ShowWindow(self.edit, SW_SHOW);
                ShowWindow(self.send_button, SW_SHOW);
            }
        }
        self.window_close_restore_settings = false;
    }

    fn restore_after_window_close_modal(&mut self) {
        let restore_settings = self.window_close_restore_settings;
        self.restore_window_close_controls();
        unsafe {
            if restore_settings {
                SetFocus(self.settings_font);
            } else if self.note_edit_target.is_some() {
                SetFocus(self.edit);
            } else {
                if self.window_close_previous_focus.is_null() {
                    SetFocus(self.window);
                } else {
                    SetFocus(self.window_close_previous_focus);
                }
            }
            InvalidateRect(self.window, ptr::null(), 0);
        }
        if !restore_settings && self.note_edit_target.is_none() {
            self.load_active_composer();
        }
    }

    fn reattach_window(&mut self, reason: &str) {
        let was_detached = self.window_detached || unsafe { IsWindowVisible(self.window) } == 0;
        self.window_detached = false;
        self.host_focus_surface = HostFocusSurface::Terminal;
        self.navigation_latch = None;
        if was_detached {
            self.restore_window_close_controls();
        }
        if no_activate_from_environment() {
            unsafe {
                ShowWindow(self.window, SW_SHOWNOACTIVATE);
                show_window_behind_foreground(self.window);
                InvalidateRect(self.window, ptr::null(), 0);
            }
        } else {
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
                if self.settings_open {
                    SetFocus(self.settings_font);
                } else if self.note_edit_target.is_some() {
                    SetFocus(self.edit);
                } else {
                    SetFocus(self.window);
                }
                InvalidateRect(self.window, ptr::null(), 0);
            }
        }
        if was_detached {
            self.event_journal.commit(
                EventKind::WindowVisibility,
                None,
                serde_json::json!({"visible": true, "reason": reason}),
            );
        }
    }

    fn show_window_no_activate(&mut self, reason: &str) {
        let was_detached = self.window_detached || unsafe { IsWindowVisible(self.window) } == 0;
        if was_detached {
            self.window_detached = false;
            self.navigation_latch = None;
            self.restore_window_close_controls();
            unsafe {
                show_window_behind_foreground(self.window);
                InvalidateRect(self.window, ptr::null(), 0);
            }
            self.event_journal.commit(
                EventKind::WindowVisibility,
                None,
                serde_json::json!({
                    "visible": true,
                    "reason": reason,
                    "activated": false,
                }),
            );
        }
    }

    fn tick(&mut self) -> bool {
        let mut changed = false;
        if self.terminal_selection_gesture.is_some_and(|gesture| {
            gesture.active()
                && (self.active != Some(gesture.tab_id())
                    || !self.tabs.iter().any(|tab| tab.id == gesture.tab_id()))
        }) {
            changed |= self.cancel_terminal_selection(true);
        }
        changed |= self.tick_terminal_selection_autoscroll();
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
                            EventKind::TabCreated,
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
        let mut completed_submissions = Vec::new();
        for tab in &mut self.tabs {
            let observation_before = tab.observation();
            let cwd_before = tab.cwd.clone();
            changed |= tab.poll();
            let mut observation_after = tab.observation();
            match observation_before.delta_to(&observation_after) {
                Ok(delta) => {
                    if delta.submission_finished {
                        completed_submissions.push((
                            tab.id,
                            observation_after.submission_enter_written.unwrap_or(false),
                            observation_after.finalized,
                        ));
                    }
                    if delta.output_advanced_by > 0 {
                        polled_events.push((
                            EventKind::TerminalOutput,
                            tab.id,
                            serde_json::json!({
                                "output_bytes": observation_after.output_bytes,
                                "advanced_by": delta.output_advanced_by,
                            }),
                        ));
                    }
                    if delta.process_state_changed || delta.lifecycle_changed {
                        let state = match observation_after.process_state() {
                            TerminalProcessState::Running => "running",
                            TerminalProcessState::Exited { .. } => "dead",
                            TerminalProcessState::Error { .. } => "error",
                        };
                        polled_events.push((
                            EventKind::TabState,
                            tab.id,
                            serde_json::json!({
                                "state": state,
                                "exit_code": observation_after.exit_code,
                                "error": observation_after.error,
                                "reader_closed": observation_after.reader_closed,
                                "parser_drained": observation_after.parser_drained,
                                "finalized": observation_after.finalized,
                                "became_finalized": delta.became_finalized,
                            }),
                        ));
                    }
                }
                Err(error) => {
                    tab.error = Some(error.to_string());
                    observation_after = tab.observation();
                    polled_events.push((
                        EventKind::TabState,
                        tab.id,
                        serde_json::json!({
                            "state": "error",
                            "exit_code": observation_after.exit_code,
                            "error": observation_after.error,
                            "reader_closed": observation_after.reader_closed,
                            "parser_drained": observation_after.parser_drained,
                            "finalized": observation_after.finalized,
                            "became_finalized": false,
                        }),
                    ));
                }
            }
            if tab.cwd != cwd_before {
                polled_events.push((
                    EventKind::WorkingContextCwd,
                    tab.id,
                    serde_json::json!({
                        "path": tab.cwd.path(),
                        "source": tab.cwd.source().as_str(),
                        "pending": tab.cwd.pending(),
                    }),
                ));
            }
        }
        for (kind, tab_id, payload) in polled_events {
            self.event_journal.commit(kind, Some(tab_id), payload);
        }
        for (tab_id, enter_written, terminal_finalized) in completed_submissions {
            let Some(pending) = self.pending_control_receipts.remove(&tab_id) else {
                continue;
            };
            let completion_event = self.event_journal.commit_correlated(
                EventKind::ComposerSubmissionFinished,
                Some(tab_id),
                Some(pending.request.request_id.as_str().to_owned()),
                Some(pending.request.operation_id.as_str().to_owned()),
                serde_json::json!({
                    "enter_written": enter_written,
                    "terminal_finalized": terminal_finalized,
                }),
            );
            let mut receipt = pending.receipt;
            receipt.after_position = Some(ControlEventPosition {
                epoch: completion_event.epoch,
                sequence: completion_event.sequence,
            });
            receipt.wait = None;
            if enter_written {
                receipt.outcome = ReceiptOutcome::Committed;
            } else {
                receipt.outcome = ReceiptOutcome::OutcomeUnknown;
                receipt.result = None;
                receipt.error = Some(ControlError::new(
                    "terminal_submission_incomplete",
                    ErrorCategory::Precondition,
                    "composer text was written, but the terminal did not accept the final Enter",
                    false,
                ));
            }
            if let Err(error) = self.replay_window.complete(&pending.request, receipt) {
                self.last_error = Some(format!(
                    "failed to finalize control receipt for tab @{tab_id}: {error}"
                ));
            }
        }
        let envelopes: Vec<IpcEnvelope> = self
            .ipc_receiver
            .try_iter()
            .take(IPC_REQUESTS_PER_TICK)
            .collect();
        let ipc_budget_exhausted = envelopes.len() == IPC_REQUESTS_PER_TICK;
        changed |= !envelopes.is_empty();
        for envelope in envelopes {
            let response = self.execute_ipc_request(envelope.request);
            let _ = envelope.respond_to.send(response);
        }
        if self.close_requested {
            unsafe { PostMessageW(self.window, WM_CLOSE, 0, 0) };
        }
        if self.wake_signal.rearm_if(ipc_budget_exhausted) {
            unsafe {
                PostMessageW(self.window, WM_APP_WAKE, 0, 0);
            }
        }
        changed
    }

    fn layout(&mut self) {
        let layout = self.workspace_layout();
        let sidebar_width = layout.effective_tabs_width;
        let content_bottom = layout.status.top;
        let content_width = layout.terminal.width().max(180);
        let edit_width = (content_width - 104).max(80);
        let button_left = 8;
        let button_gap = 4;
        let tabs_button_width = 52.min((sidebar_width - 16).max(0));
        let new_button_width = 50.min((sidebar_width - 16 - tabs_button_width).max(0));
        let settings_button_width =
            (sidebar_width - 16 - tabs_button_width - new_button_width - button_gap * 2).max(0);
        let tab_editor_geometry = self.note_edit_target.and_then(|target| {
            self.config.tabs_visible.then_some(())?;
            let rows = self.tree_rows();
            let position = rows.iter().position(|row| row.id == target)?;
            tree_row_geometry_for_mode(
                position,
                rows[position].depth,
                sidebar_width,
                TreeRowMode::Editing,
            )
            .editors
        });
        if self.note_edit_target.is_some() && tab_editor_geometry.is_none() {
            self.finish_note_edit(false);
        }
        unsafe {
            ShowWindow(
                self.tabs_button,
                if self.config.tabs_visible {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
            ShowWindow(
                self.settings_button,
                if self.config.tabs_visible {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
            ShowWindow(
                self.new_button,
                if self.config.tabs_visible {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
            MoveWindow(
                self.tabs_button,
                button_left,
                (content_bottom - 42).max(0),
                tabs_button_width,
                30,
                1,
            );
            MoveWindow(
                self.settings_button,
                button_left + tabs_button_width + button_gap,
                (content_bottom - 42).max(0),
                settings_button_width,
                30,
                1,
            );
            MoveWindow(
                self.new_button,
                sidebar_width - 8 - new_button_width,
                (content_bottom - 42).max(0),
                new_button_width,
                30,
                1,
            );
            MoveWindow(
                self.edit,
                layout.composer.left + 10,
                (layout.composer.top + 30).max(0),
                edit_width,
                COMPOSER_HEIGHT - 40,
                1,
            );
            MoveWindow(
                self.send_button,
                layout.composer.left + 20 + edit_width,
                (layout.composer.top + 32).max(0),
                74,
                34,
                1,
            );
            if let Some(editors) = tab_editor_geometry {
                MoveWindow(
                    self.tab_name_edit,
                    editors.name.left,
                    editors.name.top,
                    editors.name.width(),
                    editors.name.height(),
                    1,
                );
                MoveWindow(
                    self.tab_note_edit,
                    editors.note.left,
                    editors.note.top,
                    editors.note.width(),
                    editors.note.height(),
                    1,
                );
                ShowWindow(self.tab_name_edit, SW_SHOW);
                ShowWindow(self.tab_note_edit, SW_SHOW);
            } else {
                ShowWindow(self.tab_name_edit, SW_HIDE);
                ShowWindow(self.tab_note_edit, SW_HIDE);
            }
            let settings_left = layout.terminal.left + (content_width - 520) / 2;
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
        self.cancel_terminal_selection(true);
        self.finish_note_edit(false);
        if self.cwd_edit_target.is_some() {
            self.close_cwd_editor();
        }
        if self.proxy_edit_target.is_some() {
            self.close_proxy_editor();
        }
        self.save_active_composer();
        self.navigation_latch = None;
        self.settings_open = true;
        self.settings_theme_draft = self.config.color_theme;
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
        self.host_focus_surface = HostFocusSurface::Terminal;
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

    fn cancel_terminal_selection(&mut self, clear_completed: bool) -> bool {
        let previous_gesture = self.terminal_selection_gesture;
        let active = self
            .terminal_selection_gesture
            .is_some_and(SelectionGesture::active);
        if active || clear_completed {
            self.terminal_selection_gesture = self
                .terminal_selection_gesture
                .map(SelectionGesture::cancel);
        }
        let gesture_changed = self.terminal_selection_gesture != previous_gesture;
        let had_selection = clear_completed && self.terminal_selection.take().is_some();
        self.terminal_selection_pointer = None;
        self.terminal_selection_autoscroll = None;
        self.terminal_double_click = None;
        if active && unsafe { GetCapture() } == self.window {
            unsafe { ReleaseCapture() };
        }
        gesture_changed || had_selection
    }

    fn terminal_selection_capture_lost(&mut self) {
        if !self
            .terminal_selection_gesture
            .is_some_and(SelectionGesture::active)
        {
            return;
        }
        self.terminal_selection_gesture = self
            .terminal_selection_gesture
            .map(SelectionGesture::cancel);
        self.terminal_selection = None;
        self.terminal_selection_pointer = None;
        self.terminal_selection_autoscroll = None;
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn set_completed_terminal_selection(
        &mut self,
        tab_id: u64,
        start: TerminalPoint,
        end: TerminalPoint,
        rows: u16,
        cols: u16,
    ) -> bool {
        let Some(gesture) = SelectionGesture::completed(tab_id, start, end, rows, cols) else {
            return false;
        };
        self.terminal_selection = gesture.selection();
        self.terminal_selection_gesture = Some(gesture);
        self.terminal_selection_pointer = None;
        self.terminal_selection_autoscroll = None;
        true
    }

    fn tick_terminal_selection_autoscroll(&mut self) -> bool {
        let Some(step) = self.terminal_selection_autoscroll else {
            return false;
        };
        let Some(gesture) = self.terminal_selection_gesture else {
            return false;
        };
        if !gesture.active() || self.active != Some(gesture.tab_id()) {
            return self.cancel_terminal_selection(true);
        }
        let Some(position) = self.active_position() else {
            return self.cancel_terminal_selection(true);
        };
        let before = self.tabs[position].parser.screen().scrollback();
        let action = match step.direction {
            AutoScrollDirection::Up => "up",
            AutoScrollDirection::Down => "down",
        };
        let Ok(after) = self.tabs[position].scroll_viewport(action, Some(step.rows)) else {
            return false;
        };
        if let Some((x, y)) = self.terminal_selection_pointer
            && let Some((column, row)) = self.terminal_cell_at(x, y)
        {
            let (rows, cols) = self.tabs[position].last_size;
            let next = gesture.drag_to(TerminalPoint { row, col: column }, rows, cols);
            self.terminal_selection = next.selection();
            self.terminal_selection_gesture = Some(next);
        }
        before != after
    }

    fn left_button_down(&mut self, x: i32, y: i32) {
        let layout = self.workspace_layout();
        let sidebar_width = layout.effective_tabs_width;
        if layout.resize_grip.is_some_and(|grip| grip.contains(x, y))
            && !self.window_close_pending
            && self.pending_close.is_none()
            && !self.settings_open
            && self.cwd_edit_target.is_none()
            && self.proxy_edit_target.is_none()
        {
            self.begin_tabs_resize();
            return;
        }
        if layout
            .status_segments
            .tabs_recovery
            .is_some_and(|segment| segment.contains(x, y))
            && !self.window_close_pending
            && self.pending_close.is_none()
            && !self.settings_open
            && self.cwd_edit_target.is_none()
            && self.proxy_edit_target.is_none()
        {
            self.set_tabs_visible(true, "status-bar", UI_TABS_SHOW);
            return;
        }
        if layout.status_segments.cwd.contains(x, y)
            && !self.window_close_pending
            && self.pending_close.is_none()
            && !self.settings_open
            && self.cwd_edit_target.is_none()
            && self.proxy_edit_target.is_none()
        {
            if let Err(error) = self.open_cwd_editor(None) {
                self.last_error = Some(format!("{error:#}"));
            }
            return;
        }
        if layout.status_segments.proxy.contains(x, y)
            && !self.window_close_pending
            && self.pending_close.is_none()
            && !self.settings_open
            && self.cwd_edit_target.is_none()
            && self.proxy_edit_target.is_none()
        {
            if x >= layout.status_segments.proxy.right - 28 {
                if let Err(error) = self.toggle_proxy_endpoint(None) {
                    self.last_error = Some(format!("{error:#}"));
                }
            } else if let Err(error) = self.open_proxy_editor(None) {
                self.last_error = Some(format!("{error:#}"));
            }
            return;
        }
        if self.window_close_pending
            || self.pending_close.is_some()
            || self.settings_open
            || self.cwd_edit_target.is_some()
            || self.proxy_edit_target.is_some()
            || x < sidebar_width
        {
            self.cancel_terminal_selection(true);
            self.click(x, y);
            return;
        }
        self.set_focus_surface(FocusSurface::Terminal, "mouse");
        if self.click_scrollbar(x, y) {
            self.cancel_terminal_selection(true);
            return;
        }
        let Some((column, row)) = self.terminal_cell_at(x, y) else {
            return;
        };
        let Some(position) = self.active_position() else {
            return;
        };
        let point = TerminalPoint { row, col: column };
        let tab_id = self.tabs[position].id;
        let now = Instant::now();
        if self.terminal_double_click.is_some_and(|click| {
            click.tab_id == tab_id && click.point == point && now <= click.expires_at
        }) {
            self.terminal_double_click = None;
            if let Some((start, end)) =
                visible_row_selection(self.tabs[position].parser.screen(), row)
            {
                let (rows, cols) = self.tabs[position].last_size;
                self.set_completed_terminal_selection(tab_id, start, end, rows, cols);
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            }
            return;
        }
        self.terminal_double_click = None;
        let (rows, cols) = self.tabs[position].last_size;
        let Some(gesture) = SelectionGesture::prepare(tab_id, point, rows, cols) else {
            return;
        };
        self.terminal_selection = gesture.selection();
        self.terminal_selection_gesture = Some(gesture);
        self.terminal_selection_pointer = Some((x, y));
        self.terminal_selection_autoscroll = None;
        unsafe {
            SetCapture(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn left_button_up(&mut self) {
        if self.tabs_resize_drag.is_some() {
            self.finish_tabs_resize(true, "mouse-drag", UI_TABS_SET_WIDTH);
            return;
        }
        if self.scroll_drag.is_some() {
            self.end_scroll_drag();
            return;
        }
        let Some(gesture) = self.terminal_selection_gesture else {
            return;
        };
        if !gesture.active() {
            return;
        }
        let completed = gesture.complete();
        self.terminal_selection_gesture = Some(completed);
        self.terminal_selection_pointer = None;
        self.terminal_selection_autoscroll = None;
        if let Some(selection) = completed.completed_selection() {
            self.terminal_selection = Some(selection);
        } else {
            self.terminal_selection = None;
            if let Some(selection) = gesture.selection() {
                self.send_terminal_click(selection.anchor.col, selection.anchor.row);
            }
        }
        if unsafe { GetCapture() } == self.window {
            unsafe { ReleaseCapture() };
        }
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn mouse_move(&mut self, x: i32, y: i32) {
        if self.tabs_resize_drag.is_some() {
            self.drag_tabs_resize(x);
            return;
        }
        if self.scroll_drag.is_some() {
            self.drag_scrollbar(x, y);
            return;
        }
        let Some(gesture) = self.terminal_selection_gesture else {
            return;
        };
        if !gesture.active() {
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
        let position = self.active_position();
        let (rows, cols) = position
            .and_then(|position| self.tabs.get(position))
            .map(|tab| tab.last_size)
            .unwrap_or((0, 0));
        let next = gesture.drag_to(TerminalPoint { row, col: column }, rows, cols);
        let cell_height = (terminal.height() / i32::from(rows.max(1))).max(1);
        let next_autoscroll = autoscroll_step(y, terminal.top, terminal.bottom, cell_height);
        let changed = next != gesture || next_autoscroll != self.terminal_selection_autoscroll;
        self.terminal_selection = next.selection();
        self.terminal_selection_gesture = Some(next);
        self.terminal_selection_pointer = Some((clamped_x, clamped_y));
        self.terminal_selection_autoscroll = next_autoscroll;
        if changed {
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        }
    }

    fn left_button_double_click(&mut self, x: i32, y: i32) {
        if self.cwd_edit_target.is_some() || self.proxy_edit_target.is_some() {
            return;
        }
        if self
            .workspace_layout()
            .resize_grip
            .is_some_and(|grip| grip.contains(x, y))
        {
            self.reset_tabs_width("mouse-double-click");
            return;
        }
        if self.window_close_pending
            || self.pending_close.is_some()
            || self.settings_open
            || self.note_edit_target.is_some()
            || self.cwd_edit_target.is_some()
            || self.proxy_edit_target.is_some()
        {
            return;
        }
        let Some((column, row)) = self.terminal_cell_at(x, y) else {
            return;
        };
        let Some(position) = self.active_position() else {
            return;
        };
        let tab_id = self.tabs[position].id;
        let point = TerminalPoint { row, col: column };
        let Some((start, end)) = word_selection(self.tabs[position].parser.screen(), point) else {
            return;
        };
        self.cancel_terminal_selection(true);
        let (rows, cols) = self.tabs[position].last_size;
        if self.set_completed_terminal_selection(tab_id, start, end, rows, cols) {
            let timeout = Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }));
            self.terminal_double_click =
                Instant::now()
                    .checked_add(timeout)
                    .map(|expires_at| TerminalDoubleClick {
                        tab_id,
                        point,
                        expires_at,
                    });
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
        let layout = self.workspace_layout();
        let sidebar_width = layout.effective_tabs_width;
        if self.window_close_pending {
            let mut client: RECT = unsafe { mem::zeroed() };
            unsafe { GetClientRect(self.window, &mut client) };
            let left = layout.terminal.left + (layout.terminal.width() - 620) / 2;
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
            let modal_left = layout.terminal.left + (layout.terminal.width() - 460) / 2;
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
        if self.cwd_edit_target.is_some() || self.proxy_edit_target.is_some() {
            return;
        }
        if layout
            .status_segments
            .tabs_recovery
            .is_some_and(|segment| segment.contains(x, y))
        {
            self.set_tabs_visible(true, "status-bar", UI_TABS_SHOW);
            return;
        }
        if layout.status_segments.cwd.contains(x, y) {
            if let Err(error) = self.open_cwd_editor(None) {
                self.last_error = Some(format!("{error:#}"));
            }
            return;
        }
        if layout.terminal.contains(x, y) {
            self.set_focus_surface(FocusSurface::Terminal, "mouse");
            if self.click_scrollbar(x, y) {
                return;
            }
            if let Some((column, row)) = self.terminal_cell_at(x, y) {
                self.send_terminal_click(column, row);
            }
            return;
        }
        self.set_focus_surface(FocusSurface::Tabs, "mouse");
        let Some(row) = tree_row_at_y(y) else {
            return;
        };
        let Some(position) = self.tree_row_position(row) else {
            return;
        };
        let id = self.tabs[position].id;
        let tree_row = self.tree_rows().get(row).cloned();
        let has_children = self.tabs.iter().any(|tab| tab.parent_id == Some(id));
        let mode = if self.note_edit_target == Some(id) {
            TreeRowMode::Editing
        } else {
            TreeRowMode::Normal
        };
        let geometry = tree_row_geometry_for_mode(
            row,
            tree_row.as_ref().map_or(0, |row| row.depth),
            sidebar_width,
            mode,
        );
        if has_children && geometry.disclosure_hit.contains_x(x) {
            self.finish_note_edit(false);
            if !self.collapsed_tabs.remove(&id) {
                self.collapsed_tabs.insert(id);
            }
            self.layout();
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        let actions_visible = self.active == Some(id);
        if actions_visible && geometry.actions.secondary.contains(x, y) {
            if mode == TreeRowMode::Editing {
                self.finish_note_edit(false);
            } else {
                self.request_close_tab(id);
            }
            return;
        } else if actions_visible && geometry.actions.primary.contains(x, y) {
            if mode == TreeRowMode::Editing {
                self.finish_note_edit(true);
            } else {
                self.open_tab_editor(id);
            }
            return;
        } else if actions_visible
            && let Some(add_child) = geometry.actions.add_child
            && add_child.contains(x, y)
        {
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
                    }
                }
                Err(error) => self.last_error = Some(format!("{error:#}")),
            }
        } else {
            self.finish_note_edit(false);
            self.save_active_composer();
            self.cancel_terminal_selection(true);
            self.active = Some(id);
            self.load_active_composer();
            self.event_journal
                .commit(EventKind::TabSelected, Some(id), serde_json::json!({}));
        }
        self.set_focus_surface(FocusSurface::Tabs, "mouse");
    }

    fn right_click(&mut self, x: i32, y: i32) {
        if x >= self.effective_tabs_width()
            || self.pending_close.is_some()
            || self.window_close_pending
        {
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
        self.cancel_terminal_selection(true);
        self.finish_note_edit(false);
        if self.cwd_edit_target.is_some() {
            self.close_cwd_editor();
        }
        if self.proxy_edit_target.is_some() {
            self.close_proxy_editor();
        }
        self.save_active_composer();
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let title = tab.title.clone();
        let note = tab.note.clone();
        self.active = Some(id);
        self.note_edit_target = Some(id);
        self.last_error = None;
        unsafe {
            SetWindowTextW(self.tab_name_edit, wide(&title).as_ptr());
            SetWindowTextW(self.tab_note_edit, wide(&note).as_ptr());
        }
        self.layout();
        unsafe {
            SetFocus(self.tab_name_edit);
            InvalidateRect(self.window, ptr::null(), 0);
        }
    }

    fn finish_note_edit(&mut self, save: bool) {
        let Some(id) = self.note_edit_target else {
            return;
        };
        if save {
            let title = window_text(self.tab_name_edit);
            let note = window_text(self.tab_note_edit);
            let title = title.trim();
            if title.is_empty() {
                self.last_error = Some("Tab name cannot be empty".to_owned());
                unsafe { SetFocus(self.tab_name_edit) };
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                return;
            }
            self.last_error = None;
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                let previous_title = tab.title.clone();
                let previous_note = tab.note.clone();
                tab.title = title.to_owned();
                tab.note = note.trim_end().to_owned();
                if tab.title != previous_title {
                    self.event_journal.commit(
                        EventKind::TabRenamed,
                        Some(id),
                        serde_json::json!({
                            "previous_name": previous_title,
                            "name": tab.title,
                        }),
                    );
                }
                if tab.note != previous_note {
                    self.event_journal.commit(
                        EventKind::TabNote,
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
        unsafe {
            ShowWindow(self.tab_name_edit, SW_HIDE);
            ShowWindow(self.tab_note_edit, SW_HIDE);
            SetFocus(self.window);
        }
        self.host_focus_surface = HostFocusSurface::Tabs;
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn open_cwd_editor(&mut self, target: Option<&str>) -> Result<()> {
        self.finish_note_edit(false);
        if self.settings_open
            || self.pending_close.is_some()
            || self.window_close_pending
            || self.proxy_edit_target.is_some()
        {
            anyhow::bail!("another modal surface is active");
        }
        self.cancel_terminal_selection(true);
        let position = self
            .target_position(target)
            .or_else(|| self.active_position())
            .context("can't find tab")?;
        self.save_active_composer();
        let id = self.tabs[position].id;
        self.active = Some(id);
        self.cwd_edit_target = Some(id);
        let path = self.tabs[position]
            .cwd
            .path()
            .unwrap_or_default()
            .to_owned();
        self.navigation_latch = None;
        self.feedback = Some(
            "CWD editor: Ctrl+Enter prepare · Ctrl+Shift+Enter append · Esc cancel".to_owned(),
        );
        unsafe {
            SetWindowTextW(self.edit, wide(&path).as_ptr());
            SetWindowTextW(self.send_button, wide("Prepare").as_ptr());
            ShowWindow(self.edit, SW_SHOW);
            ShowWindow(self.send_button, SW_SHOW);
            SetFocus(self.edit);
            InvalidateRect(self.window, ptr::null(), 0);
        }
        self.event_journal.commit(
            EventKind::WorkingContextCwdEditor,
            Some(id),
            serde_json::json!({"open": true}),
        );
        Ok(())
    }

    fn close_cwd_editor(&mut self) {
        let Some(id) = self.cwd_edit_target.take() else {
            return;
        };
        unsafe { SetWindowTextW(self.send_button, wide(LABEL_SEND).as_ptr()) };
        self.load_active_composer();
        self.host_focus_surface = HostFocusSurface::Terminal;
        unsafe {
            SetFocus(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
        self.event_journal.commit(
            EventKind::WorkingContextCwdEditor,
            Some(id),
            serde_json::json!({"open": false}),
        );
    }

    fn prepare_cwd(
        &mut self,
        target: Option<&str>,
        requested_path: Option<String>,
        mode: ComposerWriteMode,
    ) -> Result<()> {
        let position = self
            .target_position(target)
            .or_else(|| {
                self.cwd_edit_target
                    .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
            })
            .or_else(|| self.active_position())
            .context("can't find tab")?;
        let path = requested_path.unwrap_or_else(|| window_text(self.edit).trim().to_owned());
        validate_path(&path)?;
        let command = cwd_command(self.tabs[position].shell_kind, &path)?;
        let previous = self.tabs[position].composer.clone();
        let next = match mode {
            ComposerWriteMode::EmptyOnly if !previous.is_empty() => anyhow::bail!(
                "Composer already has a draft; explicitly choose --mode append or --mode replace"
            ),
            ComposerWriteMode::EmptyOnly | ComposerWriteMode::Replace => command.clone(),
            ComposerWriteMode::Append => {
                if previous.is_empty() {
                    command.clone()
                } else {
                    format!("{previous}\r\n{command}")
                }
            }
        };
        let id = self.tabs[position].id;
        self.tabs[position].composer = next;
        self.tabs[position].cwd.request(path.clone())?;
        self.event_journal.commit(
            EventKind::WorkingContextCwdRequested,
            Some(id),
            serde_json::json!({
                "path": path,
                "source": CwdSource::UserRequested.as_str(),
                "pending": true,
                "disposition": "prepared",
                "composer_mode": mode.as_str(),
            }),
        );
        self.feedback = Some(format!(
            "Prepared a safely quoted {} CWD command in Composer; it has not been sent",
            self.tabs[position].shell_kind.as_str()
        ));
        if self.cwd_edit_target == Some(id) {
            self.close_cwd_editor();
        } else if self.active == Some(id) {
            self.load_active_composer();
        }
        Ok(())
    }

    fn send_cwd_now(&mut self, target: Option<&str>, requested_path: String) -> Result<()> {
        let position = self
            .target_position(target)
            .or_else(|| self.active_position())
            .context("can't find tab")?;
        validate_path(&requested_path)?;
        let shell = self.tabs[position].shell_kind;
        let command = cwd_command(shell, &requested_path)?;
        if !self.tabs[position].submit(&command) {
            anyhow::bail!("terminal is unavailable or already has a pending submission");
        }
        let id = self.tabs[position].id;
        self.tabs[position].cwd.request(requested_path.clone())?;
        self.event_journal.commit(
            EventKind::WorkingContextCwdRequested,
            Some(id),
            serde_json::json!({
                "path": requested_path,
                "source": CwdSource::UserRequested.as_str(),
                "pending": true,
                "disposition": "sent",
                "shell": shell.as_str(),
            }),
        );
        self.feedback = Some(format!(
            "Sent a safely quoted CWD command to @{id}; waiting for OSC 7 confirmation"
        ));
        if self.cwd_edit_target == Some(id) {
            self.close_cwd_editor();
        }
        Ok(())
    }

    fn open_proxy_editor(&mut self, target: Option<&str>) -> Result<()> {
        self.finish_note_edit(false);
        if self.settings_open
            || self.pending_close.is_some()
            || self.window_close_pending
            || self.cwd_edit_target.is_some()
        {
            anyhow::bail!("another modal surface is active");
        }
        self.cancel_terminal_selection(true);
        let position = self
            .target_position(target)
            .or_else(|| self.active_position())
            .context("can't find tab")?;
        self.save_active_composer();
        let id = self.tabs[position].id;
        self.active = Some(id);
        self.proxy_edit_target = Some(id);
        self.proxy_credentials_revealed = false;
        self.navigation_latch = None;
        self.feedback = Some(
            "Proxy editor: reveal credentials before editing · Ctrl+Enter prepare · Esc cancel"
                .to_owned(),
        );
        unsafe {
            SetWindowTextW(
                self.edit,
                wide("HTTP_PROXY=<hidden>\r\nHTTPS_PROXY=<hidden>").as_ptr(),
            );
            SetWindowTextW(self.send_button, wide("Prepare").as_ptr());
            ShowWindow(self.edit, SW_SHOW);
            ShowWindow(self.send_button, SW_SHOW);
            SetFocus(self.edit);
            InvalidateRect(self.window, ptr::null(), 0);
        }
        self.event_journal.commit(
            EventKind::WorkingContextProxyEditor,
            Some(id),
            serde_json::json!({"open": true}),
        );
        Ok(())
    }

    fn reveal_proxy_credentials(&mut self) -> Result<()> {
        let id = self
            .proxy_edit_target
            .context("proxy editor is not active")?;
        let position = self
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .context("can't find tab")?;
        self.proxy_credentials_revealed = true;
        let mut text = self.tabs[position].proxy.editor_text();
        unsafe {
            SetWindowTextW(self.edit, wide(&text).as_ptr());
            SetFocus(self.edit);
            InvalidateRect(self.window, ptr::null(), 0);
            text.as_mut_vec().fill(0);
        }
        Ok(())
    }

    fn remask_proxy_credentials(&mut self) {
        if !self.proxy_credentials_revealed {
            return;
        }
        self.proxy_credentials_revealed = false;
        if self.proxy_edit_target.is_some() {
            unsafe {
                SetWindowTextW(
                    self.edit,
                    wide("HTTP_PROXY=<hidden>\r\nHTTPS_PROXY=<hidden>").as_ptr(),
                );
                InvalidateRect(self.window, ptr::null(), 0);
            }
        }
    }

    fn close_proxy_editor(&mut self) {
        let Some(id) = self.proxy_edit_target.take() else {
            return;
        };
        self.proxy_credentials_revealed = false;
        unsafe {
            SetWindowTextW(self.edit, wide("").as_ptr());
            SetWindowTextW(self.send_button, wide(LABEL_SEND).as_ptr());
        }
        self.load_active_composer();
        self.host_focus_surface = HostFocusSurface::Terminal;
        unsafe {
            SetFocus(self.window);
            InvalidateRect(self.window, ptr::null(), 0);
        }
        self.event_journal.commit(
            EventKind::WorkingContextProxyEditor,
            Some(id),
            serde_json::json!({"open": false}),
        );
    }

    fn proxy_input(&self, provided: Option<String>) -> Result<(Option<String>, Option<String>)> {
        if let Some(mut text) = provided {
            let parsed = parse_proxy_editor(&text);
            unsafe {
                text.as_mut_vec().fill(0);
            }
            return parsed;
        }
        if !self.proxy_credentials_revealed {
            anyhow::bail!("reveal credentials before editing proxy values");
        }
        let mut text = window_text(self.edit);
        let parsed = parse_proxy_editor(&text);
        unsafe {
            text.as_mut_vec().fill(0);
        }
        parsed
    }

    fn prepare_proxy(&mut self, target: Option<&str>, provided: Option<String>) -> Result<()> {
        let position = self
            .target_position(target)
            .or_else(|| {
                self.proxy_edit_target
                    .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
            })
            .or_else(|| self.active_position())
            .context("can't find tab")?;
        if !self.tabs[position].composer.is_empty() {
            anyhow::bail!(
                "Composer already has a normal draft; proxy commands cannot be mixed with it"
            );
        }
        let (http, https) = self.proxy_input(provided)?;
        let command = proxy_command(
            self.tabs[position].shell_kind,
            http.as_deref(),
            https.as_deref(),
        )?;
        self.tabs[position].register_proxy_redactions(
            &[http.as_deref(), https.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
        )?;
        let requested = ProxyState::requested(http, https)?;
        let id = self.tabs[position].id;
        self.tabs[position].sensitive_composer = Some(command);
        self.tabs[position].proxy = requested;
        self.event_journal.commit(
            EventKind::WorkingContextProxyRequested,
            Some(id),
            serde_json::json!({
                "configured": self.tabs[position].proxy.configured(),
                "source": self.tabs[position].proxy.source().as_str(),
                "request_pending": true,
                "disposition": "prepared",
            }),
        );
        self.feedback = Some(
            "Prepared a sensitive proxy command; it affects only this shell and future descendants"
                .to_owned(),
        );
        if self.proxy_edit_target == Some(id) {
            self.close_proxy_editor();
        } else if self.active == Some(id) {
            self.load_active_composer();
        }
        Ok(())
    }

    fn send_proxy_now(&mut self, target: Option<&str>, provided: Option<String>) -> Result<()> {
        let position = self
            .target_position(target)
            .or_else(|| {
                self.proxy_edit_target
                    .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
            })
            .or_else(|| self.active_position())
            .context("can't find tab")?;
        let (http, https) = self.proxy_input(provided)?;
        let command = proxy_command(
            self.tabs[position].shell_kind,
            http.as_deref(),
            https.as_deref(),
        )?;
        self.tabs[position].register_proxy_redactions(
            &[http.as_deref(), https.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
        )?;
        if !self.tabs[position].submit_sensitive(command.expose()) {
            anyhow::bail!("terminal is unavailable or already has a pending submission");
        }
        let requested = ProxyState::requested(http, https)?;
        let id = self.tabs[position].id;
        self.tabs[position].proxy = requested;
        self.event_journal.commit(
            EventKind::WorkingContextProxyRequested,
            Some(id),
            serde_json::json!({
                "configured": self.tabs[position].proxy.configured(),
                "source": self.tabs[position].proxy.source().as_str(),
                "request_pending": true,
                "disposition": "sent",
            }),
        );
        self.feedback = Some(
            "Sent a proxy command; it affects only this shell and future descendants".to_owned(),
        );
        if self.proxy_edit_target == Some(id) {
            self.close_proxy_editor();
        }
        Ok(())
    }

    fn toggle_proxy_endpoint(&mut self, target: Option<&str>) -> Result<()> {
        let position = self
            .target_position(target)
            .or_else(|| self.active_position())
            .context("can't find tab")?;
        let id = self.tabs[position].id;
        if !self.proxy_endpoint_visible.remove(&id) {
            self.proxy_endpoint_visible.insert(id);
        }
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        Ok(())
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

    fn refresh_system_menu(&self) {
        let menu = unsafe { GetSystemMenu(self.window, 0) };
        if menu.is_null() {
            return;
        }
        let (copy_enabled, paste_enabled) = self.system_clipboard_menu_state();
        unsafe {
            CheckMenuItem(
                menu,
                SYSTEM_MENU_TOGGLE_TABS_ID as u32,
                MF_BYCOMMAND
                    | if self.config.tabs_visible {
                        MF_CHECKED
                    } else {
                        MF_UNCHECKED
                    },
            );
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
        if self.tabs[position].submission.is_pending() {
            self.last_error =
                Some("system menu paste failed: composer submission is pending".to_owned());
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        let tab_id = self.tabs[position].id;
        let sender = self.clipboard_sender.clone();
        let wake_window = self.window as isize;
        let wake_signal = Arc::clone(&self.wake_signal);
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
            if sender.send(ClipboardPaste { tab_id, result }).is_ok() {
                request_gui_wake(wake_window, &wake_signal);
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
        self.cancel_terminal_selection(true);
        let characters = text.chars().count();
        self.event_journal.commit(
            EventKind::TerminalPasted,
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

    fn current_focus_surface(&self) -> FocusSurface {
        let focused = unsafe { GetFocus() };
        let focused = if (focused.is_null() || focused == self.window)
            && no_activate_from_environment()
            && !self.window_close_previous_focus.is_null()
        {
            self.window_close_previous_focus
        } else {
            focused
        };
        if focused == self.settings_font
            || focused == self.settings_size
            || focused == self.settings_dark
            || focused == self.settings_light
            || focused == self.settings_cancel
            || focused == self.settings_apply
        {
            FocusSurface::Settings
        } else if focused == self.edit && self.proxy_edit_target.is_some() {
            FocusSurface::ProxyEditor
        } else if focused == self.edit && self.cwd_edit_target.is_some() {
            FocusSurface::CwdEditor
        } else if self.note_edit_target.is_some()
            && (focused == self.tab_name_edit || focused == self.tab_note_edit)
        {
            FocusSurface::NoteEditor
        } else if focused == self.edit {
            FocusSurface::Composer
        } else {
            match self.host_focus_surface {
                HostFocusSurface::Terminal => FocusSurface::Terminal,
                HostFocusSurface::Tabs => FocusSurface::Tabs,
            }
        }
    }

    fn effective_tabs_width(&self) -> i32 {
        self.workspace_layout().effective_tabs_width
    }

    fn workspace_layout(&self) -> WorkspaceLayout {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        workspace_layout(WorkspaceLayoutInput {
            client_width: client.right,
            client_height: client.bottom,
            tabs_visible: self.config.tabs_visible,
            configured_tabs_width: i32::from(self.config.tabs_width),
            composer_height: COMPOSER_HEIGHT,
            status_height: STATUS_BAR_HEIGHT,
        })
    }

    fn set_tabs_visible(&mut self, visible: bool, cause: &str, operation_id: &str) {
        if self.config.tabs_visible == visible {
            return;
        }
        if !visible {
            self.finish_note_edit(false);
        }
        self.config.tabs_visible = visible;
        self.tabs_resize_drag = None;
        if !visible && self.host_focus_surface == HostFocusSurface::Tabs {
            self.host_focus_surface = HostFocusSurface::Terminal;
            unsafe { SetFocus(self.window) };
        }
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("could not save Tabs visibility: {error:#}"));
        }
        self.event_journal.commit(
            EventKind::LayoutTabsVisibility,
            None,
            serde_json::json!({
                "visible": visible,
                "cause": cause,
                "operation_id": operation_id,
            }),
        );
        self.layout();
        self.refresh_system_menu();
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn begin_tabs_resize(&mut self) {
        self.cancel_terminal_selection(true);
        self.end_scroll_drag();
        self.tabs_resize_drag = Some(TabsResizeDrag {
            original_width: self.config.tabs_width,
        });
        unsafe { SetCapture(self.window) };
    }

    fn drag_tabs_resize(&mut self, x: i32) {
        if self.tabs_resize_drag.is_none() {
            return;
        }
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let width = tabs_width_from_drag(x, client.right) as u16;
        if self.config.tabs_width != width {
            self.config.tabs_width = width;
            self.layout();
            unsafe {
                InvalidateRect(self.window, ptr::null(), 0);
                UpdateWindow(self.window);
            };
        }
    }

    fn finish_tabs_resize(&mut self, persist: bool, cause: &str, operation_id: &str) {
        let Some(drag) = self.tabs_resize_drag.take() else {
            return;
        };
        unsafe { ReleaseCapture() };
        if !persist {
            self.config.tabs_width = drag.original_width;
            self.layout();
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("could not save Tabs width: {error:#}"));
        }
        self.event_journal.commit(
            EventKind::LayoutTabsWidth,
            None,
            serde_json::json!({
                "configured_width": self.config.tabs_width,
                "effective_width": self.effective_tabs_width(),
                "cause": cause,
                "operation_id": operation_id,
            }),
        );
        unsafe { UpdateWindow(self.window) };
    }

    fn set_tabs_width(&mut self, width: u16, cause: &str, operation_id: &str) {
        self.finish_tabs_resize(false, cause, operation_id);
        self.config.tabs_width = width;
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("could not save Tabs width: {error:#}"));
        }
        self.event_journal.commit(
            EventKind::LayoutTabsWidth,
            None,
            serde_json::json!({
                "configured_width": self.config.tabs_width,
                "effective_width": self.effective_tabs_width(),
                "cause": cause,
                "operation_id": operation_id,
            }),
        );
        self.layout();
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
    }

    fn reset_tabs_width(&mut self, cause: &str) {
        self.set_tabs_width(reset_tabs_width() as u16, cause, UI_TABS_SET_WIDTH);
    }

    fn execute_tabs_operation(
        &mut self,
        operation: &'static OperationSpec,
        args: &[String],
    ) -> Result<()> {
        match operation.id {
            UI_TABS_SHOW => self.set_tabs_visible(true, "semantic", operation.id),
            UI_TABS_HIDE => self.set_tabs_visible(false, "semantic", operation.id),
            UI_TABS_TOGGLE => {
                self.set_tabs_visible(!self.config.tabs_visible, "semantic", operation.id);
            }
            UI_TABS_SET_WIDTH => {
                let width = option_value(args, "--width")
                    .and_then(|value| value.parse::<u16>().ok())
                    .context("validated ui.tabs.set-width request lost --width")?;
                self.set_tabs_width(width, "semantic", operation.id);
            }
            _ => anyhow::bail!("unsupported typed Tabs operation: {}", operation.id),
        }
        Ok(())
    }

    fn set_resize_cursor_if_needed(&self) -> bool {
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return false;
        }
        unsafe { ScreenToClient(self.window, &mut point) };
        if self.tabs_resize_drag.is_some()
            || self
                .workspace_layout()
                .resize_grip
                .is_some_and(|grip| grip.contains(point.x, point.y))
        {
            unsafe { SetCursor(LoadCursorW(ptr::null_mut(), IDC_SIZEWE)) };
            true
        } else {
            false
        }
    }

    fn set_focus_surface(&mut self, target: FocusSurface, cause: &str) -> bool {
        let previous = self.current_focus_surface();
        let focused = match target {
            FocusSurface::Terminal => {
                if self.cwd_edit_target.is_some() || self.proxy_edit_target.is_some() {
                    unsafe { SetFocus(self.edit) };
                    return false;
                }
                self.host_focus_surface = HostFocusSurface::Terminal;
                unsafe { SetFocus(self.window) };
                true
            }
            FocusSurface::Composer => {
                if self.settings_open
                    || self.note_edit_target.is_some()
                    || self.cwd_edit_target.is_some()
                    || self.proxy_edit_target.is_some()
                    || self.window_close_pending
                    || self.pending_close.is_some()
                    || self.active.is_none()
                {
                    false
                } else {
                    unsafe { SetFocus(self.edit) };
                    true
                }
            }
            FocusSurface::Tabs => {
                if self.settings_open
                    || self.note_edit_target.is_some()
                    || self.cwd_edit_target.is_some()
                    || self.proxy_edit_target.is_some()
                    || self.window_close_pending
                    || self.pending_close.is_some()
                {
                    false
                } else {
                    if !self.config.tabs_visible {
                        self.set_tabs_visible(true, cause, UI_TABS_SHOW);
                    }
                    if self.effective_tabs_width() <= 0 {
                        self.host_focus_surface = HostFocusSurface::Terminal;
                        self.feedback =
                            Some("Tabs cannot receive focus at this window width".to_owned());
                        unsafe { SetFocus(self.window) };
                        false
                    } else {
                        self.host_focus_surface = HostFocusSurface::Tabs;
                        unsafe { SetFocus(self.window) };
                        true
                    }
                }
            }
            FocusSurface::Settings | FocusSurface::CwdEditor | FocusSurface::ProxyEditor => false,
            FocusSurface::NoteEditor => {
                if self.note_edit_target.is_some() {
                    unsafe { SetFocus(self.tab_name_edit) };
                    true
                } else {
                    false
                }
            }
        };
        let current = self.current_focus_surface();
        if focused && current != previous {
            self.event_journal.commit(
                EventKind::FocusChanged,
                self.active,
                serde_json::json!({
                    "from": previous.as_str(),
                    "to": current.as_str(),
                    "cause": cause,
                }),
            );
        }
        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        focused
    }

    fn handle_shortcut_key_up(&mut self, virtual_key: u32) {
        if self.navigation_latch == Some(virtual_key) || virtual_key == 0x11 {
            self.navigation_latch = None;
        }
    }

    fn handle_shortcut(&mut self, virtual_key: u32, lparam: LPARAM) -> bool {
        let control = unsafe { GetKeyState(0x11) } < 0;
        let shift = unsafe { GetKeyState(0x10) } < 0;
        let alt = unsafe { GetKeyState(0x12) } < 0;
        self.handle_shortcut_with_modifiers(virtual_key, lparam, control, shift, alt)
    }

    fn handle_shortcut_with_modifiers(
        &mut self,
        virtual_key: u32,
        lparam: LPARAM,
        control: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        if is_latched_navigation_repeat(self.navigation_latch, virtual_key, lparam) {
            return true;
        }
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
        if self.proxy_edit_target.is_some() {
            if virtual_key == 0x1b {
                self.close_proxy_editor();
                return true;
            }
            if control && virtual_key == b'R' as u32 {
                if let Err(error) = self.reveal_proxy_credentials() {
                    self.last_error = Some(format!("{error:#}"));
                }
                return true;
            }
            if control && virtual_key == 0x0d {
                if let Err(error) = self.prepare_proxy(None, None) {
                    self.last_error = Some(format!("{error:#}"));
                    unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                }
                return true;
            }
            if focused != self.edit {
                unsafe { SetFocus(self.edit) };
            }
        }
        if self.cwd_edit_target.is_some() {
            if virtual_key == 0x1b {
                self.close_cwd_editor();
                return true;
            }
            if control && virtual_key == 0x0d {
                let mode = if shift {
                    ComposerWriteMode::Append
                } else if alt {
                    ComposerWriteMode::Replace
                } else {
                    ComposerWriteMode::EmptyOnly
                };
                if let Err(error) = self.prepare_cwd(None, None, mode) {
                    self.last_error = Some(format!("{error:#}"));
                    unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                }
                return true;
            }
            // The native edit keeps ordinary text/edit shortcuts, but this
            // modal surface must not leak global tab or focus navigation.
            if focused != self.edit {
                unsafe { SetFocus(self.edit) };
            }
        }

        if let Some(target) = surface_navigation(
            self.current_focus_surface(),
            control,
            shift,
            alt,
            virtual_key,
        ) {
            self.navigation_latch = Some(virtual_key);
            self.set_focus_surface(target, "keyboard");
            return true;
        }

        let has_active_selection = self
            .terminal_selection
            .is_some_and(|selection| self.active == Some(selection.tab_id));
        if terminal_copy_shortcut(
            control,
            virtual_key,
            focused == self.window && self.host_focus_surface == HostFocusSurface::Terminal,
            has_active_selection,
        ) {
            if let Err(error) = self.copy_terminal_selection() {
                self.last_error = Some(format!("terminal copy failed: {error:#}"));
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            }
            return true;
        }

        let focused_edit = focused == self.edit
            || focused == self.settings_font
            || focused == self.settings_size
            || focused == self.tab_name_edit
            || focused == self.tab_note_edit;
        if focused_edit && let Some(shortcut) = edit_shortcut(control, virtual_key) {
            self.dispatch_edit_shortcut(shortcut, focused);
            return true;
        }
        if self.cwd_edit_target.is_some() || self.proxy_edit_target.is_some() {
            return false;
        }

        if self.note_edit_target.is_some()
            && (focused == self.tab_name_edit || focused == self.tab_note_edit)
        {
            if control && virtual_key == 0x0d {
                self.finish_note_edit(true);
                return true;
            }
            if virtual_key == 0x1b {
                self.finish_note_edit(false);
                return true;
            }
        }

        if focused == self.edit {
            if control && virtual_key == 0x0d {
                self.send_composer();
                self.feedback = self.active.map(|id| format!("Sent to @{id}"));
                self.set_focus_surface(FocusSurface::Terminal, "composer-submit");
                return true;
            }
            if virtual_key == 0x1b {
                self.save_active_composer();
                self.set_focus_surface(FocusSurface::Terminal, "composer-escape");
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
            self.set_focus_surface(FocusSurface::Composer, "keyboard");
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
        self.workspace_layout().terminal
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
            self.cancel_terminal_selection(true);
            self.commit_viewport_event(position, after, "mouse-wheel");
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
        }
    }

    fn commit_viewport_event(&mut self, position: usize, offset: usize, source: &str) {
        let id = self.tabs[position].id;
        self.event_journal.commit(
            EventKind::TerminalViewport,
            Some(id),
            serde_json::json!({
                "scrollback_offset": offset,
                "source": source,
            }),
        );
    }

    fn terminal_cell_at(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        let terminal = self.workspace_layout().terminal;
        if x < terminal.left
            || x >= terminal.right.saturating_sub(TERMINAL_SCROLLBAR_WIDTH)
            || y < terminal.top
            || y >= terminal.bottom
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
            ((x - terminal.left) / cell_width).clamp(0, i32::from(cols.saturating_sub(1))) as u16;
        let row =
            ((y - terminal.top) / cell_height).clamp(0, i32::from(rows.saturating_sub(1))) as u16;
        Some((column, row))
    }

    fn character(&mut self, codepoint: u32) {
        if self.host_focus_surface == HostFocusSurface::Tabs && unsafe { GetFocus() } == self.window
        {
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        if self.tabs[position].submission.is_pending() {
            self.feedback = Some("Composer submission pending; terminal input paused".to_owned());
            unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            return;
        }
        if self.cancel_terminal_selection(true) {
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
        if self.host_focus_surface == HostFocusSurface::Tabs && unsafe { GetFocus() } == self.window
        {
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        if self.tabs[position].submission.is_pending() {
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
            if self.cancel_terminal_selection(true) {
                unsafe { InvalidateRect(self.window, ptr::null(), 0) };
            }
            self.tabs[position].send(bytes);
        }
    }

    fn save_active_composer(&mut self) {
        if self.cwd_edit_target.is_some() || self.proxy_edit_target.is_some() || self.settings_open
        {
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        if self.tabs[position].sensitive_composer.is_some() {
            return;
        }
        let composer = window_text(self.edit);
        if self.tabs[position].composer != composer {
            self.tabs[position].composer = composer.clone();
            let id = self.tabs[position].id;
            self.event_journal.commit(
                EventKind::ComposerDraft,
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
            .map(|tab| {
                if tab.sensitive_composer.is_some() {
                    "<sensitive proxy command · Ctrl+Enter to send>"
                } else {
                    tab.composer.as_str()
                }
            })
            .unwrap_or("");
        let text = wide(text);
        unsafe { SetWindowTextW(self.edit, text.as_ptr()) };
    }

    fn send_composer(&mut self) {
        if self.cwd_edit_target.is_some() || self.proxy_edit_target.is_some() {
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        if let Some(secret) = self.tabs[position].sensitive_composer.take() {
            if self.tabs[position].exited.is_some() {
                self.tabs[position].sensitive_composer = Some(secret);
                return;
            }
            if self.tabs[position].submit_sensitive(secret.expose()) {
                let id = self.tabs[position].id;
                self.event_journal.commit(
                    EventKind::WorkingContextProxySubmitted,
                    Some(id),
                    serde_json::json!({"sensitive": true}),
                );
                self.feedback = Some(
                    "Sent a sensitive proxy command to the shell; existing descendants are unchanged"
                        .to_owned(),
                );
                unsafe { SetWindowTextW(self.edit, wide("").as_ptr()) };
            } else {
                self.tabs[position].sensitive_composer = Some(secret);
                self.feedback =
                    Some("A submission is already pending; sensitive draft preserved".to_owned());
            }
            return;
        }
        let text = window_text(self.edit);
        self.tabs[position].composer = text.clone();
        if text.is_empty() || self.tabs[position].exited.is_some() {
            return;
        }
        if self.tabs[position].submit(&text) {
            let id = self.tabs[position].id;
            self.tabs[position].composer.clear();
            self.event_journal.commit(
                EventKind::ComposerSubmitted,
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
        let layout = self.workspace_layout();
        let content_bottom = layout.status.top;
        let sidebar_width = layout.effective_tabs_width;

        fill(device, &client, colors.terminal);
        fill(
            device,
            &RECT {
                left: 0,
                top: 0,
                right: sidebar_width,
                bottom: content_bottom,
            },
            colors.sidebar,
        );
        fill(
            device,
            &RECT {
                left: sidebar_width,
                top: layout.composer.top,
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
        let tree_rows = if self.config.tabs_visible {
            self.tree_rows()
        } else {
            Vec::new()
        };
        for (visual_position, row) in tree_rows.iter().enumerate() {
            let Some(tab) = self.tabs.iter().find(|tab| tab.id == row.id) else {
                continue;
            };
            let mode = if self.note_edit_target == Some(tab.id) {
                TreeRowMode::Editing
            } else {
                TreeRowMode::Normal
            };
            let geometry =
                tree_row_geometry_for_mode(visual_position, row.depth, sidebar_width, mode);
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
                    let x = tree_connector_x(level, sidebar_width, mode);
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
                let branch_x = tree_connector_x(row.depth.saturating_sub(1), sidebar_width, mode);
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
            if mode == TreeRowMode::Normal {
                draw_text(
                    device,
                    &tab.title,
                    win_rect(geometry.name),
                    colors.text,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
            }
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
            if mode == TreeRowMode::Normal {
                draw_text(
                    device,
                    &secondary,
                    win_rect(geometry.note),
                    if tab.note.is_empty() {
                        colors.muted
                    } else {
                        colors.blue
                    },
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
            }
            if actions_visible {
                let compact = geometry.actions.density == TreeRowActionDensity::Compact;
                if let Some(add_child) = geometry.actions.add_child {
                    draw_text(
                        device,
                        if compact { "T+" } else { "Add" },
                        win_rect(add_child),
                        colors.green,
                        DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                    );
                }
                draw_text(
                    device,
                    match (mode, compact) {
                        (TreeRowMode::Normal, true) => "✎",
                        (TreeRowMode::Normal, false) => "Edit",
                        (TreeRowMode::Editing, _) => "Save",
                    },
                    win_rect(geometry.actions.primary),
                    colors.blue,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
                draw_text(
                    device,
                    match (mode, compact) {
                        (TreeRowMode::Normal, true) => "×",
                        (TreeRowMode::Normal, false) => "Close",
                        (TreeRowMode::Editing, _) => "Cancel",
                    },
                    win_rect(geometry.actions.secondary),
                    colors.muted,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
            }
        }

        if let Some(segment) = layout.status_segments.tabs_recovery {
            let segment = win_rect(segment);
            fill(device, &segment, colors.control);
            frame(device, &segment, colors.active_border);
            draw_text(
                device,
                "▸ Tabs",
                RECT {
                    left: segment.left + 8,
                    right: segment.right - 4,
                    ..segment
                },
                colors.text,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
        } else {
            draw_text(
                device,
                "Status",
                RECT {
                    left: 14,
                    top: content_bottom,
                    right: sidebar_width - 10,
                    bottom: client.bottom,
                },
                colors.muted,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
        }
        let cwd_label = self
            .active_position()
            .and_then(|position| self.tabs.get(position))
            .map(|tab| {
                let path = tab.cwd.path().unwrap_or("unknown");
                if tab.cwd.pending() {
                    format!("CWD · {path} · pending")
                } else {
                    format!("CWD · {path} · {}", tab.cwd.source().as_str())
                }
            })
            .unwrap_or_else(|| "CWD · unknown".to_owned());
        let cwd_segment = win_rect(layout.status_segments.cwd);
        frame(device, &cwd_segment, colors.tree);
        draw_text(
            device,
            &cwd_label,
            RECT {
                left: cwd_segment.left + 8,
                right: cwd_segment.right - 6,
                ..cwd_segment
            },
            colors.text,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        draw_text(
            device,
            "metrics · agent context · extensible providers",
            RECT {
                left: layout.status_segments.provider.left + 10,
                top: content_bottom,
                right: layout.status_segments.provider.right - 6,
                bottom: client.bottom,
            },
            colors.muted,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        let proxy_segment = win_rect(layout.status_segments.proxy);
        frame(device, &proxy_segment, colors.tree);
        let (proxy_label, proxy_endpoint_visible) = self
            .active_position()
            .and_then(|position| self.tabs.get(position))
            .map(|tab| {
                let visible = self.proxy_endpoint_visible.contains(&tab.id);
                (
                    if visible {
                        tab.proxy.sanitized_label()
                    } else if tab.proxy.configured() {
                        "Proxy · On".to_owned()
                    } else {
                        "Proxy · Off".to_owned()
                    },
                    visible,
                )
            })
            .unwrap_or_else(|| ("Proxy · Off".to_owned(), false));
        draw_text(
            device,
            &proxy_label,
            RECT {
                left: proxy_segment.left + 6,
                right: proxy_segment.right - 28,
                ..proxy_segment
            },
            colors.text,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        draw_proxy_eye(
            device,
            &RECT {
                left: proxy_segment.right - 25,
                top: proxy_segment.top + 5,
                right: proxy_segment.right - 5,
                bottom: proxy_segment.bottom - 5,
            },
            colors.blue,
            !proxy_endpoint_visible,
        );

        if let Some(position) = self.active_position() {
            unsafe { SelectObject(device, terminal_font as HGDIOBJ) };
            self.paint_terminal(device, position, layout.terminal);
        } else {
            draw_text(
                device,
                if self.startup_tab_pending {
                    "Starting cmd.exe…"
                } else {
                    "Click New to create a cmd.exe tab"
                },
                RECT {
                    left: sidebar_width + 24,
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
                &if let Some(id) = self.proxy_edit_target {
                    format!("HTTP(S) proxy for @{id} · shell and future descendants only")
                } else if let Some(id) = self.cwd_edit_target {
                    format!("Working directory for @{id}")
                } else {
                    format!("Compose input for {}:{}", tab.index, tab.title)
                },
                RECT {
                    left: sidebar_width + 10,
                    top: layout.composer.top + 4,
                    right: client.right - 270,
                    bottom: layout.composer.top + 28,
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
                if self.proxy_edit_target.is_some() {
                    if self.proxy_credentials_revealed {
                        "Credentials revealed · Ctrl+Enter prepare · Esc re-mask/cancel"
                    } else {
                        "Credentials hidden · use Reveal action before editing"
                    }
                } else if self.cwd_edit_target.is_some() {
                    "Ctrl+Enter prepare · Ctrl+Shift+Enter append · Ctrl+Alt+Enter replace · Esc cancel"
                } else if tab.exited.is_some() {
                    "Process exited · draft preserved"
                } else {
                    "Ctrl+Shift+I focus · Ctrl+Enter send · Esc terminal"
                },
                RECT {
                    left: client.right - 260,
                    top: layout.composer.top + 4,
                    right: client.right - 10,
                    bottom: layout.composer.top + 28,
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
                    left: sidebar_width,
                    top: layout.terminal.bottom - 30,
                    right: client.right,
                    bottom: layout.terminal.bottom,
                },
                colors.modal,
            );
            draw_text(
                device,
                &format!(
                    "Process exited with code {code}. Output and draft remain until you close this tab."
                ),
                RECT {
                    left: sidebar_width + 12,
                    top: layout.terminal.bottom - 30,
                    right: client.right - 12,
                    bottom: layout.terminal.bottom,
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
                    left: sidebar_width + 10,
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
                    left: sidebar_width + 10,
                    top: (layout.terminal.bottom - 28).max(0),
                    right: client.right - 10,
                    bottom: layout.terminal.bottom.max(0),
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
        if let Some(grip) = layout.resize_grip {
            let grip = win_rect(grip);
            fill(device, &grip, colors.tree);
            let center = (grip.left + grip.right) / 2;
            fill(
                device,
                &RECT {
                    left: center,
                    top: grip.top,
                    right: center + 1,
                    bottom: grip.bottom,
                },
                colors.active_border,
            );
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
        let terminal = self.workspace_layout().terminal;
        let left = terminal.left + (terminal.width() - 620) / 2;
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
            WINDOW_CLOSE_BUTTON_TEXT_FORMAT,
        );
        draw_text(
            device,
            "Stop Server & Exit",
            stop,
            colors.red,
            WINDOW_CLOSE_BUTTON_TEXT_FORMAT,
        );
        draw_text(
            device,
            "Cancel",
            cancel,
            colors.blue,
            WINDOW_CLOSE_BUTTON_TEXT_FORMAT,
        );
    }

    fn paint_close_confirmation(&self, device: HDC, client: &RECT, id: u64) {
        let colors = PaintColors::from(self.palette());
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let terminal = self.workspace_layout().terminal;
        let left = terminal.left + (terminal.width() - 460) / 2;
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
        let terminal = self.workspace_layout().terminal;
        let left = terminal.left + (terminal.width() - 520) / 2;
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

    fn paint_terminal(&mut self, device: HDC, position: usize, terminal: PixelRect) {
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
        let width = (terminal.width() - TERMINAL_SCROLLBAR_WIDTH).max(cell_width * 10);
        let height = terminal.height().max(cell_height * 2);
        let cols = (width / cell_width).clamp(10, u16::MAX as i32) as u16;
        let rows = (height / cell_height).clamp(2, u16::MAX as i32) as u16;
        self.tabs[position].resize(rows, cols);
        let (scrollback_offset, max_scrollback) = self.tabs[position].scrollback_bounds();
        let scrollbar = terminal_scrollbar_geometry(
            terminal,
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
                        left: terminal.left + start_col as i32 * cell_width,
                        top: terminal.top + row as i32 * cell_height,
                        right: terminal.left + end_col as i32 * cell_width,
                        bottom: terminal.top + (row as i32 + 1) * cell_height,
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
                let left = terminal.left + col as i32 * cell_width;
                let top = terminal.top + row as i32 * cell_height;
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
                left: terminal.left + col as i32 * cell_width,
                top: terminal.top + row as i32 * cell_height,
                right: terminal.left + (col as i32 + 1) * cell_width,
                bottom: terminal.top + (row as i32 + 1) * cell_height,
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
                    left: terminal.left + 8,
                    top: terminal.bottom - cell_height,
                    right: terminal.right - TERMINAL_SCROLLBAR_WIDTH - 8,
                    bottom: terminal.bottom,
                },
                colors.orange,
                DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
    }

    fn focus_surface(&self) -> &'static str {
        self.current_focus_surface().as_str()
    }

    fn ui_snapshot(&mut self) -> String {
        let mut client: RECT = unsafe { mem::zeroed() };
        unsafe { GetClientRect(self.window, &mut client) };
        let layout = self.workspace_layout();
        let scrollbar = self.scrollbar_state().map(|(geometry, maximum)| {
            serde_json::json!({
                "visible": true,
                "track": pixel_rect_json(geometry.track),
                "thumb": pixel_rect_json(geometry.thumb),
                "max_offset": maximum,
            })
        });
        let active_draft = window_text(self.edit);
        let tab_editor = self.note_edit_target.map(|id| {
            let focused = unsafe { GetFocus() };
            serde_json::json!({
                "target": format!("@{id}"),
                "name_length": window_text(self.tab_name_edit).chars().count(),
                "note_length": window_text(self.tab_note_edit).chars().count(),
                "focus": if focused == self.tab_name_edit {
                    Some("name")
                } else if focused == self.tab_note_edit {
                    Some("note")
                } else {
                    None
                },
            })
        });
        let visible_rows = self.tree_rows();
        let all_rows = self.all_tree_rows();
        let tabs = all_rows
            .iter()
            .filter_map(|row| {
                let tab = self.tabs.iter().find(|tab| tab.id == row.id)?;
                let visible_position = self
                    .config
                    .tabs_visible
                    .then(|| visible_rows.iter().position(|visible| visible.id == row.id))
                    .flatten();
                let mode = if self.note_edit_target == Some(tab.id) {
                    TreeRowMode::Editing
                } else {
                    TreeRowMode::Normal
                };
                let geometry = visible_position.map(|position| {
                    tree_row_geometry_for_mode(
                        position,
                        row.depth,
                        layout.effective_tabs_width,
                        mode,
                    )
                });
                let draft = if self.active == Some(tab.id)
                    && self.note_edit_target.is_none()
                    && self.cwd_edit_target.is_none()
                    && self.proxy_edit_target.is_none()
                {
                    !active_draft.is_empty() || tab.sensitive_composer.is_some()
                } else {
                    !tab.composer.is_empty() || tab.sensitive_composer.is_some()
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
                    "working_context": {
                        "cwd": {
                            "path": tab.cwd.path(),
                            "confirmed_path": tab.cwd.confirmed_path(),
                            "source": tab.cwd.source().as_str(),
                            "pending": tab.cwd.pending(),
                        },
                        "shell": tab.shell_kind.as_str(),
                        "proxy": {
                            "configured": tab.proxy.configured(),
                            "source": tab.proxy.source().as_str(),
                            "request_pending": tab.proxy.request_pending(),
                            "endpoint_visible": self.proxy_endpoint_visible.contains(&tab.id),
                            "credential_revealed": false,
                        },
                    },
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
                        "mode": match geometry.mode {
                            TreeRowMode::Normal => "normal",
                            TreeRowMode::Editing => "editing",
                        },
                        "row": pixel_rect_json(geometry.row),
                        "selection": pixel_rect_json(geometry.selection),
                        "node": {
                            "x": geometry.node_x,
                            "y": geometry.node_y,
                        },
                        "expander": pixel_rect_json(geometry.expander),
                        "status": pixel_rect_json(geometry.status),
                        "disclosure_hit": pixel_rect_json(geometry.disclosure_hit),
                        "text": pixel_rect_json(geometry.text),
                        "name": pixel_rect_json(geometry.name),
                        "note": pixel_rect_json(geometry.note),
                        "editors": geometry.editors.map(|editors| serde_json::json!({
                            "name": pixel_rect_json(editors.name),
                            "note": pixel_rect_json(editors.note),
                        })),
                    })),
                    "actions": visible_position
                        .filter(|_| self.active == Some(tab.id))
                        .map(|position| {
                            let geometry = tree_row_geometry_for_mode(
                                position,
                                row.depth,
                                layout.effective_tabs_width,
                                mode,
                            );
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
                                    "density": match geometry.actions.density {
                                        TreeRowActionDensity::Full => "full",
                                        TreeRowActionDensity::Compact => "compact",
                                    },
                                    "new_child": action(
                                        "new-child",
                                        "Add",
                                        geometry.actions.add_child.expect("normal rows have Add"),
                                    ),
                                    "edit": action("edit-tab", "Edit", geometry.actions.primary),
                                    "close": action("close-tab", "Close", geometry.actions.secondary),
                                }),
                                TreeRowMode::Editing => serde_json::json!({
                                    "mode": "editing",
                                    "density": match geometry.actions.density {
                                        TreeRowActionDensity::Full => "full",
                                        TreeRowActionDensity::Compact => "compact",
                                    },
                                    "save": action(
                                        "tab-editor-save",
                                        "Save",
                                        geometry.actions.primary,
                                    ),
                                    "cancel": action(
                                        "tab-editor-cancel",
                                        "Cancel",
                                        geometry.actions.secondary,
                                    ),
                                }),
                            }
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
        let selection_interaction = self.terminal_selection_gesture.map(|gesture| {
            let selection = self
                .terminal_selection
                .filter(|selection| selection.tab_id == gesture.tab_id())
                .map(|selection| {
                    let (start, end) = selection.bounds();
                    serde_json::json!({
                        "start": {"row": start.row, "col": start.col},
                        "end": {"row": end.row, "col": end.col},
                    })
                });
            let autoscroll = self.terminal_selection_autoscroll.map(|step| {
                serde_json::json!({
                    "active": true,
                    "direction": match step.direction {
                        AutoScrollDirection::Up => "up",
                        AutoScrollDirection::Down => "down",
                    },
                    "rows_per_tick": step.rows,
                })
            });
            serde_json::json!({
                "phase": gesture.phase().as_str(),
                "tab_id": format!("@{}", gesture.tab_id()),
                "selection": selection,
                "autoscroll": autoscroll.unwrap_or_else(|| serde_json::json!({
                    "active": false,
                })),
            })
        });
        serde_json::to_string_pretty(&serde_json::json!({
            "protocol_version": 1,
            "event_position": event_position,
            "terminal_interaction": {
                "selection": selection_interaction,
                "raw_mouse_arbitration": false,
                "rectangular_selection": false,
            },
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
                    "x": layout.sidebar.left, "y": layout.sidebar.top,
                    "visible": self.config.tabs_visible,
                    "configured_width": self.config.tabs_width,
                    "effective_width": layout.effective_tabs_width,
                    "width": layout.sidebar.width(),
                    "height": layout.sidebar.height(),
                    "bounds": pixel_rect_json(layout.sidebar),
                    "resize_grip": layout.resize_grip.map(pixel_rect_json),
                    "resizing": self.tabs_resize_drag.is_some(),
                },
                "terminal": {
                    "x": layout.terminal.left, "y": layout.terminal.top,
                    "width": layout.terminal.width(),
                    "viewport_width": (layout.terminal.width() - TERMINAL_SCROLLBAR_WIDTH).max(0),
                    "height": layout.terminal.height(),
                    "bounds": pixel_rect_json(layout.terminal),
                    "rows": rows, "cols": cols,
                    "scrollbar": scrollbar,
                },
                "composer": {
                    "visible": unsafe { IsWindowVisible(self.edit) } != 0
                        && unsafe { IsWindowVisible(self.send_button) } != 0,
                    "input_visible": unsafe { IsWindowVisible(self.edit) } != 0,
                    "send_visible": unsafe { IsWindowVisible(self.send_button) } != 0,
                    "x": layout.composer.left,
                    "y": layout.composer.top,
                    "width": layout.composer.width(),
                    "height": layout.composer.height(),
                    "bounds": pixel_rect_json(layout.composer),
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
                        "eye_bounds": pixel_rect_json(PixelRect {
                            left: (layout.status_segments.proxy.right - 28)
                                .max(layout.status_segments.proxy.left),
                            top: layout.status_segments.proxy.top,
                            right: layout.status_segments.proxy.right,
                            bottom: layout.status_segments.proxy.bottom,
                        }),
                        "available": self.active.is_some(),
                        "action": "open-proxy-editor",
                        "eye_action": "proxy-toggle-visibility",
                    },
                    "provider": "placeholder",
                }
            },
            "focus": {
                "surface": self.focus_surface(),
                "window_id": self.active.map(|id| format!("@{id}")),
            },
            "system_menu": {
                "toggle_tabs": {
                    "id": SYSTEM_MENU_TOGGLE_TABS_ID,
                    "label": "Toggle Tabs",
                    "checked": self.config.tabs_visible,
                },
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
            "tab_editor": tab_editor,
            "modal": if self.window_close_pending {
                Some(serde_json::json!({
                    "kind": "confirm-window-close",
                    "default_action": "keep-server-running",
                    "actions": [
                        "keep-server-running",
                        "stop-server-and-exit",
                        "cancel"
                    ],
                    "buttons": [
                        {
                            "action": "keep-server-running",
                            "label": "Keep Server Running",
                            "text_alignment": {
                                "horizontal": "center",
                                "vertical": "center",
                                "win32_draw_text_format": WINDOW_CLOSE_BUTTON_TEXT_FORMAT,
                            },
                        },
                        {
                            "action": "stop-server-and-exit",
                            "label": "Stop Server & Exit",
                            "text_alignment": {
                                "horizontal": "center",
                                "vertical": "center",
                                "win32_draw_text_format": WINDOW_CLOSE_BUTTON_TEXT_FORMAT,
                            },
                        },
                        {
                            "action": "cancel",
                            "label": "Cancel",
                            "text_alignment": {
                                "horizontal": "center",
                                "vertical": "center",
                                "win32_draw_text_format": WINDOW_CLOSE_BUTTON_TEXT_FORMAT,
                            },
                        },
                    ],
                }))
            } else if self.settings_open {
                Some(serde_json::json!({"kind": "settings"}))
            } else if let Some(id) = self.cwd_edit_target {
                Some(serde_json::json!({
                    "kind": "cwd-editor",
                    "window_id": format!("@{id}"),
                    "default_action": "cwd-prepare",
                    "actions": [
                        "cwd-prepare",
                        "cwd-prepare-append",
                        "cwd-prepare-replace",
                        "cwd-send-now",
                        "cancel"
                    ],
                }))
            } else if let Some(id) = self.proxy_edit_target {
                Some(serde_json::json!({
                    "kind": "proxy-editor",
                    "window_id": format!("@{id}"),
                    "default_action": "proxy-prepare",
                    "credential_revealed": false,
                    "actions": [
                        "proxy-reveal-credentials",
                        "proxy-prepare",
                        "proxy-send-now",
                        "cancel"
                    ],
                }))
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
                "tabs_visible": self.config.tabs_visible,
                "tabs_width": self.config.tabs_width,
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

    fn control_event_position(&self) -> ControlEventPosition {
        let position = self.event_journal.position();
        ControlEventPosition {
            epoch: position.epoch,
            sequence: position.sequence,
        }
    }

    fn resolved_control_target(&self, args: &[String]) -> ResolvedTarget {
        let command = args.first().map(String::as_str).unwrap_or_default();
        let tab_scoped = option_value(args, "-t").is_some()
            || matches!(
                command,
                "capture-pane"
                    | "display-message"
                    | "dump-cells"
                    | "focus"
                    | "inspect"
                    | "kill-window"
                    | "rename-window"
                    | "scroll-pane"
                    | "send-composer"
                    | "send-keys"
                    | "send-mouse"
                    | "set-composer"
                    | "set-tab-note"
                    | "show-composer"
                    | "show-tab-note"
            );
        let tab_id = tab_scoped
            .then(|| self.target_position(option_value(args, "-t")))
            .flatten()
            .map(|position| self.tabs[position].id);
        let position = self.event_journal.position();
        ResolvedTarget {
            server_pid: std::process::id(),
            server_address: ipc_address(),
            server_epoch: position.epoch,
            session: self.session_name.clone(),
            tab_id,
        }
    }

    fn response_from_receipt(receipt: ControlReceipt) -> IpcResponse {
        if let Some(error) = &receipt.error {
            return IpcResponse::typed_failure(
                error.message.clone(),
                error.code.clone(),
                error.category.as_str(),
                error.retryable,
            )
            .with_receipt(receipt);
        }
        let output = receipt
            .result
            .as_ref()
            .and_then(|result| result["output"].as_str())
            .unwrap_or_default()
            .to_owned();
        IpcResponse::success(output).with_receipt(receipt)
    }

    fn execute_ipc_request(&mut self, request: IpcRequest) -> IpcResponse {
        let Some(control) = request.control else {
            return match validate_operation_args(&request.args) {
                Ok(operation) => self.execute_command(&request.args, operation),
                Err(error) => IpcResponse::typed_failure(
                    error,
                    "operation_invalid_arguments",
                    "validation",
                    false,
                ),
            };
        };
        if control.schema_version != control_contract::CONTROL_CONTRACT_SCHEMA_VERSION {
            return IpcResponse::typed_failure(
                "unsupported control contract schema version",
                "control_schema_unsupported",
                "unsupported",
                false,
            );
        }
        let expected_identity = control_request_identity(&request.args);
        let expected_fingerprint = control_payload_fingerprint(&request.args);
        let identity_matches = expected_identity.as_ref().is_ok_and(|(operation, intent)| {
            operation == &control.operation_id && intent == &control.intent
        });
        let fingerprint_matches = expected_fingerprint
            .as_ref()
            .is_ok_and(|fingerprint| fingerprint == &control.payload_fingerprint);
        if !identity_matches || !fingerprint_matches {
            let error = ControlError::new(
                "control_request_mismatch",
                ErrorCategory::Validation,
                "control metadata does not match the command payload",
                false,
            );
            let receipt = ControlReceipt::rejected(&control, error.clone());
            return IpcResponse::typed_failure(
                error.message,
                error.code,
                error.category.as_str(),
                error.retryable,
            )
            .with_receipt(receipt);
        }

        match self.replay_window.admit(&control, unix_time_ms()) {
            Admission::Replay { receipt } | Admission::Reject { receipt } => {
                return Self::response_from_receipt(receipt);
            }
            Admission::Execute { .. } => {}
        }

        let before_position = self.control_event_position();
        let mut resolved = self.resolved_control_target(&request.args);
        let response = match validate_operation_args(&request.args) {
            Ok(operation) => self.execute_command(&request.args, operation),
            Err(error) => IpcResponse::typed_failure(
                error,
                "operation_invalid_arguments",
                "validation",
                false,
            ),
        };
        let after_position = self.control_event_position();
        if resolved.tab_id.is_none()
            && let Some(id) = response
                .output
                .trim()
                .strip_prefix('@')
                .and_then(|value| value.parse::<u64>().ok())
        {
            resolved.tab_id = Some(id);
        }
        let error = (!response.ok).then(|| {
            ControlError::new(
                if response.error_code.is_empty() {
                    "command_outcome_unknown"
                } else {
                    response.error_code.as_str()
                },
                error_category_from_wire(&response.error_category),
                response.error.clone(),
                response.retryable,
            )
        });
        let mut outcome = if response.ok {
            ReceiptOutcome::Committed
        } else if matches!(
            error.as_ref().map(|error| error.category),
            Some(
                ErrorCategory::Validation
                    | ErrorCategory::Conflict
                    | ErrorCategory::NotFound
                    | ErrorCategory::Precondition
                    | ErrorCategory::Policy
                    | ErrorCategory::Unsupported
            )
        ) {
            ReceiptOutcome::NoOp
        } else {
            ReceiptOutcome::OutcomeUnknown
        };
        let wait = (response.ok && control.operation_id.as_str() == "command.send.composer")
            .then_some(resolved.tab_id)
            .flatten()
            .and_then(|tab_id| {
                let position = self.tabs.iter().position(|tab| tab.id == tab_id)?;
                self.tabs[position]
                    .submission
                    .is_pending()
                    .then(|| WaitDescriptor {
                        condition: WaitCondition::SubmissionComplete,
                        target: resolved.clone(),
                        minimum_position: after_position.clone(),
                        event_kind: Some("composer.submission-finished".to_owned()),
                        deadline_unix_ms: control.deadline_unix_ms,
                    })
            });
        if wait.is_some() {
            outcome = ReceiptOutcome::Accepted;
        }
        let receipt = ControlReceipt {
            schema_version: control_contract::CONTROL_CONTRACT_SCHEMA_VERSION,
            request_id: control.request_id.clone(),
            operation_id: control.operation_id.clone(),
            payload_fingerprint: control.payload_fingerprint.clone(),
            outcome,
            resolved: Some(resolved),
            before_position: Some(before_position),
            after_position: Some(after_position),
            result: response
                .ok
                .then(|| serde_json::json!({ "output": response.output })),
            error,
            wait,
        };
        let completion = if receipt.outcome == ReceiptOutcome::Accepted {
            self.replay_window
                .update_accepted(&control, receipt.clone())
        } else {
            self.replay_window.complete(&control, receipt.clone())
        };
        if let Err(error) = completion {
            return IpcResponse::typed_failure(
                format!("failed to finalize control receipt: {error}"),
                "control_receipt_internal",
                "internal",
                false,
            );
        }
        if receipt.outcome == ReceiptOutcome::Accepted {
            let Some(tab_id) = receipt.resolved.as_ref().and_then(|target| target.tab_id) else {
                return IpcResponse::typed_failure(
                    "accepted control request did not resolve a terminal",
                    "control_receipt_internal",
                    "internal",
                    false,
                );
            };
            self.pending_control_receipts.insert(
                tab_id,
                PendingControlReceipt {
                    request: control,
                    receipt: receipt.clone(),
                },
            );
        }
        response.with_receipt(receipt)
    }

    fn execute_command(
        &mut self,
        args: &[String],
        operation: Option<&'static OperationSpec>,
    ) -> IpcResponse {
        let Some(command) = args.first().map(String::as_str) else {
            return IpcResponse::failure("no command specified");
        };
        match command {
            "__show-no-activate" => {
                self.show_window_no_activate("launcher-no-activate");
                IpcResponse::success("")
            }
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
                if self.proxy_edit_target.is_some() {
                    self.close_proxy_editor();
                }
                self.save_active_composer();
                let id = self.tabs[position].id;
                self.finish_note_edit(false);
                self.cancel_terminal_selection(true);
                self.active = Some(id);
                self.load_active_composer();
                self.event_journal
                    .commit(EventKind::TabSelected, Some(id), serde_json::json!({}));
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
                    EventKind::TabRenamed,
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
                    EventKind::TabNote,
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
                if self.close_tab(id) {
                    IpcResponse::success("")
                } else {
                    IpcResponse::typed_failure(
                        "terminal was removed, but its workers did not finish bounded shutdown",
                        "terminal_shutdown_incomplete",
                        "internal",
                        false,
                    )
                }
            }
            "send" | "send-keys" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::typed_failure(
                        "can't find pane",
                        "operation_target_not_found",
                        "not_found",
                        false,
                    );
                };
                if self.tabs[position].submission.is_pending() {
                    return IpcResponse::typed_failure(
                        "composer submission is pending; wait with \
                         `wait-pane --submit-complete` before sending keys",
                        "operation_conflict",
                        "conflict",
                        true,
                    );
                }
                let literal = args.iter().any(|arg| arg == "-l");
                for key in positional_values(args, &["-t"], &["-l", "-R", "-X"]) {
                    let sent = if literal {
                        self.tabs[position].send(key.as_bytes())
                    } else if let Some(bytes) = tmux_key_bytes(key) {
                        self.tabs[position].send(&bytes)
                    } else {
                        self.tabs[position].send(key.as_bytes())
                    };
                    if !sent {
                        return IpcResponse::typed_failure(
                            "terminal input was not accepted because the pane is no longer writable",
                            "terminal_not_writable",
                            "precondition",
                            false,
                        );
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
                            self.cancel_terminal_selection(true);
                        }
                        unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                        IpcResponse::success(offset.to_string())
                    }
                    Err(error) => IpcResponse::failure(format!("{error:#}")),
                }
            }
            "capturep" | "capture-pane" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::typed_failure(
                        "can't find pane",
                        "operation_target_not_found",
                        "not_found",
                        false,
                    );
                };
                let requested_maximum = match option_value(args, "--max-bytes") {
                    Some(value) => match value.parse::<usize>() {
                        Ok(value) if (1..=CAPTURE_PUBLIC_MAX_BYTES).contains(&value) => Some(value),
                        _ => {
                            return IpcResponse::failure(format!(
                                "capture-pane --max-bytes must be from 1 to {CAPTURE_PUBLIC_MAX_BYTES}"
                            ));
                        }
                    },
                    None => None,
                };
                let json = args.iter().any(|argument| argument == "--json");
                let contents = if args.iter().any(|argument| argument == "--raw-escaped") {
                    String::from_utf8_lossy(&self.tabs[position].raw_output.to_vec())
                        .escape_debug()
                        .to_string()
                } else {
                    self.tabs[position].parser.screen().contents()
                };
                let original_bytes = contents.len();
                let maximum = requested_maximum.or(json.then_some(CAPTURE_PUBLIC_MAX_BYTES));
                let text = maximum.map_or(contents.as_str(), |maximum| {
                    bounded_utf8_prefix(&contents, maximum)
                });
                if json {
                    IpcResponse::success(
                        serde_json::to_string_pretty(&serde_json::json!({
                            "tab_id": format!("@{}", self.tabs[position].id),
                            "text": text,
                            "bytes": text.len(),
                            "original_bytes": original_bytes,
                            "max_bytes": maximum,
                            "truncated": text.len() < original_bytes,
                        }))
                        .unwrap_or_else(|_| "{}".to_owned()),
                    )
                } else {
                    IpcResponse::success(text.to_owned())
                }
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
                            if self.tabs[position].send(
                                format!("\x1b[<{button};{};{}{suffix}", x + 1, y + 1).as_bytes(),
                            ) {
                                IpcResponse::success("")
                            } else {
                                IpcResponse::failure(
                                    "terminal mouse input was not accepted because the pane is no longer writable",
                                )
                            }
                        }
                    };
                }
                if protocol != "auto" && protocol != "sgr" && protocol != "native" {
                    return IpcResponse::failure(format!("unknown mouse protocol: {protocol}"));
                }
                if self.tabs[position]
                    .send(format!("\x1b[<{button};{};{}{suffix}", x + 1, y + 1).as_bytes())
                {
                    IpcResponse::success("")
                } else {
                    IpcResponse::failure(
                        "terminal mouse input was not accepted because the pane is no longer writable",
                    )
                }
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
                    Err(error) => IpcResponse::typed_failure(
                        error.to_json().to_string(),
                        error.code(),
                        "precondition",
                        false,
                    ),
                }
            }
            "protocol-info" => IpcResponse::success(protocol_info_json("running_host")),
            "focus" => {
                let surface = args.get(1).map(String::as_str).unwrap_or("terminal");
                if let Some(position) = self.target_position(option_value(args, "-t")) {
                    if self.proxy_edit_target.is_some() {
                        self.close_proxy_editor();
                    }
                    self.save_active_composer();
                    let id = self.tabs[position].id;
                    self.finish_note_edit(false);
                    self.cancel_terminal_selection(true);
                    self.active = Some(id);
                    self.load_active_composer();
                    self.event_journal.commit(
                        EventKind::TabSelected,
                        Some(id),
                        serde_json::json!({}),
                    );
                }
                let focused = match surface {
                    "terminal" => self.set_focus_surface(FocusSurface::Terminal, "semantic"),
                    "composer" => self.set_focus_surface(FocusSurface::Composer, "semantic"),
                    "tabs" | "sidebar" => self.set_focus_surface(FocusSurface::Tabs, "semantic"),
                    other => {
                        return IpcResponse::failure(format!("unknown focus surface: {other}"));
                    }
                };
                if !focused {
                    return IpcResponse::failure(format!(
                        "focus surface is unavailable: {surface}"
                    ));
                }
                IpcResponse::success(self.ui_snapshot())
            }
            "ui-action" => {
                let Some(action) = args.get(1).map(String::as_str) else {
                    return IpcResponse::failure("ui-action requires an action");
                };
                if self.cwd_edit_target.is_some()
                    && !matches!(
                        action,
                        "cwd-prepare"
                            | "cwd-prepare-append"
                            | "cwd-prepare-replace"
                            | "cwd-send-now"
                            | "cancel"
                    )
                {
                    return IpcResponse::failure(
                        "CWD editor is a focus trap; prepare, send now, or cancel it first",
                    );
                }
                if self.proxy_edit_target.is_some()
                    && !matches!(
                        action,
                        "proxy-reveal-credentials" | "proxy-prepare" | "proxy-send-now" | "cancel"
                    )
                {
                    self.remask_proxy_credentials();
                    return IpcResponse::failure(
                        "Proxy editor is a focus trap; reveal, prepare, send now, or cancel it first",
                    );
                }
                if let Some(operation) = operation.filter(|operation| {
                    matches!(
                        operation.id,
                        UI_TABS_SHOW | UI_TABS_HIDE | UI_TABS_TOGGLE | UI_TABS_SET_WIDTH
                    )
                }) {
                    if let Err(error) = self.execute_tabs_operation(operation, args) {
                        return IpcResponse::failure(format!("{error:#}"));
                    }
                    unsafe { InvalidateRect(self.window, ptr::null(), 0) };
                    return IpcResponse::success(self.ui_snapshot());
                }
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
                        self.finish_note_edit(false);
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
                        self.layout();
                        None
                    }
                    "select-tab" => {
                        let Some(position) = self.target_position(option_value(args, "-t")) else {
                            return IpcResponse::failure("can't find tab");
                        };
                        self.save_active_composer();
                        let id = self.tabs[position].id;
                        self.finish_note_edit(false);
                        self.cancel_terminal_selection(true);
                        self.active = Some(id);
                        self.load_active_composer();
                        self.event_journal.commit(
                            EventKind::TabSelected,
                            Some(id),
                            serde_json::json!({}),
                        );
                        self.set_focus_surface(FocusSurface::Terminal, "semantic");
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
                        } else if self.cwd_edit_target.is_some() {
                            self.close_cwd_editor();
                        } else if self.proxy_edit_target.is_some() {
                            self.close_proxy_editor();
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
                        if self.cwd_edit_target.is_some() || self.proxy_edit_target.is_some() {
                            return IpcResponse::failure(
                                "CWD editor is active; prepare, send now, or cancel it first",
                            );
                        }
                        if self.note_edit_target.is_some() {
                            self.finish_note_edit(true);
                        } else {
                            self.send_composer();
                        }
                        None
                    }
                    "open-cwd-editor" => match self.open_cwd_editor(option_value(args, "-t")) {
                        Ok(()) => None,
                        Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                    },
                    "cwd-prepare" => {
                        let mode = match ComposerWriteMode::parse(option_value(args, "--mode")) {
                            Ok(mode) => mode,
                            Err(error) => {
                                return IpcResponse::failure(format!("{error:#}"));
                            }
                        };
                        match self.prepare_cwd(
                            option_value(args, "-t"),
                            option_value(args, "--path").map(str::to_owned),
                            mode,
                        ) {
                            Ok(()) => None,
                            Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                        }
                    }
                    "cwd-prepare-append" | "cwd-prepare-replace" => {
                        let mode = if action == "cwd-prepare-append" {
                            ComposerWriteMode::Append
                        } else {
                            ComposerWriteMode::Replace
                        };
                        match self.prepare_cwd(
                            option_value(args, "-t"),
                            option_value(args, "--path").map(str::to_owned),
                            mode,
                        ) {
                            Ok(()) => None,
                            Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                        }
                    }
                    "cwd-send-now" => {
                        let Some(path) = option_value(args, "--path") else {
                            return IpcResponse::failure("cwd-send-now requires --path");
                        };
                        match self.send_cwd_now(option_value(args, "-t"), path.to_owned()) {
                            Ok(()) => None,
                            Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                        }
                    }
                    "open-proxy-editor" => match self.open_proxy_editor(option_value(args, "-t")) {
                        Ok(()) => None,
                        Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                    },
                    "proxy-toggle-visibility" => {
                        match self.toggle_proxy_endpoint(option_value(args, "-t")) {
                            Ok(()) => None,
                            Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                        }
                    }
                    "proxy-reveal-credentials" => match self.reveal_proxy_credentials() {
                        Ok(()) => None,
                        Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                    },
                    "proxy-prepare" => {
                        match self.prepare_proxy(
                            option_value(args, "-t"),
                            option_value(args, "--proxy-input").map(str::to_owned),
                        ) {
                            Ok(()) => None,
                            Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                        }
                    }
                    "proxy-send-now" => {
                        match self.send_proxy_now(
                            option_value(args, "-t"),
                            option_value(args, "--proxy-input").map(str::to_owned),
                        ) {
                            Ok(()) => None,
                            Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
                        }
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
                        self.remask_proxy_credentials();
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
                if self.tabs[position].sensitive_composer.is_some() {
                    return IpcResponse::failure(
                        "Composer contains a sensitive proxy draft; content is redacted",
                    );
                }
                IpcResponse::success(self.tabs[position].composer.clone())
            }
            "set-composer" => {
                let Some(position) = self.target_position(option_value(args, "-t")) else {
                    return IpcResponse::failure("can't find window");
                };
                if self.tabs[position].sensitive_composer.is_some() {
                    return IpcResponse::failure(
                        "Composer contains a sensitive proxy draft; send or discard it first",
                    );
                }
                let text = positional_values(args, &["-t"], &[]).join(" ");
                let id = self.tabs[position].id;
                if self.note_edit_target == Some(id) {
                    let normalized = text.replace("\r\n", "\n");
                    let (name, note) = normalized.split_once('\n').unwrap_or((&normalized, ""));
                    unsafe {
                        SetWindowTextW(self.tab_name_edit, wide(name).as_ptr());
                        SetWindowTextW(self.tab_note_edit, wide(note).as_ptr());
                    }
                    return IpcResponse::success("");
                }
                self.tabs[position].composer = text.clone();
                self.event_journal.commit(
                    EventKind::ComposerDraft,
                    Some(id),
                    serde_json::json!({
                        "length": text.chars().count(),
                    }),
                );
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
                if let Some(secret) = self.tabs[position].sensitive_composer.take() {
                    if !self.tabs[position].submit_sensitive(secret.expose()) {
                        self.tabs[position].sensitive_composer = Some(secret);
                        return IpcResponse::failure("a composer submission is already pending");
                    }
                    let id = self.tabs[position].id;
                    self.event_journal.commit(
                        EventKind::WorkingContextProxySubmitted,
                        Some(id),
                        serde_json::json!({"sensitive": true}),
                    );
                    if self.active == Some(id) {
                        unsafe { SetWindowTextW(self.edit, wide("").as_ptr()) };
                    }
                    return IpcResponse::success("");
                }
                let text = mem::take(&mut self.tabs[position].composer);
                if !text.is_empty() && !self.tabs[position].submit(&text) {
                    self.tabs[position].composer = text;
                    return IpcResponse::failure("a composer submission is already pending");
                }
                if !text.is_empty() {
                    let id = self.tabs[position].id;
                    self.event_journal.commit(
                        EventKind::ComposerSubmitted,
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
                        let observation = tab.observation();
                        serde_json::json!({
                            "id": format!("@{}", tab.id),
                            "index": tab.index,
                            "parent_id": tab.parent_id.map(|id| format!("@{id}")),
                            "depth": self.tab_depth(tab.id),
                            "name": tab.title,
                            "terminal_title": tab.parser.callbacks().title,
                            "note": tab.note,
                            "active": self.active == Some(tab.id),
                            "dead": observation.exit_code.is_some(),
                            "exit_code": observation.exit_code,
                            "pid": observation.process_id,
                            "command": tab.command_name,
                            "environment_names": tab.environment_names,
                            "rows": tab.last_size.0,
                            "cols": tab.last_size.1,
                            "input_bytes": observation.input_bytes,
                            "input_writes": observation.input_writes,
                            "submit_pending": observation.submit_pending,
                            "reader_closed": observation.reader_closed,
                            "parser_drained": observation.parser_drained,
                            "finalized": observation.finalized,
                            "scrollback_offset": tab.parser.screen().scrollback(),
                            "output_bytes": observation.output_bytes,
                            "composer": if tab.sensitive_composer.is_some() {
                                "<redacted>"
                            } else {
                                tab.composer.as_str()
                            },
                            "composer_sensitive": tab.sensitive_composer.is_some(),
                            "error": observation.error,
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
                match save_window_png(self.window, &path, None) {
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
                if previous != Some(self.tabs[position].id) {
                    self.finish_note_edit(false);
                }
                self.active = Some(self.tabs[position].id);
                self.load_active_composer();
                unsafe {
                    InvalidateRect(self.window, ptr::null(), 0);
                }
                self.paint();
                let path = screenshot_output_path(args, "agenterm-pane");
                let result =
                    save_window_png(self.window, &path, Some(self.workspace_layout().terminal));
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
                    return IpcResponse::failure(format!(
                        "operation_persistence_failed[workspace.shutdown]: {error:#}"
                    ));
                }
                self.event_journal.commit(
                    EventKind::WorkspaceShutdown,
                    None,
                    serde_json::json!({"saved": true}),
                );
                self.close_requested = true;
                IpcResponse::success("")
            }
            "lscm" | "list-commands" => IpcResponse::success(supported_commands()),
            "splitw" | "split-window" => IpcResponse::failure(
                "split-window is not implemented yet; AgenTerm currently maps one ConPTY pane per tab",
            ),
            "kill-session" | "kill-server" => {
                if let Some(requested) = option_value(args, "-t")
                    && requested != self.session_name
                {
                    return IpcResponse::failure(if command == "kill-session" {
                        format!("can't find session: {requested}")
                    } else {
                        format!(
                            "operation_target_not_found[server.kill]: \
                             can't find session: {requested}"
                        )
                    });
                }
                let mut terminal_shutdown_complete = true;
                self.finish_note_edit(false);
                self.cancel_terminal_selection(true);
                for tab in &mut self.tabs {
                    terminal_shutdown_complete &= tab.close_process();
                }
                self.tabs.clear();
                self.active = None;
                self.event_journal.commit(
                    EventKind::WorkspaceShutdown,
                    None,
                    serde_json::json!({
                        "saved": false,
                        "destroyed": true,
                        "terminal_shutdown_complete": terminal_shutdown_complete,
                    }),
                );
                self.close_requested = true;
                if terminal_shutdown_complete {
                    IpcResponse::success("")
                } else {
                    IpcResponse::typed_failure(
                        "server shutdown began, but a terminal worker missed its bounded deadline",
                        "terminal_shutdown_incomplete",
                        "internal",
                        false,
                    )
                }
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
        if self.proxy_edit_target.is_some() {
            self.close_proxy_editor();
        }
        self.save_active_composer();
        let id = self.tabs[position].id;
        self.finish_note_edit(false);
        self.cancel_terminal_selection(true);
        self.active = Some(id);
        self.load_active_composer();
        self.event_journal
            .commit(EventKind::TabSelected, Some(id), serde_json::json!({}));
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

fn draw_proxy_eye(device: HDC, rect: &RECT, color: COLORREF, slashed: bool) {
    if rect.right - rect.left < 8 || rect.bottom - rect.top < 6 {
        return;
    }
    let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
    let pupil = unsafe { CreateSolidBrush(color) };
    if pen.is_null() || pupil.is_null() {
        if !pen.is_null() {
            unsafe { DeleteObject(pen as HGDIOBJ) };
        }
        if !pupil.is_null() {
            unsafe { DeleteObject(pupil as HGDIOBJ) };
        }
        return;
    }
    let previous_pen = unsafe { SelectObject(device, pen as HGDIOBJ) };
    let previous_brush = unsafe { SelectObject(device, GetStockObject(NULL_BRUSH) as HGDIOBJ) };
    unsafe {
        Ellipse(device, rect.left, rect.top, rect.right, rect.bottom);
        SelectObject(device, pupil as HGDIOBJ);
        let center_x = (rect.left + rect.right) / 2;
        let center_y = (rect.top + rect.bottom) / 2;
        Ellipse(
            device,
            center_x - 2,
            center_y - 2,
            center_x + 3,
            center_y + 3,
        );
        if slashed {
            MoveToEx(device, rect.left - 1, rect.bottom, ptr::null_mut());
            LineTo(device, rect.right + 1, rect.top - 1);
        }
        SelectObject(device, previous_brush);
        SelectObject(device, previous_pen);
        DeleteObject(pupil as HGDIOBJ);
        DeleteObject(pen as HGDIOBJ);
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

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn current_upgrade_identity() -> UpgradeIdentity {
    let build = BuildIdentity::current();
    let known =
        |value: &str| (value != "unknown" && !value.trim().is_empty()).then(|| value.to_owned());
    UpgradeIdentity {
        protocol_version: Some(1),
        version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        git_commit: known(build.git_commit),
        profile: known(build.profile),
        cargo_lock_sha256: known(build.cargo_lock_sha256),
        artifact_manifest_sha256: known(build.artifact_manifest_sha256),
    }
}

fn control_request_identity(args: &[String]) -> Result<(OperationId, RequestIntent)> {
    if let Some(operation) = operation_for_args(args).map_err(anyhow::Error::msg)? {
        let intent = if operation.class == OperationClass::Observe {
            RequestIntent::Query
        } else {
            RequestIntent::Mutation
        };
        return Ok((
            OperationId::new(operation.id).map_err(anyhow::Error::msg)?,
            intent,
        ));
    }
    let command = args.first().map(String::as_str).unwrap_or("unknown");
    let intent = if matches!(
        canonical_control_command(command),
        "active-window"
            | "capture-pane"
            | "display-message"
            | "dump-cells"
            | "get-settings"
            | "has-session"
            | "inspect"
            | "list-panes"
            | "list-sessions"
            | "list-tab-tree"
            | "list-windows"
            | "read-events"
            | "show-composer"
            | "show-options"
            | "show-tab-note"
            | "show-tab-parent"
            | "workspace-info"
    ) {
        RequestIntent::Query
    } else {
        RequestIntent::Mutation
    };
    let identity = format!(
        "command.{}",
        canonical_control_command(command).replace('-', ".")
    );
    Ok((
        OperationId::new(identity).map_err(anyhow::Error::msg)?,
        intent,
    ))
}

fn control_payload_fingerprint(args: &[String]) -> Result<PayloadFingerprint> {
    Ok(PayloadFingerprint::from_bytes(&serde_json::to_vec(args)?))
}

fn error_category_from_wire(category: &str) -> ErrorCategory {
    match category {
        "validation" => ErrorCategory::Validation,
        "conflict" => ErrorCategory::Conflict,
        "not_found" => ErrorCategory::NotFound,
        "precondition" => ErrorCategory::Precondition,
        "availability" => ErrorCategory::Availability,
        "timeout" => ErrorCategory::Timeout,
        "policy" => ErrorCategory::Policy,
        "unsupported" => ErrorCategory::Unsupported,
        _ => ErrorCategory::Internal,
    }
}

fn build_control_request(
    args: &[String],
    request_id: Option<String>,
    deadline_ms: u64,
) -> Result<ControlRequest> {
    static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let request_id = request_id.unwrap_or_else(|| {
        format!(
            "client:{}:{}:{}",
            std::process::id(),
            unix_time_ms(),
            NEXT_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    });
    let (operation_id, intent) = control_request_identity(args)?;
    Ok(ControlRequest::new(
        RequestId::new(request_id).map_err(anyhow::Error::msg)?,
        operation_id,
        control_payload_fingerprint(args)?,
        intent,
        Some(unix_time_ms().saturating_add(deadline_ms)),
    ))
}

fn send_ipc_request(args: Vec<String>) -> Result<IpcResponse> {
    let control = build_control_request(&args, None, IPC_TIMEOUT.as_millis() as u64)?;
    send_ipc_request_to_with_timeout(&ipc_address(), args, Some(control), IPC_TIMEOUT)
}

fn send_control_request(args: Vec<String>, control: ControlRequest) -> Result<IpcResponse> {
    send_ipc_request_to_with_timeout(&ipc_address(), args, Some(control), IPC_TIMEOUT)
}

fn send_ipc_request_to_timeout(
    address: &str,
    args: Vec<String>,
    timeout: Duration,
) -> Result<IpcResponse> {
    let control = build_control_request(&args, None, timeout.as_millis() as u64)?;
    send_ipc_request_to_with_timeout(address, args, Some(control), timeout)
}

fn send_ipc_request_to_with_timeout(
    address: &str,
    args: Vec<String>,
    control: Option<ControlRequest>,
    timeout: Duration,
) -> Result<IpcResponse> {
    let socket = parse_loopback_ipc_address(address)?;
    let deadline = Instant::now() + timeout;
    let remaining = || {
        let duration = deadline.saturating_duration_since(Instant::now());
        if duration.is_zero() {
            anyhow::bail!("AgenTerm IPC request timed out");
        }
        Ok(duration)
    };
    let connect_timeout = timeout
        .min(Duration::from_millis(100))
        .max(Duration::from_millis(1));
    let mut connection = std::net::TcpStream::connect_timeout(&socket, connect_timeout)
        .context("AgenTerm server is not running")?;
    connection.set_write_timeout(Some(remaining()?))?;
    connection.write_all(serde_json::to_string(&IpcRequest { args, control })?.as_bytes())?;
    connection.set_write_timeout(Some(remaining()?))?;
    connection.write_all(b"\n")?;
    connection.set_write_timeout(Some(remaining()?))?;
    connection.flush()?;
    connection.set_read_timeout(Some(remaining()?))?;
    let mut reader = std::io::BufReader::new(connection);
    let response =
        read_bounded_ipc_line(&mut reader, IPC_MAX_RESPONSE_BYTES, "AgenTerm IPC response")?;
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
    let staged_identity = current_upgrade_identity();
    for instance in instances {
        if !instance_process_is_alive(instance.record.pid) {
            if let Err(error) = prune_instance(&instance) {
                eprintln!("{error:#}");
                return 1;
            }
            continue;
        }
        let response = send_ipc_request_to_timeout(
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
        let window_visible = snapshot
            .as_ref()
            .and_then(|value| value["window"]["visible"].as_bool());
        let window_detached = snapshot
            .as_ref()
            .and_then(|value| value["window"]["detached"].as_bool());
        let window_state = snapshot
            .as_ref()
            .and_then(|value| value["window"]["state"].as_str());
        let modal_kind = snapshot
            .as_ref()
            .and_then(|value| value["modal"]["kind"].as_str());
        let event_epoch = snapshot
            .as_ref()
            .and_then(|value| value["event_position"]["epoch"].as_str());
        let event_sequence = snapshot
            .as_ref()
            .and_then(|value| value["event_position"]["sequence"].as_u64());
        let running_identity = instance.record.upgrade_identity.clone().unwrap_or_default();
        let upgrade = running_identity.compare_staged(&staged_identity);
        let upgrade_explanation = upgrade.explanation();
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
            "window_visible": window_visible,
            "window_detached": window_detached,
            "window_state": window_state,
            "modal_kind": modal_kind,
            "event_epoch": event_epoch,
            "event_sequence": event_sequence,
            "running_identity": running_identity,
            "staged_identity": staged_identity,
            "upgrade": upgrade,
            "upgrade_explanation": upgrade_explanation,
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
        println!(
            "PID\tADDRESS\tVERSION\tSTATUS\tUPGRADE\tWINDOW\tTABS\tACTIVE\tSESSION\tWORKSPACE"
        );
        for view in views {
            let window = match (
                view["window_visible"].as_bool(),
                view["window_detached"].as_bool(),
                view["window_state"].as_str(),
            ) {
                (Some(false), Some(true), _) => "detached",
                (Some(true), _, Some(state)) => state,
                (Some(false), _, _) => "hidden",
                _ => "-",
            };
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                view["pid"].as_u64().unwrap_or_default(),
                view["address"].as_str().unwrap_or_default(),
                view["version"].as_str().unwrap_or_default(),
                view["status"].as_str().unwrap_or_default(),
                view["upgrade"]["status"].as_str().unwrap_or("unknown"),
                window,
                view["tab_count"].as_u64().unwrap_or_default(),
                view["active_tab"].as_str().unwrap_or("-"),
                view["session"].as_str().unwrap_or_default(),
                view["workspace_path"].as_str().unwrap_or_default(),
            );
        }
    }
    0
}

fn run_cli(arguments: Vec<String>, control_options: CliControlOptions) -> i32 {
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
    if arguments
        .first()
        .is_some_and(|command| command == "ui-action")
        && arguments
            .get(1)
            .is_some_and(|action| matches!(action.as_str(), "proxy-prepare" | "proxy-send-now"))
        && let Some(position) = arguments.iter().position(|argument| argument == "--stdin")
    {
        arguments.remove(position);
        let mut content = String::new();
        if let Err(error) = std::io::stdin()
            .take(PROXY_MAX_BYTES as u64 + 1)
            .read_to_string(&mut content)
        {
            eprintln!("failed to read proxy values from stdin: {error}");
            return 1;
        }
        if content.len() > PROXY_MAX_BYTES {
            eprintln!("proxy input exceeds {PROXY_MAX_BYTES} bytes");
            return 2;
        }
        arguments.push("--proxy-input".to_owned());
        arguments.push(content);
    }
    if let Some(command) = arguments.first_mut() {
        let canonical = canonical_control_command(command);
        if canonical != command {
            *command = canonical.to_owned();
        }
    }
    if let Err(error) = validate_operation_args(&arguments) {
        eprintln!("{error}");
        return 2;
    }
    let command = arguments.first().map(String::as_str).unwrap_or_default();
    if matches!(command, "list-commands" | "lscm") {
        print!("{}", supported_commands());
        return 0;
    }
    if command == "protocol-info" && !has_option(&arguments, "--running") {
        println!("{}", protocol_info_json("client_binary"));
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
    let control = match build_control_request(
        &arguments,
        control_options.request_id,
        control_options.deadline_ms.unwrap_or(5_000),
    ) {
        Ok(control) => control,
        Err(error) => {
            eprintln!("{error:#}");
            return 2;
        }
    };
    let mut response = send_control_request(arguments.clone(), control.clone());
    if response.is_err()
        && may_start_server
        && let Ok(executable) = gui_executable_path()
    {
        let executable = wide(&executable.to_string_lossy());
        let operation = wide("open");
        let parameters = if no_activate_from_environment() {
            wide(&format!("--no-activate --address {}", ipc_address()))
        } else {
            wide(&format!("--address {}", ipc_address()))
        };
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
            response = send_control_request(arguments.clone(), control.clone());
            if response.is_ok() {
                break;
            }
        }
    }
    match response {
        Ok(response) if response.ok => {
            if control_options.receipt_json {
                match response.receipt {
                    Some(receipt) => {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&receipt).unwrap_or_default()
                        );
                        return 0;
                    }
                    None => {
                        eprintln!("AgenTerm server did not return a control receipt");
                        return 1;
                    }
                }
            }
            if !response.output.is_empty() {
                print!("{}", response.output);
                if !response.output.ends_with('\n') {
                    println!();
                }
            }
            0
        }
        Ok(response) => {
            if control_options.receipt_json {
                if let Some(receipt) = response.receipt {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&receipt).unwrap_or_default()
                    );
                } else {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "error": response.error,
                            "error_code": response.error_code,
                            "error_category": response.error_category,
                            "retryable": response.retryable,
                        })
                    );
                }
            } else {
                eprintln!("{}", response.error);
            }
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
        None
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
    let audit_source_kind = match operation {
        ScriptOperation::Api => AuditSourceKind::Api,
        ScriptOperation::Eval => AuditSourceKind::Eval,
        ScriptOperation::Check | ScriptOperation::Run if source_label == "stdin" => {
            AuditSourceKind::Stdin
        }
        ScriptOperation::Check | ScriptOperation::Run => AuditSourceKind::File,
    };
    let profile_name = match profile {
        ScriptProfile::Pure => "pure",
        ScriptProfile::Observe => "observe",
    };
    let capabilities = if profile == ScriptProfile::Observe {
        vec!["observe".to_owned()]
    } else {
        Vec::new()
    };
    let mut audit_invocation = AuditInvocation {
        invocation_id: invocation_id.clone(),
        source_fingerprint: source_fingerprint(&source),
        source_kind: audit_source_kind,
        api_version: SCRIPT_API_VERSION,
        operation: operation_name.to_owned(),
        requested_profile: profile_name.to_owned(),
        effective_profile: profile_name.to_owned(),
        requested_capabilities: capabilities.clone(),
        effective_capabilities: capabilities,
        requested_budgets: audit_budgets(&budgets),
        effective_budgets: audit_budgets(&budgets),
        broker_operation_ids: Vec::new(),
    };
    let audit_sink = match ScriptAuditSink::discover() {
        Ok(sink) => sink,
        Err(error) => return report_audit_error(error),
    };
    let audit_started = Instant::now();
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
            let outcome = AuditOutcome {
                duration_ms: audit_duration_ms(audit_started),
                result_class: "configuration".to_owned(),
                failure_code: Some("host_worker_missing".to_owned()),
                denied: true,
                cancelled: false,
                timed_out: false,
                crashed: false,
            };
            if let Err(error) = audit_sink.append(&audit_invocation, &outcome) {
                return report_audit_error(error);
            }
            eprintln!(
                "agenterm-script.exe is not installed next to agenterm-cli.exe; \
                 scripting is an optional component"
            );
            return 2;
        }
    };
    let expected_invocation_id = invocation.invocation_id.clone();
    let deadline = Duration::from_millis(invocation.budgets.wall_time_ms);
    let broker_budgets = invocation.budgets.clone();
    let (result, cancel_requested) = match WorkerSupervisor::invoke(
        &executable,
        invocation,
        deadline,
        Duration::from_millis(150),
        |request, remaining| handle_script_broker(request, &broker_budgets, remaining),
    ) {
        Ok(outcome) => {
            let _worker_pid = outcome.worker_pid;
            audit_invocation.broker_operation_ids = outcome.broker_operation_ids;
            (outcome.result, outcome.cancel_requested)
        }
        Err(error) => {
            let audit_outcome = audit_outcome_for_supervisor_error(&error, audit_started);
            if let Err(audit_error) = audit_sink.append(&audit_invocation, &audit_outcome) {
                return report_audit_error(audit_error);
            }
            return report_supervisor_error(error);
        }
    };
    if result.envelope_version != SCRIPT_ENVELOPE_VERSION
        || result.api_version != SCRIPT_API_VERSION
        || result.invocation_id != expected_invocation_id
        || result.operation != Some(operation)
        || result.profile != Some(profile)
    {
        let audit_outcome = AuditOutcome {
            duration_ms: audit_duration_ms(audit_started),
            result_class: "host".to_owned(),
            failure_code: Some("host_worker_protocol".to_owned()),
            denied: true,
            cancelled: cancel_requested,
            timed_out: false,
            crashed: false,
        };
        if let Err(error) = audit_sink.append(&audit_invocation, &audit_outcome) {
            return report_audit_error(error);
        }
        eprintln!(
            "agenterm-script.exe returned a mismatched protocol result \
             (envelope/API/invocation/operation/profile identity)"
        );
        return 1;
    }
    let result_consistent = if result.ok {
        result.exit_class == ScriptExitClass::Success && result.failure.is_none()
    } else {
        result.exit_class != ScriptExitClass::Success && result.failure.is_some()
    };
    if !result_consistent {
        let audit_outcome = AuditOutcome {
            duration_ms: audit_duration_ms(audit_started),
            result_class: "host".to_owned(),
            failure_code: Some("host_worker_protocol".to_owned()),
            denied: true,
            cancelled: cancel_requested,
            timed_out: false,
            crashed: false,
        };
        if let Err(error) = audit_sink.append(&audit_invocation, &audit_outcome) {
            return report_audit_error(error);
        }
        eprintln!("agenterm-script.exe returned an inconsistent result envelope");
        return 1;
    }
    let audit_outcome = audit_outcome_for_result(&result, audit_started, cancel_requested);
    if let Err(error) = audit_sink.append(&audit_invocation, &audit_outcome) {
        return report_audit_error(error);
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

fn report_supervisor_error(error: SupervisorError) -> i32 {
    let (code, message, exit_class, exit_code) = match error {
        SupervisorError::ConcurrencyLimit => (
            "host_concurrency_limit",
            "script worker concurrency limit reached".to_owned(),
            "configuration",
            2,
        ),
        SupervisorError::HardTimeout { worker_pid } => (
            "host_hard_timeout",
            format!("script worker {worker_pid} exceeded the host deadline and was terminated"),
            "limit",
            3,
        ),
        SupervisorError::WorkerCrash {
            worker_pid,
            exit_code: worker_exit,
        } => (
            "host_worker_crash",
            format!("script worker {worker_pid} exited before a valid result ({worker_exit:?})"),
            "host",
            1,
        ),
        SupervisorError::Spawn(message) => ("host_worker_spawn", message, "host", 1),
        SupervisorError::Transport(message) => ("host_worker_transport", message, "host", 1),
        SupervisorError::Protocol(message) => ("host_worker_protocol", message, "host", 1),
    };
    eprintln!(
        "{}",
        serde_json::json!({
            "code": code,
            "message": message,
            "exit_class": exit_class,
        })
    );
    exit_code
}

fn script_broker_error(code: &str, message: impl Into<String>) -> ScriptBrokerResponse {
    ScriptBrokerResponse {
        ok: false,
        value: None,
        error: Some(ScriptBrokerError {
            code: code.to_owned(),
            message: message.into(),
            details: None,
        }),
    }
}

fn script_broker_ipc(
    arguments: Vec<String>,
    timeout: Duration,
) -> Result<String, ScriptBrokerResponse> {
    match send_ipc_request_to_timeout(&ipc_address(), arguments, timeout) {
        Ok(response) if response.ok => Ok(response.output),
        Ok(response) => {
            let code = match response.error_code.as_str() {
                "server_restart" | "journal_gap" | "future_sequence" => {
                    response.error_code.as_str()
                }
                _ => "broker_host_error",
            };
            Err(script_broker_error(code, response.error))
        }
        Err(error) => Err(script_broker_error(
            "broker_transport",
            format!("{error:#}"),
        )),
    }
}

fn handle_script_broker(
    request: &ScriptBrokerRequest,
    budgets: &ScriptBudgets,
    remaining: Duration,
) -> ScriptBrokerResponse {
    let started = Instant::now();
    let parse_json = |output: String| {
        serde_json::from_str::<serde_json::Value>(&output)
            .map_err(|error| script_broker_error("broker_invalid_response", error.to_string()))
    };
    let value = match request.operation.as_str() {
        "workspace.info" => script_broker_ipc(vec!["workspace-info".to_owned()], remaining)
            .and_then(parse_json)
            .and_then(|mut workspace| {
                let request_remaining = remaining.saturating_sub(started.elapsed());
                if request_remaining.is_zero() {
                    return Err(script_broker_error(
                        "broker_transport",
                        "workspace observation exceeded the host deadline",
                    ));
                }
                let snapshot = script_broker_ipc(vec!["ui-snapshot".to_owned()], request_remaining)
                    .and_then(parse_json)?;
                workspace["event_position"] = snapshot
                    .get("event_position")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                Ok(workspace)
            }),
        "ui.snapshot" | "tabs.list" | "tabs.active" => {
            script_broker_ipc(vec!["ui-snapshot".to_owned()], remaining)
                .and_then(parse_json)
                .map(|snapshot| match request.operation.as_str() {
                    "tabs.list" => snapshot
                        .get("tabs")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!([])),
                    "tabs.active" => snapshot
                        .get("tabs")
                        .and_then(serde_json::Value::as_array)
                        .and_then(|tabs| {
                            tabs.iter().find(|tab| {
                                tab.get("active").and_then(serde_json::Value::as_bool) == Some(true)
                            })
                        })
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    _ => snapshot,
                })
        }
        "pane.capture" => {
            let tab = request
                .arguments
                .get("tab")
                .and_then(serde_json::Value::as_str);
            let requested = request
                .arguments
                .get("max_bytes")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok());
            match (tab, requested) {
                (Some(tab), Some(requested))
                    if tab.starts_with('@') && (1..=budgets.capture_bytes).contains(&requested) =>
                {
                    script_broker_ipc(
                        vec![
                            "capture-pane".to_owned(),
                            "-p".to_owned(),
                            "-t".to_owned(),
                            tab.to_owned(),
                            "--max-bytes".to_owned(),
                            requested.to_string(),
                            "--json".to_owned(),
                        ],
                        remaining,
                    )
                    .and_then(parse_json)
                }
                _ => Err(script_broker_error(
                    "broker_invalid_arguments",
                    "pane.capture requires stable tab @ID and max_bytes within the capture budget",
                )),
            }
        }
        "events.read" => {
            let epoch = request
                .arguments
                .get("epoch")
                .and_then(serde_json::Value::as_str);
            let after = request
                .arguments
                .get("after")
                .and_then(serde_json::Value::as_u64);
            let limit = request
                .arguments
                .get("limit")
                .and_then(serde_json::Value::as_u64);
            match (epoch, after, limit) {
                (Some(epoch), Some(after), Some(limit))
                    if limit > 0 && limit <= budgets.event_items as u64 =>
                {
                    script_broker_ipc(
                        vec![
                            "read-events".to_owned(),
                            "--epoch".to_owned(),
                            epoch.to_owned(),
                            "--after".to_owned(),
                            after.to_string(),
                            "--limit".to_owned(),
                            limit.to_string(),
                        ],
                        remaining,
                    )
                    .and_then(parse_json)
                }
                _ => Err(script_broker_error(
                    "broker_invalid_arguments",
                    "events.read requires epoch, nonnegative after, and a bounded positive limit",
                )),
            }
        }
        "events.wait" => script_broker_wait(&request.arguments, budgets, remaining),
        _ => Err(script_broker_error(
            "broker_operation_denied",
            format!(
                "operation {} is not available to observe scripts",
                request.operation
            ),
        )),
    };
    match value {
        Ok(value) => ScriptBrokerResponse {
            ok: true,
            value: Some(value),
            error: None,
        },
        Err(response) => response,
    }
}

fn script_broker_wait(
    arguments: &serde_json::Value,
    budgets: &ScriptBudgets,
    remaining: Duration,
) -> Result<serde_json::Value, ScriptBrokerResponse> {
    let epoch = arguments
        .get("epoch")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| script_broker_error("broker_invalid_arguments", "epoch is required"))?;
    let mut after = arguments
        .get("after")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| script_broker_error("broker_invalid_arguments", "after is required"))?;
    let kind = arguments
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| script_broker_error("broker_invalid_arguments", "kind is required"))?;
    let timeout_ms = arguments
        .get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| script_broker_error("broker_invalid_arguments", "timeout_ms is required"))?;
    if timeout_ms == 0 || timeout_ms > budgets.wait_time_ms {
        return Err(script_broker_error(
            "broker_invalid_arguments",
            "timeout_ms exceeds the wait budget",
        ));
    }
    let deadline = Instant::now()
        + Duration::from_millis(timeout_ms)
            .min(remaining.saturating_sub(Duration::from_millis(10)));
    loop {
        let ipc_remaining = deadline.saturating_duration_since(Instant::now());
        if ipc_remaining.is_zero() {
            return Err(script_broker_error(
                "event_wait_timeout",
                format!("no {kind} event arrived within {timeout_ms} ms"),
            ));
        }
        let output = script_broker_ipc(
            vec![
                "read-events".to_owned(),
                "--epoch".to_owned(),
                epoch.to_owned(),
                "--after".to_owned(),
                after.to_string(),
                "--limit".to_owned(),
                budgets.event_items.min(256).to_string(),
            ],
            ipc_remaining,
        )?;
        let batch: serde_json::Value = serde_json::from_str(&output)
            .map_err(|error| script_broker_error("broker_invalid_response", error.to_string()))?;
        if let Some(events) = batch.get("events").and_then(serde_json::Value::as_array) {
            for event in events {
                if let Some(sequence) = event.get("sequence").and_then(serde_json::Value::as_u64) {
                    after = after.max(sequence);
                }
                if event.get("kind").and_then(serde_json::Value::as_str) == Some(kind) {
                    return Ok(event.clone());
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(script_broker_error(
                "event_wait_timeout",
                format!("no {kind} event arrived within {timeout_ms} ms"),
            ));
        }
        thread::sleep(
            Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn audit_budgets(budgets: &ScriptBudgets) -> AuditBudgets {
    AuditBudgets {
        source_bytes: budgets.source_bytes,
        operations: budgets.operations,
        call_depth: budgets.call_depth,
        expression_depth: budgets.expression_depth,
        collection_items: budgets.collection_items,
        string_bytes: budgets.string_bytes,
        output_bytes: budgets.output_bytes,
        wall_time_ms: budgets.wall_time_ms,
        broker_requests: budgets.broker_requests,
        broker_return_bytes: budgets.broker_return_bytes,
        capture_bytes: budgets.capture_bytes,
        event_items: budgets.event_items,
        wait_time_ms: budgets.wait_time_ms,
    }
}

fn audit_duration_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn audit_outcome_for_result(
    result: &script_protocol::ScriptResult,
    started: Instant,
    cancel_requested: bool,
) -> AuditOutcome {
    let failure_code = result.failure.as_ref().map(|failure| failure.code.clone());
    let denied = result.exit_class == ScriptExitClass::Configuration
        || failure_code.as_deref().is_some_and(|code| {
            matches!(
                code,
                "script_api_unknown"
                    | "script_api_unavailable"
                    | "configuration_observation"
                    | "protocol_broker_unavailable"
            )
        });
    let timed_out = failure_code.as_deref() == Some("limit_wall_time");
    AuditOutcome {
        duration_ms: audit_duration_ms(started),
        result_class: result.exit_class.as_str().to_owned(),
        failure_code,
        denied,
        cancelled: cancel_requested,
        timed_out,
        crashed: false,
    }
}

fn audit_outcome_for_supervisor_error(error: &SupervisorError, started: Instant) -> AuditOutcome {
    let (result_class, failure_code, denied, cancelled, timed_out, crashed) = match error {
        SupervisorError::ConcurrencyLimit => (
            "configuration",
            "host_concurrency_limit",
            true,
            false,
            false,
            false,
        ),
        SupervisorError::HardTimeout { .. } => {
            ("limit", "host_hard_timeout", false, true, true, false)
        }
        SupervisorError::WorkerCrash { .. } => {
            ("host", "host_worker_crash", false, false, false, true)
        }
        SupervisorError::Spawn(_) => ("host", "host_worker_spawn", true, false, false, false),
        SupervisorError::Transport(_) => {
            ("host", "host_worker_transport", false, false, false, false)
        }
        SupervisorError::Protocol(_) => ("host", "host_worker_protocol", true, false, false, false),
    };
    AuditOutcome {
        duration_ms: audit_duration_ms(started),
        result_class: result_class.to_owned(),
        failure_code: Some(failure_code.to_owned()),
        denied,
        cancelled,
        timed_out,
        crashed,
    }
}

fn report_audit_error(message: String) -> i32 {
    eprintln!(
        "{}",
        serde_json::json!({
            "code": "host_audit_write",
            "message": message,
            "exit_class": "host",
        })
    );
    1
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
        let running = send_ipc_request_to_timeout(
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

fn resolve_stable_wait_target(selector: &str) -> Result<String> {
    let response = send_ipc_request(vec![
        "display-message".to_owned(),
        "-p".to_owned(),
        "-t".to_owned(),
        selector.to_owned(),
        "#{window_id}".to_owned(),
    ])?;
    if !response.ok || !response.output.trim().starts_with('@') {
        anyhow::bail!(
            "wait target could not be resolved to a stable tab ID: {}",
            response.error
        );
    }
    Ok(response.output.trim().to_owned())
}

fn fetch_ui_wait_snapshot() -> Result<(String, serde_json::Value)> {
    let response = send_ipc_request(vec!["ui-snapshot".to_owned()])?;
    if !response.ok {
        anyhow::bail!("{}", response.error);
    }
    let snapshot = serde_json::from_str::<serde_json::Value>(&response.output)
        .context("wait-ui received an invalid UI snapshot")?;
    Ok((response.output, snapshot))
}

fn run_wait_ui(arguments: &[String]) -> i32 {
    let timeout_ms = match option_value(arguments, "--timeout-ms") {
        Some(value) => match value.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                eprintln!("wait-ui --timeout-ms must be a non-negative integer");
                return 2;
            }
        },
        None => 10_000,
    };
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    let requested_active = option_value(arguments, "--active");
    let expected_focus = option_value(arguments, "--focus");
    let expected_state = option_value(arguments, "--tab-state");
    let expected_window_state = option_value(arguments, "--window-state");
    let expected_modal_kind = option_value(arguments, "--modal-kind");
    let requested_modal_target = option_value(arguments, "--modal-target");
    let expected_client_width =
        option_value(arguments, "--client-width").and_then(|value| value.parse::<i64>().ok());
    let expected_client_height =
        option_value(arguments, "--client-height").and_then(|value| value.parse::<i64>().ok());
    let requested_target = option_value(arguments, "-t");
    if expected_modal_kind.is_some_and(|kind| matches!(kind, "none" | "closed"))
        && requested_modal_target.is_some()
    {
        eprintln!("wait-ui --modal-target cannot be combined with --modal-kind none or closed");
        return 2;
    }
    if requested_active.is_none()
        && expected_focus.is_none()
        && expected_state.is_none()
        && expected_window_state.is_none()
        && expected_client_width.is_none()
        && expected_client_height.is_none()
        && expected_modal_kind.is_none()
        && requested_modal_target.is_none()
    {
        eprintln!(
            "wait-ui requires an active, focus, tab-state, window-state, client-size, \
             modal-kind, or modal-target condition"
        );
        return 1;
    }
    let expected_active = match requested_active.map(resolve_stable_wait_target).transpose() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let target = match requested_target.map(resolve_stable_wait_target).transpose() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let expected_modal_target = match requested_modal_target
        .map(resolve_stable_wait_target)
        .transpose()
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let (baseline_output, baseline_snapshot) = match fetch_ui_wait_snapshot() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let baseline_position = baseline_snapshot["event_position"].clone();
    let baseline_epoch = baseline_snapshot["event_position"]["epoch"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if baseline_epoch.is_empty() {
        eprintln!("wait-ui baseline did not contain a server epoch");
        return 1;
    }
    let mut pending_snapshot = Some((baseline_output, baseline_snapshot));
    loop {
        match pending_snapshot
            .take()
            .map(Ok)
            .unwrap_or_else(fetch_ui_wait_snapshot)
        {
            Ok((output, snapshot)) => {
                let current_epoch = snapshot["event_position"]["epoch"]
                    .as_str()
                    .unwrap_or_default();
                if current_epoch != baseline_epoch {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "code": "server_restart",
                            "expected_epoch": baseline_epoch,
                            "current_epoch": current_epoch,
                            "baseline_position": baseline_position,
                            "last_state": snapshot,
                        })
                    );
                    return 1;
                }
                let active_matches = expected_active.as_deref().is_none_or(|expected| {
                    snapshot["focus"]["window_id"].as_str() == Some(expected)
                });
                let focus_matches = expected_focus
                    .is_none_or(|expected| snapshot["focus"]["surface"].as_str() == Some(expected));
                let state_matches = expected_state.is_none_or(|expected| {
                    snapshot["tabs"].as_array().is_some_and(|tabs| {
                        tabs.iter().any(|tab| {
                            let target_matches = target
                                .as_deref()
                                .is_none_or(|selector| tab["id"].as_str() == Some(selector));
                            target_matches && tab["state"].as_str() == Some(expected)
                        })
                    })
                });
                let window_state_matches = expected_window_state
                    .is_none_or(|expected| snapshot["window"]["state"].as_str() == Some(expected));
                let width_matches = expected_client_width.is_none_or(|expected| {
                    snapshot["window"]["client_width"].as_i64() == Some(expected)
                });
                let height_matches = expected_client_height.is_none_or(|expected| {
                    snapshot["window"]["client_height"].as_i64() == Some(expected)
                });
                let modal_matches = snapshot_modal_matches(
                    &snapshot,
                    expected_modal_kind,
                    expected_modal_target.as_deref(),
                );
                if active_matches
                    && focus_matches
                    && state_matches
                    && window_state_matches
                    && width_matches
                    && height_matches
                    && modal_matches
                {
                    println!("{output}");
                    return 0;
                }
                let resolved_target_closed = target.as_deref().is_some_and(|resolved| {
                    !snapshot["tabs"].as_array().is_some_and(|tabs| {
                        tabs.iter().any(|tab| tab["id"].as_str() == Some(resolved))
                    })
                });
                if resolved_target_closed {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "code": "ui_wait_target_closed",
                            "target": target,
                            "baseline_position": baseline_position,
                            "last_state": snapshot,
                        })
                    );
                    return 1;
                }
                if std::time::Instant::now() >= deadline {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "code": "ui_wait_timeout",
                            "timeout_ms": timeout_ms,
                            "expected": {
                                "active": expected_active,
                                "focus": expected_focus,
                                "tab_state": expected_state,
                                "target": target,
                                "window_state": expected_window_state,
                                "client_width": expected_client_width,
                                "client_height": expected_client_height,
                                "modal_kind": expected_modal_kind,
                                "modal_target": expected_modal_target,
                            },
                            "baseline_position": baseline_position,
                            "last_state": snapshot,
                        })
                    );
                    return 1;
                }
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
    let requested_target = option_value(arguments, "-t").map(str::to_owned);
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
    let wait_finalized = arguments.iter().any(|argument| argument == "--finalized");
    if contains.is_none() && !wait_dead && !wait_submit && !wait_finalized {
        eprintln!(
            "usage: wait-pane [-t target] \
             (--contains text | --dead | --submit-complete | --finalized) \
             [--timeout-ms ms]"
        );
        return 2;
    }
    let mut resolve_request = vec![
        "display-message".to_owned(),
        "-p".to_owned(),
        "#{window_id}".to_owned(),
    ];
    if let Some(target) = &requested_target {
        resolve_request.extend(["-t".to_owned(), target.clone()]);
    }
    let target = match send_ipc_request(resolve_request) {
        Ok(response) if response.ok && response.output.trim().starts_with('@') => {
            response.output.trim().to_owned()
        }
        Ok(response) => {
            eprintln!("{}", response.error);
            return 1;
        }
        Err(error) => {
            eprintln!("{error:#}");
            return 1;
        }
    };
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let mut matched = true;
        if wait_dead {
            let mut request = vec![
                "display-message".to_owned(),
                "-p".to_owned(),
                "#{pane_dead}".to_owned(),
            ];
            request.extend(["-t".to_owned(), target.clone()]);
            matched &= send_ipc_request(request)
                .is_ok_and(|response| response.ok && response.output.trim() == "1");
        }
        if let Some(needle) = &contains {
            let mut request = vec!["capture-pane".to_owned(), "-p".to_owned()];
            request.extend(["-t".to_owned(), target.clone()]);
            matched &= send_ipc_request(request)
                .is_ok_and(|response| response.ok && response.output.contains(needle));
        }
        if wait_submit || wait_finalized {
            let request = vec!["inspect".to_owned(), "-t".to_owned(), target.clone()];
            matched &= send_ipc_request(request).is_ok_and(|response| {
                response.ok
                    && serde_json::from_str::<serde_json::Value>(&response.output)
                        .ok()
                        .and_then(|snapshot| snapshot["windows"].as_array().cloned())
                        .is_some_and(|windows| {
                            !windows.is_empty()
                                && windows.iter().all(|window| {
                                    (!wait_submit
                                        || !window["submit_pending"].as_bool().unwrap_or(true))
                                        && (!wait_finalized
                                            || window["finalized"].as_bool().unwrap_or(false))
                                })
                        })
            });
        }
        if matched {
            return 0;
        }
        if std::time::Instant::now() >= deadline {
            eprintln!(
                "{}",
                serde_json::json!({
                    "code": "pane_wait_timeout",
                    "timeout_ms": timeout_ms,
                    "target": target,
                    "conditions": {
                        "contains": contains,
                        "dead": wait_dead,
                        "submit_complete": wait_submit,
                        "finalized": wait_finalized,
                    }
                })
            );
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
  agenterm-cli [--address HOST:PORT] [--request-id ID] [--deadline-ms MS]
                [--receipt-json] command [args...]
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
  agenterm-cli ui-action new-tab|new-child|edit-tab|toggle-tree|tabs-show|tabs-hide|tabs-toggle|toggle-tabs|tabs-set-width|select-tab|close-tab|close-window|keep-server-running|stop-server-and-exit|confirm|cancel|composer-send|copy-selection|open-cwd-editor|cwd-prepare|cwd-prepare-append|cwd-prepare-replace|cwd-send-now|open-proxy-editor|proxy-toggle-visibility|proxy-reveal-credentials|proxy-prepare|proxy-send-now
  agenterm-cli ui-action tabs-set-width --width 180..480
  agenterm-cli ui-action proxy-prepare|proxy-send-now [-t target] --stdin
  agenterm-cli focus terminal|composer|tabs [-t target]
  agenterm-cli wait-pane [-t target] (--contains text|--dead|--submit-complete) [--timeout-ms ms]
  agenterm-cli wait-ui [--active @id] [--focus surface] [-t target --tab-state state] [--modal-kind KIND|none|closed] [--modal-target target]
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

fn protocol_info_json(identity_scope: &str) -> String {
    let build_identity = BuildIdentity::current();
    serde_json::to_string_pretty(&serde_json::json!({
        "protocol_version": 1,
        "agenterm_version": env!("CARGO_PKG_VERSION"),
        "identity_scope": identity_scope,
        "pid": std::process::id(),
        "address": ipc_address(),
        "build_identity": build_identity,
        "build_identity_complete": build_identity.is_complete(),
        "upgrade_identity": current_upgrade_identity(),
        "control_contract": {
            "schema_version": control_contract::CONTROL_CONTRACT_SCHEMA_VERSION,
            "request_dedupe": true,
            "deadline_guard": true,
            "receipt_outcomes": ["committed", "accepted", "no_op", "outcome_unknown"],
        },
        "command_catalog": {
            "schema_version": COMMAND_CATALOG_SCHEMA_VERSION,
            "commands": COMMAND_CATALOG,
        },
        "operation_catalog": {
            "schema_version": OPERATION_CATALOG_SCHEMA_VERSION,
            "classification_only": true,
            "authorization_policy": false,
            "operations": OPERATION_CATALOG,
        },
        "event_catalog": {
            "schema_version": EVENT_CATALOG_SCHEMA_VERSION,
            "events": EVENT_CATALOG,
        },
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
            "instance_discovery": true,
            "typed_operations": true,
            "typed_events": true
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

fn save_window_png(window: HWND, path: &std::path::Path, pane: Option<PixelRect>) -> Result<()> {
    let mut client: RECT = unsafe { mem::zeroed() };
    let mut outer: RECT = unsafe { mem::zeroed() };
    unsafe {
        GetClientRect(window, &mut client);
        GetWindowRect(window, &mut outer);
    }
    let (source, source_x, source_y, width, height) = if let Some(pane) = pane {
        (
            unsafe { GetDC(window) },
            pane.left,
            pane.top,
            pane.width().max(1),
            pane.height().max(1),
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
        EditShortcut, FocusSurface, IpcResponse, ThemeId, bounded_utf8_prefix, edit_shortcut,
        effective_theme, gui_cli_guidance, gui_handoff_succeeded, is_latched_navigation_repeat,
        no_activate_from_value, normalize_terminal_paste, parse_gui_launch,
        parse_loopback_ipc_address, run_wait_ui, surface_navigation, terminal_copy_shortcut,
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
    fn surface_navigation_is_directional_and_preserves_native_edit_arrows() {
        assert_eq!(
            surface_navigation(FocusSurface::Terminal, true, false, false, 0x28),
            Some(FocusSurface::Composer)
        );
        assert_eq!(
            surface_navigation(FocusSurface::Composer, true, false, false, 0x26),
            Some(FocusSurface::Terminal)
        );
        assert_eq!(
            surface_navigation(FocusSurface::Terminal, true, false, false, 0x25),
            Some(FocusSurface::Tabs)
        );
        assert_eq!(
            surface_navigation(FocusSurface::Tabs, true, false, false, 0x27),
            Some(FocusSurface::Terminal)
        );
        assert_eq!(
            surface_navigation(FocusSurface::Composer, true, false, false, 0x25),
            None
        );
        assert_eq!(
            surface_navigation(FocusSurface::Settings, true, false, false, 0x25),
            None
        );
        assert_eq!(
            surface_navigation(FocusSurface::NoteEditor, true, false, false, 0x27),
            None
        );
        assert_eq!(
            surface_navigation(FocusSurface::Terminal, true, true, false, 0x28),
            None
        );
        assert_eq!(
            surface_navigation(FocusSurface::Terminal, true, false, true, 0x28),
            None
        );
    }

    #[test]
    fn only_repeat_messages_for_the_latched_navigation_key_are_consumed() {
        let repeat_lparam = 1_isize << 30;
        assert!(is_latched_navigation_repeat(
            Some(0x28),
            0x28,
            repeat_lparam
        ));
        assert!(!is_latched_navigation_repeat(Some(0x28), 0x28, 0));
        assert!(!is_latched_navigation_repeat(
            Some(0x28),
            0x25,
            repeat_lparam
        ));
        assert!(!is_latched_navigation_repeat(None, 0x28, repeat_lparam));
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

    #[test]
    fn gui_launcher_accepts_no_activate_and_address_in_either_order() {
        let (options, address) = parse_gui_launch(&[
            "--no-activate".to_owned(),
            "--address".to_owned(),
            "127.0.0.1:48815".to_owned(),
        ])
        .unwrap();
        assert!(options.no_activate);
        assert_eq!(address.as_deref(), Some("127.0.0.1:48815"));

        let (options, address) = parse_gui_launch(&[
            "--address".to_owned(),
            "127.0.0.1:48816".to_owned(),
            "--not-foreground".to_owned(),
        ])
        .unwrap();
        assert!(options.no_activate);
        assert_eq!(address.as_deref(), Some("127.0.0.1:48816"));
    }

    #[test]
    fn automation_no_activate_environment_has_explicit_false_values() {
        use std::ffi::OsStr;

        assert!(!no_activate_from_value(None));
        assert!(!no_activate_from_value(Some(OsStr::new(""))));
        assert!(!no_activate_from_value(Some(OsStr::new("0"))));
        assert!(!no_activate_from_value(Some(OsStr::new("FALSE"))));
        assert!(no_activate_from_value(Some(OsStr::new("1"))));
        assert!(no_activate_from_value(Some(OsStr::new("true"))));
    }

    #[test]
    fn gui_launcher_accepts_only_a_successful_server_handoff() {
        assert!(gui_handoff_succeeded(&IpcResponse::success("shown")));
        assert!(!gui_handoff_succeeded(&IpcResponse::failure(
            "unsupported launcher operation"
        )));
    }

    #[test]
    fn gui_launcher_rejects_duplicate_unknown_and_missing_options() {
        for arguments in [
            vec!["--no-activate", "--no-activate"],
            vec!["--no-activate", "--not-foreground"],
            vec!["--not-foreground", "--not-foreground"],
            vec![
                "--address",
                "127.0.0.1:48815",
                "--address",
                "127.0.0.1:48816",
            ],
            vec!["--address"],
            vec!["--address", "--no-activate"],
            vec!["--unknown"],
        ] {
            assert!(
                parse_gui_launch(&arguments.into_iter().map(str::to_owned).collect::<Vec<_>>())
                    .is_err()
            );
        }
    }

    #[test]
    fn wait_ui_rejects_closed_modal_with_a_target_without_polling() {
        let arguments = vec![
            "wait-ui".to_owned(),
            "--modal-kind".to_owned(),
            "closed".to_owned(),
            "--modal-target".to_owned(),
            "@1".to_owned(),
        ];
        assert_eq!(run_wait_ui(&arguments), 2);
    }

    #[test]
    fn bounded_capture_prefix_is_utf8_safe_and_preserves_legacy_unbounded_text() {
        let text = "ab终端";
        assert_eq!(bounded_utf8_prefix(text, text.len()), text);
        assert_eq!(bounded_utf8_prefix(text, 4), "ab");
        assert_eq!(bounded_utf8_prefix(text, 5), "ab终");
        assert_eq!(bounded_utf8_prefix(text, 0), "");
    }
}
