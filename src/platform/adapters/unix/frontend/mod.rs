mod clipboard;
mod cursor_blink;
pub(crate) mod font;
mod layout;
mod render;
mod screenshot;
mod wake;
pub(crate) use wake::request_gui_wake;
mod window_state;

use std::{
    collections::HashSet,
    env,
    path::Path,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use agenterm_platform::{
    input::{KeyPressState, LogicalKey as Key, NamedKey, NormalizedKeyEvent, PhysicalKeyCode},
    window_host::{
        GeometryChange, LogicalPoint, LogicalRect, LogicalSize, PixelWindow,
        PixelWindowApplication, PixelWindowDirective, PixelWindowError, PixelWindowEvent,
        PixelWindowMetrics, PixelWindowOptions, PointerButton, PointerButtonState, WheelDelta,
        XrgbPixelFrame,
    },
};
use unicode_width::UnicodeWidthStr;

use crate::{
    client::{no_activate_from_environment, resolved_ipc_endpoint},
    commands::{alternate_screen_wheel_bytes, option_value, screenshot_output_path},
    control_dispatch::{ControlHost, dispatch_shared_command, resolve_target_position},
    event_journal::{EventJournal, EventKind},
    frontend::{
        GuiHandoffResult, GuiLaunchResult, UNIX_GUI_CLI_NAME, UNIX_GUI_LAUNCH_POLICY,
        UNIX_GUI_USAGE, attempt_gui_handoff, gui_cli_guidance, gui_help_result,
        gui_launch_argument_error, is_gui_cli_guidance_error, parse_gui_launch_target,
        request_gui_wake_best_effort,
    },
    instances::{mark_intentional_shutdown, register_instance},
    ipc_transport::{IpcEnvelope, IpcServer, start_ipc_server},
    operations::{UI_TABS_SET_WIDTH, UI_TABS_SHOW},
    protocol::IpcResponse,
    pty::TerminalSize,
    settings::{
        AppConfig, MAX_TERMINAL_FONT_SIZE, MIN_TERMINAL_FONT_SIZE, config_path, load_config,
        save_config,
    },
    terminal_runtime::{TerminalLaunch, TerminalTab},
    theme::{AppearancePreset, decode_window_icon_png, window_title_for_preset},
    ui_clipboard::{
        TERMINAL_PASTE_LIMIT_BYTES, normalize_composer_paste, normalize_terminal_paste,
        terminal_paste_bytes,
    },
    ui_geometry::{
        ScrollbarHit, TERMINAL_SCROLLBAR_WIDTH, TreeRowActionDensity, TreeRowMode,
        WHEEL_ROWS_PER_NOTCH, WorkspaceToolbarLayout, composer_geometry, pixel_rect_json,
        scrollback_for_thumb_top, scrollbar_hit_test, sidebar_row_capacity,
        sidebar_tree_row_geometry, tabs_width_from_drag, terminal_cell_at, wheel_delta_units,
    },
    wake_signal::WakeSignal,
    working_context::{CwdSource, ShellKind, cwd_command, validate_path},
    workspace::{SavedTab, SavedWorkspace, save_workspace, workspace_path},
};

use self::wake::install_unix_wake;
use crate::frontend::close_confirmation::CloseConfirmation;
use crate::frontend::composer::ComposerWriteMode;
use crate::frontend::cwd_editor::CwdEditorDialog;
use crate::frontend::input;
use crate::frontend::instance_picker::{
    InstancePickerDialog, InstancePickerMode, InstancePickerRow, collect_instance_picker_rows,
};
use crate::frontend::server_strip_ui::{
    SERVER_TABS_REFRESH, ServerCloseConfirm, ServerContextAction, ServerTabContextMenu, StripRect,
    ServerContextMenuRects, layout_server_add_chip, layout_server_context_menu,
    layout_server_tab_chips, server_tab_chip_label,
};
use crate::frontend::interaction::{
    ApplicationMouseMode, CancelTarget, ConfirmTarget, FocusDirection, FocusState, FocusSurface,
    FocusTransitionGate, ModalSurface, MouseReportEncoding, MouseReportInput, MouseReportOutcome,
    ScrollbarThumbDrag, WheelAccumulator, WheelTarget, WindowCloseRequest, cancel_target,
    confirm_target, modal_surface_from_gate, mouse_report_outcome, route_wheel,
    sidebar_scroll_offset_for_thumb_top, system_menu_clipboard_state, window_close_request,
};
use crate::frontend::new_terminal;
use crate::frontend::pointer_input::{
    KeyRequest, PointerActionKind, PointerButtonKind, PointerRequest, RequestedModifiers,
};
use crate::frontend::selection::{
    AutoScrollDirection, AutoScrollStep, SelectionGesture, TerminalPoint, TerminalSelection,
    autoscroll_step, terminal_selection_text, visible_row_selection, word_selection,
};
use crate::frontend::settings::{self, SettingsDialog};
use crate::frontend::tab_editor::{TabEditorDialog, TabEditorFocus};
use crate::frontend::text_selection::{self, TextCursor};
use crate::frontend::window_close::{WindowCloseChoice, WindowCloseDialog};
use crate::ui_snapshot::{
    PROJECTION_EMBEDDED_GUI, TerminalSelectionSnapshotInput, archived_proxy_status_json,
    embedded_window_json, event_position_json, ime_status_snapshot_json, locale_json,
    schema_version_json, scrollbar_state_json, settings_json, system_menu_json,
    terminal_interaction_json, working_context_json,
};

use crate::frontend::new_terminal::{NewTerminalDialog, ui_action_open};
use cursor_blink::CursorBlink;
use font::resolved_font_name;
use render::{
    ComposerView, ConfirmCloseHit, ConfirmCloseView, FrameContent, ImePreeditView,
    NewShellChoice as RenderShellChoice, NewTerminalFocusView, NewTerminalHit,
    NewTerminalModalView, SettingsHit, SettingsModalView, SidebarTabRow, StatusBarView,
    TabEditorFocusView, TabEditorView, TerminalCursorStyle, TerminalGrid, TerminalLayerGeometry,
    TerminalPaint, ToolbarHit, WindowCloseHit, WindowCloseView, WorkspaceToolbarView,
    blit_terminal_layer, cell_metrics, effective_palette, grid_dimensions_for_terminal,
    render_frame, render_terminal_layer, scrollbar_view_from_geometry, sidebar_row_at_y,
    terminal_layer_geometry,
};
use window_state::{
    UnixAppWindowHandle, WindowStateTracker, WindowUiActionResult, apply_ui_action,
    window_snapshot_json,
};

use layout::{
    scrollbar_geometry, sidebar_width_u32, terminal_pixel_rect, u32_rect, workspace_layout_for,
};

struct PixelWindowHandle<'a> {
    window: &'a PixelWindow,
    title: &'a str,
}

impl UnixAppWindowHandle for PixelWindowHandle<'_> {
    fn focus_window(&self) {
        self.window.focus();
    }

    fn minimize_window(&self) {
        self.window.set_minimized(true);
    }

    fn maximize_window(&self) {
        self.window.set_maximized(true);
    }

    fn restore_window(&self) {
        self.window.set_minimized(false);
        self.window.set_maximized(false);
    }

    fn resize_client(&self, width: u32, height: u32) -> Result<(), String> {
        self.window
            .request_logical_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
            .map_err(|error| error.to_string())
    }

    fn client_size(&self) -> (u32, u32) {
        self.window
            .metrics()
            .map(|metrics| {
                (
                    metrics.logical_size.width.round().max(1.0) as u32,
                    metrics.logical_size.height.round().max(1.0) as u32,
                )
            })
            .unwrap_or((1, 1))
    }

    fn is_visible(&self) -> bool {
        self.window.visible()
    }

    fn window_title(&self) -> &str {
        self.title
    }
}

/// Inputs that invalidate the persistent terminal layer beyond per-row grid
/// damage. A geometry or selection difference forces a full layer repaint; a
/// cursor difference repaints only the previous and current cursor rows.
#[derive(Clone, Copy, PartialEq)]
struct TerminalLayerKey {
    geometry: TerminalLayerGeometry,
    cols: u16,
    rows: u16,
    palette: usize,
    selection: Option<TerminalSelection>,
    cursor: (u16, u16, bool),
    cursor_style: TerminalCursorStyle,
    cursor_shape: crate::terminal_cursor::TerminalCursorShape,
}

#[derive(Default)]
struct RenderBuffers {
    logical: Vec<u32>,
    terminal_layer: Vec<u32>,
    terminal_layer_key: Option<TerminalLayerKey>,
    /// Persistent physical-resolution frame. The chrome upscale runs only
    /// when the logical frame content changes; every present then costs one
    /// full-frame copy instead of a full-frame rescale.
    physical: Vec<u32>,
    physical_size: (u32, u32),
    logical_hash: u64,
    captured: Option<(u32, u32, Vec<u32>)>,
    capture_next: bool,
}

/// Picks the anchor for a shift-click selection extension: the xterm
/// convention is to keep whichever endpoint of the existing selection is
/// farther from the new click, so the click always grows/shrinks the near
/// edge rather than flipping the whole selection around.
fn shift_extend_anchor(selection: TerminalSelection, click: TerminalPoint) -> TerminalPoint {
    let (start, end) = selection.bounds();
    // Selections are line-major (row, then col), so distance is compared the
    // same way: row difference dominates, column difference only breaks ties
    // on the same row.
    let dist = |point: TerminalPoint| -> (u32, u32) {
        (
            u32::from(point.row).abs_diff(u32::from(click.row)),
            u32::from(point.col).abs_diff(u32::from(click.col)),
        )
    };
    if dist(start) >= dist(end) { start } else { end }
}

/// FNV-1a over the logical frame outside `exclude` (the terminal viewport,
/// which the persistent layer owns); cheap relative to the upscale it avoids.
fn frame_content_hash(pixels: &[u32], width: u32, exclude: Option<(u32, u32, u32, u32)>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut feed = |slice: &[u32]| {
        let mut chunks = slice.chunks_exact(2);
        for pair in &mut chunks {
            hash ^= u64::from(pair[0]) | (u64::from(pair[1]) << 32);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for pixel in chunks.remainder() {
            hash ^= u64::from(*pixel);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    let Some((left, top, exclude_width, exclude_height)) = exclude else {
        feed(pixels);
        return hash;
    };
    let width = width.max(1) as usize;
    let (left, right) = (
        (left as usize).min(width),
        ((left + exclude_width) as usize).min(width),
    );
    for (row, row_pixels) in pixels.chunks(width).enumerate() {
        let row = row as u32;
        if row < top || row >= top + exclude_height {
            feed(row_pixels);
        } else {
            feed(&row_pixels[..left]);
            feed(&row_pixels[right.min(row_pixels.len())..]);
        }
    }
    hash
}

impl RenderBuffers {
    fn logical_frame(&mut self, width: u32, height: u32) -> &mut [u32] {
        self.logical.resize(width as usize * height as usize, 0);
        &mut self.logical
    }

    fn request_capture(&mut self) {
        self.captured = None;
        self.capture_next = true;
    }

    fn capture_if_requested(&mut self, width: u32, height: u32, pixels: &[u32]) {
        if self.capture_next {
            self.capture_next = false;
            self.captured = Some((width, height, pixels.to_vec()));
        }
    }

    fn take_capture(&mut self) -> Option<(u32, u32, Vec<u32>)> {
        self.capture_next = false;
        self.captured.take()
    }
}


#[derive(Clone, Copy, Debug)]
struct RecentSidebarTextClick {
    tab_id: u64,
    at: Instant,
    geometry_generation: u64,
}

impl RecentSidebarTextClick {
    fn matches(&self, tab_id: u64, geometry_generation: u64, now: Instant) -> bool {
        self.tab_id == tab_id
            && self.geometry_generation == geometry_generation
            && now.duration_since(self.at) <= Duration::from_millis(double_click_ms())
    }
}

#[derive(Clone, Copy, Debug)]
struct TabsResizeDrag {
    original_width: u16,
}

/// Multi-click grouping window, owned by shared input policy because the value
/// is per-host (macOS and each Linux desktop expose their own user setting)
/// rather than a Unix commonality.
fn double_click_ms() -> u64 {
    crate::platform::multi_click_interval_ms()
}

/// A recent composer click, used to promote repeats into word and line
/// selection. `offset` is compared so that a second click elsewhere starts a
/// fresh caret instead of selecting a word the user did not point at.
#[derive(Clone, Copy, Debug)]
struct ComposerClick {
    offset: usize,
    at: Instant,
    count: u8,
}

/// Converts a parsed `--mods` list into the modifier state a window event
/// carries, so synthetic gestures see the same shift/ctrl handling as real ones.
/// Launches a new GUI process bound to `instance`.
///
/// Mirrors the Windows helper of the same name; stdio is null so the child
/// never inherits this process's console.
fn spawn_gui_for_instance(instance: &str, endpoint: Option<&str>) -> Result<u32, String> {
    let exe = std::env::current_exe().map_err(|error| format!("resolve agenterm path: {error}"))?;
    let short = instance.strip_prefix("custom:").unwrap_or(instance);
    let mut command = std::process::Command::new(exe);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // `--endpoint` and `--instance` are conflicting selectors and the launch
    // parser rejects both together, so prefer the row's live endpoint when we
    // have one and fall back to the name otherwise.
    match endpoint {
        Some(endpoint) => {
            command.arg("--endpoint").arg(endpoint);
        }
        None => {
            command.arg("--instance").arg(short);
        }
    }
    command
        .spawn()
        .map(|child| child.id())
        .map_err(|error| format!("launch `{short}`: {error}"))
}

fn modifier_state(modifiers: RequestedModifiers) -> agenterm_platform::input::ModifierState {
    agenterm_platform::input::ModifierState {
        control: modifiers.control,
        shift: modifiers.shift,
        alt: modifiers.alt,
        meta: modifiers.meta,
    }
}

const APP_NAME: &str = "AgenTerm™";
const INITIAL_WIDTH: u32 = 960;
const INITIAL_HEIGHT: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnixFocusSurface {
    Terminal,
    Composer,
    Sidebar,
    Settings,
}

struct PendingTerminalPaste {
    tab_id: u64,
    receiver: Receiver<Result<String, TerminalPasteFailure>>,
}

#[derive(Debug, PartialEq, Eq)]
enum TerminalPasteFailure {
    Busy,
    Clipboard(crate::platform::contract::ui_clipboard::UiClipboardError),
    Empty,
    FocusRequired,
    ModalOpen,
    NoActiveTerminal,
    NormalizedTextTooLarge,
    StaleTarget,
    TerminalRejected,
    WorkerDisconnected,
    WorkerStart(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UiFeedbackError {
    code: String,
    category: &'static str,
    retryable: bool,
    message: String,
}

impl UiFeedbackError {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code,
            "category": self.category,
            "retryable": self.retryable,
            "message": self.message,
        })
    }
}

impl std::fmt::Display for TerminalPasteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => formatter.write_str("a terminal clipboard read is already pending"),
            Self::Clipboard(error) => write!(formatter, "clipboard read failed: {error}"),
            Self::Empty => formatter.write_str("clipboard text contains no pasteable characters"),
            Self::FocusRequired => formatter.write_str("paste requires terminal focus"),
            Self::ModalOpen => formatter.write_str("paste is unavailable while a modal is open"),
            Self::NoActiveTerminal => formatter.write_str("no active terminal is available"),
            Self::NormalizedTextTooLarge => write!(
                formatter,
                "normalized clipboard text exceeds the {TERMINAL_PASTE_LIMIT_BYTES}-byte limit"
            ),
            Self::StaleTarget => formatter.write_str(
                "clipboard paste was cancelled because the active terminal or focus changed",
            ),
            Self::TerminalRejected => formatter.write_str("terminal input was rejected"),
            Self::WorkerDisconnected => {
                formatter.write_str("clipboard read worker stopped without a result")
            }
            Self::WorkerStart(message) => {
                write!(
                    formatter,
                    "could not start clipboard read worker: {message}"
                )
            }
        }
    }
}

impl TerminalPasteFailure {
    fn code(&self) -> &str {
        match self {
            Self::Busy => "terminal_paste_busy",
            Self::Clipboard(
                crate::platform::contract::ui_clipboard::UiClipboardError::Unsupported { .. },
            ) => "terminal_paste_unsupported",
            Self::Clipboard(
                crate::platform::contract::ui_clipboard::UiClipboardError::Failed { code, .. },
            ) => code.as_ref(),
            _ => "terminal_paste_failed",
        }
    }

    fn category(&self) -> &'static str {
        match self {
            Self::Busy => "state",
            Self::Clipboard(
                crate::platform::contract::ui_clipboard::UiClipboardError::Unsupported { .. },
            ) => "unsupported",
            Self::Clipboard(
                crate::platform::contract::ui_clipboard::UiClipboardError::Failed { code, .. },
            ) if code.as_ref() == "clipboard_too_large" => "resource",
            Self::Clipboard(_) | Self::WorkerDisconnected | Self::WorkerStart(_) => "availability",
            Self::NormalizedTextTooLarge => "resource",
            Self::TerminalRejected => "transport",
            Self::Empty
            | Self::FocusRequired
            | Self::ModalOpen
            | Self::NoActiveTerminal
            | Self::StaleTarget => "precondition",
        }
    }

    fn retryable(&self) -> bool {
        match self {
            Self::Clipboard(
                crate::platform::contract::ui_clipboard::UiClipboardError::Unsupported { .. },
            ) => false,
            Self::Clipboard(
                crate::platform::contract::ui_clipboard::UiClipboardError::Failed { code, .. },
            ) => code.as_ref() != "clipboard_too_large",
            _ => matches!(
                self,
                Self::Busy
                    | Self::StaleTarget
                    | Self::TerminalRejected
                    | Self::WorkerDisconnected
                    | Self::WorkerStart(_)
            ),
        }
    }

    fn feedback_error(&self) -> UiFeedbackError {
        UiFeedbackError {
            code: self.code().to_owned(),
            category: self.category(),
            retryable: self.retryable(),
            message: self.to_string(),
        }
    }

    fn ipc_response(&self) -> IpcResponse {
        IpcResponse::typed_failure(
            self.to_string(),
            self.code(),
            self.category(),
            self.retryable(),
        )
    }
}

impl UnixFocusSurface {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Composer => "composer",
            Self::Sidebar => "sidebar",
            Self::Settings => "settings",
        }
    }

    const fn to_shared(self) -> Option<FocusSurface> {
        match self {
            Self::Terminal => Some(FocusSurface::Terminal),
            Self::Composer => Some(FocusSurface::Composer),
            Self::Sidebar => Some(FocusSurface::Sidebar),
            Self::Settings => None,
        }
    }

    fn from_ipc(value: &str) -> Result<Self, String> {
        if value == "settings" {
            return Ok(Self::Settings);
        }
        Ok(match FocusSurface::from_ipc(value)? {
            FocusSurface::Terminal => Self::Terminal,
            FocusSurface::Composer => Self::Composer,
            FocusSurface::Sidebar => Self::Sidebar,
        })
    }
}

enum SidebarTabAction {
    AddChild,
    Close,
    Save,
    Cancel,
}

pub(crate) fn run_gui_entry_result() -> GuiLaunchResult {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if let Some(result) = gui_help_result(&arguments, UNIX_GUI_USAGE) {
        return result;
    }
    let options = match parse_gui_launch_target(&arguments, UNIX_GUI_LAUNCH_POLICY) {
        Ok(options) => options,
        Err(message) => {
            let rendered = if is_gui_cli_guidance_error(&message) {
                gui_cli_guidance(&arguments, UNIX_GUI_CLI_NAME, UNIX_GUI_USAGE)
            } else {
                gui_launch_argument_error(&message, UNIX_GUI_USAGE, true)
            };
            eprintln!("{rendered}");
            return GuiLaunchResult::UsageError;
        }
    };
    let no_activate = options.no_activate || no_activate_from_environment();

    match attempt_gui_handoff(no_activate, true) {
        GuiHandoffResult::HandedOff => return GuiLaunchResult::Reused,
        GuiHandoffResult::Continue => {}
        GuiHandoffResult::Blocked(error) => {
            eprintln!(
                "The running AgenTerm server rejected the launcher handoff: {error}\n\
                Restart that server to use this launcher capability."
            );
            return GuiLaunchResult::BlockedByServer(error);
        }
    }

    if !display_available() {
        eprintln!(
            "AgenTerm GUI could not start: no graphical display was detected.\n\
             Set DISPLAY (X11) or WAYLAND_DISPLAY, or run from a desktop session."
        );
        return GuiLaunchResult::StartupFailed("no graphical display was detected".to_owned());
    }

    match run_gui(no_activate) {
        Ok(()) => GuiLaunchResult::Launched,
        Err(error) => {
            eprintln!("AgenTerm GUI failed: {error:#}");
            GuiLaunchResult::StartupFailed(error.to_string())
        }
    }
}

fn display_available() -> bool {
    !agenterm_platform::window::display_backend_facts().headless
}

fn run_gui(no_activate: bool) -> anyhow::Result<()> {
    let config = load_config();
    let instance_label = resolved_ipc_endpoint()
        .ok()
        .map(|resolved| resolved.logical_instance.display_name().to_string())
        .filter(|name| name != "default");
    let title = window_title_for_preset(
        config.appearance_preset,
        env!("CARGO_PKG_VERSION"),
        instance_label.as_deref(),
    );
    let wake_signal = Arc::new(WakeSignal::new());

    let ipc_server = start_ipc_server(0, Arc::clone(&wake_signal))?;
    let session_name = format!("agenterm-{}", std::process::id());
    let _instance = register_instance(&crate::ipc_address(), &workspace_path(), &session_name)?;

    let app = UnixApp::new(
        title.clone(),
        no_activate,
        wake_signal,
        ipc_server,
        session_name,
    );
    let options = PixelWindowOptions::new(
        title,
        LogicalSize::new(f64::from(INITIAL_WIDTH), f64::from(INITIAL_HEIGHT)),
    )
    .with_no_activate(no_activate)
    .with_ime_allowed(true)
    .with_window_icon_rgba(decode_window_icon_png(
        config.appearance_preset.window_icon_png(),
    ));
    agenterm_platform::window_host::run_pixel_window(options, Box::new(app))
        .map_err(anyhow::Error::new)
}

struct UnixApp {
    title: String,
    no_activate: bool,
    wake_signal: Arc<WakeSignal>,
    ipc_server: IpcServer,
    session_name: String,
    started_at: SystemTime,
    event_journal: EventJournal,
    named_buffers: crate::named_buffer::NamedBufferStore,
    window: Option<PixelWindow>,
    grid: Option<TerminalGrid>,
    tabs: Vec<TerminalTab>,
    active: Option<u64>,
    next_tab_id: u64,
    close_requested: bool,
    last_cursor: (f64, f64),
    focus_surface: UnixFocusSurface,
    focus_state: FocusState,
    composer_buffer: String,
    composer_select_all: bool,
    /// Caret and selection extent inside `composer_buffer`, in character
    /// offsets. Drives mouse selection, which `composer_select_all` alone
    /// could never express.
    composer_cursor: TextCursor,
    /// Running server instances shown as chips in the top strip.
    server_tabs: Vec<InstancePickerRow>,
    /// Next time `refresh_server_tabs_if_due` may re-scan the registry.
    server_tabs_refresh_after: Instant,
    /// Right-click menu on a server chip: `As Window` and `Close`.
    server_tab_context_menu: Option<ServerTabContextMenu>,
    /// Pending destructive close, awaiting confirmation.
    pending_server_close: Option<ServerCloseConfirm>,
    /// Modal list of running instances to enter or open.
    instance_picker_dialog: InstancePickerDialog,
    /// Set while the pointer is down inside the composer text, so pointer
    /// motion extends the selection instead of being forwarded to the
    /// terminal's mouse protocol.
    composer_selection_dragging: bool,
    /// Last click inside the composer, for promoting a second click to
    /// select-word and a third to select-line.
    composer_click: Option<ComposerClick>,
    text_field_select_all: bool,
    config: AppConfig,
    settings_dialog: SettingsDialog,

    new_terminal_dialog: NewTerminalDialog,
    new_terminal_focus: NewTerminalFocusView,
    window_state_tracker: WindowStateTracker,
    collapsed_tabs: HashSet<u64>,
    tab_editor_dialog: TabEditorDialog,
    wheel_accumulator: WheelAccumulator,
    scroll_drag: Option<ScrollbarThumbDrag>,
    sidebar_scroll_offset: usize,
    sidebar_scroll_drag: Option<ScrollbarThumbDrag>,
    terminal_selection: Option<TerminalSelection>,
    terminal_selection_gesture: Option<SelectionGesture>,
    terminal_selection_pointer: Option<(i32, i32)>,
    terminal_selection_autoscroll: Option<AutoScrollStep>,
    terminal_click_chain: crate::frontend::selection::ClickChain<u64, TerminalPoint>,
    recent_sidebar_text_click: Option<RecentSidebarTextClick>,
    sidebar_geometry_generation: u64,
    close_confirmation: CloseConfirmation,
    window_close_dialog: WindowCloseDialog,
    cwd_editor_dialog: CwdEditorDialog,
    tabs_resize_drag: Option<TabsResizeDrag>,
    render_buffers: RenderBuffers,
    status_message: String,
    last_feedback_error: Option<UiFeedbackError>,
    pending_terminal_paste: Option<PendingTerminalPaste>,
    ime_preedit: String,
    ime_cursor: Option<(usize, usize)>,
    cursor_blink: CursorBlink,
    window_focused: bool,
    last_present: Option<Instant>,
    output_redraw_pending: bool,
    last_workspace_save: Option<Instant>,
    last_saved_workspace: Option<SavedWorkspace>,
    pointer_modifiers: agenterm_platform::input::ModifierState,
    mouse_report_button: Option<u8>,
    mouse_report_cell: Option<(u16, u16)>,
}

/// Counts presented frames and reports the rate on stderr every five seconds
/// when `AGENTERM_FRAME_LOG` is set; free when the variable is absent.
fn note_frame_for_diagnostics() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("AGENTERM_FRAME_LOG").is_some()) {
        return;
    }
    static COUNT: AtomicU64 = AtomicU64::new(0);
    static WINDOW_START: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    let count = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let Ok(mut window_start) = WINDOW_START.lock() else {
        return;
    };
    let now = Instant::now();
    let start = *window_start.get_or_insert(now);
    let elapsed = now.duration_since(start);
    if elapsed >= Duration::from_secs(5) {
        eprintln!(
            "agenterm-frame-log: {count} frames in {elapsed:.1?} ({:.1}/s)",
            count as f64 / elapsed.as_secs_f64()
        );
        COUNT.store(0, Ordering::Relaxed);
        *window_start = Some(now);
    }
}

impl UnixApp {
    fn invalidate_sidebar_text_click(&mut self) {
        self.sidebar_geometry_generation = self.sidebar_geometry_generation.wrapping_add(1);
        self.recent_sidebar_text_click = None;
    }

    fn new(
        title: String,
        no_activate: bool,
        wake_signal: Arc<WakeSignal>,
        ipc_server: IpcServer,
        session_name: String,
    ) -> Self {
        let config = load_config();
        Self {
            title,
            no_activate,
            wake_signal,
            ipc_server,
            session_name,
            started_at: SystemTime::now(),
            event_journal: EventJournal::new(),
            named_buffers: crate::named_buffer::NamedBufferStore::new(),
            window: None,
            grid: None,
            tabs: Vec::new(),
            active: None,
            next_tab_id: 1,
            close_requested: false,
            last_cursor: (0.0, 0.0),
            focus_surface: UnixFocusSurface::Terminal,
            focus_state: FocusState::new(FocusSurface::Terminal, FocusTransitionGate::default()),
            composer_buffer: String::new(),
            composer_select_all: false,
            composer_cursor: TextCursor::default(),
            server_tabs: collect_instance_picker_rows().unwrap_or_default(),
            server_tabs_refresh_after: Instant::now(),
            server_tab_context_menu: None,
            pending_server_close: None,
            instance_picker_dialog: InstancePickerDialog::default(),
            composer_selection_dragging: false,
            composer_click: None,
            text_field_select_all: false,
            settings_dialog: SettingsDialog::new(
                config.effective_terminal_appearance(&crate::client::ipc_address(), None),
            ),

            new_terminal_dialog: NewTerminalDialog::new(),
            new_terminal_focus: NewTerminalFocusView::InitialCommand,
            window_state_tracker: WindowStateTracker::new(),
            collapsed_tabs: HashSet::new(),
            tab_editor_dialog: TabEditorDialog::new(),
            wheel_accumulator: WheelAccumulator::default(),
            scroll_drag: None,
            sidebar_scroll_offset: 0,
            sidebar_scroll_drag: None,
            terminal_selection: None,
            terminal_selection_gesture: None,
            terminal_selection_pointer: None,
            terminal_selection_autoscroll: None,
            terminal_click_chain: Default::default(),
            recent_sidebar_text_click: None,
            sidebar_geometry_generation: 0,
            close_confirmation: CloseConfirmation::new(),
            window_close_dialog: WindowCloseDialog::new(),
            cwd_editor_dialog: CwdEditorDialog::new(),
            tabs_resize_drag: None,
            render_buffers: RenderBuffers::default(),
            status_message: String::from("Ready"),
            last_feedback_error: None,
            pending_terminal_paste: None,
            ime_preedit: String::new(),
            ime_cursor: None,
            cursor_blink: CursorBlink::new(Instant::now()),
            window_focused: agenterm_platform::activation::ActivationPolicy::from_no_activate(
                no_activate,
            )
            .initial_window_focused,
            last_present: None,
            output_redraw_pending: false,
            last_workspace_save: None,
            last_saved_workspace: None,
            pointer_modifiers: agenterm_platform::input::ModifierState::empty(),
            mouse_report_button: None,
            mouse_report_cell: None,
            config,
        }
    }

    fn palette(&self) -> &'static crate::theme::ThemePalette {
        let configured = self.active_terminal_appearance().appearance_preset;
        effective_palette(
            configured,
            self.settings_dialog.preset_draft(),
            self.settings_dialog.is_open(),
        )
    }

    fn active_terminal_appearance(&self) -> crate::settings::EffectiveTerminalAppearance {
        let tab_id = self
            .active_position()
            .map(|position| format!("@{}", self.tabs[position].id));
        self.config
            .effective_terminal_appearance(&crate::ipc_address(), tab_id.as_deref())
    }

    fn adjust_active_terminal_font(&mut self, delta: i16) {
        let Some(position) = self.active_position() else {
            return;
        };
        let tab_id = format!("@{}", self.tabs[position].id);
        let effective = self.active_terminal_appearance();
        let size = (i32::from(effective.terminal_font_size) + i32::from(delta)).clamp(
            i32::from(MIN_TERMINAL_FONT_SIZE),
            i32::from(MAX_TERMINAL_FONT_SIZE),
        ) as u16;
        let mut terminal_override = self
            .config
            .terminal_override(&crate::ipc_address(), &tab_id);
        terminal_override.terminal_font_size = Some(size);
        self.config
            .set_terminal_override(&crate::ipc_address(), &tab_id, terminal_override);
        if let Err(error) = save_config(&self.config) {
            self.set_status_message(format!("Could not save terminal font size: {error:#}"));
        }
        self.resize_to_window();
    }

    fn toggle_locale(&mut self) {
        self.config.locale = self.config.locale.toggled();
        if let Err(error) = save_config(&self.config) {
            self.set_status_message(format!("Could not save locale: {error:#}"));
        }
    }

    fn layout(&self) -> crate::ui_geometry::WorkspaceLayout {
        let (width, height) = self.client_size();
        workspace_layout_for(width, height, &self.config)
    }

    fn sidebar_width(&self) -> u32 {
        sidebar_width_u32(&self.layout())
    }

    fn visible_tree_rows(&self) -> Vec<crate::tab_tree::TabTreeRow> {
        crate::tab_tree::visible_tree_rows(&self.all_tree_rows(), &self.collapsed_tabs)
    }

    fn commit_composer_draft(&mut self, position: usize) {
        let id = self.tabs[position].id;
        let composer = self.tabs[position].composer.clone();
        self.event_journal_mut().commit(
            EventKind::ComposerDraft,
            Some(id),
            serde_json::json!({
                "length": composer.chars().count(),
            }),
        );
    }

    fn sync_composer_buffer_to_tab(&mut self) {
        let Some(position) = self.active_position() else {
            return;
        };
        let tab_id = self.tabs[position].id;
        if self.cwd_editor_target_id() == Some(tab_id) {
            return;
        }
        if self.tabs[position].sensitive_composer.is_some() {
            return;
        }
        if self.tabs[position].composer != self.composer_buffer {
            self.tabs[position].composer = self.composer_buffer.clone();
            self.commit_composer_draft(position);
        }
    }

    fn load_composer_buffer_from_tab(&mut self) {
        self.composer_select_all = false;
        if self.cwd_editor_dialog.is_open() {
            return;
        }
        self.composer_buffer = self
            .active_position()
            .and_then(|position| self.tabs.get(position))
            .map(|tab| {
                if tab.sensitive_composer.is_some() {
                    "<sensitive proxy command · Ctrl+Enter to send>".to_owned()
                } else {
                    tab.composer.clone()
                }
            })
            .unwrap_or_default();
    }

    fn set_focus_surface_internal(&mut self, surface: UnixFocusSurface, cause: &str) {
        let previous = self.focus_surface;
        if previous == surface {
            return;
        }
        if previous == UnixFocusSurface::Composer {
            self.sync_composer_buffer_to_tab();
            self.composer_select_all = false;
        }
        self.focus_surface = surface;
        let gate = self.focus_gate();
        if let Some(semantic) = surface.to_shared() {
            self.focus_state = FocusState::new(semantic, gate);
        }
        self.reset_ime_context();
        self.cursor_blink.reset(Instant::now());
        if surface == UnixFocusSurface::Composer {
            self.load_composer_buffer_from_tab();
        }
        let active = self.active;
        self.event_journal_mut().commit(
            EventKind::FocusChanged,
            active,
            serde_json::json!({
                "from": previous.as_str(),
                "to": surface.as_str(),
                "cause": cause,
            }),
        );
        self.request_redraw();
    }

    fn clear_ime_preedit(&mut self) {
        self.ime_preedit.clear();
        self.ime_cursor = None;
    }

    fn reset_ime_context(&mut self) {
        self.clear_ime_preedit();
        if let Some(window) = self.window.as_ref() {
            window.set_ime_allowed(false);
            window.set_ime_allowed(true);
        }
    }

    fn commit_ime_text(&mut self, raw: &str) {
        if self.window_close_dialog.is_open()
            || self.close_confirmation.is_open()
            || self.settings_dialog.is_open()
        {
            return;
        }
        let raw = {
            use crate::platform::KeyClassification;
            let classified = agenterm_platform::input::classify_ime_commit(raw);
            match classified {
                KeyClassification::TextCommit(text) => text,
                _ => return,
            }
        };
        let raw = raw.as_str();
        if self.new_terminal_dialog.is_open() {
            let multiline = self.new_terminal_focus == NewTerminalFocusView::InitialCommand;
            let text = input::normalize_ime_commit(raw, multiline);
            let draft = match self.new_terminal_focus {
                NewTerminalFocusView::InitialCommand => {
                    self.new_terminal_dialog.initial_command_draft_mut()
                }
                NewTerminalFocusView::HttpProxy => self.new_terminal_dialog.http_proxy_draft_mut(),
                NewTerminalFocusView::HttpsProxy => {
                    self.new_terminal_dialog.https_proxy_draft_mut()
                }
            };
            input::prepare_composer_edit(draft, &mut self.text_field_select_all);
            draft.push_str(&text);
            self.request_redraw();
            return;
        }
        if self.tab_editor_dialog.is_open() {
            let multiline = self.tab_editor_dialog.focus() == TabEditorFocus::Note;
            let text = input::normalize_ime_commit(raw, multiline);
            let select_all = &mut self.text_field_select_all;
            let draft = self
                .tab_editor_dialog
                .active_draft_mut()
                .expect("tab editor is open");
            input::prepare_composer_edit(draft, select_all);
            draft.push_str(&text);
            self.request_redraw();
            return;
        }
        if self.focus_surface == UnixFocusSurface::Composer {
            let text = input::normalize_ime_commit(raw, true);
            input::prepare_composer_edit(&mut self.composer_buffer, &mut self.composer_select_all);
            self.composer_buffer.push_str(&text);
            self.sync_composer_buffer_to_tab();
            self.request_redraw();
            return;
        }
        if self.focus_surface == UnixFocusSurface::Terminal {
            let text = input::normalize_ime_commit(raw, false);
            // Empty commits happen on bare IME state toggles (Shift switches
            // CN/EN in macOS Chinese IMEs); they must not clear a selection.
            if text.is_empty() {
                return;
            }
            let _ = self.cancel_terminal_selection(true);
            self.queue_pty_input(text.into_bytes());
        }
    }

    fn handle_ime(&mut self, event: agenterm_platform::ime::ImeEvent) {
        self.cursor_blink.reset(Instant::now());
        match agenterm_platform::ime::classify_event(event, self.ime_anchor().is_some()) {
            agenterm_platform::ime::ImeAction::None => {}
            agenterm_platform::ime::ImeAction::UpdatePreedit { text, cursor } => {
                self.ime_preedit = text;
                self.ime_cursor = cursor;
                self.request_redraw();
            }
            agenterm_platform::ime::ImeAction::ClearPreedit => {
                self.clear_ime_preedit();
                self.request_redraw();
            }
            agenterm_platform::ime::ImeAction::CommitText(text) => {
                self.clear_ime_preedit();
                self.commit_ime_text(&text);
            }
            _ => self.clear_ime_preedit(),
        }
    }

    fn send_active_composer(&mut self) -> Result<(), String> {
        self.sync_composer_buffer_to_tab();
        if self.cwd_editor_dialog.is_open() {
            return self
                .prepare_cwd(None, None, ComposerWriteMode::Replace)
                .map_err(|error| error.to_string());
        }
        let Some(position) = self.active_position() else {
            return Err("no active window".to_owned());
        };
        if self.tabs[position].sensitive_composer.is_some() {
            return Err(
                "Composer contains a sensitive proxy draft; use IPC send-composer".to_owned(),
            );
        }
        let text = std::mem::take(&mut self.tabs[position].composer);
        self.composer_buffer.clear();
        if text.is_empty() {
            return Ok(());
        }
        if !self.tabs[position].submit(&text) {
            self.tabs[position].composer = text;
            self.composer_buffer = self.tabs[position].composer.clone();
            return Err("a composer submission is already pending".to_owned());
        }
        let id = self.tabs[position].id;
        self.event_journal_mut().commit(
            EventKind::ComposerSubmitted,
            Some(id),
            serde_json::json!({
                "length": text.chars().count(),
            }),
        );
        self.request_redraw();
        Ok(())
    }

    fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }

    fn record_terminal_paste_failure(&mut self, error: &TerminalPasteFailure) {
        self.status_message = format!("Paste failed: {error}");
        self.last_feedback_error = Some(error.feedback_error());
        self.request_redraw();
    }

    fn composer_send_hit(&self, x: f64, y: f64) -> bool {
        let send = composer_geometry(self.layout().composer).send;
        let (left, top, right, bottom) = (
            send.left as f64,
            send.top as f64,
            send.right as f64,
            send.bottom as f64,
        );
        x >= left && x < right && y >= top && y < bottom
    }

    fn paste_clipboard_into_composer(&mut self) -> Result<(), String> {
        if self.modal_surface_active() {
            return Err("paste is unavailable while a modal is open".to_owned());
        }
        let raw = clipboard::get_clipboard_text()?;
        let text = normalize_composer_paste(&raw);
        if text.is_empty() {
            return Err("clipboard text contains no pasteable characters".to_owned());
        }
        input::prepare_composer_edit(&mut self.composer_buffer, &mut self.composer_select_all);
        self.composer_buffer.push_str(&text);
        self.sync_composer_buffer_to_tab();
        self.set_status_message(format!("Pasted {} characters into composer", text.len()));
        self.request_redraw();
        Ok(())
    }

    fn composer_region_contains(&self, x: f64, y: f64) -> bool {
        self.layout().composer.contains(x as i32, y as i32)
    }

    /// Prefix drawn ahead of the composer draft. Shared by the renderer and
    /// the click hit-test so the two agree on where the text starts.
    fn composer_label(&self) -> &'static str {
        if self.cwd_editor_dialog.is_open() {
            "CWD> "
        } else {
            ""
        }
    }

    /// Character offset in the composer draft under a client-space point.
    fn composer_offset_at(&self, x: f64, y: f64) -> Option<usize> {
        let layout = self.layout();
        let geometry = composer_geometry(layout.composer);
        render::composer_offset_at_client(
            &self.composer_buffer,
            self.composer_label(),
            layout.composer.top.max(0) as u32,
            self.sidebar_width(),
            geometry.send.left.max(0) as u32,
            x,
            y,
        )
    }

    /// Place the caret, or extend/promote a selection, from a composer click.
    ///
    /// Follows the conventions users get from every native text field: a plain
    /// click places a caret, shift+click extends from the far end of the
    /// current selection, a second click selects the word and a third selects
    /// the line.
    fn begin_composer_selection(&mut self, x: f64, y: f64) -> bool {
        let Some(offset) = self.composer_offset_at(x, y) else {
            return false;
        };
        let now = Instant::now();
        let length = self.composer_buffer.chars().count();

        if self.pointer_modifiers.shift {
            let anchor = text_selection::shift_extend_anchor(self.composer_cursor, offset);
            self.set_composer_cursor(TextCursor::new(anchor, offset));
            self.composer_selection_dragging = true;
            self.composer_click = None;
            return true;
        }

        let repeat = self
            .composer_click
            .filter(|click| {
                click.offset == offset
                    && now.duration_since(click.at) <= Duration::from_millis(double_click_ms())
            })
            .map_or(1, |click| click.count.saturating_add(1).min(3));

        match repeat {
            2 => {
                let (start, end) = text_selection::word_bounds(&self.composer_buffer, offset);
                self.set_composer_cursor(TextCursor::new(start, end));
                self.composer_selection_dragging = false;
            }
            3 => {
                let (start, end) = text_selection::line_bounds(&self.composer_buffer, offset);
                self.set_composer_cursor(TextCursor::new(start, end));
                self.composer_selection_dragging = false;
            }
            _ => {
                self.set_composer_cursor(TextCursor::at(offset.min(length)));
                self.composer_selection_dragging = true;
            }
        }
        self.composer_click = Some(ComposerClick {
            offset,
            at: now,
            count: repeat,
        });
        true
    }

    /// Extend the composer selection while the pointer is held down.
    fn drag_composer_selection(&mut self, x: f64, y: f64) {
        if !self.composer_selection_dragging {
            return;
        }
        let Some(offset) = self.composer_offset_at(x, y) else {
            return;
        };
        self.set_composer_cursor(self.composer_cursor.extended_to(offset));
    }

    fn end_composer_selection(&mut self) {
        self.composer_selection_dragging = false;
    }

    /// Store a cursor, keeping `composer_select_all` consistent with it.
    ///
    /// The legacy flag still drives the "replace everything on next edit"
    /// behaviour and the full-width highlight, so it is derived here rather
    /// than left to drift out of step with the real selection.
    fn set_composer_cursor(&mut self, cursor: TextCursor) {
        let length = self.composer_buffer.chars().count();
        let cursor = cursor.clamped(length);
        self.composer_cursor = cursor;
        self.composer_select_all = length > 0 && cursor.range() == (0, length);
        self.request_redraw();
    }

    /// Text currently selected in the composer, if any.
    /// Re-scans running instances at most every `SERVER_TABS_REFRESH`.
    ///
    /// Returns whether the chip set changed, so the caller only repaints on a
    /// real difference rather than on every frame.
    fn refresh_server_tabs_if_due(&mut self) -> bool {
        let now = Instant::now();
        if now < self.server_tabs_refresh_after && !self.server_tabs.is_empty() {
            return false;
        }
        self.server_tabs_refresh_after = now + SERVER_TABS_REFRESH;
        let Ok(rows) = collect_instance_picker_rows() else {
            return false;
        };
        if rows == self.server_tabs {
            return false;
        }
        self.server_tabs = rows;
        true
    }

    /// Chip rectangles paired with the instance each one represents.
    fn server_tab_rects(&self) -> Vec<(crate::ui_geometry::PixelRect, &InstancePickerRow)> {
        let Some(strip) = self.layout().server_strip else {
            return Vec::new();
        };
        let strip = StripRect {
            left: strip.left,
            top: strip.top,
            right: strip.right,
            bottom: strip.bottom,
        };
        layout_server_tab_chips(strip, self.server_tabs.len())
            .into_iter()
            .zip(self.server_tabs.iter())
            .map(|(chip, row)| {
                (
                    crate::ui_geometry::PixelRect {
                        left: chip.left,
                        top: chip.top,
                        right: chip.right,
                        bottom: chip.bottom,
                    },
                    row,
                )
            })
            .collect()
    }

    /// Opens the instance picker over the live registry rows.
    fn open_instance_picker(&mut self, mode: InstancePickerMode) -> Result<(), String> {
        if self
            .focus_gate()
            .modal_entry_blocked(ModalSurface::InstancePicker)
            && !self.instance_picker_dialog.is_open()
        {
            return Err("another modal is open".to_owned());
        }
        let rows = collect_instance_picker_rows()?;
        self.instance_picker_dialog.open_with_rows(mode, rows);
        self.request_redraw();
        Ok(())
    }

    fn close_instance_picker(&mut self) {
        self.instance_picker_dialog.close();
        self.request_redraw();
    }

    /// Enters the highlighted instance.
    ///
    /// Like the strip chips, this opens a window on the target rather than
    /// rebinding this window's lease, which the embedded frontend cannot do.
    /// The dialog keeps its error visible on failure instead of closing, so a
    /// dead row does not silently dismiss the picker.
    fn confirm_instance_picker(&mut self) -> Result<(), String> {
        let Some(row) = self.instance_picker_dialog.selected_row().cloned() else {
            return Err("no instance is selected".to_owned());
        };
        let short = row
            .instance
            .strip_prefix("custom:")
            .unwrap_or(row.instance.as_str());
        match spawn_gui_for_instance(short, Some(row.endpoint.as_str())) {
            Ok(pid) => {
                self.close_instance_picker();
                self.set_status_message(format!(
                    "Opened `{}` in a new window (PID {pid})",
                    row.instance
                ));
                Ok(())
            }
            Err(error) => {
                self.instance_picker_dialog.set_error(error.clone());
                self.request_redraw();
                Err(error)
            }
        }
    }

    /// Opens the chip context menu at the pointer, anchored under the chip.
    fn open_server_tab_context_menu(&mut self, x: i32, y: i32, row: &InstancePickerRow) {
        if self.focus_gate().full_modal_blocked() {
            return;
        }
        self.server_tab_context_menu = Some(ServerTabContextMenu {
            instance: row.instance.clone(),
            endpoint: row.endpoint.clone(),
            can_attach: row.can_attach,
            origin_x: x,
            origin_y: y,
        });
        self.request_redraw();
    }

    fn dismiss_server_tab_context_menu(&mut self) -> bool {
        if self.server_tab_context_menu.take().is_some() {
            self.request_redraw();
            return true;
        }
        false
    }

    /// Menu frame plus the `As Window` and `Close` item rects.
    fn server_context_menu_geometry(
        &self,
    ) -> Option<ServerContextMenuRects<crate::ui_geometry::PixelRect>> {
        let menu = self.server_tab_context_menu.as_ref()?;
        let (client_width, client_height) = self.client_size();
        let client_right = i32::try_from(client_width).unwrap_or(i32::MAX);
        let client_bottom = i32::try_from(client_height).unwrap_or(i32::MAX);
        // Anchor under the owning chip while it is still on screen, so the menu
        // reads as belonging to that chip rather than floating at the cursor.
        let anchor_left = self
            .server_tab_rects()
            .into_iter()
            .find(|(_, row)| row.instance == menu.instance)
            .map(|(rect, _)| rect.left);
        let rects = layout_server_context_menu(
            menu.origin_x,
            menu.origin_y,
            client_right,
            client_bottom,
            anchor_left,
        );
        Some(rects.map(|r| crate::ui_geometry::PixelRect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }))
    }

    fn server_context_action_at(&self, x: i32, y: i32) -> Option<ServerContextAction> {
        let menu = self.server_context_menu_geometry()?;
        if menu.as_window.contains(x, y) {
            return Some(ServerContextAction::NewWindow);
        }
        if menu.close.contains(x, y) {
            return Some(ServerContextAction::Close);
        }
        None
    }

    /// Handles a click while the context menu is open.
    ///
    /// Returns whether the click was consumed. A click outside the menu only
    /// dismisses it and is *not* consumed, so the same press still reaches the
    /// strip or workbench underneath -- matching how native menus behave.
    fn handle_server_context_menu_click(&mut self, x: i32, y: i32) -> bool {
        let Some(menu_rects) = self.server_context_menu_geometry() else {
            return false;
        };
        if !menu_rects.frame.contains(x, y) {
            self.dismiss_server_tab_context_menu();
            return false;
        }
        let action = self.server_context_action_at(x, y);
        let Some(menu) = self.server_tab_context_menu.take() else {
            return true;
        };
        match action {
            Some(ServerContextAction::NewWindow) => {
                if let Err(error) = self.select_server_tab_by_instance(&menu.instance) {
                    self.set_status_message(error);
                }
            }
            Some(ServerContextAction::Close) => self.open_server_close_confirm(menu),
            None => {}
        }
        self.request_redraw();
        true
    }

    /// Shuts the target server down over IPC.
    ///
    /// Closing the server this window is attached to would strand the GUI, so
    /// that case is refused rather than half-performed; the user can close the
    /// window itself instead.
    fn shutdown_server_instance(&mut self, pending: &ServerCloseConfirm) -> Result<(), String> {
        if !pending.can_attach {
            return Err(format!(
                "server `{}` is not live and cannot be shut down from the strip",
                pending.instance
            ));
        }
        if pending.endpoint == crate::client::ipc_address() {
            return Err(format!(
                "`{}` is this window's own server; close the window instead",
                pending.instance
            ));
        }
        use crate::ipc_endpoint::IpcEndpoint;
        let parsed = pending
            .endpoint
            .parse::<IpcEndpoint>()
            .map_err(|error| format!("invalid endpoint {}: {error}", pending.endpoint))?;
        let previous = crate::client::resolved_ipc_endpoint().ok();
        let short = pending
            .instance
            .strip_prefix("custom:")
            .unwrap_or(pending.instance.as_str());
        // Pin the client selectors at the peer for one request, then always put
        // them back so this window keeps its own attachment even on failure.
        crate::frontend_server::pin_client_peer_for_gui(&parsed, Some(short))?;
        let shutdown = crate::client::send_ipc_request(vec!["shutdown".to_owned()]);
        if let Some(previous) = previous.as_ref() {
            let _ = crate::frontend_server::pin_client_peer_for_gui(
                &previous.endpoint,
                Some(&previous.logical_instance.canonical_name()),
            );
        }
        let response = shutdown.map_err(|error| format!("{error:#}"))?;
        if !response.ok {
            return Err(format!(
                "shutdown of `{}` failed: {}",
                pending.instance, response.error
            ));
        }
        Ok(())
    }

    /// Closes the server behind a chip.
    ///
    /// Windows stages this behind its own confirm modal. Unix has no
    /// `ServerClose` modal surface yet, and wiring a pending state with no way
    /// to confirm or cancel would strand the user, so this acts directly and
    /// relies on `shutdown_server_instance` to refuse the two dangerous cases
    /// (a stale row, and this window's own server).
    ///
    /// TODO(macos): add the confirm modal for parity once Unix grows a
    /// `ModalSurface::ServerClose`, so an accidental click on a *live* peer is
    /// recoverable rather than immediate.
    fn open_server_close_confirm(&mut self, menu: ServerTabContextMenu) {
        if !menu.can_attach {
            // A stale registration has no live owner to shut down; saying so
            // beats a confirm that could only fail.
            self.set_status_message(format!(
                "Server `{}` is not live; nothing to close",
                menu.instance
            ));
            return;
        }
        self.pending_server_close = Some(ServerCloseConfirm {
            instance: menu.instance,
            endpoint: menu.endpoint,
            can_attach: menu.can_attach,
        });
        if let Err(error) = self.finish_server_close_confirm(true) {
            self.set_status_message(error);
        }
    }

    fn finish_server_close_confirm(&mut self, confirm: bool) -> Result<(), String> {
        let Some(pending) = self.pending_server_close.take() else {
            return Ok(());
        };
        if !confirm {
            self.request_redraw();
            return Ok(());
        }
        let instance = pending.instance.clone();
        let result = self.shutdown_server_instance(&pending);
        self.server_tabs = collect_instance_picker_rows().unwrap_or_default();
        self.server_tabs_refresh_after = Instant::now() + SERVER_TABS_REFRESH;
        self.request_redraw();
        match result {
            Ok(()) => {
                self.set_status_message(format!("Server `{instance}` closed"));
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Opens the chip menu on a right-click inside the strip.
    ///
    /// Returns whether the press was consumed, so a right-click on chrome never
    /// reaches the terminal's mouse reporting.
    fn handle_server_strip_secondary_click(&mut self, x: i32, y: i32) -> bool {
        let Some(strip) = self.layout().server_strip else {
            return false;
        };
        if !strip.contains(x, y) {
            return false;
        }
        let row = self
            .server_tab_rects()
            .into_iter()
            .find(|(chip, _)| chip.contains(x, y))
            .map(|(_, row)| row.clone());
        match row {
            Some(row) => self.open_server_tab_context_menu(x, y, &row),
            // Right-clicking empty strip background just closes any open menu.
            None => {
                self.dismiss_server_tab_context_menu();
            }
        }
        true
    }

    /// Routes a click in the top strip to a chip or the add button.
    ///
    /// Returns whether the click was consumed, so the workbench below never
    /// sees a press aimed at the strip.
    fn handle_server_strip_click(&mut self, x: i32, y: i32) -> bool {
        let Some(strip) = self.layout().server_strip else {
            return false;
        };
        if !strip.contains(x, y) {
            return false;
        }
        let instance = self
            .server_tab_rects()
            .into_iter()
            .find(|(chip, _)| chip.contains(x, y))
            .map(|(_, row)| row.instance.clone());
        if let Some(instance) = instance {
            if let Err(error) = self.select_server_tab_by_instance(&instance) {
                self.set_status_message(error);
            }
            self.request_redraw();
        }
        // Clicks on the strip background (including the add button, which has
        // no Unix dialog yet) are still consumed: the strip is chrome, and
        // letting a press fall through to the terminal would be worse.
        true
    }

    /// Opens a window on `instance`, the way clicking a Windows chip does.
    ///
    /// Unix does not rebind the current GUI's lease the way Windows does, so
    /// this always spawns a window for the target rather than pretending to
    /// switch in place; that keeps the visible behaviour honest.
    fn select_server_tab_by_instance(&mut self, instance: &str) -> Result<(), String> {
        let row = self
            .server_tabs
            .iter()
            .find(|row| {
                row.instance == instance
                    || row.instance.strip_prefix("custom:") == Some(instance)
                    || row.instance_label == instance
            })
            .cloned()
            .ok_or_else(|| format!("server tab `{instance}` is not listed"))?;
        if row.endpoint == crate::client::ipc_address() {
            self.set_status_message(format!("Already on `{}`", row.instance));
            return Ok(());
        }
        let short = row
            .instance
            .strip_prefix("custom:")
            .unwrap_or(row.instance.as_str());
        let pid = spawn_gui_for_instance(short, Some(row.endpoint.as_str()))?;
        self.set_status_message(format!("Opened `{}` in a new window (PID {pid})", row.instance));
        Ok(())
    }

    fn composer_selected_text(&self) -> Option<String> {
        text_selection::selected_text(&self.composer_buffer, self.composer_cursor)
    }

    /// Deletes the composer selection when `event` is about to replace it.
    ///
    /// Returns whether anything was removed. Only keys that produce text or
    /// delete backwards count: a bare arrow key or a shortcut must move or act
    /// without destroying the draft, which is why this inspects the event
    /// instead of clearing the selection on every keystroke.
    fn take_composer_selection_for_edit(&mut self, event: &NormalizedKeyEvent) -> bool {
        if event.state != KeyPressState::Pressed || event.repeat {
            return false;
        }
        // A primary-shortcut chord is a command (copy, select-all), not text.
        if input::primary_shortcut(event.modifiers) {
            return false;
        }
        if !self.composer_cursor.has_selection() {
            return false;
        }
        let typed = match &event.logical {
            Key::Character(text) if !text.starts_with(char::is_control) => Some(text.clone()),
            Key::Named(NamedKey::Space) => Some(" ".to_owned()),
            Key::Named(NamedKey::Backspace | NamedKey::Delete) => Some(String::new()),
            _ => None,
        };
        let Some(typed) = typed else {
            return false;
        };
        // Replace in place. Deleting here and letting the shared key path
        // append would put the character at the end of the draft instead of
        // where the selection was -- selecting "hello" in "hello world" and
        // typing X has to give "X world", not " worldX".
        let cursor =
            text_selection::insert(&mut self.composer_buffer, self.composer_cursor, &typed);
        self.composer_select_all = false;
        self.set_composer_cursor(cursor);
        self.sync_composer_buffer_to_tab();
        self.request_redraw();
        true
    }

    fn focus_gate(&self) -> FocusTransitionGate {
        FocusTransitionGate {
            window_close_pending: self.window_close_dialog.is_open(),
            settings_open: self.settings_dialog.is_open(),
            new_terminal_open: self.new_terminal_dialog.is_open(),
            tab_editor_open: self.tab_editor_dialog.is_open(),
            close_confirmation_open: self.close_confirmation.is_open(),
            cwd_editor_open: self.cwd_editor_dialog.is_open(),
            instance_picker_open: self.instance_picker_dialog.is_open(),
            server_new_open: false,
            server_close_pending: false,
        }
    }

    fn modal_surface_active(&self) -> bool {
        self.focus_gate().blocked()
    }

    fn render_shell_choice(&self) -> RenderShellChoice {
        match self.new_terminal_dialog.shell_choice() {
            new_terminal::NewShellChoice::Default => RenderShellChoice::Default,
            new_terminal::NewShellChoice::Primary => RenderShellChoice::Primary,
            new_terminal::NewShellChoice::Alternate => RenderShellChoice::Bash,
        }
    }

    fn open_new_terminal_dialog(&mut self) {
        if self
            .focus_gate()
            .modal_entry_blocked(ModalSurface::NewTerminal)
        {
            return;
        }
        if self.settings_dialog.is_open() {
            let _ = self.close_settings(false);
        }
        if self.tab_editor_dialog.is_open() {
            let _ = self.complete_tab_editor(false);
        }
        if self.cwd_editor_dialog.is_open() {
            self.close_cwd_editor();
        }
        let _ = self.cancel_terminal_selection(true);
        self.reset_ime_context();
        ui_action_open(&mut self.new_terminal_dialog);
        self.new_terminal_focus = NewTerminalFocusView::InitialCommand;
        self.text_field_select_all = false;
        self.request_redraw();
    }

    fn finish_new_terminal_dialog(&mut self, create: bool) {
        self.reset_ime_context();
        self.text_field_select_all = false;
        let result = self.new_terminal_dialog.finish(create);
        match result {
            Ok(Some(params)) => {
                if let Ok(index) = self.create_tab(
                    None,
                    params.command_line,
                    params.tab_environment,
                    true,
                    None,
                ) && let Some(id) = self
                    .tabs
                    .iter()
                    .find(|tab| tab.index == index)
                    .map(|tab| tab.id)
                {
                    self.after_create_tab(id, None);
                }
            }
            Ok(None) => {}
            Err(error) => self.set_status_message(error),
        }
        self.request_redraw();
    }

    fn handle_new_terminal_click(&mut self, hit: NewTerminalHit) {
        let previous_focus = self.new_terminal_focus;
        match hit {
            NewTerminalHit::DefaultShell => {
                self.new_terminal_dialog
                    .choose_shell(new_terminal::NewShellChoice::Default);
            }
            NewTerminalHit::PrimaryShell => {
                self.new_terminal_dialog
                    .choose_shell(new_terminal::NewShellChoice::Primary);
            }
            NewTerminalHit::BashShell => {
                self.new_terminal_dialog
                    .choose_shell(new_terminal::NewShellChoice::Alternate);
            }
            NewTerminalHit::InitialCommand => {
                self.new_terminal_focus = NewTerminalFocusView::InitialCommand;
            }
            NewTerminalHit::HttpProxy => {
                self.new_terminal_focus = NewTerminalFocusView::HttpProxy;
            }
            NewTerminalHit::HttpsProxy => {
                self.new_terminal_focus = NewTerminalFocusView::HttpsProxy;
            }
            NewTerminalHit::Create => self.finish_new_terminal_dialog(true),
            NewTerminalHit::Cancel => self.finish_new_terminal_dialog(false),
        }
        if self.new_terminal_focus != previous_focus {
            self.text_field_select_all = false;
            self.reset_ime_context();
        }
        self.request_redraw();
    }

    fn handle_new_terminal_key(&mut self, event: &NormalizedKeyEvent) {
        if !self.new_terminal_dialog.is_open() {
            return;
        }
        let multiline = self.new_terminal_focus == NewTerminalFocusView::InitialCommand;
        let action = {
            let select_all = &mut self.text_field_select_all;
            let draft = match self.new_terminal_focus {
                NewTerminalFocusView::InitialCommand => {
                    self.new_terminal_dialog.initial_command_draft_mut()
                }
                NewTerminalFocusView::HttpProxy => self.new_terminal_dialog.http_proxy_draft_mut(),
                NewTerminalFocusView::HttpsProxy => {
                    self.new_terminal_dialog.https_proxy_draft_mut()
                }
            };
            input::text_field_key_action(event, draft, multiline, select_all)
        };
        match action {
            input::TextFieldKeyAction::Edited => self.request_redraw(),
            input::TextFieldKeyAction::NextField => {
                self.text_field_select_all = false;
                self.reset_ime_context();
                self.new_terminal_focus = match self.new_terminal_focus {
                    NewTerminalFocusView::InitialCommand => NewTerminalFocusView::HttpProxy,
                    NewTerminalFocusView::HttpProxy => NewTerminalFocusView::HttpsProxy,
                    NewTerminalFocusView::HttpsProxy => NewTerminalFocusView::InitialCommand,
                };
                self.request_redraw();
            }
            input::TextFieldKeyAction::Submit => self.finish_new_terminal_dialog(true),
            input::TextFieldKeyAction::Escape => self.finish_new_terminal_dialog(false),
            input::TextFieldKeyAction::SelectAll => self.request_redraw(),
            input::TextFieldKeyAction::Copy => {
                let draft = match self.new_terminal_focus {
                    NewTerminalFocusView::InitialCommand => {
                        self.new_terminal_dialog.initial_command_draft()
                    }
                    NewTerminalFocusView::HttpProxy => self.new_terminal_dialog.http_proxy_draft(),
                    NewTerminalFocusView::HttpsProxy => {
                        self.new_terminal_dialog.https_proxy_draft()
                    }
                };
                if clipboard::set_clipboard_text(draft).is_ok() {
                    self.set_status_message("Copied new-terminal draft");
                }
            }
            input::TextFieldKeyAction::Cut => {
                let draft = match self.new_terminal_focus {
                    NewTerminalFocusView::InitialCommand => {
                        self.new_terminal_dialog.initial_command_draft_mut()
                    }
                    NewTerminalFocusView::HttpProxy => {
                        self.new_terminal_dialog.http_proxy_draft_mut()
                    }
                    NewTerminalFocusView::HttpsProxy => {
                        self.new_terminal_dialog.https_proxy_draft_mut()
                    }
                };
                if clipboard::set_clipboard_text(draft).is_ok() {
                    draft.clear();
                    self.text_field_select_all = false;
                    self.set_status_message("Cut new-terminal draft");
                    self.request_redraw();
                }
            }
            input::TextFieldKeyAction::Paste => {
                if let Ok(raw) = clipboard::get_clipboard_text() {
                    let draft = match self.new_terminal_focus {
                        NewTerminalFocusView::InitialCommand => {
                            self.new_terminal_dialog.initial_command_draft_mut()
                        }
                        NewTerminalFocusView::HttpProxy => {
                            self.new_terminal_dialog.http_proxy_draft_mut()
                        }
                        NewTerminalFocusView::HttpsProxy => {
                            self.new_terminal_dialog.https_proxy_draft_mut()
                        }
                    };
                    input::prepare_composer_edit(draft, &mut self.text_field_select_all);
                    draft.push_str(&raw.replace("\r\n", "\n"));
                    self.request_redraw();
                }
            }
            input::TextFieldKeyAction::Ignored => {}
        }
    }

    fn handle_settings_key(&mut self, event: &NormalizedKeyEvent) {
        if !self.settings_dialog.is_open() {
            return;
        }
        match event.logical {
            Key::Named(NamedKey::Escape) => {
                let _ = self.close_settings(false);
            }
            Key::Named(NamedKey::Enter) => {
                let _ = self.close_settings(true);
            }
            _ => {}
        }
    }

    fn target_position(&self, target: Option<&str>) -> Option<usize> {
        resolve_target_position(&self.tabs, self.active, target)
    }

    fn active_cwd_status_text(&self) -> String {
        self.active_position()
            .and_then(|position| self.tabs.get(position))
            .map(|tab| {
                let path = tab.cwd.path().unwrap_or("unknown");
                let home_dir = env::var_os("HOME");
                let path = compact_cwd_for_status(path, home_dir.as_deref().map(Path::new));
                if tab.cwd.pending() {
                    format!("CWD · {path} · pending")
                } else {
                    format!("CWD · {path} · {}", tab.cwd.source().as_str())
                }
            })
            .unwrap_or_else(|| "CWD · unknown".to_owned())
    }

    fn cwd_editor_target_id(&self) -> Option<u64> {
        self.cwd_editor_dialog
            .target()
            .and_then(|target| target.strip_prefix('@'))
            .and_then(|value| value.parse().ok())
    }

    fn open_cwd_editor(&mut self, target: Option<&str>) -> Result<(), String> {
        if self
            .focus_gate()
            .modal_entry_blocked(ModalSurface::CwdEditor)
        {
            return Err("another modal surface is active".to_owned());
        }
        let _ = self.cancel_terminal_selection(true);
        let position = self
            .target_position(target)
            .or_else(|| self.active_position())
            .ok_or_else(|| "can't find tab".to_owned())?;
        self.sync_composer_buffer_to_tab();
        let id = self.tabs[position].id;
        self.active = Some(id);
        self.cwd_editor_dialog.open(format!("@{id}"));
        self.composer_buffer = self.tabs[position]
            .cwd
            .path()
            .unwrap_or_default()
            .to_owned();
        self.set_focus_surface_internal(UnixFocusSurface::Composer, "cwd-editor");
        self.event_journal_mut().commit(
            EventKind::WorkingContextCwdEditor,
            Some(id),
            serde_json::json!({"open": true}),
        );
        self.request_redraw();
        Ok(())
    }

    fn close_cwd_editor(&mut self) {
        let Some(id) = self
            .cwd_editor_dialog
            .close_and_take_target()
            .and_then(|target| {
                target
                    .strip_prefix('@')
                    .and_then(|value| value.parse().ok())
            })
        else {
            return;
        };
        self.load_composer_buffer_from_tab();
        self.set_focus_surface_internal(UnixFocusSurface::Terminal, "cwd-editor-close");
        self.event_journal_mut().commit(
            EventKind::WorkingContextCwdEditor,
            Some(id),
            serde_json::json!({"open": false}),
        );
        self.request_redraw();
    }

    fn prepare_cwd(
        &mut self,
        target: Option<&str>,
        requested_path: Option<String>,
        mode: ComposerWriteMode,
    ) -> Result<(), String> {
        let position = self
            .target_position(target)
            .or_else(|| {
                self.cwd_editor_target_id()
                    .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
            })
            .or_else(|| self.active_position())
            .ok_or_else(|| "can't find tab".to_owned())?;
        let path = requested_path.unwrap_or_else(|| self.composer_buffer.trim().to_owned());
        validate_path(&path).map_err(|error| error.to_string())?;
        let shell = ShellKind::from_program(&self.tabs[position].command_name);
        let command = cwd_command(shell, &path).map_err(|error| error.to_string())?;
        let previous = self.tabs[position].composer.clone();
        let next = match mode {
            ComposerWriteMode::EmptyOnly if !previous.is_empty() => {
                return Err(
                    "Composer already has a draft; explicitly choose --mode append or --mode replace"
                        .to_owned(),
                );
            }
            ComposerWriteMode::EmptyOnly | ComposerWriteMode::Replace => command.clone(),
            ComposerWriteMode::Append => {
                if previous.is_empty() {
                    command.clone()
                } else {
                    format!("{previous}\n{command}")
                }
            }
        };
        let id = self.tabs[position].id;
        self.tabs[position].composer = next;
        self.tabs[position]
            .cwd
            .request(path.clone())
            .map_err(|error| error.to_string())?;
        self.event_journal_mut().commit(
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
        if self.cwd_editor_target_id() == Some(id) {
            self.close_cwd_editor();
        } else if self.active == Some(id) {
            self.load_composer_buffer_from_tab();
            self.request_redraw();
        }
        Ok(())
    }

    fn send_cwd_now(&mut self, target: Option<&str>, requested_path: String) -> Result<(), String> {
        let position = self
            .target_position(target)
            .or_else(|| self.active_position())
            .ok_or_else(|| "can't find tab".to_owned())?;
        validate_path(&requested_path).map_err(|error| error.to_string())?;
        let shell = ShellKind::from_program(&self.tabs[position].command_name);
        let command = cwd_command(shell, &requested_path).map_err(|error| error.to_string())?;
        if !self.tabs[position].submit(&command) {
            return Err("terminal is unavailable or already has a pending submission".to_owned());
        }
        let id = self.tabs[position].id;
        self.tabs[position]
            .cwd
            .request(requested_path.clone())
            .map_err(|error| error.to_string())?;
        self.event_journal_mut().commit(
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
        if self.cwd_editor_target_id() == Some(id) {
            self.close_cwd_editor();
        }
        self.request_redraw();
        Ok(())
    }

    fn request_window_close(&mut self) {
        match window_close_request(self.focus_gate()) {
            WindowCloseRequest::AlreadyOpen => return,
            WindowCloseRequest::CancelLiveClose => {
                self.finish_close_confirmation(false);
                return;
            }
            WindowCloseRequest::Prepare => {}
        }
        if self.cwd_editor_dialog.is_open() {
            self.close_cwd_editor();
        }
        let _ = self.cancel_terminal_selection(true);
        if self.settings_dialog.is_open() {
            let _ = self.close_settings(false);
        }
        if self.new_terminal_dialog.is_open() {
            self.finish_new_terminal_dialog(false);
        }
        self.sync_composer_buffer_to_tab();
        if self.tab_editor_dialog.is_open() {
            let _ = self.complete_tab_editor(false);
        }
        self.window_close_dialog.open();
        self.request_redraw();
    }

    fn finish_window_close(&mut self, choice: WindowCloseChoice) {
        if !self.window_close_dialog.is_open() {
            return;
        }
        self.window_close_dialog.close();
        if !matches!(choice, WindowCloseChoice::Cancel) {
            let _ = self.persist_workspace();
        }
        match choice {
            WindowCloseChoice::KeepServerRunning => {
                if let Some(window) = self.window.as_ref() {
                    window.set_visible(false);
                }
                self.event_journal_mut().commit(
                    EventKind::WindowVisibility,
                    None,
                    serde_json::json!({"visible": false, "reason": "detach"}),
                );
            }
            WindowCloseChoice::StopServerAndExit => {
                self.close_requested = true;
            }
            WindowCloseChoice::Cancel => {}
        }
        self.request_redraw();
    }

    fn begin_tabs_resize(&mut self) {
        self.invalidate_sidebar_text_click();
        let _ = self.cancel_terminal_selection(true);
        self.end_scroll_drag();
        self.tabs_resize_drag = Some(TabsResizeDrag {
            original_width: self.config.tabs_width,
        });
    }

    fn drag_tabs_resize(&mut self, x: i32) {
        if self.tabs_resize_drag.is_none() {
            return;
        }
        let (client_width, _) = self.client_size();
        let width = tabs_width_from_drag(x, client_width as i32) as u16;
        if self.config.tabs_width != width {
            self.config.tabs_width = width;
            self.relayout_after_config_change();
        }
    }

    fn finish_tabs_resize(&mut self, persist: bool, cause: &str, operation_id: &str) {
        let Some(drag) = self.tabs_resize_drag.take() else {
            return;
        };
        if !persist {
            self.config.tabs_width = drag.original_width;
            self.relayout_after_config_change();
            return;
        }
        if let Err(error) = save_config(&self.config) {
            eprintln!("could not save Tabs width: {error:#}");
            return;
        }
        let configured_width = self.config.tabs_width;
        let effective_width = self.layout().effective_tabs_width;
        self.event_journal_mut().commit(
            EventKind::LayoutTabsWidth,
            None,
            serde_json::json!({
                "configured_width": configured_width,
                "effective_width": effective_width,
                "cause": cause,
                "operation_id": operation_id,
            }),
        );
        self.request_redraw();
    }

    fn handle_status_click(&mut self, x: i32, y: i32) -> bool {
        let layout = self.layout();
        if !layout.status.contains(x, y) {
            return false;
        }
        if self.modal_surface_active() {
            return true;
        }
        if layout
            .status_segments
            .tabs_recovery
            .is_some_and(|segment| segment.contains(x, y))
        {
            let _ = self.set_tabs_visible(true, "status-bar", UI_TABS_SHOW);
            return true;
        }
        if layout.status_segments.cwd.contains(x, y) {
            let _ = self.open_cwd_editor(None);
            return true;
        }
        true
    }

    fn handle_window_close_click(&mut self, x: f64, y: f64) -> bool {
        if !self.window_close_dialog.is_open() {
            return false;
        }
        let (width, height) = self.client_size();
        let modal = WindowCloseView::for_client(width, height);
        match modal.hit_test(x, y) {
            Some(WindowCloseHit::KeepServer) => {
                self.finish_window_close(WindowCloseChoice::KeepServerRunning);
            }
            Some(WindowCloseHit::StopServer) => {
                self.finish_window_close(WindowCloseChoice::StopServerAndExit);
            }
            Some(WindowCloseHit::Cancel) => {
                self.finish_window_close(WindowCloseChoice::Cancel);
            }
            None => {}
        }
        true
    }

    fn relayout_after_config_change(&mut self) {
        self.resize_to_window();
        self.request_redraw();
    }

    fn open_settings(&mut self) {
        if self
            .focus_gate()
            .modal_entry_blocked(ModalSurface::Settings)
        {
            return;
        }
        if self.cwd_editor_dialog.is_open() {
            self.close_cwd_editor();
        }
        if self.tab_editor_dialog.is_open() {
            let _ = self.complete_tab_editor(false);
        }
        self.sync_composer_buffer_to_tab();
        // Carry the active tab and its current override in, so the dialog can
        // offer the Current Terminal scope. Opening with `None` here is what
        // made `switch_scope` silently refuse on Unix: it declines whenever
        // there is no target tab, so the whole per-terminal appearance surface
        // was unreachable even though the shared state machine supports it.
        let address = crate::client::ipc_address();
        let target_tab_id = self.active_id().map(|id| format!("@{id}"));
        let override_draft = target_tab_id
            .as_deref()
            .map(|tab_id| self.config.terminal_override(&address, tab_id))
            .unwrap_or_default();
        settings::ui_action_open(
            &mut self.settings_dialog,
            self.config.effective_terminal_appearance(&address, None),
            target_tab_id,
            override_draft,
        );
        self.set_focus_surface_internal(UnixFocusSurface::Settings, "semantic");
    }

    fn close_settings(&mut self, apply: bool) -> Result<(), String> {
        if !self.settings_dialog.is_open() {
            return Err("settings are not open".to_owned());
        }
        if apply {
            self.settings_dialog.capture()?;
            let changes = self.settings_dialog.changes();
            self.config.terminal_font_family = changes.default_appearance.terminal_font_family;
            self.config.terminal_font_size = changes.default_appearance.terminal_font_size;
            self.config.appearance_preset = changes.default_appearance.appearance_preset;
            // Persist the per-terminal override too. Applying only the default
            // appearance silently discarded everything the Current Terminal
            // scope had just edited, so the scope switch would appear to work
            // and then lose the user's change on apply.
            if let Some(tab_id) = changes.target_tab_id.as_deref() {
                self.config.set_terminal_override(
                    &crate::client::ipc_address(),
                    tab_id,
                    changes.override_draft.clone(),
                );
            }
            save_config(&self.config).map_err(|error| format!("{error:#}"))?;
            self.refresh_window_title();
        }
        self.settings_dialog.close_without_apply();
        self.set_focus_surface_internal(UnixFocusSurface::Terminal, "settings-close");
        Ok(())
    }

    fn settings_size_draft(&self) -> u16 {
        self.settings_dialog
            .font_size_draft()
            .trim()
            .parse::<u16>()
            .unwrap_or(self.config.terminal_font_size)
    }

    fn refresh_window_title(&mut self) {
        let instance_label = resolved_ipc_endpoint()
            .ok()
            .map(|resolved| resolved.logical_instance.display_name().to_string())
            .filter(|name| name != "default");
        self.title = window_title_for_preset(
            self.config.appearance_preset,
            env!("CARGO_PKG_VERSION"),
            instance_label.as_deref(),
        );
        if let Some(window) = self.window.as_ref() {
            window.set_title(&self.title);
        }
    }

    fn tab_editor_target_id(&self) -> Option<u64> {
        self.tab_editor_dialog
            .target()
            .and_then(|target| target.strip_prefix('@'))
            .and_then(|id| id.parse().ok())
    }

    fn open_tab_editor_for(&mut self, tab_id: u64) -> Result<(), String> {
        if self.settings_dialog.is_open() {
            let _ = self.close_settings(false);
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) else {
            return Err(format!("can't find tab: @{tab_id}"));
        };
        self.tab_editor_dialog
            .open(format!("@{tab_id}"), tab.title.clone(), tab.note.clone());
        self.text_field_select_all = false;
        self.active = Some(tab_id);
        self.ensure_editing_tab_visible();
        self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "tab-editor");
        Ok(())
    }

    fn ensure_editing_tab_visible(&mut self) {
        let Some(tab_id) = self.tab_editor_target_id() else {
            return;
        };
        let rows = self.visible_tree_rows();
        let Some(position) = rows.iter().position(|row| row.id == tab_id) else {
            return;
        };
        let offset = self.sidebar_offset();
        let capacity = self.sidebar_row_capacity();
        if position < offset {
            self.sidebar_scroll_offset = position;
        } else if position >= offset + capacity {
            self.sidebar_scroll_offset = position.saturating_sub(capacity.saturating_sub(1));
        }
    }

    fn complete_tab_editor(&mut self, save: bool) -> Result<(), String> {
        let Some(tab_id) = self.tab_editor_target_id() else {
            return Err("tab editor is not open".to_owned());
        };
        if save {
            let changes = self
                .tab_editor_dialog
                .capture(true)?
                .expect("tab editor capture returns changes when saving");
            let name = changes.name;
            let note = changes.note;
            let Some(position) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                return Err(format!("can't find tab: @{tab_id}"));
            };
            let previous_name = self.tabs[position].title.clone();
            let previous_note = self.tabs[position].note.clone();
            self.tabs[position].title = name.clone();
            self.tabs[position].note = note.clone();
            if previous_name != name {
                self.event_journal_mut().commit(
                    EventKind::TabRenamed,
                    Some(tab_id),
                    serde_json::json!({
                        "previous_name": previous_name,
                        "name": name,
                    }),
                );
            }
            if previous_note != note {
                self.event_journal_mut().commit(
                    EventKind::TabNote,
                    Some(tab_id),
                    serde_json::json!({
                        "previous_note": previous_note,
                        "note": note,
                    }),
                );
            }
        }
        self.tab_editor_dialog.close();
        self.text_field_select_all = false;
        self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "tab-editor-close");
        self.request_redraw();
        Ok(())
    }

    fn tab_editor_draft_mut(&mut self) -> Option<&mut String> {
        self.tab_editor_dialog.active_draft_mut()
    }

    fn handle_tab_editor_key(&mut self, event: &NormalizedKeyEvent) -> bool {
        if !self.tab_editor_dialog.is_open() {
            return false;
        }
        let multiline = self.tab_editor_dialog.focus() == TabEditorFocus::Note;
        let action = {
            let select_all = &mut self.text_field_select_all;
            let draft = self
                .tab_editor_dialog
                .active_draft_mut()
                .expect("tab editor is open");
            input::text_field_key_action(event, draft, multiline, select_all)
        };
        match action {
            input::TextFieldKeyAction::Edited => {
                self.request_redraw();
            }
            input::TextFieldKeyAction::NextField => {
                self.text_field_select_all = false;
                self.reset_ime_context();
                self.tab_editor_dialog.next_field();
                self.request_redraw();
            }
            input::TextFieldKeyAction::Submit => {
                let _ = self.complete_tab_editor(true);
            }
            input::TextFieldKeyAction::Escape => {
                let _ = self.complete_tab_editor(false);
            }
            input::TextFieldKeyAction::SelectAll => {
                self.request_redraw();
            }
            input::TextFieldKeyAction::Copy => {
                if let Some(text) = self.tab_editor_draft_mut()
                    && clipboard::set_clipboard_text(text).is_ok()
                {
                    self.set_status_message("Copied tab editor draft");
                }
            }
            input::TextFieldKeyAction::Cut => {
                if let Some(text) = self.tab_editor_draft_mut()
                    && clipboard::set_clipboard_text(text).is_ok()
                {
                    text.clear();
                    self.text_field_select_all = false;
                    self.set_status_message("Cut tab editor draft");
                    self.request_redraw();
                }
            }
            input::TextFieldKeyAction::Paste => {
                if let Ok(raw) = clipboard::get_clipboard_text() {
                    let normalized = raw.replace("\r\n", "\n");
                    let select_all = &mut self.text_field_select_all;
                    let text = self
                        .tab_editor_dialog
                        .active_draft_mut()
                        .expect("tab editor is open");
                    input::prepare_composer_edit(text, select_all);
                    text.push_str(&normalized);
                    self.set_status_message(format!(
                        "Pasted {} characters into tab editor",
                        normalized.len()
                    ));
                    self.request_redraw();
                }
            }
            input::TextFieldKeyAction::Ignored => {}
        }
        true
    }

    fn toggle_collapsed(&mut self, tab_id: u64) -> Result<(), String> {
        if !self.tabs.iter().any(|tab| tab.parent_id == Some(tab_id)) {
            return Err("tab has no child nodes".to_owned());
        }
        if !self.collapsed_tabs.remove(&tab_id) {
            self.collapsed_tabs.insert(tab_id);
        }
        self.request_redraw();
        Ok(())
    }

    fn handle_settings_click(&mut self, hit: SettingsHit) {
        match hit {
            SettingsHit::ClassicDay => {
                self.settings_dialog
                    .preview_preset(AppearancePreset::classic_day());
                self.request_redraw();
            }
            SettingsHit::ClassicNight => {
                self.settings_dialog
                    .preview_preset(AppearancePreset::classic_night());
                self.request_redraw();
            }
            SettingsHit::FancyDay => {
                self.settings_dialog
                    .preview_preset(AppearancePreset::fancy_day());
                self.request_redraw();
            }
            SettingsHit::FancyNight => {
                self.settings_dialog
                    .preview_preset(AppearancePreset::fancy_night());
                self.request_redraw();
            }
            SettingsHit::SizeDecrease => {
                let size = self
                    .settings_dialog
                    .font_size_draft()
                    .trim()
                    .parse::<u16>()
                    .unwrap_or(MIN_TERMINAL_FONT_SIZE)
                    .saturating_sub(1)
                    .max(MIN_TERMINAL_FONT_SIZE);
                self.settings_dialog.set_font_size_draft(size.to_string());
                self.request_redraw();
            }
            SettingsHit::SizeIncrease => {
                let size = self
                    .settings_dialog
                    .font_size_draft()
                    .trim()
                    .parse::<u16>()
                    .unwrap_or(MAX_TERMINAL_FONT_SIZE)
                    .saturating_add(1)
                    .min(MAX_TERMINAL_FONT_SIZE);
                self.settings_dialog.set_font_size_draft(size.to_string());
                self.request_redraw();
            }
            SettingsHit::Cancel => {
                let _ = self.close_settings(false);
            }
            SettingsHit::Apply => {
                let _ = self.close_settings(true);
            }
        }
    }

    fn sidebar_rows(&self) -> Vec<SidebarTabRow> {
        self.visible_tree_rows()
            .into_iter()
            .filter_map(|row| {
                let tab = self.tabs.iter().find(|tab| tab.id == row.id)?;
                let has_children = self
                    .tabs
                    .iter()
                    .any(|child| child.parent_id == Some(tab.id));
                Some(SidebarTabRow {
                    id: tab.id,
                    depth: row.depth,
                    title: tab.title.clone(),
                    note: tab.note.clone(),
                    active: self.active == Some(tab.id),
                    collapsed: self.collapsed_tabs.contains(&tab.id),
                    has_children,
                    is_last: row.is_last,
                    guides: row.guides.clone(),
                })
            })
            .collect()
    }

    fn sidebar_row_capacity(&self) -> usize {
        sidebar_row_capacity(self.layout().sidebar_tree.height())
    }

    fn sidebar_row_count(&self) -> usize {
        self.visible_tree_rows().len()
    }

    fn sidebar_viewport(&self) -> crate::ui_geometry::SidebarViewport {
        crate::ui_geometry::SidebarViewport {
            row_count: self.sidebar_row_count(),
            capacity: self.sidebar_row_capacity(),
            requested_offset: self.sidebar_scroll_offset,
        }
    }

    fn sidebar_max_offset(&self) -> usize {
        self.sidebar_viewport().max_offset()
    }

    fn sidebar_offset(&self) -> usize {
        self.sidebar_viewport().offset()
    }

    fn sidebar_scrollbar_state(
        &self,
    ) -> Option<(crate::ui_geometry::TerminalScrollbarGeometry, usize, usize)> {
        if !self.config.tabs_visible {
            return None;
        }
        Some(self.sidebar_viewport().scrollbar(self.layout().sidebar_tree))
    }

    fn sidebar_viewport_rows(&self) -> Vec<SidebarTabRow> {
        let offset = self.sidebar_offset();
        self.sidebar_rows()
            .into_iter()
            .skip(offset)
            .take(self.sidebar_row_capacity())
            .collect()
    }

    fn sidebar_row_geometry(
        &self,
        viewport_position: usize,
        depth: usize,
        tab_id: u64,
    ) -> crate::ui_geometry::TreeRowGeometry {
        let mode = if self.tab_editor_target_id() == Some(tab_id) {
            TreeRowMode::Editing
        } else {
            TreeRowMode::Normal
        };
        sidebar_tree_row_geometry(self.layout().sidebar_tree, viewport_position, depth, mode)
    }

    fn tree_action_density_name(density: TreeRowActionDensity) -> &'static str {
        match density {
            TreeRowActionDensity::Full => "full",
            TreeRowActionDensity::Compact => "compact",
        }
    }

    fn tab_position_for_sidebar_y(&self, y: u32) -> Option<usize> {
        let tree_height = self.layout().sidebar_tree.height().max(0) as u32;
        let row_index = sidebar_row_at_y(y, tree_height)?;
        let source_index = self.sidebar_offset() + row_index;
        let row_id = self.visible_tree_rows().get(source_index)?.id;
        self.tabs.iter().position(|tab| tab.id == row_id)
    }

    fn tab_state(tab: &TerminalTab) -> &'static str {
        if tab.error.is_some() {
            "error"
        } else if tab.exited.is_some() {
            "dead"
        } else {
            "running"
        }
    }

    fn is_edit_focus(&self) -> bool {
        self.focus_surface == UnixFocusSurface::Composer
            || self.tab_editor_dialog.is_open()
            || self.new_terminal_dialog.is_open()
    }

    fn terminal_ready_for_system_menu(&self) -> bool {
        self.focus_surface == UnixFocusSurface::Terminal
            && !self.focus_gate().full_modal_blocked()
            && self
                .active_position()
                .is_some_and(|position| self.tabs[position].exited.is_none())
    }

    fn system_menu_clipboard_state(&self) -> (bool, bool) {
        // A pending paste already owns the one clipboard read. Do not start a
        // second helper from snapshot/menu rendering while that asynchronous
        // read is in flight.
        let clipboard_has_text =
            self.pending_terminal_paste.is_none() && clipboard::clipboard_has_unicode_text();
        system_menu_clipboard_state(
            self.is_edit_focus(),
            self.terminal_ready_for_system_menu(),
            self.terminal_selection
                .as_ref()
                .is_some_and(|selection| !selection.is_empty()),
            clipboard_has_text,
        )
    }

    fn build_ui_snapshot_json(&mut self) -> String {
        let active = self.active;
        let (client_width, client_height) = self.client_size();
        let layout = self.layout();
        let visible_rows = self.visible_tree_rows();
        let all_rows = self.all_tree_rows();
        let (terminal_rows, terminal_cols) = self
            .active_position()
            .map(|position| self.tabs[position].last_size)
            .unwrap_or((0, 0));
        let (alternate_screen, application_cursor) = self
            .active_position()
            .map(|position| {
                let screen = self.tabs[position].parser.screen();
                (screen.alternate_screen(), screen.application_cursor())
            })
            .unwrap_or_default();
        let terminal_scrollbar = self.active_position().map(|position| {
            let visible_rows = usize::from(self.tabs[position].last_size.0);
            let (offset, maximum) = self.tabs[position].scrollback_bounds();
            let geometry = scrollbar_geometry(&layout, visible_rows, offset, maximum);
            scrollbar_state_json(&geometry, offset, maximum)
        });
        let journal_position = self.event_journal.position();
        let ime_status = agenterm_platform::ime::status();
        let (copy_enabled, paste_enabled) = self.system_menu_clipboard_state();
        let sidebar_scrollbar = self
            .sidebar_scrollbar_state()
            .map(|(geometry, offset, maximum)| scrollbar_state_json(&geometry, offset, maximum));
        let composer_geometry = composer_geometry(layout.composer);
        let composer_input = composer_geometry.input;
        let workspace_controls_visible = self.focus_gate().workspace_controls_visible();
        let interaction_selection = self.terminal_selection.map(|selection| {
            let (start, end) = selection.bounds();
            TerminalSelectionSnapshotInput {
                tab_id: selection.tab_id,
                start_row: start.row,
                start_col: start.col,
                end_row: end.row,
                end_col: end.col,
                dragging: selection.dragging,
            }
        });
        let tab_editor = self
            .tab_editor_dialog
            .is_open()
            .then(|| self.tab_editor_dialog.snapshot_modal());
        let tabs = all_rows
            .iter()
            .filter_map(|row| {
                let tab = self.tabs.iter().find(|tab| tab.id == row.id)?;
                let visible_position = self
                    .config
                    .tabs_visible
                    .then(|| {
                        visible_rows
                            .iter()
                            .position(|visible| visible.id == row.id)
                            .and_then(|source_position| {
                                source_position
                                    .checked_sub(self.sidebar_offset())
                                    .filter(|position| *position < self.sidebar_row_capacity())
                            })
                    })
                    .flatten();
                let geometry = visible_position
                    .map(|position| self.sidebar_row_geometry(position, row.depth, tab.id));
                let actions = visible_position
                    .filter(|_| active == Some(tab.id))
                    .map(|position| {
                        let geometry = self.sidebar_row_geometry(position, row.depth, tab.id);
                        let action =
                            |id: &str, label: &str, bounds: crate::ui_geometry::PixelRect| {
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
                        match geometry.mode {
                            TreeRowMode::Normal => serde_json::json!({
                                "mode": "normal",
                                "density": Self::tree_action_density_name(geometry.actions.density),
                                "new_child": action(
                                    "new-child",
                                    "Add",
                                    geometry.actions.add_child.expect("normal row has Add"),
                                ),
                                "close": action("close-tab", "Close", geometry.actions.secondary),
                            }),
                            TreeRowMode::Editing => serde_json::json!({
                                "mode": "editing",
                                "density": Self::tree_action_density_name(geometry.actions.density),
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
                    });
                let per_tab_selection = self
                    .terminal_selection
                    .filter(|selection| selection.tab_id == tab.id)
                    .map(|selection| {
                        let (start, end) = selection.bounds();
                        serde_json::json!({
                            "start": {"row": start.row, "col": start.col},
                            "end": {"row": end.row, "col": end.col},
                            "dragging": selection.dragging,
                        })
                    });
                let draft = if self.active == Some(tab.id) {
                    !self.composer_buffer.is_empty() || tab.sensitive_composer.is_some()
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
                    "terminal_title": tab.title,
                    "note": tab.note,
                    "active": active == Some(tab.id),
                    "pid": tab.process_id,
                    "state": Self::tab_state(tab),
                    "exit_code": tab.exited,
                    "working_context": working_context_json(
                        &tab.cwd,
                        tab.shell_kind,
                        &tab.proxy,
                    ),
                    "scrollback_offset": tab.parser.screen().scrollback(),
                    "selection": per_tab_selection,
                    "draft": draft,
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
                }))
            })
            .collect::<Vec<_>>();
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": schema_version_json(),
            "protocol_version": 1,
            "projection": PROJECTION_EMBEDDED_GUI,
            // Bounds below come from a real laid-out window, unlike the
            // headless projection's synthetic geometry.
            "geometry_source": crate::ui_snapshot::GEOMETRY_SOURCE_RENDERED,
            "client_pid": std::process::id(),
            "server_pid": std::process::id(),
            "event_position": event_position_json(&journal_position.epoch, journal_position.sequence),
            "session": self.session_name,
            "active_window_id": active.map(|id| format!("@{id}")),
            "tabs_visible": self.config.tabs_visible,
            "window": if let Some(window) = self.window.as_ref() {
                window_snapshot_json(
                    &PixelWindowHandle {
                        window,
                        title: &self.title,
                    },
                    &self.window_state_tracker,
                )
            } else {
                embedded_window_json(self.title.as_str(), client_width, client_height)
            },
            "layout": {
                "sidebar": {
                    "x": layout.sidebar.left,
                    "y": layout.sidebar.top,
                    "visible": self.config.tabs_visible,
                    "configured_width": self.config.tabs_width,
                    "effective_width": layout.effective_tabs_width,
                    "width": layout.sidebar.width(),
                    "height": layout.sidebar.height(),
                    "bounds": pixel_rect_json(layout.sidebar),
                    "resize_grip": layout.resize_grip.map(pixel_rect_json),
                    "resizing": self.tabs_resize_drag.is_some(),
                    "scrollbar": sidebar_scrollbar,
                },
                "toolbar": layout.workspace_toolbar.map(workspace_toolbar_snapshot_json),
                // Chips carry their own bounds so an agent can click one the
                // same way a human does, via `ui-input pointer`.
                "server_strip": layout.server_strip.map(|strip| {
                    serde_json::json!({
                        "bounds": pixel_rect_json(strip),
                        "tabs": self
                            .server_tab_rects()
                            .into_iter()
                            .map(|(chip, row)| serde_json::json!({
                                "instance": row.instance,
                                "label": server_tab_chip_label(
                                    &row.instance,
                                    row.can_attach,
                                ),
                                "can_attach": row.can_attach,
                                "pid": row.pid,
                                "tab_count": row.tab_count,
                                "bounds": pixel_rect_json(chip),
                            }))
                            .collect::<Vec<_>>(),
                        // Menu item bounds so an agent can drive `As Window` /
                        // `Close` with `ui-input pointer`, as a human does.
                        "menu": self.server_context_menu_geometry().map(
                            |menu| serde_json::json!({
                                "bounds": pixel_rect_json(menu.frame),
                                "as_window": pixel_rect_json(menu.as_window),
                                "close": pixel_rect_json(menu.close),
                            }),
                        ),
                        "add": pixel_rect_json({
                            let add = layout_server_add_chip(StripRect {
                                left: strip.left,
                                top: strip.top,
                                right: strip.right,
                                bottom: strip.bottom,
                            });
                            crate::ui_geometry::PixelRect {
                                left: add.left,
                                top: add.top,
                                right: add.right,
                                bottom: add.bottom,
                            }
                        }),
                    })
                }),
                "terminal": {
                    "x": layout.terminal.left,
                    "y": layout.terminal.top,
                    "width": layout.terminal.width(),
                    "viewport_width": (
                        layout.terminal.width() - TERMINAL_SCROLLBAR_WIDTH
                    ).max(0),
                    "height": layout.terminal.height(),
                    "bounds": pixel_rect_json(layout.terminal),
                    "rows": terminal_rows,
                    "cols": terminal_cols,
                    "alternate_screen": alternate_screen,
                    "application_cursor": application_cursor,
                    "scrollbar": terminal_scrollbar,
                },
                "composer": {
                    "visible": workspace_controls_visible,
                    "input_visible": workspace_controls_visible,
                    "send_visible": workspace_controls_visible,
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
                    "ime": ime_status_snapshot_json(
                        layout.status_segments.ime,
                        ime_status.as_ref(),
                    ),
                    "proxy": archived_proxy_status_json(layout.status_segments.proxy),
                    "provider": "placeholder",
                },
            },
            "focus": {
                "surface": if let Some(surface) = modal_surface_from_gate(self.focus_gate()) {
                    surface.as_str()
                } else if self.tab_editor_dialog.is_open() {
                    "tab-editor"
                } else {
                    self.focus_state.surface().as_str()
                },
                "window_id": active.map(|id| format!("@{id}")),
                // This fact is updated only by the native FocusChanged event. An
                // activation request must not optimistically claim compositor focus.
                "window_focused": self.window_focused,
            },
            "terminal_paste": {
                "state": if self.pending_terminal_paste.is_some() { "pending" } else { "idle" },
                "target": self.pending_terminal_paste
                    .as_ref()
                    .map(|pending| format!("@{}", pending.tab_id)),
            },
            // Selection state is reported so an automated client can verify a
            // caret move or a drag without a human watching the window; a GUI
            // affordance that no machine can observe cannot be regression
            // tested.
            "composer": {
                "draft_length": self.composer_buffer.chars().count(),
                "focused": self.focus_surface == UnixFocusSurface::Composer,
                "caret": self.composer_cursor.focus(),
                "anchor": self.composer_cursor.anchor(),
                "selection": self.composer_cursor.has_selection().then(|| {
                    let (start, end) = self.composer_cursor.range();
                    serde_json::json!({
                        "start": start,
                        "end": end,
                        "text": self.composer_selected_text(),
                    })
                }),
            },
            "modal": modal_surface_from_gate(self.focus_gate()).map(|surface| match surface {
                ModalSurface::WindowClose => self.window_close_dialog.snapshot_modal(),
                ModalSurface::Settings => self.settings_dialog.snapshot_modal(),
                ModalSurface::NewTerminal => self.new_terminal_dialog.snapshot_modal(),
                ModalSurface::CwdEditor => self.cwd_editor_dialog.snapshot_modal(),
                ModalSurface::TabClose => self.close_confirmation.snapshot_modal(),
                ModalSurface::InstancePicker => self.instance_picker_dialog.snapshot_modal(),
                ModalSurface::ServerNew | ModalSurface::ServerClose => serde_json::json!({
                    "kind": "server-strip",
                    "error": "server strip dialogs are Windows-first in this build",
                }),
            }),
            "system_menu": system_menu_json(
                self.config.tabs_visible,
                copy_enabled,
                paste_enabled,
            ),
            "tab_editor": tab_editor,
            "tabs": tabs,
            "terminal_interaction": terminal_interaction_json(
                interaction_selection,
                self.terminal_selection_autoscroll.is_some(),
            ),
            "settings": settings_json(
                &self.config,
                self.config.locale,
                self.settings_dialog.is_open(),
                Some(self.settings_dialog.preset_draft().as_str()),
                &crate::ipc_address(),
                self.active_position()
                    .map(|position| format!("@{}", self.tabs[position].id))
                    .as_deref(),
            ),
            "locale": locale_json(self.config.locale),
            "feedback": {
                "message": self.status_message,
                "error": self.last_feedback_error
                    .as_ref()
                    .map(UiFeedbackError::json)
                    .unwrap_or(serde_json::Value::Null),
            },
        }))
        .unwrap_or_else(|_| "{}".to_owned())
    }

    fn client_size(&self) -> (u32, u32) {
        self.window
            .as_ref()
            .and_then(|window| window.metrics().ok())
            .map(|metrics| {
                (
                    metrics.logical_size.width.round().max(1.0) as u32,
                    metrics.logical_size.height.round().max(1.0) as u32,
                )
            })
            .unwrap_or((INITIAL_WIDTH, INITIAL_HEIGHT))
    }

    fn handle_geometry_event(&mut self, _change: GeometryChange, _metrics: PixelWindowMetrics) {
        if let Some(window) = self.window.as_ref() {
            self.window_state_tracker
                .sync_from_native_flags(window.minimized(), window.maximized());
            window.request_redraw();
        }
        self.resize_to_window();
    }

    fn sidebar_tab_action_at(&self, x: i32, y: i32) -> Option<SidebarTabAction> {
        let tree_height = self.layout().sidebar_tree.height().max(0) as u32;
        let row_index = sidebar_row_at_y(y as u32, tree_height)?;
        let source_index = self.sidebar_offset() + row_index;
        let visible_rows = self.visible_tree_rows();
        let row = visible_rows.get(source_index)?;
        let active_id = self.active?;
        if row.id != active_id {
            return None;
        }
        let mode = if self.tab_editor_target_id() == Some(row.id) {
            TreeRowMode::Editing
        } else {
            TreeRowMode::Normal
        };
        let geometry =
            sidebar_tree_row_geometry(self.layout().sidebar_tree, row_index, row.depth, mode);
        match mode {
            TreeRowMode::Editing => {
                if geometry.actions.primary.contains(x, y) {
                    Some(SidebarTabAction::Save)
                } else if geometry.actions.secondary.contains(x, y) {
                    Some(SidebarTabAction::Cancel)
                } else {
                    None
                }
            }
            TreeRowMode::Normal => {
                if geometry
                    .actions
                    .add_child
                    .is_some_and(|bounds| bounds.contains(x, y))
                {
                    Some(SidebarTabAction::AddChild)
                } else if geometry.actions.secondary.contains(x, y) {
                    Some(SidebarTabAction::Close)
                } else {
                    None
                }
            }
        }
    }

    fn sidebar_tab_editor_hit(&self, x: i32, y: i32) -> Option<TabEditorFocus> {
        if !self.tab_editor_dialog.is_open() {
            return None;
        }
        let tree_height = self.layout().sidebar_tree.height().max(0) as u32;
        let row_index = sidebar_row_at_y(y as u32, tree_height)?;
        let source_index = self.sidebar_offset() + row_index;
        let visible_rows = self.visible_tree_rows();
        let row = visible_rows.get(source_index)?;
        if self.tab_editor_target_id() != Some(row.id) {
            return None;
        }
        let geometry = sidebar_tree_row_geometry(
            self.layout().sidebar_tree,
            row_index,
            row.depth,
            TreeRowMode::Editing,
        );
        let editors = geometry.editors?;
        if editors.name.contains(x, y) {
            Some(TabEditorFocus::Name)
        } else if editors.note.contains(x, y) {
            Some(TabEditorFocus::Note)
        } else {
            None
        }
    }

    /// Return a row only when the pointer is over its name/note body. Actions,
    /// disclosure/tree guides, status dots, and the sidebar scrollbar are
    /// deliberately outside this surface.
    fn sidebar_tab_text_at(&self, x: i32, y: i32) -> Option<u64> {
        let tree_height = self.layout().sidebar_tree.height().max(0) as u32;
        let row_index = sidebar_row_at_y(y.max(0) as u32, tree_height)?;
        let source_index = self.sidebar_offset() + row_index;
        let visible_rows = self.visible_tree_rows();
        let row = visible_rows.get(source_index)?;
        let geometry = sidebar_tree_row_geometry(
            self.layout().sidebar_tree,
            row_index,
            row.depth,
            TreeRowMode::Normal,
        );
        geometry.text.contains(x, y).then_some(row.id)
    }

    fn handle_sidebar_click(&mut self, x: f64, y: f64) {
        let previous_text_click = self.recent_sidebar_text_click.take();
        if self.handle_window_close_click(x, y) {
            return;
        }
        let layout = self.layout();
        if self.handle_status_click(x as i32, y as i32) {
            return;
        }
        if layout
            .resize_grip
            .is_some_and(|grip| grip.contains(x as i32, y as i32))
            && !self.modal_surface_active()
        {
            self.begin_tabs_resize();
            return;
        }
        let sidebar_width = sidebar_width_u32(&layout);
        if x >= f64::from(sidebar_width) {
            return;
        }
        if self.click_sidebar_scrollbar(x as i32, y as i32) {
            return;
        }
        let tree_height = layout.sidebar_tree.height().max(0) as u32;
        let click_y = y.max(0.0) as i32;
        let click_x = x as i32;
        if let Some(row_index) = sidebar_row_at_y(y.max(0.0) as u32, tree_height) {
            let source_index = self.sidebar_offset() + row_index;
            if let Some(row) = self.visible_tree_rows().get(source_index)
                && self.tabs.iter().any(|tab| tab.parent_id == Some(row.id))
            {
                let geometry = self.sidebar_row_geometry(row_index, row.depth, row.id);
                if geometry.disclosure_hit.contains(click_x, click_y) {
                    let _ = self.toggle_collapsed(row.id);
                    return;
                }
            }
        }
        if let Some(action) = self.sidebar_tab_action_at(click_x, click_y) {
            match action {
                SidebarTabAction::AddChild => {
                    let Some(position) = self.tab_position_for_sidebar_y(y.max(0.0) as u32) else {
                        return;
                    };
                    let parent_id = self.tabs[position].id;
                    self.sync_composer_buffer_to_tab();
                    if let Ok(index) = self.create_tab(
                        Some("New child".to_owned()),
                        Vec::new(),
                        Vec::new(),
                        true,
                        Some(parent_id),
                    ) && let Some(id) = self
                        .tabs
                        .iter()
                        .find(|tab| tab.index == index)
                        .map(|tab| tab.id)
                    {
                        self.after_create_tab(id, Some(parent_id));
                    }
                }
                SidebarTabAction::Close => {
                    let Some(position) = self.tab_position_for_sidebar_y(y.max(0.0) as u32) else {
                        return;
                    };
                    self.request_close_tab(self.tabs[position].id);
                }
                SidebarTabAction::Save => {
                    let _ = self.complete_tab_editor(true);
                }
                SidebarTabAction::Cancel => {
                    let _ = self.complete_tab_editor(false);
                }
            }
            return;
        }
        if let Some(focus) = self.sidebar_tab_editor_hit(click_x, click_y) {
            if self.tab_editor_dialog.focus() != focus {
                self.text_field_select_all = false;
                self.reset_ime_context();
            }
            self.tab_editor_dialog.set_focus(focus);
            self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "tab-editor-focus");
            return;
        }
        if self.tab_editor_dialog.is_open() {
            let _ = self.complete_tab_editor(false);
        }
        let text_tab = self.sidebar_tab_text_at(click_x, click_y);
        let Some(position) = self.tab_position_for_sidebar_y(y.max(0.0) as u32) else {
            self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "mouse");
            return;
        };
        let _ = self.select_tab_at(position);
        self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "mouse");
        let now = Instant::now();
        if let Some(tab_id) = text_tab {
            let is_double_click = previous_text_click
                .is_some_and(|click| click.matches(tab_id, self.sidebar_geometry_generation, now));
            if is_double_click {
                self.recent_sidebar_text_click = None;
                let _ = self.open_tab_editor_for(tab_id);
            } else {
                self.recent_sidebar_text_click = Some(RecentSidebarTextClick {
                    tab_id,
                    at: now,
                    geometry_generation: self.sidebar_geometry_generation,
                });
            }
        }
    }

    fn handle_toolbar_hit(&mut self, hit: ToolbarHit) {
        self.dispatch_toolbar_action(platform_toolbar_action_id(hit));
        self.request_redraw();
    }

    /// Platform hot-path: toolbar hits resolve through stable adapter action ids
    /// before shared product handlers run.
    fn dispatch_toolbar_action(&mut self, action_id: &str) {
        use crate::frontend::action;
        if !action::is_toolbar_action_id(action_id) {
            return;
        }
        match action_id {
            action::NEW_TAB => {
                self.open_new_terminal_dialog();
            }
            action::TOGGLE_TABS => {
                let visible = !self.config.tabs_visible;
                let _ =
                    self.set_tabs_visible(visible, "toolbar", crate::operations::UI_TABS_TOGGLE);
            }
            action::OPEN_CONTROL_CENTER => {
                match crate::control_center::open_control_center(
                    self.no_activate,
                    &crate::client::ipc_address(),
                ) {
                    Ok(()) => self.set_status_message("Control Center opened"),
                    Err(error) => {
                        self.set_status_message(format!("Control Center unavailable: {error:#}"))
                    }
                }
            }
            action::OPEN_SETTINGS => {
                self.open_settings();
            }
            action::TOGGLE_LOCALE => self.toggle_locale(),
            action::FONT_DECREASE => self.adjust_active_terminal_font(-1),
            action::FONT_INCREASE => self.adjust_active_terminal_font(1),
            _ => {}
        }
    }

    fn handle_content_click(&mut self, x: f64, y: f64) {
        if self.handle_window_close_click(x, y) {
            return;
        }
        if self.handle_server_context_menu_click(x as i32, y as i32) {
            return;
        }
        if self.handle_server_strip_click(x as i32, y as i32) {
            return;
        }
        if self.tab_editor_dialog.is_open() {
            let _ = self.complete_tab_editor(false);
        }
        if self.handle_status_click(x as i32, y as i32) {
            return;
        }
        if self.close_confirmation.is_open() {
            let (width, height) = self.client_size();
            if let Some(id) = self.close_confirmation_target_id() {
                let modal = ConfirmCloseView::for_client(width, height, id);
                match modal.hit_test(x, y) {
                    Some(ConfirmCloseHit::Confirm) => self.finish_close_confirmation(true),
                    Some(ConfirmCloseHit::Cancel) => self.finish_close_confirmation(false),
                    None => {}
                }
            }
            return;
        }
        if self.settings_dialog.is_open() {
            let (width, height) = self.client_size();
            let modal = SettingsModalView::for_client(
                width,
                height,
                self.settings_size_draft(),
                self.settings_dialog.preset_draft(),
                self.config.locale,
            );
            if let Some(hit) = modal.hit_test(x, y) {
                self.handle_settings_click(hit);
            }
            return;
        }
        if self.new_terminal_dialog.is_open() {
            let (width, height) = self.client_size();
            let modal = NewTerminalModalView::for_client(
                width,
                height,
                self.render_shell_choice(),
                self.new_terminal_dialog.initial_command_draft(),
                self.new_terminal_dialog.http_proxy_draft(),
                self.new_terminal_dialog.https_proxy_draft(),
                self.new_terminal_focus,
            );
            if let Some(hit) = modal.hit_test(x, y) {
                self.handle_new_terminal_click(hit);
            }
            return;
        }
        if self.composer_send_hit(x, y) {
            if self.send_active_composer().is_ok() {
                self.set_focus_surface_internal(UnixFocusSurface::Terminal, "composer-send");
            }
            return;
        }
        let layout = self.layout();
        if !self.modal_surface_active()
            && let Some(toolbar) = layout.workspace_toolbar
            && toolbar.bounds.contains(x as i32, y as i32)
        {
            let view = WorkspaceToolbarView::from_layout(
                toolbar,
                self.config.tabs_visible,
                self.config.locale,
            );
            if let Some(hit) = view.hit_test(x, y) {
                self.handle_toolbar_hit(hit);
            }
            return;
        }
        if x < f64::from(self.sidebar_width()) {
            return;
        }
        if self.click_scrollbar(x as i32, y as i32) {
            return;
        }
        if self.forward_terminal_mouse(x, y, Some(0), true, false) {
            let _ = self.cancel_terminal_selection(true);
            self.mouse_report_button = Some(0);
            self.set_focus_surface_internal(UnixFocusSurface::Terminal, "mouse");
            return;
        }
        if self.begin_terminal_selection(x, y) {
            return;
        }
        if self.composer_region_contains(x, y) {
            self.set_focus_surface_internal(UnixFocusSurface::Composer, "mouse");
            self.begin_composer_selection(x, y);
        } else {
            self.set_focus_surface_internal(UnixFocusSurface::Terminal, "mouse");
        }
    }

    fn cell_at_client(&self, x: f64, y: f64) -> Option<(u16, u16)> {
        let (rows, cols) = self
            .active_position()
            .map(|position| self.tabs[position].last_size)
            .or_else(|| self.grid.as_ref().map(|grid| (grid.rows, grid.cols)))?;
        let (cell_width, cell_height) = self.cell_dimensions();
        terminal_cell_at(
            terminal_pixel_rect(&self.layout()),
            x as i32,
            y as i32,
            rows,
            cols,
            cell_width as i32,
            cell_height as i32,
        )
    }

    fn begin_terminal_selection(&mut self, x: f64, y: f64) -> bool {
        let Some(position) = self.active_position() else {
            return false;
        };
        let Some((col, row)) = self.cell_at_client(x, y) else {
            return false;
        };
        let tab_id = self.tabs[position].id;
        let point = TerminalPoint { row, col };
        let (rows, cols) = self.tabs[position].last_size;
        let now = Instant::now();

        // Shift+click extends an existing completed selection instead of
        // starting a fresh gesture or double-click, matching the xterm
        // convention that forward_terminal_mouse's doc comment already
        // claims (shift bypasses mouse reporting so local selection stays
        // reachable). The anchor becomes whichever endpoint of the current
        // selection is farther from the click, per that same convention.
        if self.pointer_modifiers.shift
            && let Some(selection) = self.terminal_selection
            && selection.tab_id == tab_id
        {
            let anchor = shift_extend_anchor(selection, point);
            if self.set_completed_terminal_selection(tab_id, anchor, point, rows, cols) {
                self.terminal_click_chain.clear();
                if let Err(error) = self.copy_terminal_selection() {
                    self.set_status_message(format!("Copy failed: {error}"));
                }
                self.set_focus_surface_internal(UnixFocusSurface::Terminal, "selection");
                self.request_redraw();
            }
            return true;
        }

        let window = Duration::from_millis(double_click_ms());
        match self
            .terminal_click_chain
            .classify(&tab_id, point, now, window, true)
        {
            crate::frontend::selection::ClickStage::Triple => {
                if let Some((start, end)) =
                    visible_row_selection(self.tabs[position].parser.screen(), row)
                    && self.set_completed_terminal_selection(tab_id, start, end, rows, cols)
                    && let Err(error) = self.copy_terminal_selection()
                {
                    self.set_status_message(format!("Copy failed: {error}"));
                }
                self.set_focus_surface_internal(UnixFocusSurface::Terminal, "selection");
                self.request_redraw();
                return true;
            }
            crate::frontend::selection::ClickStage::Double => {
                if let Some((start, end)) =
                    word_selection(self.tabs[position].parser.screen(), point)
                {
                    if self.set_completed_terminal_selection(tab_id, start, end, rows, cols)
                        && let Err(error) = self.copy_terminal_selection()
                    {
                        self.set_status_message(format!("Copy failed: {error}"));
                    }
                    self.terminal_click_chain.arm_double(tab_id, point, now, window);
                    self.set_focus_surface_internal(UnixFocusSurface::Terminal, "selection");
                    self.request_redraw();
                    return true;
                }
            }
            crate::frontend::selection::ClickStage::Single => {}
        }

        self.terminal_click_chain.record_single(tab_id, point, now);
        let Some(gesture) = SelectionGesture::prepare(tab_id, point, rows, cols) else {
            return false;
        };
        self.terminal_selection = gesture.selection();
        self.terminal_selection_gesture = Some(gesture);
        self.terminal_selection_pointer = Some((x as i32, y as i32));
        self.terminal_selection_autoscroll = None;
        self.set_focus_surface_internal(UnixFocusSurface::Terminal, "selection");
        self.request_redraw();
        true
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

    fn drag_terminal_selection(&mut self, x: f64, y: f64) {
        let Some(gesture) = self.terminal_selection_gesture.clone() else {
            return;
        };
        if !gesture.active() {
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        let terminal = terminal_pixel_rect(&self.layout());
        let max_x = (terminal.right - layout::SCROLLBAR_WIDTH as i32 - 1).max(terminal.left);
        let max_y = terminal.bottom.saturating_sub(1).max(terminal.top);
        let clamped_x = (x as i32).clamp(terminal.left, max_x);
        let clamped_y = (y as i32).clamp(terminal.top, max_y);
        let (rows, cols) = self.tabs[position].last_size;
        let (cell_width, cell_height) = self.cell_dimensions();
        let Some((col, row)) = terminal_cell_at(
            terminal,
            clamped_x,
            clamped_y,
            rows,
            cols,
            cell_width as i32,
            cell_height as i32,
        ) else {
            return;
        };
        let updated = gesture.drag_to_clamped(TerminalPoint { row, col }, rows, cols);
        let next_autoscroll =
            autoscroll_step(y as i32, terminal.top, terminal.bottom, cell_height as i32);
        self.terminal_selection = updated.selection();
        self.terminal_selection_gesture = Some(updated);
        self.terminal_selection_pointer = Some((clamped_x, clamped_y));
        self.terminal_selection_autoscroll = next_autoscroll;
        self.request_redraw();
    }

    fn complete_terminal_selection(&mut self) {
        let Some(gesture) = self.terminal_selection_gesture.take() else {
            return;
        };
        if !gesture.active() {
            self.terminal_selection_gesture = Some(gesture);
            return;
        }
        let completed = gesture.complete();
        self.terminal_selection_pointer = None;
        self.terminal_selection_autoscroll = None;
        if let Some(selection) = completed.completed_selection() {
            self.terminal_selection = Some(selection);
            self.terminal_selection_gesture = Some(completed);
            if let Err(error) = self.copy_terminal_selection() {
                self.set_status_message(format!("Copy failed: {error}"));
            }
        } else {
            self.terminal_selection = None;
            self.terminal_selection_gesture = None;
        }
        self.request_redraw();
    }

    fn cancel_terminal_selection(&mut self, clear_completed: bool) -> bool {
        let mut changed = false;
        if let Some(gesture) = self.terminal_selection_gesture.take() {
            if gesture.active() {
                changed = true;
            }
            let _ = gesture.cancel();
        }
        if clear_completed && self.terminal_selection.take().is_some() {
            changed = true;
        }
        self.terminal_selection_pointer = None;
        self.terminal_selection_autoscroll = None;
        if clear_completed {
            self.terminal_click_chain.disarm_double();
        }
        if changed {
            self.request_redraw();
        }
        changed
    }

    fn tick_terminal_selection_autoscroll(&mut self) -> bool {
        let Some(step) = self.terminal_selection_autoscroll else {
            return false;
        };
        let Some(gesture) = self.terminal_selection_gesture.clone() else {
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
        if let Some((x, y)) = self.terminal_selection_pointer {
            let (cell_width, cell_height) = self.cell_dimensions();
            if let Some((col, row)) = terminal_cell_at(
                terminal_pixel_rect(&self.layout()),
                x,
                y,
                self.tabs[position].last_size.0,
                self.tabs[position].last_size.1,
                cell_width as i32,
                cell_height as i32,
            ) {
                let (rows, cols) = self.tabs[position].last_size;
                let updated = gesture.drag_to_clamped(TerminalPoint { row, col }, rows, cols);
                self.terminal_selection = updated.selection();
                self.terminal_selection_gesture = Some(updated);
            }
        }
        if after != before {
            self.on_viewport_scrolled(position, after, "selection-autoscroll");
            true
        } else {
            self.request_redraw();
            false
        }
    }

    fn copy_terminal_selection(&mut self) -> Result<(), String> {
        let selection = self
            .terminal_selection
            .ok_or_else(|| "no terminal text is selected".to_owned())?;
        let position = self
            .tabs
            .iter()
            .position(|tab| tab.id == selection.tab_id)
            .ok_or_else(|| "selected terminal is no longer available".to_owned())?;
        if self.active != Some(selection.tab_id) {
            return Err("selected terminal is not active".to_owned());
        }
        let text = terminal_selection_text(self.tabs[position].parser.screen(), selection);
        clipboard::set_clipboard_text(&text)?;
        // Count characters, not bytes: `String::len` is a byte count, so a
        // CJK selection reported three times its real size ("Copied 12
        // characters" for four Chinese characters).
        self.set_status_message(format!("Copied {} characters", text.chars().count()));
        Ok(())
    }

    fn request_terminal_clipboard_paste(&mut self) -> Result<(), TerminalPasteFailure> {
        if self.pending_terminal_paste.is_some() {
            return Err(TerminalPasteFailure::Busy);
        }
        if self.modal_surface_active() {
            return Err(TerminalPasteFailure::ModalOpen);
        }
        if self.focus_surface != UnixFocusSurface::Terminal || !self.window_focused {
            return Err(TerminalPasteFailure::FocusRequired);
        }
        let Some(position) = self.active_position() else {
            return Err(TerminalPasteFailure::NoActiveTerminal);
        };
        let tab_id = self.tabs[position].id;
        let (sender, receiver) = mpsc::channel();
        let wake_signal = Arc::clone(&self.wake_signal);
        let _worker = thread::Builder::new()
            .name("agenterm-unix-clipboard-read".to_owned())
            .spawn(move || {
                let result = clipboard::get_clipboard_text_bounded(TERMINAL_PASTE_LIMIT_BYTES)
                    .map_err(TerminalPasteFailure::Clipboard);
                let _ = sender.send(result);
                request_gui_wake_best_effort(0, &wake_signal, "unix-clipboard-read");
            })
            .map_err(|error| TerminalPasteFailure::WorkerStart(error.to_string()))?;
        self.pending_terminal_paste = Some(PendingTerminalPaste { tab_id, receiver });
        self.last_feedback_error = None;
        self.set_status_message(format!("Reading clipboard for @{tab_id}…"));
        self.request_redraw();
        Ok(())
    }

    fn drain_terminal_clipboard_paste(&mut self) -> bool {
        let Some(pending) = self.pending_terminal_paste.as_ref() else {
            return false;
        };
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err(TerminalPasteFailure::WorkerDisconnected),
        };
        let tab_id = pending.tab_id;
        self.pending_terminal_paste = None;
        let result = result.and_then(|raw| self.finish_terminal_clipboard_paste(tab_id, &raw));
        if let Err(error) = result {
            self.record_terminal_paste_failure(&error);
        }
        true
    }

    fn finish_terminal_clipboard_paste(
        &mut self,
        tab_id: u64,
        raw: &str,
    ) -> Result<(), TerminalPasteFailure> {
        if !terminal_paste_target_is_current(
            tab_id,
            self.active,
            self.focus_surface,
            self.window_focused,
            self.modal_surface_active(),
        ) {
            return Err(TerminalPasteFailure::StaleTarget);
        }
        let position = self
            .active_position()
            .ok_or(TerminalPasteFailure::StaleTarget)?;
        let text = normalize_terminal_paste(raw);
        if text.is_empty() {
            return Err(TerminalPasteFailure::Empty);
        }
        if text.len() > TERMINAL_PASTE_LIMIT_BYTES {
            return Err(TerminalPasteFailure::NormalizedTextTooLarge);
        }
        let bracketed = self.tabs[position].parser.screen().bracketed_paste();
        let bytes = terminal_paste_bytes(&text, bracketed);
        if !self.tabs[position].send(&bytes) {
            return Err(TerminalPasteFailure::TerminalRejected);
        }
        let _ = self.cancel_terminal_selection(true);
        self.event_journal_mut().commit(
            EventKind::TerminalPasted,
            Some(tab_id),
            serde_json::json!({
                "characters": text.chars().count(),
                "bytes": text.len(),
                "bracketed": bracketed,
                "source": "keyboard",
                "operation_id": crate::operations::TERMINAL_PASTE,
            }),
        );
        self.set_status_message(format!("Pasted {} characters into @{tab_id}", text.len()));
        self.last_feedback_error = None;
        self.request_redraw();
        Ok(())
    }

    fn scrollbar_state(
        &mut self,
    ) -> Option<(crate::ui_geometry::TerminalScrollbarGeometry, usize)> {
        let position = self.active_position()?;
        let layout = self.layout();
        let visible_rows = usize::from(self.tabs[position].last_size.0);
        let (offset, maximum) = self.tabs[position].scrollback_bounds();
        Some((
            scrollbar_geometry(&layout, visible_rows, offset, maximum),
            maximum,
        ))
    }

    fn click_scrollbar(&mut self, x: i32, y: i32) -> bool {
        let Some((geometry, maximum)) = self.scrollbar_state() else {
            return false;
        };
        let Some(hit) = scrollbar_hit_test(&geometry, x, y) else {
            return false;
        };
        if maximum == 0 {
            return true;
        }
        match hit {
            ScrollbarHit::Thumb => {
                self.scroll_drag = Some(ScrollbarThumbDrag::begin(y, geometry.thumb.top));
            }
            ScrollbarHit::TrackAbove | ScrollbarHit::TrackBelow => {
                if let Some(position) = self.active_position() {
                    let action = if matches!(hit, ScrollbarHit::TrackAbove) {
                        "page-up"
                    } else {
                        "page-down"
                    };
                    if let Ok(offset) = self.tabs[position].scroll_viewport(action, None) {
                        self.on_viewport_scrolled(position, offset, "scrollbar-track");
                    }
                }
            }
        }
        true
    }

    fn drag_scrollbar(&mut self, y: i32) {
        let Some(drag) = self.scroll_drag else {
            return;
        };
        let Some((geometry, maximum)) = self.scrollbar_state() else {
            self.end_scroll_drag();
            return;
        };
        let offset = scrollback_for_thumb_top(geometry, drag.thumb_top(y), maximum);
        if let Some(position) = self.active_position() {
            let offset = self.tabs[position].set_scrollback(offset);
            self.on_viewport_scrolled(position, offset, "scrollbar-drag");
        }
    }

    fn end_scroll_drag(&mut self) {
        self.scroll_drag = None;
    }

    fn scroll_sidebar(&mut self, wheel_delta_notches: i32) {
        self.invalidate_sidebar_text_click();
        let steps = wheel_delta_notches.unsigned_abs() as usize * WHEEL_ROWS_PER_NOTCH;
        let maximum = self.sidebar_max_offset();
        self.sidebar_scroll_offset = if wheel_delta_notches > 0 {
            self.sidebar_offset().saturating_sub(steps)
        } else {
            self.sidebar_offset().saturating_add(steps).min(maximum)
        };
    }

    fn click_sidebar_scrollbar(&mut self, x: i32, y: i32) -> bool {
        let Some((geometry, current, maximum)) = self.sidebar_scrollbar_state() else {
            return false;
        };
        let Some(hit) = scrollbar_hit_test(&geometry, x, y) else {
            return false;
        };
        self.invalidate_sidebar_text_click();
        if maximum == 0 {
            return true;
        }
        match hit {
            ScrollbarHit::Thumb => {
                self.sidebar_scroll_drag = Some(ScrollbarThumbDrag::begin(y, geometry.thumb.top));
            }
            ScrollbarHit::TrackAbove | ScrollbarHit::TrackBelow => {
                let page = self.sidebar_row_capacity().max(1);
                self.sidebar_scroll_offset = if matches!(hit, ScrollbarHit::TrackAbove) {
                    current.saturating_sub(page)
                } else {
                    current.saturating_add(page).min(maximum)
                };
                self.request_redraw();
            }
        }
        true
    }

    fn drag_sidebar_scrollbar(&mut self, y: i32) {
        let Some(drag) = self.sidebar_scroll_drag else {
            return;
        };
        let Some((geometry, _, maximum)) = self.sidebar_scrollbar_state() else {
            self.end_sidebar_scroll_drag();
            return;
        };
        self.invalidate_sidebar_text_click();
        self.sidebar_scroll_offset =
            sidebar_scroll_offset_for_thumb_top(geometry, drag.thumb_top(y), maximum);
        self.request_redraw();
    }

    fn end_sidebar_scroll_drag(&mut self) {
        self.sidebar_scroll_drag = None;
    }

    /// Forwards one pointer event to the running application when it
    /// negotiated xterm mouse tracking on the active tab.
    ///
    /// Shift bypasses reporting so local selection and scrollback stay
    /// reachable (the xterm convention), and reports are suppressed while the
    /// viewport is scrolled back because reported cells would not match what
    /// the application drew.
    fn forward_terminal_mouse(
        &mut self,
        x: f64,
        y: f64,
        button: Option<u8>,
        pressed: bool,
        motion: bool,
    ) -> bool {
        if self.focus_gate().full_modal_blocked() {
            return false;
        }
        let Some(position) = self.active_position() else {
            return false;
        };
        let (mode, encoding, scrollback, alternate_screen) = {
            let screen = self.tabs[position].parser.screen();
            (
                screen.mouse_protocol_mode(),
                screen.mouse_protocol_encoding(),
                screen.scrollback(),
                screen.alternate_screen(),
            )
        };
        let product_mode = match mode {
            vt100::MouseProtocolMode::None => ApplicationMouseMode::None,
            vt100::MouseProtocolMode::Press => ApplicationMouseMode::Press,
            vt100::MouseProtocolMode::PressRelease => ApplicationMouseMode::PressRelease,
            vt100::MouseProtocolMode::ButtonMotion => ApplicationMouseMode::ButtonMotion,
            vt100::MouseProtocolMode::AnyMotion => ApplicationMouseMode::AnyMotion,
        };
        let dragging = self.mouse_report_button.is_some();
        let encoding = match encoding {
            vt100::MouseProtocolEncoding::Default => MouseReportEncoding::Default,
            vt100::MouseProtocolEncoding::Sgr => MouseReportEncoding::Sgr,
            vt100::MouseProtocolEncoding::Utf8 => MouseReportEncoding::Utf8,
        };
        let Some((column, row)) = self.cell_at_client(x, y) else {
            return false;
        };
        let input = MouseReportInput {
            mode: product_mode,
            encoding,
            shift: self.pointer_modifiers.shift,
            alt: self.pointer_modifiers.alt,
            control: self.pointer_modifiers.control,
            // Alt-screen TUIs need pass-through even if offset is stale.
            scrolled_back: scrollback != 0 && !alternate_screen,
            motion,
            dragging,
            pressed,
            button,
            current_button: self.mouse_report_button,
            current_cell: self.mouse_report_cell,
            column,
            row,
        };
        match mouse_report_outcome(input) {
            MouseReportOutcome::LocalSelection => false,
            MouseReportOutcome::Deduplicated => true,
            MouseReportOutcome::Send(bytes) => {
                self.mouse_report_cell = Some((column, row));
                self.queue_pty_input(bytes);
                true
            }
        }
    }

    fn mouse_wheel(&mut self, x: f64, y: f64, vertical_delta: f64, line_based: bool) {
        if self.focus_gate().full_modal_blocked() {
            return;
        }
        let layout = self.layout();
        let terminal = terminal_pixel_rect(&layout);
        let units = wheel_delta_units(vertical_delta, line_based);
        let target = route_wheel(
            self.config.tabs_visible && layout.sidebar_tree.contains(x as i32, y as i32),
            terminal.contains(x as i32, y as i32),
        );
        if target == WheelTarget::Ignored {
            return;
        }
        let notches = self.wheel_accumulator.push(units);
        if notches == 0 {
            return;
        }
        if target == WheelTarget::Sidebar {
            self.scroll_sidebar(notches);
            self.request_redraw();
            return;
        }
        let Some(position) = self.active_position() else {
            return;
        };
        let wheel_button = if notches > 0 { 64 } else { 65 };
        let mut reported = false;
        for _ in 0..notches.unsigned_abs().min(40) {
            if self.forward_terminal_mouse(x, y, Some(wheel_button), true, false) {
                reported = true;
            } else {
                break;
            }
        }
        if reported {
            return;
        }
        let (before, alternate_screen, application_cursor) = {
            let screen = self.tabs[position].parser.screen();
            (
                screen.scrollback(),
                screen.alternate_screen(),
                screen.application_cursor(),
            )
        };
        let rows = notches.unsigned_abs() as usize * WHEEL_ROWS_PER_NOTCH;
        let action = if notches > 0 { "up" } else { "down" };
        let after = self.tabs[position]
            .scroll_viewport(action, Some(rows))
            .unwrap_or(before);
        if after != before {
            self.on_viewport_scrolled(position, after, "mouse-wheel");
        } else if alternate_screen || application_cursor {
            let _ = self.cancel_terminal_selection(true);
            self.queue_pty_input(alternate_screen_wheel_bytes(
                notches > 0,
                rows,
                application_cursor,
            ));
        }
    }

    fn active_position(&self) -> Option<usize> {
        let active = self.active?;
        self.tabs.iter().position(|tab| tab.id == active)
    }

    fn initial_tab_size(&self) -> (u16, u16) {
        self.active_position()
            .and_then(|position| self.tabs.get(position))
            .or_else(|| self.tabs.first())
            .map(|tab| tab.last_size)
            .unwrap_or_else(|| {
                self.grid
                    .as_ref()
                    .map(|grid| (grid.rows, grid.cols))
                    .unwrap_or((24, 80))
            })
    }

    fn cell_dimensions(&self) -> (u32, u32) {
        cell_metrics(self.active_terminal_appearance().terminal_font_size)
    }

    fn ime_anchor(&self) -> Option<(u32, u32, u32, u32)> {
        if self.window_close_dialog.is_open()
            || self.close_confirmation.is_open()
            || self.settings_dialog.is_open()
        {
            return None;
        }
        let (client_width, client_height) = self.client_size();
        if self.new_terminal_dialog.is_open() {
            let modal = NewTerminalModalView::for_client(
                client_width,
                client_height,
                self.render_shell_choice(),
                self.new_terminal_dialog.initial_command_draft(),
                self.new_terminal_dialog.http_proxy_draft(),
                self.new_terminal_dialog.https_proxy_draft(),
                self.new_terminal_focus,
            );
            return Some(match self.new_terminal_focus {
                NewTerminalFocusView::InitialCommand => modal.initial_command_field,
                NewTerminalFocusView::HttpProxy => modal.http_proxy_field,
                NewTerminalFocusView::HttpsProxy => modal.https_proxy_field,
            });
        }
        if let Some(tab_id) = self.tab_editor_target_id() {
            let rows = self.sidebar_viewport_rows();
            let (viewport_position, row) =
                rows.iter().enumerate().find(|(_, row)| row.id == tab_id)?;
            let geometry = self.sidebar_row_geometry(viewport_position, row.depth, tab_id);
            let editors = geometry.editors?;
            let field = match self.tab_editor_dialog.focus() {
                TabEditorFocus::Name => editors.name,
                TabEditorFocus::Note => editors.note,
            };
            return Some(u32_rect(field));
        }
        let layout = self.layout();
        if self.focus_surface == UnixFocusSurface::Composer {
            let line_columns = self
                .composer_buffer
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .width() as u32;
            let left = layout.composer.left.max(0) as u32 + 8;
            let right = layout.composer.right.max(0) as u32;
            let x = (left + 2 * 8 + line_columns * 8).min(right.saturating_sub(16));
            return Some((x, layout.composer.top.max(0) as u32 + 18, 8, 20));
        }
        if self.focus_surface == UnixFocusSurface::Terminal {
            let position = self.active_position()?;
            let (row, column) = self.tabs[position].parser.screen().cursor_position();
            let (cell_width, cell_height) = self.cell_dimensions();
            return Some((
                layout.terminal.left.max(0) as u32 + u32::from(column) * cell_width,
                layout.terminal.top.max(0) as u32 + u32::from(row) * cell_height,
                cell_width,
                cell_height,
            ));
        }
        None
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn open_window(&mut self, window: &PixelWindow) -> Result<(), PixelWindowError> {
        if self.window.is_some() {
            return Ok(());
        }
        self.window = Some(window.clone());
        let waker = window.waker();
        install_unix_wake(move || {
            let _ = waker.wake();
        });
        let layout = self.layout();
        let (cell_width, cell_height) = self.cell_dimensions();
        let (cols, rows) = grid_dimensions_for_terminal(
            layout.terminal.width().max(0) as u32,
            layout.terminal.height().max(0) as u32,
            cell_width,
            cell_height,
        );
        let grid = TerminalGrid::new(cols, rows, self.palette());

        // Normal restart restores the saved tab tree with honestly restarted
        // PTY processes; a missing or empty workspace starts one fresh tab.
        let saved = crate::workspace::load_workspace().filter(|saved| !saved.tabs.is_empty());
        let mut restore_errors = Vec::new();
        if let Some(saved) = saved {
            for saved_tab in &saved.tabs {
                match TerminalTab::spawn(TerminalLaunch {
                    id: saved_tab.id,
                    index: saved_tab.index,
                    parent_id: saved_tab.parent_id,
                    title: None,
                    command_line: saved_tab.command_line.clone(),
                    tab_environment: Vec::new(),
                    session_name: self.session_name.clone(),
                    window: 0,
                    wake_signal: Arc::clone(&self.wake_signal),
                    initial_size: TerminalSize { rows, cols },
                }) {
                    Ok(mut tab) => {
                        tab.note = saved_tab.note.clone();
                        tab.composer = saved_tab.composer.clone();
                        self.next_tab_id = self.next_tab_id.max(saved_tab.id + 1);
                        self.tabs.push(tab);
                        self.event_journal_mut().commit(
                            EventKind::TabCreated,
                            Some(saved_tab.id),
                            serde_json::json!({
                                "index": saved_tab.index,
                                "parent_id": saved_tab.parent_id,
                                "selected": false,
                                "restored": true,
                            }),
                        );
                    }
                    Err(error) => restore_errors.push(format!("@{}: {error:#}", saved_tab.id)),
                }
            }
            self.tabs.sort_by_key(|tab| tab.index);
            self.collapsed_tabs = saved
                .collapsed_ids
                .iter()
                .copied()
                .filter(|id| self.tabs.iter().any(|tab| tab.id == *id))
                .collect();
            self.active = saved
                .active_id
                .filter(|id| self.tabs.iter().any(|tab| tab.id == *id))
                .or_else(|| self.tabs.first().map(|tab| tab.id));
        }
        if !restore_errors.is_empty() {
            self.set_status_message(format!(
                "Could not restore {} tab(s): {}",
                restore_errors.len(),
                restore_errors.join("; ")
            ));
        }
        if self.tabs.is_empty() {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            let tab = TerminalTab::spawn(TerminalLaunch {
                id,
                index: 0,
                parent_id: None,
                title: None,
                command_line: Vec::new(),
                tab_environment: Vec::new(),
                session_name: self.session_name.clone(),
                window: 0,
                wake_signal: Arc::clone(&self.wake_signal),
                initial_size: TerminalSize { rows, cols },
            })
            .map_err(|error| {
                PixelWindowError::failed("pixel_window_initial_terminal_failed", error)
            })?;
            self.active = Some(id);
            self.tabs.push(tab);
            self.event_journal_mut().commit(
                EventKind::TabCreated,
                Some(id),
                serde_json::json!({
                    "index": 0,
                    "parent_id": None::<u64>,
                    "selected": true,
                }),
            );
        }

        window.request_redraw();
        self.grid = Some(grid);
        let active = self.active;
        if let Some(id) = active {
            self.event_journal_mut().commit(
                EventKind::TabSelected,
                Some(id),
                serde_json::json!({}),
            );
        }
        self.load_composer_buffer_from_tab();
        self.sync_grid_from_tab();
        Ok(())
    }

    fn resize_to_window(&mut self) {
        self.invalidate_sidebar_text_click();
        let Some(window) = self.window.clone() else {
            return;
        };
        if !self.resize_active_tab_to_layout() {
            return;
        }
        self.window_state_tracker
            .sync_from_native_flags(window.minimized(), window.maximized());
        self.sync_grid_from_tab();
    }

    /// Resizes the active tab's PTY and the shared grid to the current layout.
    ///
    /// A tab keeps its last PTY size while it is not active, so every
    /// activation must reconcile it against the layout the window has now;
    /// otherwise a background tab sized under an older layout renders more
    /// rows than the viewport can show and its bottom rows stay clipped.
    fn resize_active_tab_to_layout(&mut self) -> bool {
        let layout = self.layout();
        let (cell_width, cell_height) = self.cell_dimensions();
        let (cols, rows) = grid_dimensions_for_terminal(
            layout.terminal.width().max(0) as u32,
            layout.terminal.height().max(0) as u32,
            cell_width,
            cell_height,
        );
        if let Some(position) = self.active_position()
            && self.tabs[position].last_size != (rows, cols)
            && let Err(error) = self.tabs[position].resize(rows, cols)
        {
            self.set_status_message(format!("Could not resize terminal: {error}"));
            return false;
        }
        if let Some(grid) = self.grid.as_mut() {
            grid.resize(cols, rows);
        }
        true
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

    fn persist_workspace(&mut self) -> anyhow::Result<()> {
        let workspace = self.saved_workspace();
        save_workspace(&workspace)?;
        self.last_saved_workspace = Some(workspace);
        self.last_workspace_save = Some(Instant::now());
        self.event_journal
            .commit(EventKind::WorkspaceSaved, None, serde_json::json!({}));
        Ok(())
    }

    /// Debounced workspace autosave. Every structural or draft change lands
    /// on disk within a second, so a quit or crash never loses the tab tree;
    /// nothing is written while the workspace is unchanged. Returns the next
    /// deadline while a change is still waiting on the debounce interval.
    fn autosave_workspace(&mut self, now: Instant) -> Option<Instant> {
        const WORKSPACE_AUTOSAVE_INTERVAL: Duration = Duration::from_secs(1);
        let workspace = self.saved_workspace();
        if self.last_saved_workspace.as_ref() == Some(&workspace) {
            return None;
        }
        let due = self
            .last_workspace_save
            .map(|at| at + WORKSPACE_AUTOSAVE_INTERVAL)
            .unwrap_or(now);
        if now < due {
            return Some(due);
        }
        if let Err(error) = save_workspace(&workspace) {
            self.set_status_message(format!("Could not save workspace: {error:#}"));
            self.last_workspace_save = Some(now);
            return None;
        }
        self.last_saved_workspace = Some(workspace);
        self.last_workspace_save = Some(now);
        self.event_journal
            .commit(EventKind::WorkspaceSaved, None, serde_json::json!({}));
        None
    }

    fn handle_ipc(&mut self, envelope: IpcEnvelope) {
        let command = envelope.request.args.first().map(String::as_str);
        let response = match dispatch_shared_command(self, &envelope.request.args) {
            Some(response) => response,
            None if command == Some("save-workspace") => match self.persist_workspace() {
                Ok(()) => IpcResponse::success(workspace_path().display().to_string()),
                Err(error) => IpcResponse::typed_failure(
                    format!("{error:#}"),
                    "operation_persistence_failed",
                    "precondition",
                    false,
                ),
            },
            None if command == Some("shutdown") => {
                if let Err(error) = self.persist_workspace() {
                    IpcResponse::typed_failure(
                        format!("{error:#}"),
                        "operation_persistence_failed",
                        "precondition",
                        false,
                    )
                } else if let Err(error) = mark_intentional_shutdown(&crate::ipc_address()) {
                    IpcResponse::typed_failure(
                        format!("{error:#}"),
                        "operation_persistence_failed",
                        "precondition",
                        false,
                    )
                } else {
                    self.event_journal.commit(
                        EventKind::WorkspaceShutdown,
                        None,
                        serde_json::json!({"saved": true}),
                    );
                    self.close_requested = true;
                    IpcResponse::success("")
                }
            }
            None if matches!(
                command,
                Some("screenshot") | Some("screenshot-pane") | Some("screenshot-tab")
            ) =>
            {
                let pane_only = !matches!(command, Some("screenshot"));
                match self.save_screenshot(&envelope.request.args, pane_only) {
                    Ok(path) => IpcResponse::success(path),
                    Err(error) => IpcResponse::failure(error),
                }
            }
            None if command == Some("ui-input") => {
                match crate::frontend::pointer_input::parse_pointer_request(&envelope.request.args)
                {
                    Ok(request) => match self.apply_pointer_request(request) {
                        // Returning the fresh snapshot lets a caller act and
                        // observe the result in one round trip.
                        Ok(()) => IpcResponse::success(self.build_ui_snapshot_json()),
                        Err(error) => IpcResponse::failure(error),
                    },
                    Err(error) => IpcResponse::typed_failure(
                        error,
                        "operation_invalid_arguments",
                        "validation",
                        false,
                    ),
                }
            }
            None if command == Some("ui-action") => {
                let args = &envelope.request.args;
                let action = args.get(1).map(String::as_str).unwrap_or("");
                if self.cwd_editor_dialog.is_open()
                    && !matches!(
                        action,
                        "cwd-prepare"
                            | "cwd-prepare-append"
                            | "cwd-prepare-replace"
                            | "cwd-send-now"
                            | "cancel"
                    )
                {
                    IpcResponse::failure(
                        "CWD editor is a focus trap; prepare, send now, or cancel it first",
                    )
                } else {
                    let response = match action {
                        "close-window" => {
                            self.request_window_close();
                            None
                        }
                        "keep-server-running" => {
                            if !self.window_close_dialog.is_open() {
                                Some(IpcResponse::failure(
                                    "no window-close confirmation is pending",
                                ))
                            } else {
                                self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                                None
                            }
                        }
                        "stop-server-and-exit" => {
                            if !self.window_close_dialog.is_open() {
                                Some(IpcResponse::failure(
                                    "no window-close confirmation is pending",
                                ))
                            } else {
                                self.finish_window_close(WindowCloseChoice::StopServerAndExit);
                                None
                            }
                        }
                        "open-cwd-editor" => match self.open_cwd_editor(option_value(args, "-t")) {
                            Ok(()) => None,
                            Err(error) => Some(IpcResponse::failure(error)),
                        },
                        "cwd-prepare" => {
                            match ComposerWriteMode::parse(option_value(args, "--mode")) {
                                Ok(mode) => match self.prepare_cwd(
                                    option_value(args, "-t"),
                                    option_value(args, "--path").map(str::to_owned),
                                    mode,
                                ) {
                                    Ok(()) => None,
                                    Err(error) => Some(IpcResponse::failure(error)),
                                },
                                Err(error) => Some(IpcResponse::failure(error)),
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
                                Ok(()) => {
                                    self.set_status_message("Control Center opened");
                                    None
                                }
                                Err(error) => Some(IpcResponse::failure(error)),
                            }
                        }
                        "cwd-send-now" => match option_value(args, "--path") {
                            Some(path) => {
                                match self.send_cwd_now(option_value(args, "-t"), path.to_owned()) {
                                    Ok(()) => None,
                                    Err(error) => Some(IpcResponse::failure(error)),
                                }
                            }
                            None => Some(IpcResponse::failure("cwd-send-now requires --path")),
                        },
                        "open-new-terminal" => {
                            self.open_new_terminal_dialog();
                            None
                        }
                        "open-control-center" => {
                            match crate::control_center::open_control_center(
                                self.no_activate,
                                &crate::client::ipc_address(),
                            ) {
                                Ok(()) => None,
                                Err(error) => Some(IpcResponse::typed_failure(
                                    format!("{error:#}"),
                                    "control_center_unavailable",
                                    "availability",
                                    true,
                                )),
                            }
                        }
                        "terminal-paste" => match self.request_terminal_clipboard_paste() {
                            Ok(()) => None,
                            Err(error) => {
                                self.record_terminal_paste_failure(&error);
                                Some(error.ipc_response())
                            }
                        },
                        // Same product verbs as toolbar `action::*` (shared with Windows).
                        "toggle-locale" => {
                            self.toggle_locale();
                            None
                        }
                        "font-decrease" => {
                            self.adjust_active_terminal_font(-1);
                            None
                        }
                        "font-increase" => {
                            self.adjust_active_terminal_font(1);
                            None
                        }
                        other => {
                            if let Some(window) = self.window.as_ref() {
                                let handle = PixelWindowHandle {
                                    window,
                                    title: &self.title,
                                };
                                match apply_ui_action(
                                    other,
                                    args,
                                    &handle,
                                    &mut self.window_state_tracker,
                                ) {
                                    WindowUiActionResult::Applied => None,
                                    WindowUiActionResult::ActivationRequested => {
                                        self.set_status_message("Window activation requested");
                                        self.request_redraw();
                                        None
                                    }
                                    WindowUiActionResult::Invalid(error) => {
                                        Some(IpcResponse::failure(error))
                                    }
                                    WindowUiActionResult::NotHandled => match other {
                                        "create"
                                        | "cancel"
                                        | "shell-default"
                                        | "shell-primary"
                                        | "shell-zsh"
                                        | "shell-sh"
                                        | "shell-bash"
                                        | "shell-cmd"
                                        | "shell-powershell"
                                        | "new-terminal-set-initial-command"
                                        | "new-terminal-set-http-proxy"
                                        | "new-terminal-set-https-proxy" => {
                                            let text = args.get(2).map(String::as_str);
                                            match new_terminal::dispatch_ui_action(
                                                &mut self.new_terminal_dialog,
                                                other,
                                                text,
                                            ) {
                                                Ok(Some(params)) => {
                                                    if let Ok(index) = self.create_tab(
                                                        None,
                                                        params.command_line,
                                                        params.tab_environment,
                                                        true,
                                                        None,
                                                    ) && let Some(id) = self
                                                        .tabs
                                                        .iter()
                                                        .find(|tab| tab.index == index)
                                                        .map(|tab| tab.id)
                                                    {
                                                        self.after_create_tab(id, None);
                                                    }
                                                    None
                                                }
                                                Ok(None) => self
                                                    .new_terminal_dialog
                                                    .last_error()
                                                    .map(|error| {
                                                        IpcResponse::failure(error.to_owned())
                                                    }),
                                                Err(error) => Some(IpcResponse::failure(error)),
                                            }
                                        }
                                        _ => Some(IpcResponse::failure(format!(
                                            "unknown UI action: {other}"
                                        ))),
                                    },
                                }
                            } else {
                                Some(if other == "window-activate" {
                                    IpcResponse::typed_failure(
                                        "window is not available for activation",
                                        "ui_window_activation_failed",
                                        "availability",
                                        true,
                                    )
                                } else {
                                    IpcResponse::failure("window is not available for UI action")
                                })
                            }
                        }
                    };
                    match response {
                        Some(response) => response,
                        None => IpcResponse::success(self.build_ui_snapshot_json()),
                    }
                }
            }
            None => IpcResponse::typed_failure(
                format!(
                    "Unix GUI does not implement `{}` yet",
                    envelope
                        .request
                        .args
                        .first()
                        .map(String::as_str)
                        .unwrap_or("<empty>")
                ),
                "unix_gui_unsupported",
                "unsupported",
                false,
            ),
        };
        let _ = envelope.respond_to.send(response);
    }

    fn drain_wake_and_pty(&mut self) -> bool {
        self.wake_signal.begin_drain();

        let mut changed = false;
        let mut terminal_changed = false;
        while let Ok(envelope) = self.ipc_server.try_recv() {
            changed = true;
            self.handle_ipc(envelope);
        }

        for tab in &mut self.tabs {
            if tab.poll() {
                changed = true;
                terminal_changed = true;
            }
        }
        if terminal_changed {
            self.cursor_blink.reset(Instant::now());
        }
        if changed {
            self.sync_grid_from_tab();
        }
        changed
    }

    fn sync_grid_from_tab(&mut self) {
        let Some(position) = self.active_position() else {
            return;
        };
        let Some(grid) = self.grid.as_mut() else {
            return;
        };
        grid.sync_from_screen(self.tabs[position].parser.screen());
    }

    fn queue_pty_input(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Some(position) = self.active_position() {
            let _ = self.tabs[position].send(&bytes);
        }
        self.cursor_blink.reset(Instant::now());
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn render_window(&mut self, frame: &mut XrgbPixelFrame<'_>) {
        note_frame_for_diagnostics();
        self.last_present = Some(Instant::now());
        let width = frame.width();
        let height = frame.height();
        self.render_pixels(width, height, frame.pixels_mut());
    }

    fn render_pixels(&mut self, width: u32, height: u32, buffer: &mut [u32]) {
        self.sync_grid_from_tab();
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let (logical_width, logical_height) = self.client_size();
        let ime_anchor = self.ime_anchor();
        if let Some((x, y, w, h)) = ime_anchor
            && let Err(error) = window.set_ime_cursor_area(LogicalRect::new(
                f64::from(x),
                f64::from(y),
                f64::from(w.max(1)),
                f64::from(h.max(1)),
            ))
        {
            let message = format!("IME cursor update failed: {error}");
            if self.status_message != message {
                self.status_message = message;
            }
        }

        let sidebar_rows = self.sidebar_viewport_rows();
        let palette = self.palette();
        let layout = self.layout();
        let sidebar_width = self.sidebar_width();
        let (cell_width, cell_height) = self.cell_dimensions();
        let content_height = layout.terminal.bottom.max(0) as u32;
        let cwd_label = self.active_cwd_status_text();
        let composer_label = self.composer_label();
        let scrollbar = self.active_position().map(|position| {
            let visible_rows = usize::from(self.tabs[position].last_size.0);
            let (offset, maximum) = self.tabs[position].scrollback_bounds();
            scrollbar_view_from_geometry(scrollbar_geometry(&layout, visible_rows, offset, maximum))
        });
        let sidebar_scrollbar = self
            .sidebar_scrollbar_state()
            .map(|geometry| scrollbar_view_from_geometry(geometry.0));
        let settings = self.settings_dialog.is_open().then(|| {
            SettingsModalView::for_client(
                logical_width,
                logical_height,
                self.settings_size_draft(),
                self.settings_dialog.preset_draft(),
                self.config.locale,
            )
        });
        let new_terminal = if self.new_terminal_dialog.is_open() {
            let shell = self.render_shell_choice();
            Some(
                NewTerminalModalView::for_client(
                    logical_width,
                    logical_height,
                    shell,
                    self.new_terminal_dialog.initial_command_draft(),
                    self.new_terminal_dialog.http_proxy_draft(),
                    self.new_terminal_dialog.https_proxy_draft(),
                    self.new_terminal_focus,
                )
                .with_selected_all(self.text_field_select_all),
            )
        } else {
            None
        };
        let Some(grid) = self.grid.as_ref() else {
            return;
        };

        let modal_active = self.modal_surface_active();
        // Rows carry their own geometry so the painted list and any future
        // hit-test agree; keyboard and ui-action drive it today.
        let instance_picker_view = self.instance_picker_dialog.is_open().then(|| {
            let (client_width, client_height) = self.client_size();
            let width = render::INSTANCE_PICKER_WIDTH.min(client_width.saturating_sub(32).max(1));
            let rows: Vec<_> = self
                .instance_picker_dialog
                .rows()
                .iter()
                .enumerate()
                .map(|(index, row)| render::InstancePickerRowView {
                    label: row.instance_label.clone(),
                    detail: format!("pid {} · {}", row.pid, row.classification),
                    selected: index == self.instance_picker_dialog.selected_index(),
                    can_attach: row.can_attach,
                })
                .collect();
            let body = 56 + rows.len().max(1) as u32 * render::INSTANCE_PICKER_ROW_HEIGHT + 40;
            let height = body.min(client_height.saturating_sub(32).max(1));
            let left = client_width.saturating_sub(width) / 2;
            let top = client_height.saturating_sub(height) / 2;
            render::InstancePickerView {
                bounds: (left, top, width, height),
                rows,
                row_height: render::INSTANCE_PICKER_ROW_HEIGHT,
                first_row_top: top + 48,
                error: self
                    .instance_picker_dialog
                    .last_error()
                    .map(ToOwned::to_owned),
            }
        });

        // Chip geometry comes from the shared strip layout, so the painted
        // chips and the snapshot bounds an agent clicks are the same rects.
        let active_instance = crate::client::ipc_address();
        let server_strip_view = layout.server_strip.map(|strip| {
            let chips = self
                .server_tab_rects()
                .into_iter()
                .map(|(chip, row)| render::ServerStripChipView {
                    bounds: u32_rect(chip),
                    label: server_tab_chip_label(&row.instance, row.can_attach),
                    can_attach: row.can_attach,
                    active: row.endpoint == active_instance,
                })
                .collect();
            let add = layout_server_add_chip(StripRect {
                left: strip.left,
                top: strip.top,
                right: strip.right,
                bottom: strip.bottom,
            });
            render::ServerStripView {
                menu: self.server_context_menu_geometry().map(
                    |menu| render::ServerStripMenuView {
                        frame: u32_rect(menu.frame),
                        as_window: u32_rect(menu.as_window),
                        close: u32_rect(menu.close),
                    },
                ),
                bounds: u32_rect(strip),
                chips,
                add: u32_rect(crate::ui_geometry::PixelRect {
                    left: add.left,
                    top: add.top,
                    right: add.right,
                    bottom: add.bottom,
                }),
            }
        });
        let workspace_toolbar = if !self.focus_gate().workspace_controls_visible() {
            None
        } else {
            layout.workspace_toolbar.map(|toolbar| {
                WorkspaceToolbarView::from_layout(
                    toolbar,
                    self.config.tabs_visible,
                    self.config.locale,
                )
            })
        };
        let composer_top = layout.composer.top.max(0) as u32;
        let composer_geometry = composer_geometry(layout.composer);
        let send = composer_geometry.send;
        let composer_view = ComposerView {
            text: &self.composer_buffer,
            focused: self.focus_surface == UnixFocusSurface::Composer,
            selected_all: self.composer_select_all,
            selection: self
                .composer_cursor
                .has_selection()
                .then(|| self.composer_cursor.range()),
            caret: self.composer_cursor.focus(),
            top: composer_top,
            label: composer_label,
            send_button: (
                send.left.max(0) as u32,
                send.top.max(0) as u32,
                send.width().max(0) as u32,
                send.height().max(0) as u32,
            ),
        };
        let ime_label = agenterm_platform::ime::status()
            .map(|status| status.label())
            .unwrap_or_else(|| "IME: off".to_owned());
        let status_view = StatusBarView {
            bounds: u32_rect(layout.status),
            cwd_bounds: u32_rect(layout.status_segments.cwd),
            provider_bounds: if layout.status_segments.provider.width() > 0 {
                Some(u32_rect(layout.status_segments.provider))
            } else {
                None
            },
            ime_bounds: if layout.status_segments.ime.width() > 0 {
                Some(u32_rect(layout.status_segments.ime))
            } else {
                None
            },
            tabs_recovery: layout.status_segments.tabs_recovery.map(u32_rect),
            cwd_text: &cwd_label,
            provider_text: &self.status_message,
            ime_text: &ime_label,
        };
        let terminal_selection = self
            .terminal_selection
            .filter(|selection| self.active == Some(selection.tab_id));
        let confirm_close = self
            .close_confirmation_target_id()
            .map(|id| ConfirmCloseView::for_client(logical_width, logical_height, id));
        let window_close = self
            .window_close_dialog
            .is_open()
            .then(|| WindowCloseView::for_client(logical_width, logical_height));
        let resize_grip = layout.resize_grip.map(u32_rect);

        let tab_editor = self.tab_editor_dialog.is_open().then(|| TabEditorView {
            name_draft: self.tab_editor_dialog.name_draft().to_owned(),
            note_draft: self.tab_editor_dialog.note_draft().to_owned(),
            focus: match self.tab_editor_dialog.focus() {
                TabEditorFocus::Name => TabEditorFocusView::Name,
                TabEditorFocus::Note => TabEditorFocusView::Note,
            },
            selected_all: self.text_field_select_all,
        });
        let editing_tab_id = self.tab_editor_target_id();
        let ime_preedit = ime_anchor
            .filter(|_| !self.ime_preedit.is_empty())
            .map(|anchor| ImePreeditView {
                text: &self.ime_preedit,
                cursor: self.ime_cursor,
                anchor,
            });
        let cursor_appearance = self
            .active_position()
            .map(|position| self.tabs[position].cursor_appearance())
            .unwrap_or_default();
        let cursor_style =
            if self.focus_surface != UnixFocusSurface::Terminal || self.modal_surface_active() {
                TerminalCursorStyle::Hidden
            } else if !self.window_focused {
                TerminalCursorStyle::Inactive
            } else if self.cursor_blink.visible() {
                TerminalCursorStyle::Active
            } else {
                TerminalCursorStyle::Hidden
            };
        let hidpi_terminal_visible = !modal_active && ime_preedit.is_none();
        let hidpi_terminal_active =
            hidpi_terminal_visible && (width != logical_width || height != logical_height);
        if self.render_buffers.physical_size != (width, height) {
            self.render_buffers.physical.clear();
            self.render_buffers
                .physical
                .resize(width as usize * height as usize, 0);
            self.render_buffers.physical_size = (width, height);
            self.render_buffers.logical_hash = 0;
        }
        let logical_pixels = self
            .render_buffers
            .logical_frame(logical_width, logical_height);
        render_frame(
            logical_pixels,
            logical_width,
            logical_width,
            logical_height,
            palette,
            FrameContent {
                sidebar_width,
                content_height,
                tree_height: layout.sidebar_tree.height().max(0) as u32,
                cell_width,
                cell_height,
                terminal: TerminalPaint {
                    grid,
                    selection: terminal_selection,
                    cursor_style,
                    cursor_shape: cursor_appearance.shape,
                },
                terminal_at_logical_resolution: !hidpi_terminal_active,
                sidebar_rows: &sidebar_rows,
                sidebar_tree: layout.sidebar_tree,
                editing_tab_id,
                tab_editor,
                workspace_toolbar,
                server_strip: server_strip_view,
                terminal_top: layout.terminal.top.max(0) as u32,
                composer: composer_view,
                scrollbar,
                sidebar_scrollbar,
                settings,
                confirm_close,
                instance_picker: instance_picker_view,
                window_close,
                new_terminal,
                status: Some(status_view),
                ime_preedit,
                resize_grip,
            },
        );
        let layer_geometry = if hidpi_terminal_active {
            terminal_layer_geometry(
                width,
                height,
                logical_width,
                logical_height,
                content_height,
                sidebar_width,
                layout.terminal.top.max(0) as u32,
                cell_width,
                cell_height,
                grid.cols,
                grid.rows,
            )
        } else {
            None
        };
        // While the persistent layer owns the terminal viewport, the skip and
        // hash regions cover the whole logical terminal rect (scrollbar strip
        // included); its fringe is rescaled per present below, so per-frame
        // scrollbar movement never forces a full chrome rescale.
        let logical_terminal_rect = layer_geometry.map(|_| {
            (
                sidebar_width,
                layout.terminal.top.max(0) as u32,
                (layout.terminal.right.max(0) as u32).saturating_sub(sidebar_width),
                layout.terminal.height().max(0) as u32,
            )
        });
        let physical_terminal_rect = logical_terminal_rect.map(|rect| {
            scale_rect_to_frame(rect, (logical_width, logical_height), (width, height))
        });
        let RenderBuffers {
            logical,
            physical,
            logical_hash,
            ..
        } = &mut self.render_buffers;
        let content_hash = frame_content_hash(logical, logical_width, logical_terminal_rect);
        if content_hash != *logical_hash {
            *logical_hash = content_hash;
            scale_frame_nearest(
                logical,
                logical_width,
                logical_height,
                physical,
                width,
                height,
                physical_terminal_rect,
            );
        }
        if let (Some((skip_x, skip_y, skip_width, skip_height)), Some(geometry)) =
            (physical_terminal_rect, layer_geometry)
        {
            let layer_right = geometry.offset_x + geometry.width;
            let layer_bottom = geometry.offset_y + geometry.height;
            let skip_right = skip_x + skip_width;
            let skip_bottom = skip_y + skip_height;
            if layer_right < skip_right {
                scale_frame_region(
                    logical,
                    logical_width,
                    logical_height,
                    physical,
                    width,
                    height,
                    (layer_right, skip_y, skip_right - layer_right, skip_height),
                );
            }
            if layer_bottom < skip_bottom {
                scale_frame_region(
                    logical,
                    logical_width,
                    logical_height,
                    physical,
                    width,
                    height,
                    (
                        skip_x,
                        layer_bottom,
                        layer_right.saturating_sub(skip_x),
                        skip_bottom - layer_bottom,
                    ),
                );
            }
            if geometry.offset_y > skip_y {
                scale_frame_region(
                    logical,
                    logical_width,
                    logical_height,
                    physical,
                    width,
                    height,
                    (
                        skip_x,
                        skip_y,
                        layer_right.saturating_sub(skip_x),
                        geometry.offset_y - skip_y,
                    ),
                );
            }
        }
        if let Some(geometry) = layer_geometry {
            let key = TerminalLayerKey {
                geometry,
                cols: grid.cols,
                rows: grid.rows,
                palette: std::ptr::from_ref(palette) as usize,
                selection: terminal_selection,
                cursor: grid.cursor_key(),
                cursor_style,
                cursor_shape: cursor_appearance.shape,
            };
            let previous = self.render_buffers.terminal_layer_key;
            let layer_len = geometry.width as usize * geometry.height as usize;
            let repaint_all = match previous {
                Some(previous) => {
                    previous.geometry != key.geometry
                        || previous.cols != key.cols
                        || previous.rows != key.rows
                        || previous.palette != key.palette
                        || previous.selection != key.selection
                        || self.render_buffers.terminal_layer.len() != layer_len
                }
                None => true,
            };
            let cursor_rows = match previous {
                Some(previous)
                    if !repaint_all
                        && (previous.cursor != key.cursor
                            || previous.cursor_style != key.cursor_style
                            || previous.cursor_shape != key.cursor_shape) =>
                {
                    [Some(previous.cursor.0), Some(key.cursor.0)]
                }
                _ => [None, None],
            };
            if repaint_all || cursor_rows.iter().any(Option::is_some) || grid.any_row_dirty() {
                self.render_buffers.terminal_layer.resize(layer_len, 0);
                render_terminal_layer(
                    &mut self.render_buffers.terminal_layer,
                    geometry,
                    TerminalPaint {
                        grid,
                        selection: terminal_selection,
                        cursor_style,
                        cursor_shape: cursor_appearance.shape,
                    },
                    palette,
                    repaint_all,
                    cursor_rows,
                );
            }
            self.render_buffers.terminal_layer_key = Some(key);
            blit_terminal_layer(
                &mut self.render_buffers.physical,
                width,
                height,
                &self.render_buffers.terminal_layer,
                geometry,
            );
        }
        let frame_pixels = (width as usize * height as usize).min(buffer.len());
        buffer[..frame_pixels].copy_from_slice(&self.render_buffers.physical[..frame_pixels]);
        self.render_buffers
            .capture_if_requested(width, height, buffer);
        if hidpi_terminal_active && let Some(grid) = self.grid.as_mut() {
            grid.clear_dirty_rows();
        }
    }

    fn request_close_tab(&mut self, id: u64) {
        if self
            .focus_gate()
            .modal_entry_blocked(ModalSurface::TabClose)
        {
            return;
        }
        let _ = self.cancel_terminal_selection(true);
        if self.cwd_editor_target_id() == Some(id) {
            self.close_cwd_editor();
        }
        if self.tab_editor_dialog.is_open() {
            let _ = self.complete_tab_editor(false);
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.exited.is_some() {
            let _ = self.close_tab_id(id);
            return;
        }
        self.sync_composer_buffer_to_tab();
        self.close_confirmation.open(format!("@{id}"));
        self.request_redraw();
    }

    fn close_confirmation_target_id(&self) -> Option<u64> {
        self.close_confirmation
            .target()
            .and_then(|target| target.strip_prefix('@'))
            .and_then(|id| id.parse().ok())
    }

    fn finish_close_confirmation(&mut self, confirm: bool) {
        let pending = self.close_confirmation_target_id();
        if confirm && let Some(id) = pending {
            let _ = self.close_tab_id(id);
        }
        self.close_confirmation.close();
        self.request_redraw();
    }

    fn save_screenshot(&mut self, args: &[String], pane_only: bool) -> Result<String, String> {
        self.cursor_blink.reset(Instant::now());
        self.render_buffers.request_capture();
        let metrics = self
            .window
            .as_ref()
            .ok_or_else(|| "no native window is available".to_owned())?
            .metrics()
            .map_err(|error| error.to_string())?;
        if !metrics.is_drawable() {
            return Err("native window has no drawable screenshot surface".to_owned());
        }
        if metrics.physical_width > agenterm_platform::screenshot::MAX_FRAME_SIDE
            || metrics.physical_height > agenterm_platform::screenshot::MAX_FRAME_SIDE
        {
            return Err(format!(
                "screenshot {}x{} exceeds side limit {}",
                metrics.physical_width,
                metrics.physical_height,
                agenterm_platform::screenshot::MAX_FRAME_SIDE
            ));
        }
        let pixel_count = usize::try_from(metrics.physical_width)
            .ok()
            .and_then(|width| {
                usize::try_from(metrics.physical_height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| "rendered frame dimensions overflow".to_owned())?;
        if pixel_count > agenterm_platform::screenshot::MAX_FRAME_PIXELS {
            return Err(format!(
                "screenshot exceeds the {}-pixel limit",
                agenterm_platform::screenshot::MAX_FRAME_PIXELS
            ));
        }
        let mut frame = Vec::new();
        frame
            .try_reserve_exact(pixel_count)
            .map_err(|error| format!("screenshot frame allocation failed: {error}"))?;
        frame.resize(pixel_count, 0_u32);
        self.render_pixels(metrics.physical_width, metrics.physical_height, &mut frame);
        let (width, height, pixels) = self
            .render_buffers
            .take_capture()
            .ok_or_else(|| "no rendered frame is available".to_owned())?;
        let path = screenshot_output_path(
            args,
            if pane_only {
                "agenterm-pane"
            } else {
                "agenterm-window"
            },
        );
        let clip = if pane_only {
            let terminal = terminal_pixel_rect(&self.layout());
            let logical_size = self.client_size();
            Some(scale_rect_to_frame(
                (
                    terminal.left.max(0) as u32,
                    terminal.top.max(0) as u32,
                    terminal.width().max(0) as u32,
                    terminal.height().max(0) as u32,
                ),
                logical_size,
                (width, height),
            ))
        } else {
            None
        };
        screenshot::write_xrgb_png(&path, width, height, &pixels, clip)?;
        Ok(path.display().to_string())
    }
}

fn platform_toolbar_action_id(hit: ToolbarHit) -> &'static str {
    match hit {
        ToolbarHit::NewTab => crate::frontend::action::NEW_TAB,
        ToolbarHit::ToggleTabs => crate::frontend::action::TOGGLE_TABS,
        ToolbarHit::ControlCenter => crate::frontend::action::OPEN_CONTROL_CENTER,
        ToolbarHit::Settings => crate::frontend::action::OPEN_SETTINGS,
        ToolbarHit::ToggleLocale => crate::frontend::action::TOGGLE_LOCALE,
        ToolbarHit::FontDecrease => crate::frontend::action::FONT_DECREASE,
        ToolbarHit::FontIncrease => crate::frontend::action::FONT_INCREASE,
    }
}

impl ControlHost for UnixApp {
    fn session_name(&self) -> &str {
        &self.session_name
    }

    fn event_journal(&self) -> &EventJournal {
        &self.event_journal
    }

    fn event_journal_mut(&mut self) -> &mut EventJournal {
        &mut self.event_journal
    }

    fn named_buffers(&self) -> &crate::named_buffer::NamedBufferStore {
        &self.named_buffers
    }

    fn named_buffers_mut(&mut self) -> &mut crate::named_buffer::NamedBufferStore {
        &mut self.named_buffers
    }

    fn request_ui_redraw(&mut self) {
        self.request_redraw();
    }

    fn ui_snapshot_json(&mut self) -> Option<String> {
        Some(self.build_ui_snapshot_json())
    }

    fn sync_composer_from_ui(&mut self) {
        self.sync_composer_buffer_to_tab();
    }

    fn prepare_composer_send(&mut self) -> Result<bool, String> {
        if self.cwd_editor_dialog.is_open() {
            self.prepare_cwd(None, None, ComposerWriteMode::Replace)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn after_create_tab(&mut self, id: u64, parent_id: Option<u64>) {
        if parent_id.is_some() {
            let _ = self.open_tab_editor_for(id);
        }
    }

    fn load_composer_to_ui(&mut self) {
        self.load_composer_buffer_from_tab();
        self.request_redraw();
    }

    fn focus_surface(&self) -> &str {
        self.focus_surface.as_str()
    }

    fn set_ipc_focus_surface(&mut self, surface: &str) -> Result<(), String> {
        let surface = UnixFocusSurface::from_ipc(surface)?;
        if surface == UnixFocusSurface::Composer && self.active.is_none() {
            return Err(format!(
                "focus surface is unavailable: {}",
                surface.as_str()
            ));
        }
        if surface == UnixFocusSurface::Settings
            && !self.settings_dialog.is_open()
            && !self.tab_editor_dialog.is_open()
        {
            return Err(format!(
                "focus surface is unavailable: {}",
                surface.as_str()
            ));
        }
        self.set_focus_surface_internal(surface, "semantic");
        Ok(())
    }

    fn settings_json(&self) -> String {
        serde_json::to_string_pretty(&serde_json::json!({
            "terminal_font_family": self.config.terminal_font_family,
            "terminal_font_size": self.config.terminal_font_size,
            "appearance_preset": self.config.appearance_preset.as_str(),
            "color_theme": self.config.appearance_preset.color_theme().as_str(),
            "tabs_visible": self.config.tabs_visible,
            "tabs_width": self.config.tabs_width,
            "resolved_font_family": resolved_font_name(),
            "config_path": config_path(),
            "recommended_cjk_font": "Sarasa Fixed SC",
            "recommended_font_license": "SIL Open Font License 1.1",
        }))
        .unwrap_or_default()
    }

    fn apply_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "terminal.font-family" if !value.trim().is_empty() => {
                self.config.terminal_font_family = value.to_owned();
            }
            "terminal.font-size" => {
                let Ok(size) = value.parse::<u16>() else {
                    return Err("font size must be a number from 8 to 36".to_owned());
                };
                if !(8..=36).contains(&size) {
                    return Err("font size must be from 8 to 36".to_owned());
                }
                self.config.terminal_font_size = size;
            }
            "terminal.font-family" => return Err("font family cannot be empty".to_owned()),
            other => return Err(format!("unknown setting: {other}")),
        }
        save_config(&self.config).map_err(|error| format!("{error:#}"))?;
        self.relayout_after_config_change();
        Ok(())
    }

    fn apply_set_composer(&mut self, position: usize, text: String) -> Result<(), String> {
        let id = self.tabs[position].id;
        if let Some(editing_id) = self.tab_editor_target_id() {
            if editing_id != id {
                return Err("set-composer target is not open in the inline tab editor".to_owned());
            }
            let normalized = text.replace("\r\n", "\n");
            let (name, note) = normalized.split_once('\n').unwrap_or((&normalized, ""));
            self.tab_editor_dialog.set_name_draft(name.to_owned());
            self.tab_editor_dialog.set_note_draft(note.to_owned());
            self.request_redraw();
            return Ok(());
        }
        // Same no-op rule as ControlHost default / Windows remote: tab switches
        // flush composer; unchanged text must not emit ComposerDraft + full
        // screen deltas that look like a workspace refresh.
        if self.tabs[position].composer == text {
            return Ok(());
        }
        self.tabs_mut()[position].composer = text.clone();
        self.event_journal_mut().commit(
            EventKind::ComposerDraft,
            Some(id),
            serde_json::json!({
                "length": text.chars().count(),
            }),
        );
        if self.active_id() == Some(id) {
            self.load_composer_to_ui();
        }
        Ok(())
    }

    fn config_tabs_visible(&self) -> bool {
        self.config.tabs_visible
    }

    fn set_tabs_visible(
        &mut self,
        visible: bool,
        cause: &str,
        operation_id: &str,
    ) -> Result<(), String> {
        if self.config.tabs_visible == visible {
            return Ok(());
        }
        self.invalidate_sidebar_text_click();
        if !visible && self.tab_editor_dialog.is_open() {
            self.complete_tab_editor(false)?;
        }
        self.config.tabs_visible = visible;
        save_config(&self.config).map_err(|error| format!("{error:#}"))?;
        self.event_journal_mut().commit(
            EventKind::LayoutTabsVisibility,
            None,
            serde_json::json!({
                "visible": visible,
                "cause": cause,
                "operation_id": operation_id,
            }),
        );
        if !visible && self.focus_surface == UnixFocusSurface::Sidebar {
            self.set_focus_surface_internal(UnixFocusSurface::Terminal, "tabs-hide");
        }
        self.relayout_after_config_change();
        Ok(())
    }

    fn set_tabs_width(
        &mut self,
        width: u16,
        cause: &str,
        operation_id: &str,
    ) -> Result<(), String> {
        self.config.tabs_width = width;
        save_config(&self.config).map_err(|error| format!("{error:#}"))?;
        let configured_width = self.config.tabs_width;
        let effective_width = self.layout().effective_tabs_width;
        self.event_journal_mut().commit(
            EventKind::LayoutTabsWidth,
            None,
            serde_json::json!({
                "configured_width": configured_width,
                "effective_width": effective_width,
                "cause": cause,
                "operation_id": operation_id,
            }),
        );
        self.relayout_after_config_change();
        Ok(())
    }

    fn toggle_tab_collapsed(&mut self, tab_id: u64) -> Result<(), String> {
        self.toggle_collapsed(tab_id)
    }

    fn open_settings_modal(&mut self) -> Result<(), String> {
        self.open_settings();
        Ok(())
    }

    fn close_settings_modal(&mut self, apply: bool) -> Result<(), String> {
        self.close_settings(apply)
    }

    fn preview_settings_preset(&mut self, preset: AppearancePreset) {
        if self.settings_dialog.preview_preset(preset) {
            self.request_redraw();
        }
    }

    fn open_instance_picker_modal(&mut self, mode: &str) -> Result<(), String> {
        let mode = InstancePickerMode::parse(mode).unwrap_or(InstancePickerMode::Attach);
        self.open_instance_picker(mode)
    }

    fn instance_picker_select(
        &mut self,
        target: crate::control_dispatch::InstancePickerTarget,
    ) -> Result<(), String> {
        use crate::control_dispatch::InstancePickerTarget;
        if !self.instance_picker_dialog.is_open() {
            return Err("instance picker is not open".to_owned());
        }
        match target {
            InstancePickerTarget::Next => self.instance_picker_dialog.select_next(),
            InstancePickerTarget::Prev => self.instance_picker_dialog.select_prev(),
            InstancePickerTarget::Name(name) => {
                self.instance_picker_dialog.select_by_instance(&name)?;
            }
            InstancePickerTarget::Pid(pid) => {
                self.instance_picker_dialog.select_by_pid(pid)?;
            }
        }
        self.request_redraw();
        Ok(())
    }

    fn instance_picker_confirm(&mut self) -> Result<(), String> {
        if !self.instance_picker_dialog.is_open() {
            return Err("instance picker is not open".to_owned());
        }
        self.confirm_instance_picker()
    }

    fn instance_picker_cancel(&mut self) -> Result<(), String> {
        if !self.instance_picker_dialog.is_open() {
            return Err("instance picker is not open".to_owned());
        }
        self.close_instance_picker();
        Ok(())
    }

    fn select_server_tab(&mut self, instance: &str) -> Result<(), String> {
        // Always re-read on an explicit select: the 2s cache otherwise makes a
        // click look dead right after a second server starts.
        self.server_tabs = collect_instance_picker_rows().unwrap_or_default();
        self.server_tabs_refresh_after = Instant::now() + SERVER_TABS_REFRESH;
        self.select_server_tab_by_instance(instance)?;
        self.request_redraw();
        Ok(())
    }

    fn switch_settings_scope(&mut self, scope: settings::SettingsScope) -> Result<(), String> {
        if !self.settings_dialog.is_open() {
            return Err("Settings is not open".to_owned());
        }
        // `switch_scope` reports `false` when the move is a no-op (already in
        // that scope, or no target terminal to override). Redraw only when it
        // actually changed something.
        if self.settings_dialog.switch_scope(scope)? {
            self.request_redraw();
        }
        Ok(())
    }

    fn toggle_settings_inheritance(
        &mut self,
        field: settings::AppearanceField,
    ) -> Result<(), String> {
        if !self.settings_dialog.is_open() {
            return Err("Settings is not open".to_owned());
        }
        if self.settings_dialog.toggle_inheritance(field)? {
            self.request_redraw();
        }
        Ok(())
    }

    fn reset_settings_overrides(&mut self) -> Result<(), String> {
        if !self.settings_dialog.is_open() {
            return Err("Settings is not open".to_owned());
        }
        if self.settings_dialog.reset_overrides() {
            self.request_redraw();
        }
        Ok(())
    }

    fn open_tab_editor(&mut self, tab_id: u64) -> Result<(), String> {
        self.open_tab_editor_for(tab_id)
    }

    fn finish_tab_editor(&mut self, save: bool) -> Result<(), String> {
        self.complete_tab_editor(save)
    }

    fn ui_action_cancel(&mut self) -> Result<bool, String> {
        match cancel_target(self.focus_gate()) {
            CancelTarget::WindowClose => {
                self.finish_window_close(WindowCloseChoice::Cancel);
                return Ok(true);
            }
            CancelTarget::LiveTabClose => {
                self.finish_close_confirmation(false);
                return Ok(true);
            }
            CancelTarget::Settings => {
                self.close_settings(false)?;
                return Ok(true);
            }
            CancelTarget::NewTerminal => {
                self.finish_new_terminal_dialog(false);
                return Ok(true);
            }
            CancelTarget::CwdEditor => {
                self.close_cwd_editor();
                return Ok(true);
            }
            CancelTarget::TabEditor => {
                self.complete_tab_editor(false)?;
                return Ok(true);
            }
            CancelTarget::InstancePicker => {
                self.close_instance_picker();
                return Ok(true);
            }
            CancelTarget::None => {}
        }
        if self.cancel_terminal_selection(true) {
            return Ok(true);
        }
        Ok(false)
    }

    fn ui_action_confirm(&mut self) -> Result<bool, String> {
        match confirm_target(self.focus_gate()) {
            ConfirmTarget::WindowClose => {
                self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                Ok(true)
            }
            ConfirmTarget::LiveTabClose => {
                self.finish_close_confirmation(true);
                Ok(true)
            }
            ConfirmTarget::InstancePicker => {
                self.confirm_instance_picker()?;
                Ok(true)
            }
            ConfirmTarget::None => Ok(false),
        }
    }

    fn close_tab_by_ui_action(&mut self, id: u64) -> Result<(), String> {
        if !self.tabs.iter().any(|tab| tab.id == id) {
            return Err(format!("can't find tab: @{id}"));
        }
        self.request_close_tab(id);
        Ok(())
    }

    fn copy_selection(&mut self) -> Result<(), String> {
        self.copy_terminal_selection()
    }

    fn started_at_unix_secs(&self) -> u64 {
        self.started_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default()
    }

    fn tabs(&self) -> &[TerminalTab] {
        &self.tabs
    }

    fn tabs_mut(&mut self) -> &mut Vec<TerminalTab> {
        &mut self.tabs
    }

    fn active_id(&self) -> Option<u64> {
        self.active
    }

    fn set_active_id(&mut self, id: Option<u64>) {
        self.active = id;
    }

    fn request_shutdown(&mut self) {
        self.close_requested = true;
    }

    fn set_session_name(&mut self, name: String) {
        self.session_name = name;
    }

    fn create_tab(
        &mut self,
        title: Option<String>,
        command_line: Vec<String>,
        tab_environment: Vec<(String, String)>,
        select: bool,
        parent_id: Option<u64>,
    ) -> Result<u32, String> {
        if let Some(parent_id) = parent_id
            && !self.tabs.iter().any(|tab| tab.id == parent_id)
        {
            return Err(format!("can't find parent tab: @{parent_id}"));
        }

        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let index = (0..)
            .find(|candidate| !self.tabs.iter().any(|tab| tab.index == *candidate))
            .unwrap_or(self.tabs.len() as u32);
        let (rows, cols) = self.initial_tab_size();
        let tab = TerminalTab::spawn(TerminalLaunch {
            id,
            index,
            parent_id,
            title,
            command_line,
            tab_environment,
            session_name: self.session_name.clone(),
            window: 0,
            wake_signal: Arc::clone(&self.wake_signal),
            initial_size: TerminalSize { rows, cols },
        })
        .map_err(|error| error.to_string())?;

        self.tabs.push(tab);
        self.tabs.sort_by_key(|tab| tab.index);
        self.event_journal_mut().commit(
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
            self.event_journal_mut().commit(
                EventKind::TabSelected,
                Some(id),
                serde_json::json!({}),
            );
            self.sync_grid_from_tab();
            self.request_ui_redraw();
        }
        Ok(index)
    }

    fn select_tab_at(&mut self, position: usize) -> Result<(), String> {
        if position >= self.tabs.len() {
            return Err("can't find window".to_owned());
        }
        let id = self.tabs[position].id;
        if self.active != Some(id) {
            self.invalidate_sidebar_text_click();
        }
        let _ = self.cancel_terminal_selection(true);
        if self.cwd_editor_dialog.is_open() {
            self.close_cwd_editor();
        }
        if self.tab_editor_dialog.is_open() {
            let _ = self.complete_tab_editor(false);
        }
        if self.focus_surface == UnixFocusSurface::Composer {
            self.sync_composer_buffer_to_tab();
        }
        self.active = Some(id);
        self.load_composer_buffer_from_tab();
        self.event_journal_mut()
            .commit(EventKind::TabSelected, Some(id), serde_json::json!({}));
        self.resize_active_tab_to_layout();
        self.sync_grid_from_tab();
        self.request_ui_redraw();
        Ok(())
    }

    fn close_tab_id(&mut self, id: u64) -> Result<bool, String> {
        let Some(position) = self.tabs.iter().position(|tab| tab.id == id) else {
            return Err(format!("can't find window: @{id}"));
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

        let terminal_shutdown_complete = self.tabs[position].close_process();
        self.tabs.remove(position);

        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(position)
                .or_else(|| {
                    position
                        .checked_sub(1)
                        .and_then(|index| self.tabs.get(index))
                })
                .map(|tab| tab.id);
            self.resize_active_tab_to_layout();
            self.sync_grid_from_tab();
            self.request_ui_redraw();
        }

        let active_id = self.active;
        self.event_journal_mut().commit(
            EventKind::TabClosed,
            Some(id),
            serde_json::json!({
                "index": index,
                "parent_id": parent_id,
                "exit_code": exit_code,
                "promoted_children": promoted_children,
                "active_id": active_id,
                "terminal_shutdown_complete": terminal_shutdown_complete,
            }),
        );

        Ok(terminal_shutdown_complete)
    }
}

impl UnixApp {
    fn handle_surface_navigation(&mut self, event: &NormalizedKeyEvent) -> bool {
        let direction = match &event.logical {
            Key::Named(NamedKey::ArrowUp) => FocusDirection::Up,
            Key::Named(NamedKey::ArrowDown) => FocusDirection::Down,
            Key::Named(NamedKey::ArrowLeft) => FocusDirection::Left,
            Key::Named(NamedKey::ArrowRight) => FocusDirection::Right,
            _ => return false,
        };
        let source = self.focus_state.surface();
        let state = FocusState::new(source, self.focus_gate());
        let Some(target) = state.navigate(
            direction,
            event.modifiers.control,
            event.modifiers.shift,
            event.modifiers.alt,
        ) else {
            return false;
        };
        let target = match target {
            FocusSurface::Terminal => UnixFocusSurface::Terminal,
            FocusSurface::Composer => UnixFocusSurface::Composer,
            FocusSurface::Sidebar => UnixFocusSurface::Sidebar,
        };
        self.set_focus_surface_internal(target, "keyboard-navigation");
        true
    }
    /// Drives a `ui-input` request through the ordinary window event path.
    ///
    /// Everything a human can do must be reachable from the CLI, and the only
    /// way to keep the two honest is to make them the *same* code. So this
    /// builds real `PixelWindowEvent`s and hands them to `handle_pixel_event`
    /// rather than calling `handle_content_click` (or any hit-test) directly:
    /// a synthetic gesture then cannot take a shortcut a human gesture lacks,
    /// and cannot rot when the real path changes.
    ///
    /// Multi-click is delivered as N press/release pairs in immediate
    /// succession, because that is literally what a double click is — the
    /// surfaces promote a repeat by consulting their own recent-click state.
    /// Seeding that state directly would be a second implementation of the
    /// promotion rule, and would drift from the one users exercise.
    fn apply_pointer_request(&mut self, request: PointerRequest) -> Result<(), String> {
        match request {
            PointerRequest::Pointer {
                x,
                y,
                button,
                action,
                count,
                modifiers,
            } => {
                let modifiers = modifier_state(modifiers);
                let button = match button {
                    PointerButtonKind::Left => PointerButton::Left,
                    PointerButtonKind::Right => PointerButton::Right,
                    PointerButtonKind::Middle => PointerButton::Middle,
                };
                let position = Some(LogicalPoint { x, y });
                match action {
                    PointerActionKind::Move => {
                        self.handle_pixel_event(PixelWindowEvent::PointerMoved {
                            position: LogicalPoint { x, y },
                            modifiers,
                        });
                    }
                    PointerActionKind::Press => {
                        // Move first so the surfaces see the pointer arrive,
                        // exactly as they would for a real cursor.
                        self.handle_pixel_event(PixelWindowEvent::PointerMoved {
                            position: LogicalPoint { x, y },
                            modifiers,
                        });
                        for click in 0..count {
                            self.handle_pixel_event(PixelWindowEvent::PointerButton {
                                button,
                                state: PointerButtonState::Pressed,
                                position,
                                modifiers,
                            });
                            // Release between repeats, but leave the button
                            // held after the final press so a caller can drag.
                            if click + 1 < count {
                                self.handle_pixel_event(PixelWindowEvent::PointerButton {
                                    button,
                                    state: PointerButtonState::Released,
                                    position,
                                    modifiers,
                                });
                            }
                        }
                    }
                    PointerActionKind::Release => {
                        self.handle_pixel_event(PixelWindowEvent::PointerButton {
                            button,
                            state: PointerButtonState::Released,
                            position,
                            modifiers,
                        });
                    }
                }
                Ok(())
            }
            PointerRequest::Wheel {
                x,
                y,
                delta_y,
                line_based,
                modifiers,
            } => {
                let delta = if line_based {
                    WheelDelta::Lines {
                        x: 0.0,
                        y: delta_y as f32,
                    }
                } else {
                    WheelDelta::LogicalPixels { x: 0.0, y: delta_y }
                };
                self.handle_pixel_event(PixelWindowEvent::MouseWheel {
                    delta,
                    position: Some(LogicalPoint { x, y }),
                    modifiers: modifier_state(modifiers),
                });
                Ok(())
            }
            PointerRequest::Key { key, modifiers } => {
                let modifiers = modifier_state(modifiers);
                let (logical, physical, text) = match &key {
                    KeyRequest::Text(text) => {
                        let physical = text
                            .chars()
                            .next()
                            .filter(char::is_ascii_alphabetic)
                            .map_or(PhysicalKeyCode::Other, PhysicalKeyCode::Letter);
                        (Key::Character(text.clone()), physical, Some(text.clone()))
                    }
                    KeyRequest::Enter => {
                        (Key::Named(NamedKey::Enter), PhysicalKeyCode::Enter, None)
                    }
                    KeyRequest::Backspace => (
                        Key::Named(NamedKey::Backspace),
                        PhysicalKeyCode::Backspace,
                        None,
                    ),
                    KeyRequest::Delete => {
                        (Key::Named(NamedKey::Delete), PhysicalKeyCode::Other, None)
                    }
                    KeyRequest::Tab => (Key::Named(NamedKey::Tab), PhysicalKeyCode::Tab, None),
                    KeyRequest::Escape => {
                        (Key::Named(NamedKey::Escape), PhysicalKeyCode::Other, None)
                    }
                    KeyRequest::ArrowLeft => (
                        Key::Named(NamedKey::ArrowLeft),
                        PhysicalKeyCode::Other,
                        None,
                    ),
                    KeyRequest::ArrowRight => (
                        Key::Named(NamedKey::ArrowRight),
                        PhysicalKeyCode::Other,
                        None,
                    ),
                    KeyRequest::ArrowUp => {
                        (Key::Named(NamedKey::ArrowUp), PhysicalKeyCode::Other, None)
                    }
                    KeyRequest::ArrowDown => (
                        Key::Named(NamedKey::ArrowDown),
                        PhysicalKeyCode::Other,
                        None,
                    ),
                    KeyRequest::Home => (Key::Named(NamedKey::Home), PhysicalKeyCode::Other, None),
                    KeyRequest::End => (Key::Named(NamedKey::End), PhysicalKeyCode::Other, None),
                };
                // Press and release, so surfaces that track key state see a
                // complete stroke rather than a key stuck down.
                for state in [KeyPressState::Pressed, KeyPressState::Released] {
                    self.handle_pixel_event(PixelWindowEvent::Keyboard(NormalizedKeyEvent {
                        logical: logical.clone(),
                        physical,
                        text: text.clone(),
                        state,
                        repeat: false,
                        modifiers,
                    }));
                }
                Ok(())
            }
        }
    }

    fn handle_pixel_event(&mut self, event: PixelWindowEvent) {
        match event {
            PixelWindowEvent::Wake => {
                let pty_changed = self.drain_wake_and_pty();
                let clipboard_changed = self.drain_terminal_clipboard_paste();
                if pty_changed || clipboard_changed {
                    self.request_output_redraw();
                }
            }
            PixelWindowEvent::Reopen => {
                if let Some(window) = self.window.clone() {
                    let was_visible = window.visible();
                    window.set_minimized(false);
                    window.set_visible(true);
                    window.focus();
                    if !was_visible {
                        self.event_journal_mut().commit(
                            EventKind::WindowVisibility,
                            None,
                            serde_json::json!({"visible": true, "reason": "dock-reopen"}),
                        );
                    }
                    self.request_redraw();
                }
            }
            PixelWindowEvent::CloseRequested => self.request_window_close(),
            PixelWindowEvent::GeometryChanged { change, metrics } => {
                self.handle_geometry_event(change, metrics);
            }
            PixelWindowEvent::FocusChanged(focused) => {
                self.window_focused = focused;
                // A modifier held (or released) while this window wasn't
                // focused never reaches us as an event, so pointer_modifiers
                // could otherwise stay stale until the next real keyboard or
                // pointer event corrects it - clear it here rather than risk
                // misclassifying the very next click after refocus.
                self.pointer_modifiers = agenterm_platform::input::ModifierState::empty();
                self.cursor_blink.reset(Instant::now());
                if let Some(window) = self.window.as_ref() {
                    self.window_state_tracker
                        .sync_from_native_flags(window.minimized(), window.maximized());
                    window.request_redraw();
                }
            }
            PixelWindowEvent::Ime(event) => self.handle_ime(event),
            PixelWindowEvent::Keyboard(event) => {
                self.pointer_modifiers = event.modifiers;
                if event.state != KeyPressState::Pressed {
                    return;
                }
                self.cursor_blink.reset(Instant::now());
                if self.window_close_dialog.is_open() {
                    if let Key::Named(NamedKey::Escape) = event.logical {
                        self.finish_window_close(WindowCloseChoice::Cancel);
                    } else if matches!(event.logical, Key::Named(NamedKey::Enter)) {
                        self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                    }
                    return;
                }
                if self.settings_dialog.is_open() {
                    self.handle_settings_key(&event);
                    return;
                }
                if self.new_terminal_dialog.is_open() {
                    self.handle_new_terminal_key(&event);
                    return;
                }
                if self.close_confirmation.is_open() {
                    if let Key::Named(NamedKey::Escape) = event.logical {
                        self.finish_close_confirmation(false);
                    }
                    return;
                }
                if self.tab_editor_dialog.is_open() {
                    self.handle_tab_editor_key(&event);
                    return;
                }
                if self.handle_surface_navigation(&event) {
                    return;
                }
                if self.focus_surface == UnixFocusSurface::Sidebar
                    && matches!(event.logical, Key::Named(NamedKey::F2))
                {
                    if let Some(tab_id) = self.active {
                        let _ = self.open_tab_editor_for(tab_id);
                    }
                    return;
                }
                if matches!(event.logical, Key::Named(NamedKey::Escape))
                    && self.cancel_terminal_selection(true)
                {
                    return;
                }
                if self.focus_surface == UnixFocusSurface::Composer {
                    // Some IMEs deliver the Enter that confirms a composed
                    // candidate as a regular keydown in addition to their own
                    // commit event, rather than exclusively through the IME
                    // channel. Without this guard that Enter also reaches the
                    // submit paths below, so composing text ("提交两次") ends
                    // in two submissions: one for the confirm keystroke, one
                    // for the user's actual follow-up Enter.
                    if matches!(event.logical, Key::Named(NamedKey::Enter))
                        && !self.ime_preedit.is_empty()
                    {
                        return;
                    }
                    if self.cwd_editor_dialog.is_open() {
                        if matches!(event.logical, Key::Named(NamedKey::Escape)) {
                            self.close_cwd_editor();
                            return;
                        }
                        if matches!(event.logical, Key::Named(NamedKey::Enter))
                            && let Some(mode) = CwdEditorDialog::submit_mode(event.modifiers)
                        {
                            let _ = self.prepare_cwd(None, None, mode);
                            return;
                        }
                    }
                    if !self.cwd_editor_dialog.is_open()
                        && let Some(bytes) = input::composer_passthrough_bytes(&event)
                    {
                        self.queue_pty_input(bytes);
                        return;
                    }
                    // Typing over a selection replaces it, the way every native
                    // text field behaves. The shared key path only knows a
                    // select-all flag, so the range deletion happens here where
                    // the cursor model lives; afterwards the buffer holds just
                    // the surviving text and the key action appends to it.
                    // A keystroke that replaces a selection is fully handled
                    // there, in place. Running the shared append-based path
                    // afterwards would duplicate the character (or, for
                    // Backspace, eat a second unselected one).
                    if self.take_composer_selection_for_edit(&event) {
                        return;
                    }
                    match input::composer_key_action(
                        &event,
                        &mut self.composer_buffer,
                        &mut self.composer_select_all,
                    ) {
                        input::ComposerKeyAction::Edited => {
                            // Editing still appends at the end of the draft,
                            // so the caret follows it; clamping also drops a
                            // selection the edit has just invalidated.
                            self.set_composer_cursor(TextCursor::at(
                                self.composer_buffer.chars().count(),
                            ));
                            self.sync_composer_buffer_to_tab();
                            self.request_redraw();
                        }
                        input::ComposerKeyAction::Submit => {
                            if self.send_active_composer().is_ok() {
                                self.set_focus_surface_internal(
                                    UnixFocusSurface::Terminal,
                                    "composer-submit",
                                );
                            }
                        }
                        input::ComposerKeyAction::Escape => {
                            if self.cwd_editor_dialog.is_open() {
                                self.close_cwd_editor();
                            } else {
                                self.sync_composer_buffer_to_tab();
                                self.set_focus_surface_internal(
                                    UnixFocusSurface::Terminal,
                                    "composer-escape",
                                );
                            }
                        }
                        // Copy and cut act on the selection when there is one,
                        // which is what a text field is expected to do; with
                        // only a caret they still fall back to the whole
                        // draft so the previous shortcut keeps working.
                        input::ComposerKeyAction::Copy => {
                            let (text, label) = match self.composer_selected_text() {
                                Some(selected) => (selected, "Copied selection"),
                                None => (self.composer_buffer.clone(), "Copied composer draft"),
                            };
                            match clipboard::set_clipboard_text(&text) {
                                Ok(()) => self.set_status_message(label),
                                Err(error) => {
                                    self.set_status_message(format!("Copy failed: {error}"))
                                }
                            }
                        }
                        input::ComposerKeyAction::Cut => {
                            let selected = self.composer_selected_text();
                            let text = selected
                                .clone()
                                .unwrap_or_else(|| self.composer_buffer.clone());
                            match clipboard::set_clipboard_text(&text) {
                                Ok(()) => {
                                    if selected.is_some() {
                                        let cursor = text_selection::delete_selection(
                                            &mut self.composer_buffer,
                                            self.composer_cursor,
                                        )
                                        .unwrap_or_default();
                                        self.set_composer_cursor(cursor);
                                        self.set_status_message("Cut selection");
                                    } else {
                                        self.composer_buffer.clear();
                                        self.set_composer_cursor(TextCursor::default());
                                        self.set_status_message("Cut composer draft");
                                    }
                                    self.sync_composer_buffer_to_tab();
                                    self.request_redraw();
                                }
                                Err(error) => {
                                    self.set_status_message(format!("Cut failed: {error}"))
                                }
                            }
                        }
                        input::ComposerKeyAction::Paste => {
                            let _ = self.paste_clipboard_into_composer();
                        }
                        input::ComposerKeyAction::SelectAll => {
                            self.composer_select_all = !self.composer_buffer.is_empty();
                            self.request_redraw();
                        }
                        input::ComposerKeyAction::Ignored => {}
                    }
                    return;
                }
                let has_selection = self.terminal_selection.is_some_and(|selection| {
                    !selection.is_empty() && self.active == Some(selection.tab_id)
                });
                match input::terminal_shortcut_action(
                    &event.logical,
                    event.modifiers,
                    has_selection,
                ) {
                    input::TerminalShortcutAction::Copy => {
                        if let Err(error) = self.copy_terminal_selection() {
                            self.set_status_message(format!("Copy failed: {error}"));
                            self.request_redraw();
                        }
                        return;
                    }
                    input::TerminalShortcutAction::Paste => {
                        if let Err(error) = self.request_terminal_clipboard_paste() {
                            self.set_status_message(format!("Paste failed: {error}"));
                            self.request_redraw();
                        }
                        return;
                    }
                    input::TerminalShortcutAction::Suppress => return,
                    input::TerminalShortcutAction::Forward => {}
                }
                if let Some(bytes) = input::key_event_to_bytes(&event) {
                    let _ = self.cancel_terminal_selection(true);
                    self.queue_pty_input(bytes);
                }
            }
            PixelWindowEvent::PointerMoved {
                position,
                modifiers,
            } => {
                self.pointer_modifiers = modifiers;
                let (x, y) = (position.x, position.y);
                self.last_cursor = (x, y);
                if self.tabs_resize_drag.is_some() {
                    self.drag_tabs_resize(x as i32);
                } else if self.sidebar_scroll_drag.is_some() {
                    self.drag_sidebar_scrollbar(y as i32);
                } else if self.scroll_drag.is_some() {
                    self.drag_scrollbar(y as i32);
                } else if self.composer_selection_dragging {
                    self.drag_composer_selection(x, y);
                } else if self
                    .terminal_selection_gesture
                    .as_ref()
                    .is_some_and(|gesture| gesture.active())
                {
                    self.drag_terminal_selection(x, y);
                } else if self.forward_terminal_mouse(x, y, None, true, true)
                    || self.mouse_report_button.is_some()
                {
                    // App mouse gesture owns the pointer until release.
                } else {
                    // no local selection drag without an active gesture
                }
            }
            PixelWindowEvent::MouseWheel {
                delta,
                position,
                modifiers,
            } => {
                self.pointer_modifiers = modifiers;
                let (x, y) = position
                    .map(|position| (position.x, position.y))
                    .unwrap_or(self.last_cursor);
                match delta {
                    WheelDelta::Lines { y: lines, .. } => {
                        self.mouse_wheel(x, y, f64::from(lines), true)
                    }
                    WheelDelta::LogicalPixels { y: pixels, .. } => {
                        self.mouse_wheel(x, y, pixels, false)
                    }
                    _ => {}
                }
            }
            PixelWindowEvent::PointerButton {
                state: PointerButtonState::Pressed,
                button: PointerButton::Left,
                position,
                modifiers,
            } => {
                self.pointer_modifiers = modifiers;
                self.cursor_blink.reset(Instant::now());
                let (x, y) = position
                    .map(|position| (position.x, position.y))
                    .unwrap_or(self.last_cursor);
                if x < f64::from(self.sidebar_width()) {
                    let _ = self.cancel_terminal_selection(true);
                    self.handle_sidebar_click(x, y);
                } else {
                    self.handle_content_click(x, y);
                }
            }
            PixelWindowEvent::PointerButton {
                state: PointerButtonState::Released,
                button: PointerButton::Left,
                modifiers,
                ..
            } => {
                self.pointer_modifiers = modifiers;
                if let Some(code) = self.mouse_report_button.take() {
                    let (x, y) = self.last_cursor;
                    let _ = self.forward_terminal_mouse(x, y, Some(code), false, false);
                    self.mouse_report_cell = None;
                } else if self.tabs_resize_drag.is_some() {
                    self.finish_tabs_resize(true, "mouse-drag", UI_TABS_SET_WIDTH);
                } else if self.scroll_drag.is_some() {
                    self.end_scroll_drag();
                } else if self.sidebar_scroll_drag.is_some() {
                    self.end_sidebar_scroll_drag();
                } else if self.composer_selection_dragging {
                    self.end_composer_selection();
                } else {
                    self.complete_terminal_selection();
                }
            }
            PixelWindowEvent::PointerButton {
                state,
                button: button @ (PointerButton::Right | PointerButton::Middle),
                position,
                modifiers,
            } => {
                self.pointer_modifiers = modifiers;
                let code = if button == PointerButton::Right { 2 } else { 1 };
                let (x, y) = position
                    .map(|position| (position.x, position.y))
                    .unwrap_or(self.last_cursor);
                match state {
                    PointerButtonState::Pressed => {
                        // Chrome wins over terminal mouse reporting: a
                        // right-click on the server strip must open its menu,
                        // not travel to the shell as an SGR report.
                        if button == PointerButton::Right
                            && self.handle_server_strip_secondary_click(x as i32, y as i32)
                        {
                            return;
                        }
                        if self.forward_terminal_mouse(x, y, Some(code), true, false) {
                            self.mouse_report_button = Some(code);
                        }
                    }
                    PointerButtonState::Released if self.mouse_report_button == Some(code) => {
                        self.mouse_report_button = None;
                        let _ = self.forward_terminal_mouse(x, y, Some(code), false, false);
                        self.mouse_report_cell = None;
                    }
                    _ => {}
                }
            }
            PixelWindowEvent::PointerLeft | PixelWindowEvent::PointerButton { .. } => {}
            _ => {}
        }
    }

    /// Coalesces PTY-output-driven redraws to at most ~30 presents per
    /// second. Interactive paths keep calling `request_redraw` directly, so
    /// input latency is unaffected; only streaming output is paced.
    fn request_output_redraw(&mut self) {
        const OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(33);
        let due = self
            .last_present
            .map(|at| at + OUTPUT_FRAME_INTERVAL)
            .unwrap_or_else(Instant::now);
        if Instant::now() >= due {
            self.output_redraw_pending = false;
            self.request_redraw();
        } else {
            self.output_redraw_pending = true;
        }
    }

    fn next_window_directive(&mut self, now: Instant) -> PixelWindowDirective {
        const OUTPUT_FRAME_INTERVAL: Duration = Duration::from_millis(33);
        let mut changed = self.drain_wake_and_pty();
        changed |= self.drain_terminal_clipboard_paste();
        if changed {
            self.output_redraw_pending = true;
            changed = false;
        }
        let cursor_active = self.window_focused
            && self.focus_surface == UnixFocusSurface::Terminal
            && !self.modal_surface_active()
            && self.grid.as_ref().is_some_and(TerminalGrid::cursor_visible)
            && self
                .active_position()
                .is_some_and(|position| self.tabs[position].cursor_appearance().blinking);
        if cursor_active {
            changed |= self.cursor_blink.tick(now);
        } else {
            changed |= self.cursor_blink.reset(now);
        }
        let mut wake_at = cursor_active.then(|| self.cursor_blink.next_toggle());
        if self
            .terminal_selection_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.active())
            && self.terminal_selection_autoscroll.is_some()
        {
            changed |= self.tick_terminal_selection_autoscroll();
            let autoscroll_at = now + Duration::from_millis(33);
            wake_at = Some(
                wake_at
                    .map(|deadline| deadline.min(autoscroll_at))
                    .unwrap_or(autoscroll_at),
            );
        }
        if changed && let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        if let Some(save_due) = self.autosave_workspace(now) {
            wake_at = Some(wake_at.map_or(save_due, |deadline| deadline.min(save_due)));
        }
        if self.output_redraw_pending {
            let due = self
                .last_present
                .map(|at| at + OUTPUT_FRAME_INTERVAL)
                .unwrap_or(now);
            if now >= due {
                self.output_redraw_pending = false;
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            } else {
                wake_at = Some(wake_at.map_or(due, |deadline| deadline.min(due)));
            }
        }
        if self.close_requested {
            PixelWindowDirective::Exit
        } else {
            wake_at
                .map(PixelWindowDirective::WaitUntil)
                .unwrap_or(PixelWindowDirective::Wait)
        }
    }
}

impl PixelWindowApplication for UnixApp {
    fn opened(&mut self, window: &PixelWindow) -> Result<PixelWindowDirective, PixelWindowError> {
        self.open_window(window)?;
        Ok(PixelWindowDirective::Continue)
    }

    fn event(
        &mut self,
        _window: &PixelWindow,
        event: PixelWindowEvent,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        self.handle_pixel_event(event);
        Ok(if self.close_requested {
            PixelWindowDirective::Exit
        } else {
            PixelWindowDirective::Continue
        })
    }

    fn render(
        &mut self,
        _window: &PixelWindow,
        frame: &mut XrgbPixelFrame<'_>,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        self.drain_wake_and_pty();
        self.render_window(frame);
        Ok(if self.close_requested {
            PixelWindowDirective::Exit
        } else {
            PixelWindowDirective::Continue
        })
    }

    fn about_to_wait(
        &mut self,
        _window: &PixelWindow,
        now: Instant,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        Ok(self.next_window_directive(now))
    }
}

/// Nearest-neighbour upscale of the logical frame. `skip` excludes a
/// destination rectangle that the persistent terminal layer overwrites
/// afterwards, so those pixels are never produced twice.
fn scale_frame_nearest(
    source: &[u32],
    source_width: u32,
    source_height: u32,
    destination: &mut [u32],
    destination_width: u32,
    destination_height: u32,
    skip: Option<(u32, u32, u32, u32)>,
) {
    if source_width == 0 || source_height == 0 || destination_width == 0 || destination_height == 0
    {
        return;
    }
    for y in 0..destination_height {
        let source_y =
            (u64::from(y) * u64::from(source_height) / u64::from(destination_height)) as u32;
        let skip_span = skip.filter(|(_, top, _, height)| y >= *top && y < top + height);
        let mut scale_span = |from: u32, to: u32| {
            for x in from..to {
                let source_x =
                    (u64::from(x) * u64::from(source_width) / u64::from(destination_width)) as u32;
                destination[(y * destination_width + x) as usize] =
                    source[(source_y * source_width + source_x) as usize];
            }
        };
        match skip_span {
            Some((left, _, width, _)) => {
                scale_span(0, left.min(destination_width));
                scale_span((left + width).min(destination_width), destination_width);
            }
            None => scale_span(0, destination_width),
        }
    }
}

/// Nearest-neighbour upscale of one destination rectangle only; used for the
/// scrollbar strip and layer fringe that live inside the skipped terminal
/// rect but are not covered by the terminal layer.
#[allow(clippy::too_many_arguments)]
fn scale_frame_region(
    source: &[u32],
    source_width: u32,
    source_height: u32,
    destination: &mut [u32],
    destination_width: u32,
    destination_height: u32,
    rect: (u32, u32, u32, u32),
) {
    if source_width == 0 || source_height == 0 || destination_width == 0 || destination_height == 0
    {
        return;
    }
    let (left, top, rect_width, rect_height) = rect;
    let right = (left + rect_width).min(destination_width);
    let bottom = (top + rect_height).min(destination_height);
    for y in top..bottom {
        let source_y =
            (u64::from(y) * u64::from(source_height) / u64::from(destination_height)) as u32;
        for x in left..right {
            let source_x =
                (u64::from(x) * u64::from(source_width) / u64::from(destination_width)) as u32;
            destination[(y * destination_width + x) as usize] =
                source[(source_y * source_width + source_x) as usize];
        }
    }
}

fn scale_rect_to_frame(
    rect: (u32, u32, u32, u32),
    logical_size: (u32, u32),
    frame_size: (u32, u32),
) -> (u32, u32, u32, u32) {
    let scale_axis = |value: u32, logical: u32, physical: u32| {
        if logical == 0 {
            0
        } else {
            (u64::from(value) * u64::from(physical) / u64::from(logical)) as u32
        }
    };
    (
        scale_axis(rect.0, logical_size.0, frame_size.0),
        scale_axis(rect.1, logical_size.1, frame_size.1),
        scale_axis(rect.2, logical_size.0, frame_size.0),
        scale_axis(rect.3, logical_size.1, frame_size.1),
    )
}

fn compact_cwd_for_status(path: &str, home_dir: Option<&Path>) -> String {
    let path = Path::new(path);
    if let Some(home_dir) = home_dir
        && let Ok(relative) = path.strip_prefix(home_dir)
    {
        return if relative.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            let relative = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            format!("~/{relative}")
        };
    }
    if path.has_root() {
        return path
            .file_name()
            .map(|name| format!(".../{}", name.to_string_lossy()))
            .unwrap_or_else(|| std::path::MAIN_SEPARATOR_STR.to_owned());
    }
    path.to_string_lossy().into_owned()
}

fn terminal_paste_target_is_current(
    request_tab_id: u64,
    active_tab_id: Option<u64>,
    focus_surface: UnixFocusSurface,
    window_focused: bool,
    modal_surface_active: bool,
) -> bool {
    active_tab_id == Some(request_tab_id)
        && focus_surface == UnixFocusSurface::Terminal
        && window_focused
        && !modal_surface_active
}

/// Delegates to the shared shape so the embedded GUI and the headless
/// projection can never publish different toolbar keys for the same layout.
fn workspace_toolbar_snapshot_json(toolbar: WorkspaceToolbarLayout) -> serde_json::Value {
    crate::ui_snapshot::workspace_toolbar_snapshot_json(toolbar)
}

#[cfg(test)]
mod system_menu_tests {
    use super::{
        GuiLaunchResult, RecentSidebarTextClick, RenderBuffers, TerminalPasteFailure,
        UNIX_GUI_LAUNCH_POLICY, UNIX_GUI_USAGE, UnixFocusSurface, compact_cwd_for_status,
        gui_help_result, parse_gui_launch_target, scale_frame_nearest, scale_rect_to_frame,
        shift_extend_anchor, terminal_paste_bytes, terminal_paste_target_is_current,
        workspace_toolbar_snapshot_json,
    };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use super::{ToolbarHit, platform_toolbar_action_id};
    use crate::frontend::selection::{TerminalPoint, TerminalSelection};
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    #[test]
    fn gui_launch_help_is_supported_by_frontend_contract() {
        assert!(matches!(
            gui_help_result(&["--help".to_owned()], UNIX_GUI_USAGE),
            Some(GuiLaunchResult::UsageHelpPrinted)
        ));
    }

    #[test]
    fn gui_launch_parser_accepts_each_selector_and_preserves_no_activate() {
        for option in ["--endpoint", "--address", "--instance"] {
            let value = match option {
                "--endpoint" => "tcp:127.0.0.1:48815",
                "--address" => "127.0.0.1:48815",
                "--instance" => "dev",
                _ => unreachable!(),
            };
            let options = parse_gui_launch_target(
                &[
                    "--no-activate".to_owned(),
                    option.to_owned(),
                    value.to_owned(),
                ],
                UNIX_GUI_LAUNCH_POLICY,
            )
            .unwrap();
            assert!(options.no_activate);
            assert_eq!(
                options.selectors.endpoint.as_deref(),
                (option == "--endpoint").then_some(value)
            );
            assert_eq!(
                options.selectors.address.as_deref(),
                (option == "--address").then_some(value)
            );
            assert_eq!(
                options.selectors.instance.as_deref(),
                (option == "--instance").then_some(value)
            );
        }
    }

    #[test]
    fn gui_launch_parser_rejects_selector_conflicts_duplicates_and_missing_values() {
        let invalid = [
            vec![
                "--endpoint".to_owned(),
                "tcp:127.0.0.1:48815".to_owned(),
                "--instance".to_owned(),
                "dev".to_owned(),
            ],
            vec![
                "--instance".to_owned(),
                "main".to_owned(),
                "--instance".to_owned(),
                "dev".to_owned(),
            ],
            vec!["--address".to_owned()],
            vec!["--endpoint".to_owned(), "--no-activate".to_owned()],
        ];
        for arguments in invalid {
            assert!(
                parse_gui_launch_target(&arguments, UNIX_GUI_LAUNCH_POLICY).is_err(),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn terminal_paste_failures_keep_stable_machine_classification() {
        let cases = [
            (
                TerminalPasteFailure::Busy,
                "terminal_paste_busy",
                "state",
                true,
            ),
            (
                TerminalPasteFailure::Clipboard(
                    crate::platform::contract::ui_clipboard::UiClipboardError::failed(
                        "clipboard_backend_error",
                        "unavailable",
                    ),
                ),
                "clipboard_backend_error",
                "availability",
                true,
            ),
            (
                TerminalPasteFailure::NormalizedTextTooLarge,
                "terminal_paste_failed",
                "resource",
                false,
            ),
            (
                TerminalPasteFailure::StaleTarget,
                "terminal_paste_failed",
                "precondition",
                true,
            ),
            (
                TerminalPasteFailure::TerminalRejected,
                "terminal_paste_failed",
                "transport",
                true,
            ),
            (
                TerminalPasteFailure::WorkerDisconnected,
                "terminal_paste_failed",
                "availability",
                true,
            ),
        ];

        for (failure, code, category, retryable) in cases {
            let feedback = failure.feedback_error();
            assert_eq!(feedback.code, code);
            assert_eq!(feedback.category, category);
            assert_eq!(feedback.retryable, retryable);
        }
    }

    #[test]
    fn visual_cwd_compacts_home_and_other_absolute_paths() {
        let root = Path::new(std::path::MAIN_SEPARATOR_STR);
        let home = root.join("users").join("example");
        let home_project = home.join("repos").join("agenterm");
        let temporary_project = root.join("var").join("tmp").join("agenterm-review");

        assert_eq!(
            compact_cwd_for_status(&home_project.to_string_lossy(), Some(&home)),
            "~/repos/agenterm"
        );
        assert_eq!(
            compact_cwd_for_status(&temporary_project.to_string_lossy(), Some(&home)),
            ".../agenterm-review"
        );
        assert_eq!(
            compact_cwd_for_status("workspace/subdir", Some(&home)),
            "workspace/subdir"
        );
    }

    #[test]
    fn nearest_scaling_expands_logical_pixels_to_retina_framebuffer() {
        let source = [1, 2, 3, 4];
        let mut destination = [0; 16];
        scale_frame_nearest(&source, 2, 2, &mut destination, 4, 4, None);
        assert_eq!(
            destination,
            [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn toolbar_hits_resolve_through_platform_action_ids() {
        use crate::frontend::action;
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::NewTab),
            action::NEW_TAB
        );
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::ToggleTabs),
            action::TOGGLE_TABS
        );
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::ControlCenter),
            action::OPEN_CONTROL_CENTER
        );
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::Settings),
            action::OPEN_SETTINGS
        );
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::ToggleLocale),
            action::TOGGLE_LOCALE
        );
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::FontDecrease),
            action::FONT_DECREASE
        );
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::FontIncrease),
            action::FONT_INCREASE
        );
    }

    #[test]
    fn toolbar_snapshot_matches_all_rendered_native_controls() {
        let layout = super::workspace_layout_for(960, 600, &crate::settings::AppConfig::default());
        let toolbar = layout
            .workspace_toolbar
            .expect("workspace toolbar should be visible");
        let snapshot = workspace_toolbar_snapshot_json(toolbar);

        assert_eq!(
            snapshot["bounds"],
            crate::ui_geometry::pixel_rect_json(toolbar.bounds)
        );
        for field in [
            "new",
            "tabs",
            "control_center",
            "settings",
            "locale",
            "font_decrease",
            "font_increase",
        ] {
            assert!(
                snapshot[field].is_object(),
                "missing toolbar field: {field}"
            );
        }
    }

    #[test]
    fn render_buffers_reuse_logical_allocation_and_capture_only_on_request() {
        let mut buffers = RenderBuffers::default();
        buffers.logical_frame(100, 60).fill(7);
        let capacity = buffers.logical.capacity();

        assert_eq!(buffers.logical_frame(80, 40).len(), 3_200);
        assert_eq!(buffers.logical.capacity(), capacity);
        assert_eq!(buffers.take_capture(), None);

        buffers.capture_if_requested(2, 1, &[1, 2]);
        assert_eq!(buffers.take_capture(), None);
        buffers.request_capture();
        buffers.capture_if_requested(2, 1, &[1, 2]);
        assert_eq!(buffers.take_capture(), Some((2, 1, vec![1, 2])));

        buffers.capture_if_requested(1, 1, &[3]);
        assert_eq!(buffers.take_capture(), None);
    }

    #[test]
    fn screenshot_clip_maps_logical_rect_to_retina_framebuffer() {
        assert_eq!(
            scale_rect_to_frame((250, 46, 710, 480), (960, 600), (1920, 1200)),
            (500, 92, 1420, 960)
        );
    }

    #[test]
    fn sidebar_double_click_candidate_requires_stable_tab_geometry_and_deadline() {
        let now = Instant::now();
        let click = RecentSidebarTextClick {
            tab_id: 7,
            at: now,
            geometry_generation: 11,
        };
        assert!(click.matches(7, 11, now + Duration::from_millis(499)));
        assert!(!click.matches(8, 11, now + Duration::from_millis(100)));
        assert!(!click.matches(7, 12, now + Duration::from_millis(100)));
        assert!(!click.matches(7, 11, now + Duration::from_millis(501)));
    }

    #[test]
    fn terminal_paste_completion_requires_the_original_active_terminal_focus() {
        assert!(terminal_paste_target_is_current(
            7,
            Some(7),
            UnixFocusSurface::Terminal,
            true,
            false
        ));
        assert!(!terminal_paste_target_is_current(
            7,
            Some(8),
            UnixFocusSurface::Terminal,
            true,
            false
        ));
        assert!(!terminal_paste_target_is_current(
            7,
            Some(7),
            UnixFocusSurface::Composer,
            true,
            false
        ));
        assert!(!terminal_paste_target_is_current(
            7,
            Some(7),
            UnixFocusSurface::Terminal,
            true,
            true
        ));
        assert!(!terminal_paste_target_is_current(
            7,
            Some(7),
            UnixFocusSurface::Terminal,
            false,
            false
        ));
    }

    #[test]
    fn terminal_paste_framing_matches_bracketed_mode() {
        assert_eq!(terminal_paste_bytes("a\rb", false), b"a\rb");
        assert_eq!(
            terminal_paste_bytes("a\rb", true),
            b"\x1b[200~a\rb\x1b[201~"
        );
    }

    fn selection(anchor: TerminalPoint, focus: TerminalPoint) -> TerminalSelection {
        TerminalSelection {
            tab_id: 1,
            anchor,
            focus,
            dragging: false,
            moved: true,
        }
    }

    #[test]
    fn shift_extend_anchor_keeps_the_far_endpoint_when_click_is_below_selection() {
        // Selection spans rows 1..3; a shift-click further down (row 5)
        // should keep the top of the selection (row 1) as the anchor so the
        // selection grows downward, xterm-style.
        let sel = selection(
            TerminalPoint { row: 1, col: 0 },
            TerminalPoint { row: 3, col: 4 },
        );
        let click = TerminalPoint { row: 5, col: 0 };
        assert_eq!(
            shift_extend_anchor(sel, click),
            TerminalPoint { row: 1, col: 0 }
        );
    }

    #[test]
    fn shift_extend_anchor_flips_when_click_is_above_selection() {
        // A shift-click above the existing selection is closer to its start,
        // so the anchor flips to the bottom endpoint (row 3) and the
        // selection now grows upward from there.
        let sel = selection(
            TerminalPoint { row: 1, col: 0 },
            TerminalPoint { row: 3, col: 4 },
        );
        let click = TerminalPoint { row: 0, col: 0 };
        assert_eq!(
            shift_extend_anchor(sel, click),
            TerminalPoint { row: 3, col: 4 }
        );
    }

    #[test]
    fn shift_extend_anchor_breaks_ties_on_same_row_by_column_distance() {
        // Click lands exactly between start and end rows... use same-row
        // selection so the tie-break falls to column distance.
        let sel = selection(
            TerminalPoint { row: 2, col: 2 },
            TerminalPoint { row: 2, col: 8 },
        );
        // Click closer to the end (col 7) than the start (col 2) keeps start
        // as the anchor.
        let click = TerminalPoint { row: 2, col: 7 };
        assert_eq!(
            shift_extend_anchor(sel, click),
            TerminalPoint { row: 2, col: 2 }
        );
    }
}
