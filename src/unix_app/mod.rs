mod clipboard;
mod cursor_blink;
mod font;
mod input;
mod layout;
mod new_terminal;
mod render;
mod screenshot;
mod window_state;

use std::{
    collections::HashSet,
    env,
    path::Path,
    rc::Rc,
    sync::{Arc, mpsc::Receiver},
    time::{Duration, Instant, SystemTime},
};

use softbuffer::{Context, Surface};
use unicode_width::UnicodeWidthStr;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Window, WindowAttributes, WindowId},
};

use crate::{
    client::no_activate_from_environment,
    commands::{option_value, screenshot_output_path},
    control_dispatch::{ControlHost, dispatch_shared_command, resolve_target_position},
    event_journal::{EventJournal, EventKind},
    gui_wake::{UnixWake, install_unix_wake},
    instances::{mark_intentional_shutdown, register_instance},
    ipc_transport::{IpcEnvelope, start_ipc_server},
    operations::{UI_TABS_SET_WIDTH, UI_TABS_SHOW},
    protocol::IpcResponse,
    pty::TerminalSize,
    settings::{
        AppConfig, MAX_TERMINAL_FONT_SIZE, MIN_TERMINAL_FONT_SIZE, config_path, load_config,
        save_config,
    },
    terminal_runtime::{TerminalLaunch, TerminalTab},
    terminal_selection::{
        AutoScrollDirection, AutoScrollStep, SelectionGesture, TerminalPoint, TerminalSelection,
        autoscroll_step, terminal_selection_text, visible_row_selection, word_selection,
    },
    theme::ThemeId,
    ui_clipboard::{normalize_composer_paste, normalize_terminal_paste},
    ui_geometry::{
        ScrollbarHit, TERMINAL_SCROLLBAR_WIDTH, TreeRowActionDensity, TreeRowMode, WHEEL_DELTA,
        WHEEL_ROWS_PER_NOTCH, pixel_rect_json, scrollback_for_thumb_top, scrollbar_hit_test,
        sidebar_row_capacity, sidebar_scrollbar_geometry, sidebar_scrollbar_track,
        sidebar_tree_row_geometry, tabs_width_from_drag, terminal_cell_at, wheel_delta_units,
    },
    ui_snapshot::{
        PROJECTION_EMBEDDED_GUI, TerminalSelectionSnapshotInput, archived_proxy_status_json,
        embedded_window_json, event_position_json, locale_json, schema_version_json,
        scrollbar_state_json, settings_json, system_menu_json, terminal_interaction_json,
        working_context_json,
    },
    wake_signal::WakeSignal,
    working_context::{CwdSource, ShellKind, cwd_command, validate_path},
    workspace::{SavedTab, SavedWorkspace, save_workspace, workspace_path},
};

use cursor_blink::CursorBlink;
use font::resolved_font_name;
use new_terminal::{NewTerminalDialog, ui_action_open};
use render::{
    COMPOSER_HEIGHT, ComposerView, ConfirmCloseHit, ConfirmCloseView, FrameContent, ImePreeditView,
    NewShellChoice as RenderShellChoice, NewTerminalFocusView, NewTerminalHit,
    NewTerminalModalView, STATUS_HEIGHT, SettingsHit, SettingsModalView, SidebarTabRow,
    StatusBarView, TabEditorFocusView, TabEditorView, TerminalCursorStyle, TerminalGrid,
    TerminalPaint, ToolbarHit, WindowCloseHit, WindowCloseView, WorkspaceToolbarView, cell_metrics,
    effective_palette, grid_dimensions_for_pixels, render_frame, render_terminal_grid_hidpi,
    scrollbar_view_from_geometry, sidebar_row_at_y,
};
use window_state::{
    WindowStateTracker, WindowUiActionResult, WinitWindowHandle, absorb_window_event,
    apply_ui_action, window_snapshot_json,
};

use layout::{
    scrollbar_geometry, sidebar_width_u32, terminal_pixel_rect, u32_rect, workspace_layout_for,
};

#[derive(Clone, Copy, Debug)]
struct ScrollDrag {
    thumb_grab_offset: i32,
}

#[derive(Clone, Copy, Debug)]
struct SidebarScrollDrag {
    thumb_grab_offset: i32,
}

#[derive(Clone, Copy, Debug)]
struct TerminalDoubleClick {
    tab_id: u64,
    point: TerminalPoint,
    expires_at: Instant,
}

#[derive(Default)]
struct RenderBuffers {
    logical: Vec<u32>,
    captured: Option<(u32, u32, Vec<u32>)>,
    capture_next: bool,
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
struct RecentTerminalClick {
    tab_id: u64,
    point: TerminalPoint,
    at: Instant,
}

#[derive(Clone, Copy, Debug)]
struct TabsResizeDrag {
    original_width: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComposerWriteMode {
    EmptyOnly,
    Append,
    Replace,
}

impl ComposerWriteMode {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("empty-only") {
            "empty" | "empty-only" => Ok(Self::EmptyOnly),
            "append" => Ok(Self::Append),
            "replace" => Ok(Self::Replace),
            other => Err(format!("unknown composer write mode: {other}")),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowCloseChoice {
    KeepServerRunning,
    StopServerAndExit,
    Cancel,
}

const DOUBLE_CLICK_MS: u64 = 500;

const APP_NAME: &str = "AgenTerm";
const INITIAL_WIDTH: u32 = 960;
const INITIAL_HEIGHT: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnixFocusSurface {
    Terminal,
    Composer,
    Sidebar,
    Settings,
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

    fn from_ipc(value: &str) -> Result<Self, String> {
        match value {
            "terminal" => Ok(Self::Terminal),
            "composer" => Ok(Self::Composer),
            "tabs" | "sidebar" => Ok(Self::Sidebar),
            "settings" => Ok(Self::Settings),
            other => Err(format!("unknown focus surface: {other}")),
        }
    }
}

enum SidebarTabAction {
    AddChild,
    Edit,
    Close,
    Save,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabEditorFocus {
    Name,
    Note,
}

impl TabEditorFocus {
    const fn snapshot_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Note => "note",
        }
    }
}

pub fn run_gui_entry() -> i32 {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if let Err(message) = validate_gui_arguments(&arguments) {
        eprintln!("AgenTerm GUI argument error: {message}");
        return 2;
    }

    let no_activate = arguments
        .iter()
        .any(|arg| matches!(arg.as_str(), "--no-activate" | "--not-foreground"))
        || no_activate_from_environment();

    if !display_available() {
        eprintln!(
            "AgenTerm GUI could not start: no graphical display was detected.\n\
             Set DISPLAY (X11) or WAYLAND_DISPLAY, or run from a desktop session."
        );
        return 1;
    }

    match run_gui(no_activate) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("AgenTerm GUI failed: {error:#}");
            1
        }
    }
}

fn validate_gui_arguments(arguments: &[String]) -> Result<(), String> {
    for argument in arguments {
        match argument.as_str() {
            "--no-activate" | "--not-foreground" => {}
            other if other.starts_with("--") => {
                return Err(format!("unknown option: {other}"));
            }
            other => {
                return Err(format!(
                    "unexpected positional argument: {other}\n\
                     The GUI launcher does not accept shell commands."
                ));
            }
        }
    }
    Ok(())
}

fn display_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(target_os = "linux")]
    {
        !crate::platform::linux::display_facts_from_env().headless
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        env::var_os("WAYLAND_DISPLAY").is_some() || env::var_os("DISPLAY").is_some()
    }
}

fn run_gui(no_activate: bool) -> anyhow::Result<()> {
    let title = format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
    let mut event_loop_builder = EventLoop::<UnixWake>::with_user_event();
    #[cfg(target_os = "macos")]
    crate::platform::macos::activation::configure_event_loop(&mut event_loop_builder, no_activate);
    let event_loop = event_loop_builder.build()?;
    let proxy = event_loop.create_proxy();
    install_unix_wake(proxy);
    let context = Context::new(event_loop.owned_display_handle())
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let wake_signal = Arc::new(WakeSignal::new());

    let ipc_receiver = start_ipc_server(0, Arc::clone(&wake_signal))?;
    let session_name = format!("agenterm-{}", std::process::id());
    let _instance = register_instance(&crate::ipc_address(), &workspace_path(), &session_name)?;

    let mut app = UnixApp::new(
        title,
        no_activate,
        context,
        wake_signal,
        ipc_receiver,
        session_name,
    );
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct UnixApp {
    title: String,
    no_activate: bool,
    context: Context<winit::event_loop::OwnedDisplayHandle>,
    wake_signal: Arc<WakeSignal>,
    ipc_receiver: Receiver<IpcEnvelope>,
    session_name: String,
    started_at: SystemTime,
    event_journal: EventJournal,
    window: Option<Rc<Window>>,
    surface: Option<Surface<winit::event_loop::OwnedDisplayHandle, Rc<Window>>>,
    grid: Option<TerminalGrid>,
    tabs: Vec<TerminalTab>,
    active: Option<u64>,
    next_tab_id: u64,
    close_requested: bool,
    last_cursor: (f64, f64),
    focus_surface: UnixFocusSurface,
    composer_buffer: String,
    composer_select_all: bool,
    text_field_select_all: bool,
    config: AppConfig,
    modifiers: ModifiersState,
    settings_open: bool,
    settings_theme_draft: ThemeId,
    settings_size_draft: u16,
    new_terminal_dialog: NewTerminalDialog,
    new_terminal_focus: NewTerminalFocusView,
    window_state_tracker: WindowStateTracker,
    collapsed_tabs: HashSet<u64>,
    note_edit_target: Option<u64>,
    tab_name_draft: String,
    tab_note_draft: String,
    tab_editor_focus: TabEditorFocus,
    wheel_remainder: i32,
    scroll_drag: Option<ScrollDrag>,
    sidebar_scroll_offset: usize,
    sidebar_scroll_drag: Option<SidebarScrollDrag>,
    terminal_selection: Option<TerminalSelection>,
    terminal_selection_gesture: Option<SelectionGesture>,
    terminal_selection_pointer: Option<(i32, i32)>,
    terminal_selection_autoscroll: Option<AutoScrollStep>,
    terminal_double_click: Option<TerminalDoubleClick>,
    recent_terminal_click: Option<RecentTerminalClick>,
    pending_close: Option<u64>,
    window_close_pending: bool,
    cwd_edit_target: Option<u64>,
    tabs_resize_drag: Option<TabsResizeDrag>,
    render_buffers: RenderBuffers,
    status_message: String,
    ime_preedit: String,
    ime_cursor: Option<(usize, usize)>,
    cursor_blink: CursorBlink,
    window_focused: bool,
}

impl UnixApp {
    fn new(
        title: String,
        no_activate: bool,
        context: Context<winit::event_loop::OwnedDisplayHandle>,
        wake_signal: Arc<WakeSignal>,
        ipc_receiver: Receiver<IpcEnvelope>,
        session_name: String,
    ) -> Self {
        let config = load_config();
        Self {
            title,
            no_activate,
            context,
            wake_signal,
            ipc_receiver,
            session_name,
            started_at: SystemTime::now(),
            event_journal: EventJournal::new(),
            window: None,
            surface: None,
            grid: None,
            tabs: Vec::new(),
            active: None,
            next_tab_id: 1,
            close_requested: false,
            last_cursor: (0.0, 0.0),
            focus_surface: UnixFocusSurface::Terminal,
            composer_buffer: String::new(),
            composer_select_all: false,
            text_field_select_all: false,
            settings_open: false,
            settings_theme_draft: config.color_theme,
            settings_size_draft: config.terminal_font_size,
            new_terminal_dialog: NewTerminalDialog::new(),
            new_terminal_focus: NewTerminalFocusView::InitialCommand,
            window_state_tracker: WindowStateTracker::new(),
            collapsed_tabs: HashSet::new(),
            note_edit_target: None,
            tab_name_draft: String::new(),
            tab_note_draft: String::new(),
            tab_editor_focus: TabEditorFocus::Name,
            wheel_remainder: 0,
            scroll_drag: None,
            sidebar_scroll_offset: 0,
            sidebar_scroll_drag: None,
            terminal_selection: None,
            terminal_selection_gesture: None,
            terminal_selection_pointer: None,
            terminal_selection_autoscroll: None,
            terminal_double_click: None,
            recent_terminal_click: None,
            pending_close: None,
            window_close_pending: false,
            cwd_edit_target: None,
            tabs_resize_drag: None,
            render_buffers: RenderBuffers::default(),
            status_message: String::from("Ready"),
            ime_preedit: String::new(),
            ime_cursor: None,
            cursor_blink: CursorBlink::new(Instant::now()),
            window_focused: {
                #[cfg(target_os = "linux")]
                {
                    crate::platform::linux::activation::initial_window_focused(no_activate)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    !no_activate
                }
            },
            config,
            modifiers: ModifiersState::empty(),
        }
    }

    fn palette(&self) -> &'static crate::theme::ThemePalette {
        let configured = self.active_terminal_appearance().color_theme;
        effective_palette(configured, self.settings_theme_draft, self.settings_open)
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
        if self.cwd_edit_target == Some(tab_id) {
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
        if self.cwd_edit_target.is_some() {
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
        if self.window_close_pending || self.pending_close.is_some() || self.settings_open {
            return;
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let raw = {
            use crate::platform::KeyClassification;
            #[cfg(target_os = "linux")]
            let classified = crate::platform::linux::input::classify_ime_commit(raw);
            #[cfg(target_os = "macos")]
            let classified = crate::platform::macos::input::classify_ime_commit(raw);
            match classified {
                KeyClassification::TextCommit(text) => text,
                _ => return,
            }
        };
        #[cfg(any(target_os = "linux", target_os = "macos"))]
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
        if self.note_edit_target.is_some() {
            let multiline = self.tab_editor_focus == TabEditorFocus::Note;
            let text = input::normalize_ime_commit(raw, multiline);
            let select_all = &mut self.text_field_select_all;
            let draft = match self.tab_editor_focus {
                TabEditorFocus::Name => &mut self.tab_name_draft,
                TabEditorFocus::Note => &mut self.tab_note_draft,
            };
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
            let _ = self.cancel_terminal_selection(true);
            self.queue_pty_input(text.into_bytes());
        }
    }

    fn handle_ime(&mut self, event: Ime) {
        self.cursor_blink.reset(Instant::now());
        #[cfg(target_os = "linux")]
        {
            use crate::platform::linux::ime::{LinuxImeAction, LinuxImeEvent, classify_ime_event};
            let linux_event = match event {
                Ime::Enabled => LinuxImeEvent::Enabled,
                Ime::Preedit(text, cursor) => LinuxImeEvent::Preedit { text, cursor },
                Ime::Commit(text) => LinuxImeEvent::Commit(text),
                Ime::Disabled => LinuxImeEvent::Disabled,
            };
            match classify_ime_event(linux_event, self.ime_anchor().is_some()) {
                LinuxImeAction::None => {}
                LinuxImeAction::UpdatePreedit { text, cursor } => {
                    self.ime_preedit = text;
                    self.ime_cursor = cursor;
                    self.request_redraw();
                }
                LinuxImeAction::ClearPreedit => {
                    self.clear_ime_preedit();
                    self.request_redraw();
                }
                LinuxImeAction::CommitText(text) => {
                    self.clear_ime_preedit();
                    self.commit_ime_text(&text);
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            use crate::platform::macos::ime::{MacosImeAction, MacosImeEvent, classify_ime_event};
            let macos_event = match event {
                Ime::Enabled => MacosImeEvent::Enabled,
                Ime::Preedit(text, cursor) => MacosImeEvent::Preedit { text, cursor },
                Ime::Commit(text) => MacosImeEvent::Commit(text),
                Ime::Disabled => MacosImeEvent::Disabled,
            };
            match classify_ime_event(macos_event, self.ime_anchor().is_some()) {
                MacosImeAction::None => {}
                MacosImeAction::UpdatePreedit { text, cursor } => {
                    self.ime_preedit = text;
                    self.ime_cursor = cursor;
                    self.request_redraw();
                }
                MacosImeAction::ClearPreedit => {
                    self.clear_ime_preedit();
                    self.request_redraw();
                }
                MacosImeAction::CommitText(text) => {
                    self.clear_ime_preedit();
                    self.commit_ime_text(&text);
                }
            }
        }
    }

    fn send_active_composer(&mut self) -> Result<(), String> {
        self.sync_composer_buffer_to_tab();
        if self.cwd_edit_target.is_some() {
            return self
                .prepare_cwd(None, None, ComposerWriteMode::EmptyOnly)
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

    fn composer_send_hit(&self, x: f64, y: f64) -> bool {
        let layout = self.layout();
        let sidebar_width = self.sidebar_width();
        let composer_top = layout.composer.top.max(0) as u32;
        let composer_width = self.client_size().0.saturating_sub(sidebar_width);
        const SEND_W: u32 = 72;
        let send_x = sidebar_width + composer_width.saturating_sub(SEND_W + 8);
        let send_y = composer_top + 7;
        let send_h = COMPOSER_HEIGHT - 14;
        x >= f64::from(send_x)
            && x < f64::from(send_x + SEND_W)
            && y >= f64::from(send_y)
            && y < f64::from(send_y + send_h)
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

    fn modal_surface_active(&self) -> bool {
        self.window_close_pending
            || self.pending_close.is_some()
            || self.settings_open
            || self.new_terminal_dialog.is_open()
            || self.cwd_edit_target.is_some()
            || self.note_edit_target.is_some()
    }

    fn render_shell_choice(&self) -> RenderShellChoice {
        match self.new_terminal_dialog.shell_choice() {
            new_terminal::NewShellChoice::Default => RenderShellChoice::Default,
            new_terminal::NewShellChoice::Primary => RenderShellChoice::Primary,
            new_terminal::NewShellChoice::Bash => RenderShellChoice::Bash,
        }
    }

    fn open_new_terminal_dialog(&mut self) {
        if self.settings_open {
            let _ = self.close_settings(false);
        }
        if self.note_edit_target.is_some() {
            let _ = self.complete_tab_editor(false);
        }
        if self.cwd_edit_target.is_some() {
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
                    .choose_shell(new_terminal::NewShellChoice::Bash);
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

    fn handle_new_terminal_key(&mut self, event: &winit::event::KeyEvent) {
        if !self.new_terminal_dialog.is_open() {
            return;
        }
        let multiline = self.new_terminal_focus == NewTerminalFocusView::InitialCommand;
        let modifiers = self.modifiers;
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
            input::text_field_key_action(event, modifiers, draft, multiline, select_all)
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

    fn handle_settings_key(&mut self, event: &winit::event::KeyEvent) {
        if !self.settings_open {
            return;
        }
        match event.logical_key {
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

    fn open_cwd_editor(&mut self, target: Option<&str>) -> Result<(), String> {
        if self.settings_open
            || self.pending_close.is_some()
            || self.window_close_pending
            || self.note_edit_target.is_some()
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
        self.cwd_edit_target = Some(id);
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
        let Some(id) = self.cwd_edit_target.take() else {
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
                self.cwd_edit_target
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
        if self.cwd_edit_target == Some(id) {
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
        if self.cwd_edit_target == Some(id) {
            self.close_cwd_editor();
        }
        self.request_redraw();
        Ok(())
    }

    fn request_window_close(&mut self) {
        if self.window_close_pending {
            return;
        }
        let _ = self.cancel_terminal_selection(true);
        if self.note_edit_target.is_some() {
            let _ = self.complete_tab_editor(false);
        }
        if self.settings_open {
            let _ = self.close_settings(false);
        }
        if self.pending_close.is_some() {
            self.finish_close_confirmation(false);
        }
        if self.cwd_edit_target.is_some() {
            self.close_cwd_editor();
        }
        self.sync_composer_buffer_to_tab();
        self.window_close_pending = true;
        self.request_redraw();
    }

    fn finish_window_close(&mut self, choice: WindowCloseChoice) {
        if !self.window_close_pending {
            return;
        }
        self.window_close_pending = false;
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
        if !self.window_close_pending {
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
        if self.cwd_edit_target.is_some() {
            self.close_cwd_editor();
        }
        if self.note_edit_target.is_some() {
            let _ = self.complete_tab_editor(false);
        }
        self.sync_composer_buffer_to_tab();
        self.settings_open = true;
        self.settings_theme_draft = self.config.color_theme;
        self.settings_size_draft = self.config.terminal_font_size;
        self.set_focus_surface_internal(UnixFocusSurface::Settings, "semantic");
    }

    fn close_settings(&mut self, apply: bool) -> Result<(), String> {
        if !self.settings_open {
            return Err("settings are not open".to_owned());
        }
        if apply {
            if !(8..=36).contains(&self.settings_size_draft) {
                return Err("font size must be from 8 to 36".to_owned());
            }
            self.config.terminal_font_size = self.settings_size_draft;
            self.config.color_theme = self.settings_theme_draft;
            save_config(&self.config).map_err(|error| format!("{error:#}"))?;
        } else {
            self.settings_theme_draft = self.config.color_theme;
            self.settings_size_draft = self.config.terminal_font_size;
        }
        self.settings_open = false;
        self.set_focus_surface_internal(UnixFocusSurface::Terminal, "settings-close");
        Ok(())
    }

    fn open_tab_editor_for(&mut self, tab_id: u64) -> Result<(), String> {
        if self.settings_open {
            let _ = self.close_settings(false);
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) else {
            return Err(format!("can't find tab: @{tab_id}"));
        };
        self.note_edit_target = Some(tab_id);
        self.tab_name_draft = tab.title.clone();
        self.tab_note_draft = tab.note.clone();
        self.tab_editor_focus = TabEditorFocus::Name;
        self.text_field_select_all = false;
        self.active = Some(tab_id);
        self.ensure_editing_tab_visible();
        self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "tab-editor");
        Ok(())
    }

    fn ensure_editing_tab_visible(&mut self) {
        let Some(tab_id) = self.note_edit_target else {
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
        let Some(tab_id) = self.note_edit_target else {
            return Err("tab editor is not open".to_owned());
        };
        if save {
            let name = self.tab_name_draft.trim();
            if name.is_empty() {
                return Err("Tab title cannot be empty".to_owned());
            }
            if name.len() > crate::ui_bridge::UI_TAB_TITLE_MAX_BYTES {
                return Err(format!(
                    "Tab title exceeds the {}-byte UI limit",
                    crate::ui_bridge::UI_TAB_TITLE_MAX_BYTES
                ));
            }
            let note = self.tab_note_draft.clone();
            if note.len() > crate::ui_bridge::UI_TAB_NOTE_MAX_BYTES {
                return Err(format!(
                    "Tab note exceeds the {}-byte UI limit",
                    crate::ui_bridge::UI_TAB_NOTE_MAX_BYTES
                ));
            }
            let Some(position) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                return Err(format!("can't find tab: @{tab_id}"));
            };
            let previous_name = self.tabs[position].title.clone();
            let previous_note = self.tabs[position].note.clone();
            let name = name.to_owned();
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
        self.note_edit_target = None;
        self.tab_name_draft.clear();
        self.tab_note_draft.clear();
        self.tab_editor_focus = TabEditorFocus::Name;
        self.text_field_select_all = false;
        self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "tab-editor-close");
        self.request_redraw();
        Ok(())
    }

    fn tab_editor_draft_mut(&mut self) -> Option<&mut String> {
        self.note_edit_target?;
        match self.tab_editor_focus {
            TabEditorFocus::Name => Some(&mut self.tab_name_draft),
            TabEditorFocus::Note => Some(&mut self.tab_note_draft),
        }
    }

    fn handle_tab_editor_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.note_edit_target.is_none() {
            return false;
        }
        let multiline = self.tab_editor_focus == TabEditorFocus::Note;
        let modifiers = self.modifiers;
        let action = {
            let select_all = &mut self.text_field_select_all;
            let draft = match self.tab_editor_focus {
                TabEditorFocus::Name => &mut self.tab_name_draft,
                TabEditorFocus::Note => &mut self.tab_note_draft,
            };
            input::text_field_key_action(event, modifiers, draft, multiline, select_all)
        };
        match action {
            input::TextFieldKeyAction::Edited => {
                self.request_redraw();
            }
            input::TextFieldKeyAction::NextField => {
                self.text_field_select_all = false;
                self.reset_ime_context();
                self.tab_editor_focus = TabEditorFocus::Note;
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
                    let text = match self.tab_editor_focus {
                        TabEditorFocus::Name => &mut self.tab_name_draft,
                        TabEditorFocus::Note => &mut self.tab_note_draft,
                    };
                    if self.note_edit_target.is_some() {
                        input::prepare_composer_edit(text, select_all);
                        text.push_str(&normalized);
                        self.set_status_message(format!(
                            "Pasted {} characters into tab editor",
                            normalized.len()
                        ));
                        self.request_redraw();
                    }
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
            SettingsHit::Dark => {
                self.settings_theme_draft = ThemeId::Dark;
                self.request_redraw();
            }
            SettingsHit::Light => {
                self.settings_theme_draft = ThemeId::Light;
                self.request_redraw();
            }
            SettingsHit::SizeDecrease => {
                self.settings_size_draft = self.settings_size_draft.saturating_sub(1).max(8);
                self.request_redraw();
            }
            SettingsHit::SizeIncrease => {
                self.settings_size_draft = (self.settings_size_draft + 1).min(36);
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

    fn sidebar_max_offset(&self) -> usize {
        self.sidebar_row_count()
            .saturating_sub(self.sidebar_row_capacity())
    }

    fn sidebar_offset(&self) -> usize {
        self.sidebar_scroll_offset.min(self.sidebar_max_offset())
    }

    fn sidebar_scrollbar_state(
        &self,
    ) -> Option<(crate::ui_geometry::TerminalScrollbarGeometry, usize, usize)> {
        if !self.config.tabs_visible {
            return None;
        }
        let layout = self.layout();
        let track = sidebar_scrollbar_track(layout.sidebar_tree);
        let maximum = self.sidebar_max_offset();
        let offset = self.sidebar_offset();
        let geometry = sidebar_scrollbar_geometry(
            track,
            offset,
            maximum,
            self.sidebar_row_capacity(),
            self.sidebar_row_count(),
        );
        Some((geometry, offset, maximum))
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
        let mode = if self.note_edit_target == Some(tab_id) {
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
            || self.note_edit_target.is_some()
            || self.new_terminal_dialog.is_open()
    }

    fn terminal_ready_for_system_menu(&self) -> bool {
        self.focus_surface == UnixFocusSurface::Terminal
            && !self.window_close_pending
            && !self.settings_open
            && !self.new_terminal_dialog.is_open()
            && self
                .active_position()
                .is_some_and(|position| self.tabs[position].exited.is_none())
    }

    fn system_menu_clipboard_state(&self) -> (bool, bool) {
        system_menu_clipboard_state_pure(
            self.is_edit_focus(),
            self.terminal_ready_for_system_menu(),
            self.terminal_selection
                .as_ref()
                .is_some_and(|selection| !selection.is_empty()),
            clipboard::clipboard_has_unicode_text(),
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
        let terminal_scrollbar = self.active_position().map(|position| {
            let visible_rows = usize::from(self.tabs[position].last_size.0);
            let (offset, maximum) = self.tabs[position].scrollback_bounds();
            let geometry = scrollbar_geometry(&layout, visible_rows, offset, maximum);
            scrollbar_state_json(&geometry, offset, maximum)
        });
        let journal_position = self.event_journal.position();
        let (copy_enabled, paste_enabled) = self.system_menu_clipboard_state();
        let sidebar_scrollbar = self
            .sidebar_scrollbar_state()
            .map(|(geometry, offset, maximum)| scrollbar_state_json(&geometry, offset, maximum));
        const COMPOSER_SEND_WIDTH: i32 = 72;
        let composer_send_left = layout.composer.right - COMPOSER_SEND_WIDTH - 8;
        let composer_input = crate::ui_geometry::PixelRect {
            left: layout.composer.left,
            top: layout.composer.top,
            right: composer_send_left,
            bottom: layout.composer.bottom,
        };
        let composer_visible = !self.modal_surface_active();
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
        let tab_editor = self.note_edit_target.map(|id| {
            serde_json::json!({
                "target": format!("@{id}"),
                "name_length": self.tab_name_draft.chars().count(),
                "note_length": self.tab_note_draft.chars().count(),
                "focus": self.tab_editor_focus.snapshot_str(),
            })
        });
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
                                "edit": action("edit-tab", "Edit", geometry.actions.primary),
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
            "client_pid": std::process::id(),
            "server_pid": std::process::id(),
            "event_position": event_position_json(&journal_position.epoch, journal_position.sequence),
            "session": self.session_name,
            "active_window_id": active.map(|id| format!("@{id}")),
            "tabs_visible": self.config.tabs_visible,
            "window": if let Some(window) = self.window.as_ref() {
                window_snapshot_json(
                    &WinitWindowHandle {
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
                "toolbar": layout.workspace_toolbar.map(|toolbar| serde_json::json!({
                    "bounds": pixel_rect_json(toolbar.bounds),
                    "new": pixel_rect_json(toolbar.new_tab),
                    "tabs": pixel_rect_json(toolbar.tabs),
                    "settings": pixel_rect_json(toolbar.settings),
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
                    "rows": terminal_rows,
                    "cols": terminal_cols,
                    "scrollbar": terminal_scrollbar,
                },
                "composer": {
                    "visible": composer_visible,
                    "input_visible": composer_visible,
                    "send_visible": composer_visible,
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
                    "proxy": archived_proxy_status_json(layout.status_segments.proxy),
                    "provider": "placeholder",
                },
            },
            "focus": {
                "surface": self.focus_surface.as_str(),
                "window_id": active.map(|id| format!("@{id}")),
            },
            "composer": {
                "draft_length": self.composer_buffer.chars().count(),
                "focused": self.focus_surface == UnixFocusSurface::Composer,
            },
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
            } else if let Some(id) = self.cwd_edit_target {
                Some(serde_json::json!({
                    "kind": "cwd-editor",
                    "target": format!("@{id}"),
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
            } else if self.new_terminal_dialog.is_open() {
                Some(self.new_terminal_dialog.snapshot_modal())
            } else if self.note_edit_target.is_some() {
                Some(serde_json::json!({"kind": "tab-editor"}))
            } else {
                self.pending_close.map(|id| {
                    serde_json::json!({
                        "kind": "confirm-close-live",
                        "window_id": format!("@{id}"),
                    })
                })
            },
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
                self.settings_open,
                Some(self.settings_theme_draft.as_str()),
                &crate::ipc_address(),
                self.active_position()
                    .map(|position| format!("@{}", self.tabs[position].id))
                    .as_deref(),
            ),
            "locale": locale_json(self.config.locale),
            "feedback": {
                "message": self.status_message,
                "error": serde_json::Value::Null,
            },
        }))
        .unwrap_or_else(|_| "{}".to_owned())
    }

    fn client_size(&self) -> (u32, u32) {
        self.window
            .as_ref()
            .map(|window| {
                #[cfg(target_os = "linux")]
                {
                    let physical = window.inner_size();
                    match crate::platform::linux::scale::LinuxWindowMetrics::from_physical(
                        physical.width,
                        physical.height,
                        window.scale_factor(),
                    ) {
                        Ok((metrics, _)) => {
                            (metrics.logical_width.max(1), metrics.logical_height.max(1))
                        }
                        Err(_) => {
                            // Typed failure already recorded by scale helpers; fall back
                            // to a safe 1×1 floor so layout does not claim fake extents.
                            (1, 1)
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let physical = window.inner_size();
                    match crate::platform::macos::scale::MacosWindowMetrics::from_physical(
                        physical.width,
                        physical.height,
                        window.scale_factor(),
                    ) {
                        Ok((metrics, _)) => {
                            (metrics.logical_width.max(1), metrics.logical_height.max(1))
                        }
                        Err(_) => (1, 1),
                    }
                }
                #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                {
                    let size = window.inner_size().to_logical::<u32>(window.scale_factor());
                    (size.width.max(1), size.height.max(1))
                }
            })
            .unwrap_or((INITIAL_WIDTH, INITIAL_HEIGHT))
    }

    /// Linux hot-path: resize / scale-factor changes go through `platform::linux::scale`
    /// (contract revision 1) before PTY/layout updates.
    #[cfg(target_os = "linux")]
    fn handle_linux_geometry_event(
        &mut self,
        event: crate::platform::linux::scale::LinuxGeometryEvent,
    ) {
        use crate::platform::linux::scale::{LinuxGeometryAction, classify_geometry_event};
        match classify_geometry_event(event) {
            Ok(LinuxGeometryAction::Apply(_metrics)) => {
                if let Some(window) = self.window.as_ref() {
                    absorb_window_event(
                        &WindowEvent::Resized(window.inner_size()),
                        window,
                        &mut self.window_state_tracker,
                    );
                }
                self.resize_to_window();
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            Ok(LinuxGeometryAction::Ignore) => {}
            Err(_error) => {
                // Typed CapabilityStatus::Failed — do not invent extents.
            }
        }
    }

    /// macOS hot-path: Retina resize / scale changes are validated by the
    /// platform adapter before PTY, layout, and rendering updates.
    #[cfg(target_os = "macos")]
    fn handle_macos_geometry_event(
        &mut self,
        event: crate::platform::macos::scale::MacosGeometryEvent,
    ) {
        use crate::platform::macos::scale::{MacosGeometryAction, classify_geometry_event};
        match classify_geometry_event(event) {
            Ok(MacosGeometryAction::Apply(_metrics)) => {
                if let Some(window) = self.window.as_ref() {
                    absorb_window_event(
                        &WindowEvent::Resized(window.inner_size()),
                        window,
                        &mut self.window_state_tracker,
                    );
                }
                self.resize_to_window();
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            Ok(MacosGeometryAction::Ignore) | Err(_) => {}
        }
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
        let mode = if self.note_edit_target == Some(row.id) {
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
                } else if geometry.actions.primary.contains(x, y) {
                    Some(SidebarTabAction::Edit)
                } else if geometry.actions.secondary.contains(x, y) {
                    Some(SidebarTabAction::Close)
                } else {
                    None
                }
            }
        }
    }

    fn sidebar_tab_editor_hit(&self, x: i32, y: i32) -> Option<TabEditorFocus> {
        self.note_edit_target?;
        let tree_height = self.layout().sidebar_tree.height().max(0) as u32;
        let row_index = sidebar_row_at_y(y as u32, tree_height)?;
        let source_index = self.sidebar_offset() + row_index;
        let visible_rows = self.visible_tree_rows();
        let row = visible_rows.get(source_index)?;
        if self.note_edit_target != Some(row.id) {
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

    fn handle_sidebar_click(&mut self, x: f64, y: f64) {
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
                SidebarTabAction::Edit => {
                    let Some(position) = self.tab_position_for_sidebar_y(y.max(0.0) as u32) else {
                        return;
                    };
                    let id = self.tabs[position].id;
                    let _ = self.open_tab_editor_for(id);
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
            if self.tab_editor_focus != focus {
                self.text_field_select_all = false;
                self.reset_ime_context();
            }
            self.tab_editor_focus = focus;
            self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "tab-editor-focus");
            return;
        }
        if self.note_edit_target.is_some() {
            let _ = self.complete_tab_editor(false);
        }
        let Some(position) = self.tab_position_for_sidebar_y(y.max(0.0) as u32) else {
            self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "mouse");
            return;
        };
        let _ = self.select_tab_at(position);
        self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "mouse");
    }

    fn handle_toolbar_hit(&mut self, hit: ToolbarHit) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            self.dispatch_toolbar_action(platform_toolbar_action_id(hit));
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            match hit {
                ToolbarHit::NewTab => {
                    self.open_new_terminal_dialog();
                }
                ToolbarHit::ToggleTabs => {
                    let visible = !self.config.tabs_visible;
                    let _ = self.set_tabs_visible(
                        visible,
                        "toolbar",
                        crate::operations::UI_TABS_TOGGLE,
                    );
                }
                ToolbarHit::Settings => {
                    self.open_settings();
                }
                ToolbarHit::ToggleLocale => self.toggle_locale(),
                ToolbarHit::FontDecrease => self.adjust_active_terminal_font(-1),
                ToolbarHit::FontIncrease => self.adjust_active_terminal_font(1),
            }
        }
        self.request_redraw();
    }

    /// Platform hot-path: toolbar hits resolve through stable adapter action ids
    /// before shared product handlers run.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn dispatch_toolbar_action(&mut self, action_id: &str) {
        use crate::platform::action;
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
        if self.note_edit_target.is_some() {
            let _ = self.complete_tab_editor(false);
        }
        if self.handle_status_click(x as i32, y as i32) {
            return;
        }
        if let Some(id) = self.pending_close {
            let (width, height) = self.client_size();
            let modal = ConfirmCloseView::for_client(width, height, id);
            match modal.hit_test(x, y) {
                Some(ConfirmCloseHit::Confirm) => self.finish_close_confirmation(true),
                Some(ConfirmCloseHit::Cancel) => self.finish_close_confirmation(false),
                None => {}
            }
            return;
        }
        if self.settings_open {
            let (width, height) = self.client_size();
            let modal = SettingsModalView::for_client(
                width,
                height,
                self.settings_size_draft,
                self.settings_theme_draft,
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
        if self.begin_terminal_selection(x, y) {
            return;
        }
        if self.composer_region_contains(x, y) {
            self.set_focus_surface_internal(UnixFocusSurface::Composer, "mouse");
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

        if self.terminal_double_click.is_some_and(|click| {
            click.tab_id == tab_id && click.point == point && now <= click.expires_at
        }) {
            self.terminal_double_click = None;
            self.recent_terminal_click = None;
            if let Some((start, end)) =
                visible_row_selection(self.tabs[position].parser.screen(), row)
            {
                let _ = self.set_completed_terminal_selection(tab_id, start, end, rows, cols);
            }
            self.set_focus_surface_internal(UnixFocusSurface::Terminal, "selection");
            self.request_redraw();
            return true;
        }

        if self.recent_terminal_click.is_some_and(|click| {
            click.tab_id == tab_id
                && click.point == point
                && now.duration_since(click.at) <= Duration::from_millis(DOUBLE_CLICK_MS)
        }) {
            self.recent_terminal_click = None;
            if let Some((start, end)) = word_selection(self.tabs[position].parser.screen(), point) {
                let _ = self.set_completed_terminal_selection(tab_id, start, end, rows, cols);
                self.terminal_double_click = now
                    .checked_add(Duration::from_millis(DOUBLE_CLICK_MS))
                    .map(|expires_at| TerminalDoubleClick {
                        tab_id,
                        point,
                        expires_at,
                    });
                self.set_focus_surface_internal(UnixFocusSurface::Terminal, "selection");
                self.request_redraw();
                return true;
            }
        }

        self.terminal_double_click = None;
        self.recent_terminal_click = Some(RecentTerminalClick {
            tab_id,
            point,
            at: now,
        });
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
        let Some(gesture) = self.terminal_selection_gesture else {
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
        let updated = gesture.drag_to(TerminalPoint { row, col }, rows, cols);
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
            self.terminal_double_click = None;
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
                let updated = gesture.drag_to(TerminalPoint { row, col }, rows, cols);
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
        self.set_status_message(format!("Copied {} characters", text.len()));
        Ok(())
    }

    fn paste_clipboard_into_terminal(&mut self) -> Result<(), String> {
        if self.settings_open
            || self.window_close_pending
            || self.note_edit_target.is_some()
            || self.cwd_edit_target.is_some()
        {
            return Err("paste is unavailable while a modal is open".to_owned());
        }
        if self.focus_surface != UnixFocusSurface::Terminal {
            return Err("paste requires terminal focus".to_owned());
        }
        let Some(position) = self.active_position() else {
            return Err("no active window".to_owned());
        };
        let tab_id = self.tabs[position].id;
        let raw = clipboard::get_clipboard_text()?;
        let text = normalize_terminal_paste(&raw);
        if text.is_empty() {
            return Err("clipboard text contains no pasteable characters".to_owned());
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
            return Err("terminal input was rejected".to_owned());
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
            }),
        );
        self.set_status_message(format!("Pasted {} characters into @{tab_id}", text.len()));
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
                self.scroll_drag = Some(ScrollDrag {
                    thumb_grab_offset: y - geometry.thumb.top,
                });
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
        let offset = scrollback_for_thumb_top(geometry, y - drag.thumb_grab_offset, maximum);
        if let Some(position) = self.active_position() {
            let offset = self.tabs[position].set_scrollback(offset);
            self.on_viewport_scrolled(position, offset, "scrollbar-drag");
        }
    }

    fn end_scroll_drag(&mut self) {
        self.scroll_drag = None;
    }

    fn scroll_sidebar(&mut self, wheel_delta_notches: i32) {
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
        if !geometry.track.contains(x, y) {
            return false;
        }
        if maximum == 0 {
            return true;
        }
        if geometry.thumb.contains(x, y) {
            self.sidebar_scroll_drag = Some(SidebarScrollDrag {
                thumb_grab_offset: y - geometry.thumb.top,
            });
        } else {
            let page = self.sidebar_row_capacity().max(1);
            self.sidebar_scroll_offset = if y < geometry.thumb.top {
                current.saturating_sub(page)
            } else {
                current.saturating_add(page).min(maximum)
            };
            self.request_redraw();
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
        self.request_redraw();
    }

    fn end_sidebar_scroll_drag(&mut self) {
        self.sidebar_scroll_drag = None;
    }

    fn mouse_wheel(&mut self, x: f64, y: f64, delta: MouseScrollDelta) {
        if self.settings_open || self.window_close_pending {
            return;
        }
        let layout = self.layout();
        let units = match delta {
            MouseScrollDelta::LineDelta(_, lines) => wheel_delta_units(f64::from(lines), true),
            MouseScrollDelta::PixelDelta(pos) => wheel_delta_units(pos.y, false),
        };
        if self.config.tabs_visible && layout.sidebar_tree.contains(x as i32, y as i32) {
            self.wheel_remainder += units;
            let notches = self.wheel_remainder / WHEEL_DELTA;
            self.wheel_remainder %= WHEEL_DELTA;
            if notches != 0 {
                self.scroll_sidebar(notches);
                self.request_redraw();
            }
            return;
        }
        let terminal = terminal_pixel_rect(&layout);
        if !terminal.contains(x as i32, y as i32) {
            return;
        }
        self.wheel_remainder += units;
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
            self.on_viewport_scrolled(position, after, "mouse-wheel");
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
        if self.window_close_pending || self.pending_close.is_some() || self.settings_open {
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
        if let Some(tab_id) = self.note_edit_target {
            let rows = self.sidebar_viewport_rows();
            let (viewport_position, row) =
                rows.iter().enumerate().find(|(_, row)| row.id == tab_id)?;
            let geometry = self.sidebar_row_geometry(viewport_position, row.depth, tab_id);
            let editors = geometry.editors?;
            let field = match self.tab_editor_focus {
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

    fn ensure_window(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        if self.window.is_some() {
            return Ok(());
        }

        let attributes = {
            let base = WindowAttributes::default()
                .with_title(self.title.clone())
                .with_inner_size(winit::dpi::LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT));
            #[cfg(target_os = "linux")]
            {
                crate::platform::linux::activation::configure_window_attributes(
                    base,
                    self.no_activate,
                )
            }
            #[cfg(not(target_os = "linux"))]
            {
                base.with_active(!self.no_activate)
            }
        };

        let window = Rc::new(event_loop.create_window(attributes)?);
        window.set_ime_allowed(true);
        let surface = Surface::new(&self.context, Rc::clone(&window))
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let (width, height) = self.client_size();
        let sidebar_width = self.sidebar_width();
        let (cell_width, cell_height) = self.cell_dimensions();
        let (cols, rows) = grid_dimensions_for_pixels(
            width,
            height,
            sidebar_width,
            COMPOSER_HEIGHT,
            STATUS_HEIGHT,
            cell_width,
            cell_height,
        );
        let grid = TerminalGrid::new(cols, rows, self.palette());

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
        })?;

        window.request_redraw();
        self.window = Some(window);
        self.surface = Some(surface);
        self.grid = Some(grid);
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
        self.event_journal_mut()
            .commit(EventKind::TabSelected, Some(id), serde_json::json!({}));
        Ok(())
    }

    fn resize_to_window(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let (width, height) = self.client_size();
        let sidebar_width = self.sidebar_width();
        let (cell_width, cell_height) = self.cell_dimensions();
        let (cols, rows) = grid_dimensions_for_pixels(
            width,
            height,
            sidebar_width,
            COMPOSER_HEIGHT,
            STATUS_HEIGHT,
            cell_width,
            cell_height,
        );
        if let Some(grid) = self.grid.as_mut() {
            grid.resize(cols, rows);
        }
        if let Some(position) = self.active_position() {
            self.tabs[position].resize(rows, cols);
        }
        self.window_state_tracker.sync_from_window(window);
        self.sync_grid_from_tab();
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
        save_workspace(&self.saved_workspace())?;
        self.event_journal
            .commit(EventKind::WorkspaceSaved, None, serde_json::json!({}));
        Ok(())
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
            None if command == Some("ui-action") => {
                let args = &envelope.request.args;
                let action = args.get(1).map(String::as_str).unwrap_or("");
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
                            if !self.window_close_pending {
                                Some(IpcResponse::failure(
                                    "no window-close confirmation is pending",
                                ))
                            } else {
                                self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                                None
                            }
                        }
                        "stop-server-and-exit" => {
                            if !self.window_close_pending {
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
                                Ok(()) => None,
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
                        other => {
                            if let Some(window) = self.window.as_ref() {
                                let handle = WinitWindowHandle {
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
                                Some(IpcResponse::failure(
                                    "window is not available for UI action",
                                ))
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
        while let Ok(envelope) = self.ipc_receiver.try_recv() {
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

    fn redraw(&mut self) {
        self.sync_grid_from_tab();
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let physical_size = window.inner_size();
        let (logical_width, logical_height) = self.client_size();
        if physical_size.width == 0 || physical_size.height == 0 {
            return;
        }
        let ime_anchor = self.ime_anchor();
        if let Some((x, y, w, h)) = ime_anchor {
            window.set_ime_cursor_area(
                LogicalPosition::new(x, y),
                LogicalSize::new(w.max(1), h.max(1)),
            );
        }

        let sidebar_rows = self.sidebar_viewport_rows();
        let palette = self.palette();
        let layout = self.layout();
        let sidebar_width = self.sidebar_width();
        let (cell_width, cell_height) = self.cell_dimensions();
        let content_height = layout.terminal.bottom.max(0) as u32;
        let cwd_label = self.active_cwd_status_text();
        let composer_label = if self.cwd_edit_target.is_some() {
            "CWD> "
        } else {
            ""
        };
        let scrollbar = self.active_position().map(|position| {
            let visible_rows = usize::from(self.tabs[position].last_size.0);
            let (offset, maximum) = self.tabs[position].scrollback_bounds();
            scrollbar_view_from_geometry(scrollbar_geometry(&layout, visible_rows, offset, maximum))
        });
        let sidebar_scrollbar = self
            .sidebar_scrollbar_state()
            .map(|geometry| scrollbar_view_from_geometry(geometry.0));
        let settings = self.settings_open.then(|| {
            SettingsModalView::for_client(
                logical_width,
                logical_height,
                self.settings_size_draft,
                self.settings_theme_draft,
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
        let workspace_toolbar = if modal_active {
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
        let composer_width = logical_width.saturating_sub(sidebar_width);
        const SEND_W: u32 = 72;
        let send_x = sidebar_width + composer_width.saturating_sub(SEND_W + 8);
        let composer_view = ComposerView {
            text: &self.composer_buffer,
            focused: self.focus_surface == UnixFocusSurface::Composer,
            selected_all: self.composer_select_all,
            top: composer_top,
            label: composer_label,
            send_button: (send_x, composer_top + 7, SEND_W, COMPOSER_HEIGHT - 14),
        };
        let status_view = StatusBarView {
            bounds: u32_rect(layout.status),
            cwd_bounds: u32_rect(layout.status_segments.cwd),
            provider_bounds: if layout.status_segments.provider.width() > 0 {
                Some(u32_rect(layout.status_segments.provider))
            } else {
                None
            },
            tabs_recovery: layout.status_segments.tabs_recovery.map(u32_rect),
            cwd_text: &cwd_label,
            provider_text: &self.status_message,
        };
        let terminal_selection = self
            .terminal_selection
            .filter(|selection| self.active == Some(selection.tab_id));
        let confirm_close = self
            .pending_close
            .map(|id| ConfirmCloseView::for_client(logical_width, logical_height, id));
        let window_close = self
            .window_close_pending
            .then(|| WindowCloseView::for_client(logical_width, logical_height));
        let resize_grip = layout.resize_grip.map(u32_rect);

        let tab_editor = self.note_edit_target.map(|_| TabEditorView {
            name_draft: self.tab_name_draft.clone(),
            note_draft: self.tab_note_draft.clone(),
            focus: match self.tab_editor_focus {
                TabEditorFocus::Name => TabEditorFocusView::Name,
                TabEditorFocus::Note => TabEditorFocusView::Note,
            },
            selected_all: self.text_field_select_all,
        });
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
        let Some(surface) = self.surface.as_mut() else {
            return;
        };

        if let (Some(width), Some(height)) = (
            std::num::NonZeroU32::new(physical_size.width),
            std::num::NonZeroU32::new(physical_size.height),
        ) {
            let _ = surface.resize(width, height);
        }

        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let width = buffer.width().get();
        let height = buffer.height().get();
        let hidpi_terminal_active =
            hidpi_terminal_visible && (width != logical_width || height != logical_height);
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
                editing_tab_id: self.note_edit_target,
                tab_editor,
                workspace_toolbar,
                terminal_top: layout.terminal.top.max(0) as u32,
                composer: composer_view,
                scrollbar,
                sidebar_scrollbar,
                settings,
                confirm_close,
                window_close,
                new_terminal,
                status: Some(status_view),
                ime_preedit,
                resize_grip,
            },
        );
        scale_frame_nearest(
            logical_pixels,
            logical_width,
            logical_height,
            &mut buffer,
            width,
            height,
        );
        if hidpi_terminal_active {
            render_terminal_grid_hidpi(
                &mut buffer,
                width,
                width,
                height,
                logical_width,
                logical_height,
                content_height,
                sidebar_width,
                layout.terminal.top.max(0) as u32,
                cell_width,
                cell_height,
                TerminalPaint {
                    grid,
                    selection: terminal_selection,
                    cursor_style,
                    cursor_shape: cursor_appearance.shape,
                },
                palette,
            );
        }
        self.render_buffers
            .capture_if_requested(width, height, &buffer);
        let _ = buffer.present();
    }

    fn request_close_tab(&mut self, id: u64) {
        let _ = self.cancel_terminal_selection(true);
        if self.cwd_edit_target == Some(id) {
            self.close_cwd_editor();
        }
        if self.note_edit_target.is_some() {
            let _ = self.complete_tab_editor(false);
        }
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        if tab.exited.is_some() {
            let _ = self.close_tab_id(id);
            return;
        }
        self.pending_close = Some(id);
        self.request_redraw();
    }

    fn finish_close_confirmation(&mut self, confirm: bool) {
        let pending = self.pending_close.take();
        if confirm && let Some(id) = pending {
            let _ = self.close_tab_id(id);
        }
        self.request_redraw();
    }

    fn save_screenshot(&mut self, args: &[String], pane_only: bool) -> Result<String, String> {
        self.cursor_blink.reset(Instant::now());
        self.render_buffers.request_capture();
        self.redraw();
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

/// Map a Unix toolbar hit through the active platform adapter action ids.
#[cfg(target_os = "linux")]
fn platform_toolbar_action_id(hit: ToolbarHit) -> &'static str {
    use crate::platform::linux::toolbar::LinuxToolbarHit;
    let linux_hit = match hit {
        ToolbarHit::NewTab => LinuxToolbarHit::NewTab,
        ToolbarHit::ToggleTabs => LinuxToolbarHit::ToggleTabs,
        ToolbarHit::Settings => LinuxToolbarHit::Settings,
        ToolbarHit::ToggleLocale => LinuxToolbarHit::ToggleLocale,
        ToolbarHit::FontDecrease => LinuxToolbarHit::FontDecrease,
        ToolbarHit::FontIncrease => LinuxToolbarHit::FontIncrease,
    };
    linux_hit.action_id()
}

#[cfg(target_os = "macos")]
fn platform_toolbar_action_id(hit: ToolbarHit) -> &'static str {
    use crate::platform::macos::toolbar::MacosToolbarHit;
    let macos_hit = match hit {
        ToolbarHit::NewTab => MacosToolbarHit::NewTab,
        ToolbarHit::ToggleTabs => MacosToolbarHit::ToggleTabs,
        ToolbarHit::Settings => MacosToolbarHit::Settings,
        ToolbarHit::ToggleLocale => MacosToolbarHit::ToggleLocale,
        ToolbarHit::FontDecrease => MacosToolbarHit::FontDecrease,
        ToolbarHit::FontIncrease => MacosToolbarHit::FontIncrease,
    };
    macos_hit.action_id()
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
        if self.cwd_edit_target.is_some() {
            self.prepare_cwd(None, None, ComposerWriteMode::EmptyOnly)?;
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
            && !self.settings_open
            && self.note_edit_target.is_none()
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
            "color_theme": self.config.color_theme,
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
        if let Some(editing_id) = self.note_edit_target {
            if editing_id != id {
                return Err("set-composer target is not open in the inline tab editor".to_owned());
            }
            let normalized = text.replace("\r\n", "\n");
            let (name, note) = normalized.split_once('\n').unwrap_or((&normalized, ""));
            self.tab_name_draft = name.to_owned();
            self.tab_note_draft = note.to_owned();
            self.request_redraw();
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

    fn preview_settings_theme(&mut self, theme: ThemeId) {
        if self.settings_open {
            self.settings_theme_draft = theme;
            self.request_redraw();
        }
    }

    fn open_tab_editor(&mut self, tab_id: u64) -> Result<(), String> {
        self.open_tab_editor_for(tab_id)
    }

    fn finish_tab_editor(&mut self, save: bool) -> Result<(), String> {
        self.complete_tab_editor(save)
    }

    fn ui_action_cancel(&mut self) -> Result<bool, String> {
        if self.window_close_pending {
            self.finish_window_close(WindowCloseChoice::Cancel);
            return Ok(true);
        }
        if self.pending_close.is_some() {
            self.finish_close_confirmation(false);
            return Ok(true);
        }
        if self.settings_open {
            self.close_settings(false)?;
            return Ok(true);
        }
        if self.new_terminal_dialog.is_open() {
            self.finish_new_terminal_dialog(false);
            return Ok(true);
        }
        if self.cwd_edit_target.is_some() {
            self.close_cwd_editor();
            return Ok(true);
        }
        if self.note_edit_target.is_some() {
            self.complete_tab_editor(false)?;
            return Ok(true);
        }
        if self.cancel_terminal_selection(true) {
            return Ok(true);
        }
        Ok(false)
    }

    fn ui_action_confirm(&mut self) -> Result<bool, String> {
        if self.window_close_pending {
            self.finish_window_close(WindowCloseChoice::KeepServerRunning);
            return Ok(true);
        }
        if self.pending_close.is_some() {
            self.finish_close_confirmation(true);
            return Ok(true);
        }
        Ok(false)
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
        let _ = self.cancel_terminal_selection(true);
        if self.cwd_edit_target.is_some() {
            self.close_cwd_editor();
        }
        if self.note_edit_target.is_some() {
            let _ = self.complete_tab_editor(false);
        }
        if self.focus_surface == UnixFocusSurface::Composer {
            self.sync_composer_buffer_to_tab();
        }
        let id = self.tabs[position].id;
        self.active = Some(id);
        self.load_composer_buffer_from_tab();
        self.event_journal_mut()
            .commit(EventKind::TabSelected, Some(id), serde_json::json!({}));
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
            self.active = self.tabs.first().map(|tab| tab.id);
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

impl ApplicationHandler<UnixWake> for UnixApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.ensure_window(event_loop) {
            eprintln!("AgenTerm GUI failed to create window: {error:#}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: UnixWake) {
        if self.drain_wake_and_pty()
            && let Some(window) = self.window.as_ref()
        {
            window.request_redraw();
        }
        if self.close_requested {
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => self.request_window_close(),
            WindowEvent::Resized(size) => {
                #[cfg(target_os = "linux")]
                {
                    self.handle_linux_geometry_event(
                        crate::platform::linux::scale::LinuxGeometryEvent::Resized {
                            physical_width: size.width,
                            physical_height: size.height,
                            scale_factor: self
                                .window
                                .as_ref()
                                .map(|window| window.scale_factor())
                                .unwrap_or(1.0),
                        },
                    );
                }
                #[cfg(target_os = "macos")]
                {
                    self.handle_macos_geometry_event(
                        crate::platform::macos::scale::MacosGeometryEvent::Resized {
                            physical_width: size.width,
                            physical_height: size.height,
                            scale_factor: self
                                .window
                                .as_ref()
                                .map(|window| window.scale_factor())
                                .unwrap_or(1.0),
                        },
                    );
                }
                #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                {
                    if let Some(window) = self.window.as_ref() {
                        absorb_window_event(
                            &WindowEvent::Resized(size),
                            window,
                            &mut self.window_state_tracker,
                        );
                    }
                    self.resize_to_window();
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                #[cfg(target_os = "linux")]
                {
                    let physical = self
                        .window
                        .as_ref()
                        .map(|window| window.inner_size())
                        .unwrap_or_default();
                    self.handle_linux_geometry_event(
                        crate::platform::linux::scale::LinuxGeometryEvent::ScaleFactorChanged {
                            scale_factor,
                            physical_width: physical.width,
                            physical_height: physical.height,
                        },
                    );
                }
                #[cfg(target_os = "macos")]
                {
                    let physical = self
                        .window
                        .as_ref()
                        .map(|window| window.inner_size())
                        .unwrap_or_default();
                    self.handle_macos_geometry_event(
                        crate::platform::macos::scale::MacosGeometryEvent::ScaleFactorChanged {
                            scale_factor,
                            physical_width: physical.width,
                            physical_height: physical.height,
                        },
                    );
                }
                #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                {
                    self.resize_to_window();
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                self.cursor_blink.reset(Instant::now());
                if let Some(window) = self.window.as_ref() {
                    absorb_window_event(
                        &WindowEvent::Focused(focused),
                        window,
                        &mut self.window_state_tracker,
                    );
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.drain_wake_and_pty();
                self.redraw();
                if self.close_requested {
                    event_loop.exit();
                }
            }
            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers.state();
            }
            WindowEvent::Ime(event) => self.handle_ime(event),
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {
                    return;
                }
                self.cursor_blink.reset(Instant::now());
                if self.window_close_pending {
                    if let Key::Named(NamedKey::Escape) = event.logical_key {
                        self.finish_window_close(WindowCloseChoice::Cancel);
                    } else if matches!(event.logical_key, Key::Named(NamedKey::Enter)) {
                        self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                    }
                    return;
                }
                if self.settings_open {
                    self.handle_settings_key(&event);
                    return;
                }
                if self.new_terminal_dialog.is_open() {
                    self.handle_new_terminal_key(&event);
                    return;
                }
                if self.pending_close.is_some() {
                    if let Key::Named(NamedKey::Escape) = event.logical_key {
                        self.finish_close_confirmation(false);
                    }
                    return;
                }
                if self.note_edit_target.is_some() {
                    self.handle_tab_editor_key(&event);
                    return;
                }
                if matches!(event.logical_key, Key::Named(NamedKey::Escape))
                    && self.cancel_terminal_selection(true)
                {
                    return;
                }
                if self.focus_surface == UnixFocusSurface::Composer {
                    if self.cwd_edit_target.is_some() {
                        if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                            self.close_cwd_editor();
                            return;
                        }
                        if input::primary_shortcut(self.modifiers)
                            && matches!(event.logical_key, Key::Named(NamedKey::Enter))
                        {
                            let mode = if self.modifiers.shift_key() {
                                ComposerWriteMode::Append
                            } else if self.modifiers.alt_key() {
                                ComposerWriteMode::Replace
                            } else {
                                ComposerWriteMode::EmptyOnly
                            };
                            let _ = self.prepare_cwd(None, None, mode);
                            return;
                        }
                    }
                    match input::composer_key_action(
                        &event,
                        self.modifiers,
                        &mut self.composer_buffer,
                        &mut self.composer_select_all,
                    ) {
                        input::ComposerKeyAction::Edited => {
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
                            if self.cwd_edit_target.is_some() {
                                self.close_cwd_editor();
                            } else {
                                self.sync_composer_buffer_to_tab();
                                self.set_focus_surface_internal(
                                    UnixFocusSurface::Terminal,
                                    "composer-escape",
                                );
                            }
                        }
                        input::ComposerKeyAction::Copy => {
                            if clipboard::set_clipboard_text(&self.composer_buffer).is_ok() {
                                self.set_status_message("Copied composer draft");
                            }
                        }
                        input::ComposerKeyAction::Cut => {
                            if clipboard::set_clipboard_text(&self.composer_buffer).is_ok() {
                                self.composer_buffer.clear();
                                self.composer_select_all = false;
                                self.sync_composer_buffer_to_tab();
                                self.set_status_message("Cut composer draft");
                                self.request_redraw();
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
                    &event.logical_key,
                    self.modifiers,
                    has_selection,
                ) {
                    input::TerminalShortcutAction::Copy => {
                        let _ = self.copy_terminal_selection();
                        return;
                    }
                    input::TerminalShortcutAction::Paste => {
                        let _ = self.paste_clipboard_into_terminal();
                        return;
                    }
                    input::TerminalShortcutAction::Suppress => return,
                    input::TerminalShortcutAction::Forward => {}
                }
                if let Some(bytes) = input::key_event_to_bytes(&event, self.modifiers) {
                    let _ = self.cancel_terminal_selection(true);
                    self.queue_pty_input(bytes);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = self
                    .window
                    .as_ref()
                    .map(|window| {
                        let logical = position.to_logical::<f64>(window.scale_factor());
                        (logical.x, logical.y)
                    })
                    .unwrap_or((position.x, position.y));
                self.last_cursor = (x, y);
                if self.tabs_resize_drag.is_some() {
                    self.drag_tabs_resize(x as i32);
                } else if self.sidebar_scroll_drag.is_some() {
                    self.drag_sidebar_scrollbar(y as i32);
                } else if self.scroll_drag.is_some() {
                    self.drag_scrollbar(y as i32);
                } else if self
                    .terminal_selection_gesture
                    .is_some_and(|gesture| gesture.active())
                {
                    self.drag_terminal_selection(x, y);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = self.last_cursor;
                self.mouse_wheel(x, y, delta);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.cursor_blink.reset(Instant::now());
                let (x, y) = self.last_cursor;
                if x < f64::from(self.sidebar_width()) {
                    let _ = self.cancel_terminal_selection(true);
                    self.handle_sidebar_click(x, y);
                } else {
                    self.handle_content_click(x, y);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if self.tabs_resize_drag.is_some() {
                    self.finish_tabs_resize(true, "mouse-drag", UI_TABS_SET_WIDTH);
                } else if self.scroll_drag.is_some() {
                    self.end_scroll_drag();
                } else if self.sidebar_scroll_drag.is_some() {
                    self.end_sidebar_scroll_drag();
                } else {
                    self.complete_terminal_selection();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let mut changed = self.drain_wake_and_pty();
        let now = Instant::now();
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
            .is_some_and(SelectionGesture::active)
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
        event_loop.set_control_flow(
            wake_at
                .map(ControlFlow::WaitUntil)
                .unwrap_or(ControlFlow::Wait),
        );
        if changed && let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        if self.close_requested {
            event_loop.exit();
        }
    }
}

fn scale_frame_nearest(
    source: &[u32],
    source_width: u32,
    source_height: u32,
    destination: &mut [u32],
    destination_width: u32,
    destination_height: u32,
) {
    if source_width == 0 || source_height == 0 || destination_width == 0 || destination_height == 0
    {
        return;
    }
    for y in 0..destination_height {
        let source_y =
            (u64::from(y) * u64::from(source_height) / u64::from(destination_height)) as u32;
        for x in 0..destination_width {
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
            format!("~/{}", relative.to_string_lossy())
        };
    }
    if path.is_absolute() {
        return path
            .file_name()
            .map(|name| format!(".../{}", name.to_string_lossy()))
            .unwrap_or_else(|| std::path::MAIN_SEPARATOR_STR.to_owned());
    }
    path.to_string_lossy().into_owned()
}

fn system_menu_clipboard_state_pure(
    edit_focus: bool,
    terminal_ready: bool,
    selection_nonempty: bool,
    clipboard_has_text: bool,
) -> (bool, bool) {
    if edit_focus {
        return (true, clipboard_has_text);
    }
    (
        terminal_ready && selection_nonempty,
        terminal_ready && clipboard_has_text,
    )
}

#[cfg(test)]
mod system_menu_tests {
    use super::{
        RenderBuffers, compact_cwd_for_status, scale_frame_nearest, scale_rect_to_frame,
        system_menu_clipboard_state_pure,
    };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use super::{ToolbarHit, platform_toolbar_action_id};
    use std::path::Path;

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
        scale_frame_nearest(&source, 2, 2, &mut destination, 4, 4);
        assert_eq!(
            destination,
            [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn toolbar_hits_resolve_through_platform_action_ids() {
        use crate::platform::action;
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::NewTab),
            action::NEW_TAB
        );
        assert_eq!(
            platform_toolbar_action_id(ToolbarHit::ToggleTabs),
            action::TOGGLE_TABS
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
    fn edit_focus_enables_copy_and_paste_follows_clipboard() {
        assert_eq!(
            system_menu_clipboard_state_pure(true, false, false, false),
            (true, false)
        );
        assert_eq!(
            system_menu_clipboard_state_pure(true, false, false, true),
            (true, true)
        );
    }

    #[test]
    fn terminal_ready_requires_selection_for_copy_and_clipboard_for_paste() {
        assert_eq!(
            system_menu_clipboard_state_pure(false, true, false, false),
            (false, false)
        );
        assert_eq!(
            system_menu_clipboard_state_pure(false, true, true, false),
            (true, false)
        );
        assert_eq!(
            system_menu_clipboard_state_pure(false, true, false, true),
            (false, true)
        );
        assert_eq!(
            system_menu_clipboard_state_pure(false, true, true, true),
            (true, true)
        );
    }

    #[test]
    fn terminal_not_ready_disables_copy_and_paste() {
        assert_eq!(
            system_menu_clipboard_state_pure(false, false, true, true),
            (false, false)
        );
    }
}
