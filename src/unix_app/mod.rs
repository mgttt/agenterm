mod clipboard;
mod font;
mod input;
mod layout;
mod render;
mod screenshot;

use std::{
    collections::HashSet,
    env,
    rc::Rc,
    sync::{Arc, mpsc::Receiver},
    time::{Duration, Instant, SystemTime},
};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
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
    instances::register_instance,
    ipc_transport::{IpcEnvelope, start_ipc_server},
    operations::{UI_TABS_SET_WIDTH, UI_TABS_SHOW},
    protocol::IpcResponse,
    pty::TerminalSize,
    settings::{AppConfig, config_path, load_config, save_config},
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
        sidebar_tree_row_geometry, tabs_width_from_drag, terminal_cell_at,
        wheel_delta_units,
    },
    ui_snapshot::{
        PROJECTION_EMBEDDED_GUI, TerminalSelectionSnapshotInput, archived_proxy_status_json,
        embedded_window_json, event_position_json, locale_json, schema_version_json,
        scrollbar_state_json, settings_json, system_menu_json, terminal_interaction_json,
        working_context_json,
    },
    wake_signal::WakeSignal,
    working_context::{CwdSource, ShellKind, cwd_command, validate_path},
    workspace::workspace_path,
};

use render::{
    COMPOSER_HEIGHT, ComposerView, ConfirmCloseHit, ConfirmCloseView, FrameContent,
    RESOLVED_UNIX_FONT, STATUS_HEIGHT, SettingsHit, SettingsModalView, SidebarTabRow,
    StatusBarView, TerminalGrid, TerminalPaint, ToolbarHit, WindowCloseHit, WindowCloseView,
    WorkspaceToolbarView, cell_metrics, effective_palette, grid_dimensions_for_pixels,
    render_frame, scrollbar_view_from_geometry, sidebar_row_at_y,
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
    #[cfg(not(target_os = "macos"))]
    {
        env::var_os("WAYLAND_DISPLAY").is_some() || env::var_os("DISPLAY").is_some()
    }
}

fn run_gui(no_activate: bool) -> anyhow::Result<()> {
    let title = format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
    let event_loop = EventLoop::<UnixWake>::with_user_event().build()?;
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
    config: AppConfig,
    modifiers: ModifiersState,
    settings_open: bool,
    settings_theme_draft: ThemeId,
    settings_font_draft: String,
    settings_size_draft: u16,
    collapsed_tabs: HashSet<u64>,
    note_edit_target: Option<u64>,
    tab_name_draft: String,
    tab_note_draft: String,
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
    last_frame: Option<(u32, u32, Vec<u32>)>,
    status_message: String,
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
            settings_open: false,
            settings_theme_draft: config.color_theme,
            settings_font_draft: config.terminal_font_family.clone(),
            settings_size_draft: config.terminal_font_size,
            collapsed_tabs: HashSet::new(),
            note_edit_target: None,
            tab_name_draft: String::new(),
            tab_note_draft: String::new(),
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
            last_frame: None,
            status_message: String::from("Ready"),
            config,
            modifiers: ModifiersState::empty(),
        }
    }

    fn palette(&self) -> &'static crate::theme::ThemePalette {
        effective_palette(
            self.config.color_theme,
            self.settings_theme_draft,
            self.settings_open,
        )
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
        if self.note_edit_target == Some(tab_id) {
            let normalized = self.composer_buffer.replace("\r\n", "\n");
            let (name, note) = normalized.split_once('\n').unwrap_or((&normalized, ""));
            self.tab_name_draft = name.to_owned();
            self.tab_note_draft = note.to_owned();
            return;
        }
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
        }
        self.focus_surface = surface;
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

    fn send_active_composer(&mut self) -> Result<(), String> {
        self.sync_composer_buffer_to_tab();
        if self.cwd_edit_target.is_some() {
            return self
                .prepare_cwd(None, None, ComposerWriteMode::EmptyOnly)
                .map_err(|error| error.to_string());
        }
        if self.note_edit_target.is_some() {
            return self.complete_tab_editor(true);
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
            || self.cwd_edit_target.is_some()
            || self.note_edit_target.is_some()
    }

    fn target_position(&self, target: Option<&str>) -> Option<usize> {
        resolve_target_position(&self.tabs, self.active, target)
    }

    fn active_cwd_status_text(&self) -> String {
        self.active_position()
            .and_then(|position| self.tabs.get(position))
            .map(|tab| {
                let path = tab.cwd.path().unwrap_or("unknown");
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
        self.settings_font_draft = self.config.terminal_font_family.clone();
        self.settings_size_draft = self.config.terminal_font_size;
        self.set_focus_surface_internal(UnixFocusSurface::Settings, "semantic");
    }

    fn close_settings(&mut self, apply: bool) -> Result<(), String> {
        if !self.settings_open {
            return Err("settings are not open".to_owned());
        }
        if apply {
            if self.settings_font_draft.trim().is_empty() {
                return Err("font family cannot be empty".to_owned());
            }
            if !(8..=36).contains(&self.settings_size_draft) {
                return Err("font size must be from 8 to 36".to_owned());
            }
            self.config.terminal_font_family = self.settings_font_draft.clone();
            self.config.terminal_font_size = self.settings_size_draft;
            self.config.color_theme = self.settings_theme_draft;
            save_config(&self.config).map_err(|error| format!("{error:#}"))?;
        } else {
            self.settings_theme_draft = self.config.color_theme;
            self.settings_font_draft = self.config.terminal_font_family.clone();
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
        self.composer_buffer = if tab.note.is_empty() {
            tab.title.clone()
        } else {
            format!("{}\n{}", tab.title, tab.note)
        };
        self.set_focus_surface_internal(UnixFocusSurface::Composer, "tab-editor");
        Ok(())
    }

    fn complete_tab_editor(&mut self, save: bool) -> Result<(), String> {
        let Some(tab_id) = self.note_edit_target.take() else {
            return Err("tab editor is not open".to_owned());
        };
        if save {
            let Some(position) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                return Err(format!("can't find tab: @{tab_id}"));
            };
            let previous_name = self.tabs[position].title.clone();
            let previous_note = self.tabs[position].note.clone();
            let name = self.tab_name_draft.clone();
            let note = self.tab_note_draft.clone();
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
        self.tab_name_draft.clear();
        self.tab_note_draft.clear();
        self.set_focus_surface_internal(UnixFocusSurface::Terminal, "tab-editor-close");
        self.request_redraw();
        Ok(())
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

    fn system_menu_clipboard_state(&self) -> (bool, bool) {
        let modal_free = !self.modal_surface_active();
        let terminal_alive = self
            .active_position()
            .is_some_and(|position| self.tabs[position].exited.is_none());
        let terminal_ready = modal_free && terminal_alive;
        let copy = terminal_ready
            && self
                .terminal_selection
                .as_ref()
                .is_some_and(|selection| selection.bounds().0 != selection.bounds().1);
        let paste = terminal_ready
            && matches!(
                self.focus_surface,
                UnixFocusSurface::Terminal | UnixFocusSurface::Composer
            );
        (copy, paste)
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
        let gesture_phase = self
            .terminal_selection_gesture
            .map(|gesture| gesture.phase().as_str());
        let tab_editor = self.note_edit_target.map(|id| {
            serde_json::json!({
                "target": format!("@{id}"),
                "name_length": self.tab_name_draft.chars().count(),
                "note_length": self.tab_note_draft.chars().count(),
                "focus": serde_json::Value::Null,
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
            "window": embedded_window_json(self.title.as_str(), client_width, client_height),
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
                gesture_phase,
            ),
            "settings": settings_json(
                &self.config,
                self.settings_open,
                Some(self.settings_theme_draft.as_str()),
            ),
            "locale": locale_json(),
            "feedback": {
                "message": self.status_message,
                "error": serde_json::Value::Null,
            },
            "selection": self.terminal_selection.map(|selection| {
                let (start, end) = selection.bounds();
                serde_json::json!({
                    "tab_id": format!("@{}", selection.tab_id),
                    "start": {"row": start.row, "col": start.col},
                    "end": {"row": end.row, "col": end.col},
                    "dragging": selection.dragging,
                })
            }),
        }))
        .unwrap_or_else(|_| "{}".to_owned())
    }

    fn client_size(&self) -> (u32, u32) {
        self.window
            .as_ref()
            .map(|window| window.inner_size())
            .map(|size| (size.width, size.height))
            .unwrap_or((INITIAL_WIDTH, INITIAL_HEIGHT))
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
        let Some(position) = self.tab_position_for_sidebar_y(y.max(0.0) as u32) else {
            self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "mouse");
            return;
        };
        if self.active_position() == Some(position) {
            self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "mouse");
            return;
        }
        let _ = self.select_tab_at(position);
        self.set_focus_surface_internal(UnixFocusSurface::Sidebar, "mouse");
    }

    fn handle_toolbar_hit(&mut self, hit: ToolbarHit) {
        match hit {
            ToolbarHit::NewTab => {
                let _ = self.create_tab(None, Vec::new(), Vec::new(), true, None);
            }
            ToolbarHit::ToggleTabs => {
                let visible = !self.config.tabs_visible;
                let _ =
                    self.set_tabs_visible(visible, "toolbar", crate::operations::UI_TABS_TOGGLE);
            }
            ToolbarHit::Settings => {
                self.open_settings();
            }
        }
        self.request_redraw();
    }

    fn handle_content_click(&mut self, x: f64, y: f64) {
        if self.handle_window_close_click(x, y) {
            return;
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
                &self.settings_font_draft,
                self.settings_size_draft,
                self.settings_theme_draft,
            );
            if let Some(hit) = modal.hit_test(x, y) {
                self.handle_settings_click(hit);
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
            let view = WorkspaceToolbarView::from_layout(toolbar, self.config.tabs_visible);
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
        cell_metrics(self.config.terminal_font_size)
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

        let attributes = WindowAttributes::default()
            .with_title(self.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(INITIAL_WIDTH, INITIAL_HEIGHT))
            .with_active(!self.no_activate);

        let window = Rc::new(event_loop.create_window(attributes)?);
        let surface = Surface::new(&self.context, Rc::clone(&window))
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let size = window.inner_size();
        let sidebar_width = self.sidebar_width();
        let (cell_width, cell_height) = self.cell_dimensions();
        let (cols, rows) = grid_dimensions_for_pixels(
            size.width,
            size.height,
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
        let size = window.inner_size();
        let sidebar_width = self.sidebar_width();
        let (cell_width, cell_height) = self.cell_dimensions();
        let (cols, rows) = grid_dimensions_for_pixels(
            size.width,
            size.height,
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
        self.sync_grid_from_tab();
    }

    fn handle_ipc(&mut self, envelope: IpcEnvelope) {
        let command = envelope.request.args.first().map(String::as_str);
        let response = match dispatch_shared_command(self, &envelope.request.args) {
            Some(response) => response,
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
                        other => Some(IpcResponse::failure(format!("unknown UI action: {other}"))),
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
        while let Ok(envelope) = self.ipc_receiver.try_recv() {
            changed = true;
            self.handle_ipc(envelope);
        }

        for tab in &mut self.tabs {
            if tab.poll() {
                changed = true;
            }
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
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn redraw(&mut self) {
        self.sync_grid_from_tab();
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
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
                size.width,
                size.height,
                &self.settings_font_draft,
                self.settings_size_draft,
                self.settings_theme_draft,
            )
        });
        let Some(grid) = self.grid.as_ref() else {
            return;
        };

        let modal_active = self.modal_surface_active();
        let workspace_toolbar = if modal_active {
            None
        } else {
            layout
                .workspace_toolbar
                .map(|toolbar| WorkspaceToolbarView::from_layout(toolbar, self.config.tabs_visible))
        };
        let composer_top = layout.composer.top.max(0) as u32;
        let composer_width = size.width.saturating_sub(sidebar_width);
        const SEND_W: u32 = 72;
        let send_x = sidebar_width + composer_width.saturating_sub(SEND_W + 8);
        let composer_view = ComposerView {
            text: &self.composer_buffer,
            focused: self.focus_surface == UnixFocusSurface::Composer,
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
            .map(|id| ConfirmCloseView::for_client(size.width, size.height, id));
        let window_close = self
            .window_close_pending
            .then(|| WindowCloseView::for_client(size.width, size.height));
        let resize_grip = layout.resize_grip.map(u32_rect);

        let Some(surface) = self.surface.as_mut() else {
            return;
        };

        if let (Some(width), Some(height)) = (
            std::num::NonZeroU32::new(size.width),
            std::num::NonZeroU32::new(size.height),
        ) {
            let _ = surface.resize(width, height);
        }

        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let width = buffer.width().get();
        let height = buffer.height().get();
        render_frame(
            &mut buffer,
            width,
            width,
            height,
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
                },
                sidebar_rows: &sidebar_rows,
                editing_tab_id: self.note_edit_target,
                workspace_toolbar,
                terminal_top: layout.terminal.top.max(0) as u32,
                composer: composer_view,
                scrollbar,
                sidebar_scrollbar,
                settings,
                confirm_close,
                window_close,
                status: Some(status_view),
                resize_grip,
            },
        );
        self.last_frame = Some((width, height, buffer.to_vec()));
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
        self.redraw();
        let (width, height, pixels) = self
            .last_frame
            .clone()
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
            Some((
                terminal.left.max(0) as u32,
                terminal.top.max(0) as u32,
                terminal.width().max(0) as u32,
                terminal.height().max(0) as u32,
            ))
        } else {
            None
        };
        screenshot::write_xrgb_png(&path, width, height, &pixels, clip)?;
        Ok(path.display().to_string())
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
        if self.note_edit_target.is_some() {
            self.sync_composer_buffer_to_tab();
            self.complete_tab_editor(true)?;
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
        if self.note_edit_target.is_some() {
            self.composer_buffer = if self.tab_note_draft.is_empty() {
                self.tab_name_draft.clone()
            } else {
                format!("{}\n{}", self.tab_name_draft, self.tab_note_draft)
            };
            self.request_redraw();
            return;
        }
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
            "resolved_font_family": RESOLVED_UNIX_FONT,
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
        if self.note_edit_target == Some(id) {
            let normalized = text.replace("\r\n", "\n");
            let (name, note) = normalized.split_once('\n').unwrap_or((&normalized, ""));
            self.tab_name_draft = name.to_owned();
            self.tab_note_draft = note.to_owned();
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
            WindowEvent::Resized(_) => {
                self.resize_to_window();
                if let Some(window) = self.window.as_ref() {
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
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {
                    return;
                }
                if self.window_close_pending {
                    if let Key::Named(NamedKey::Escape) = event.logical_key {
                        self.finish_window_close(WindowCloseChoice::Cancel);
                    } else if matches!(event.logical_key, Key::Named(NamedKey::Enter)) {
                        self.finish_window_close(WindowCloseChoice::KeepServerRunning);
                    }
                    return;
                }
                if self.settings_open {
                    if let Key::Named(NamedKey::Escape) = event.logical_key {
                        let _ = self.close_settings(false);
                    }
                    return;
                }
                if self.pending_close.is_some() {
                    if let Key::Named(NamedKey::Escape) = event.logical_key {
                        self.finish_close_confirmation(false);
                    }
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
                        if self.modifiers.control_key()
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
                            } else if self.note_edit_target.is_some() {
                                let _ = self.complete_tab_editor(false);
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
                                self.sync_composer_buffer_to_tab();
                                self.set_status_message("Cut composer draft");
                                self.request_redraw();
                            }
                        }
                        input::ComposerKeyAction::Paste => {
                            let _ = self.paste_clipboard_into_composer();
                        }
                        input::ComposerKeyAction::SelectAll => {}
                        input::ComposerKeyAction::Ignored => {}
                    }
                    return;
                }
                if self.modifiers.control_key()
                    && matches!(event.logical_key, Key::Character(ref value) if value.eq_ignore_ascii_case("c"))
                    && self.terminal_selection.is_some_and(|selection| {
                        selection.moved && self.active == Some(selection.tab_id)
                    })
                {
                    let _ = self.copy_terminal_selection();
                    return;
                }
                if self.modifiers.control_key()
                    && matches!(event.logical_key, Key::Character(ref value) if value.eq_ignore_ascii_case("v"))
                {
                    let _ = self.paste_clipboard_into_terminal();
                    return;
                }
                if let Some(bytes) = input::key_event_to_bytes(&event) {
                    let _ = self.cancel_terminal_selection(true);
                    self.queue_pty_input(bytes);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.last_cursor = (position.x, position.y);
                if self.tabs_resize_drag.is_some() {
                    self.drag_tabs_resize(position.x as i32);
                } else if self.sidebar_scroll_drag.is_some() {
                    self.drag_sidebar_scrollbar(position.y as i32);
                } else if self.scroll_drag.is_some() {
                    self.drag_scrollbar(position.y as i32);
                } else if self
                    .terminal_selection_gesture
                    .is_some_and(|gesture| gesture.active())
                {
                    self.drag_terminal_selection(position.x, position.y);
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
        if self
            .terminal_selection_gesture
            .is_some_and(SelectionGesture::active)
            && self.terminal_selection_autoscroll.is_some()
        {
            changed |= self.tick_terminal_selection_autoscroll();
            event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(33)));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
        if changed && let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        if self.close_requested {
            event_loop.exit();
        }
    }
}
