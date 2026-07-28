mod clipboard;
mod font;
mod input;
mod layout;
mod render;

use std::{
    collections::HashSet,
    env,
    rc::Rc,
    sync::{Arc, mpsc::Receiver},
    time::SystemTime,
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
    control_dispatch::{ControlHost, dispatch_shared_command},
    event_journal::{EventJournal, EventKind},
    gui_wake::{UnixWake, install_unix_wake},
    instances::register_instance,
    ipc_transport::{IpcEnvelope, start_ipc_server},
    protocol::IpcResponse,
    pty::TerminalSize,
    settings::{AppConfig, config_path, load_config, save_config},
    terminal_runtime::{TerminalLaunch, TerminalTab},
    terminal_selection::{
        SelectionGesture, TerminalPoint, TerminalSelection, terminal_selection_text,
    },
    theme::ThemeId,
    wake_signal::WakeSignal,
    workspace::workspace_path,
};

use render::{
    CELL_HEIGHT, CELL_WIDTH, COMPOSER_HEIGHT, ComposerView, FrameContent, SettingsHit,
    SettingsModalView, SidebarTabRow, TerminalGrid, TerminalPaint, effective_palette,
    grid_dimensions_for_pixels, render_frame, scrollbar_view_from_geometry, sidebar_row_at_y,
};

use layout::{
    ScrollbarHit, WHEEL_DELTA, WHEEL_ROWS_PER_NOTCH, pixel_rect_json, scrollbar_geometry,
    scrollbar_hit_test, sidebar_width_u32, terminal_cell_at, terminal_pixel_rect,
    wheel_delta_units, workspace_layout_for,
};

use crate::ui_geometry::scrollback_for_thumb_top;

#[derive(Clone, Copy, Debug)]
struct ScrollDrag {
    thumb_grab_offset: i32,
}

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
    if env::var_os("WAYLAND_DISPLAY").is_some() || env::var_os("DISPLAY").is_some() {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        return true;
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
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
    terminal_selection: Option<TerminalSelection>,
    terminal_selection_gesture: Option<SelectionGesture>,
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
            terminal_selection: None,
            terminal_selection_gesture: None,
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
        self.all_tree_rows()
            .into_iter()
            .filter(|row| {
                !row.ancestors
                    .iter()
                    .any(|id| self.collapsed_tabs.contains(id))
            })
            .collect()
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
        if self.tabs[position].sensitive_composer.is_some() {
            return;
        }
        if self.tabs[position].composer != self.composer_buffer {
            self.tabs[position].composer = self.composer_buffer.clone();
            self.commit_composer_draft(position);
        }
    }

    fn load_composer_buffer_from_tab(&mut self) {
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

    fn composer_region_contains(&self, x: f64, y: f64, window_height: u32) -> bool {
        x >= f64::from(self.sidebar_width())
            && y >= f64::from(window_height.saturating_sub(COMPOSER_HEIGHT))
    }

    fn relayout_after_config_change(&mut self) {
        self.resize_to_window();
        self.request_redraw();
    }

    fn open_settings(&mut self) {
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
                    active: self.active == Some(tab.id),
                    collapsed: self.collapsed_tabs.contains(&tab.id),
                    has_children,
                })
            })
            .collect()
    }

    fn tab_position_for_sidebar_y(&self, y: u32) -> Option<usize> {
        let row_index = sidebar_row_at_y(y)?;
        let rows = self.all_tree_rows();
        let row_id = rows.get(row_index)?.id;
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

    fn build_ui_snapshot_json(&mut self) -> String {
        let active = self.active;
        let (client_width, client_height) = self.client_size();
        let layout = self.layout();
        let sidebar_width = sidebar_width_u32(&layout);
        let visible_rows = self.visible_tree_rows();
        let all_rows = self.all_tree_rows();
        let scrollbar = self.active_position().map(|position| {
            let visible_rows = usize::from(self.tabs[position].last_size.0);
            let (offset, maximum) = self.tabs[position].scrollback_bounds();
            let geometry = scrollbar_geometry(&layout, visible_rows, offset, maximum);
            serde_json::json!({
                "visible": true,
                "track": pixel_rect_json(geometry.track),
                "thumb": pixel_rect_json(geometry.thumb),
                "max_offset": maximum,
            })
        });
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
                    .then(|| visible_rows.iter().position(|visible| visible.id == row.id))
                    .flatten();
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
                    "note": tab.note,
                    "active": active == Some(tab.id),
                    "state": Self::tab_state(tab),
                    "scrollback_offset": tab.parser.screen().scrollback(),
                    "draft": draft,
                    "bounds": visible_position.map(|position| pixel_rect_json(crate::ui_geometry::PixelRect {
                        left: 0,
                        top: (position as i32) * render::SIDEBAR_TAB_ROW_HEIGHT as i32,
                        right: sidebar_width as i32,
                        bottom: ((position + 1) as i32) * render::SIDEBAR_TAB_ROW_HEIGHT as i32,
                    })),
                }))
            })
            .collect::<Vec<_>>();
        serde_json::to_string_pretty(&serde_json::json!({
            "protocol_version": 1,
            "session": self.session_name,
            "active_window_id": active.map(|id| format!("@{id}")),
            "tabs_visible": self.config.tabs_visible,
            "window": {
                "client_width": client_width,
                "client_height": client_height,
            },
            "client": {
                "width": client_width,
                "height": client_height,
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
                },
                "terminal": {
                    "x": layout.terminal.left,
                    "y": layout.terminal.top,
                    "width": layout.terminal.width(),
                    "height": layout.terminal.height(),
                    "bounds": pixel_rect_json(layout.terminal),
                },
                "composer": {
                    "x": layout.composer.left,
                    "y": layout.composer.top,
                    "width": layout.composer.width(),
                    "height": layout.composer.height(),
                    "bounds": pixel_rect_json(layout.composer),
                },
                "scrollbar": scrollbar,
            },
            "focus": {
                "surface": self.focus_surface.as_str(),
                "window_id": active.map(|id| format!("@{id}")),
            },
            "composer": {
                "draft_length": self.composer_buffer.chars().count(),
                "focused": self.focus_surface == UnixFocusSurface::Composer,
            },
            "modal": if self.settings_open {
                Some(serde_json::json!({"kind": "settings"}))
            } else if self.note_edit_target.is_some() {
                Some(serde_json::json!({"kind": "tab-editor"}))
            } else {
                None
            },
            "system_menu": {
                "toggle_tabs": {
                    "label": "Toggle Tabs",
                    "checked": self.config.tabs_visible,
                },
            },
            "tab_editor": tab_editor,
            "event_position": self.event_journal.position(),
            "tabs": tabs,
            "selection": self.terminal_selection.map(|selection| {
                let (start, end) = selection.bounds();
                serde_json::json!({
                    "tab_id": format!("@{}", selection.tab_id),
                    "start": {"row": start.row, "col": start.col},
                    "end": {"row": end.row, "col": end.col},
                    "dragging": selection.dragging,
                })
            }),
            "terminal_interaction": {
                "selection": self
                    .terminal_selection_gesture
                    .map(|gesture| gesture.phase().as_str())
                    .unwrap_or("none"),
            },
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
        let sidebar_width = self.sidebar_width();
        if x >= f64::from(sidebar_width) {
            return;
        }
        if x < 28.0
            && let Some(row_index) = sidebar_row_at_y(y.max(0.0) as u32)
            && let Some(row) = self.visible_tree_rows().get(row_index)
            && self.tabs.iter().any(|tab| tab.parent_id == Some(row.id))
        {
            let _ = self.toggle_collapsed(row.id);
            return;
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

    fn handle_content_click(&mut self, x: f64, y: f64, window_height: u32) {
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
        if x < f64::from(self.sidebar_width()) {
            return;
        }
        if self.click_scrollbar(x as i32, y as i32) {
            return;
        }
        if self.begin_terminal_selection(x, y) {
            return;
        }
        if self.composer_region_contains(x, y, window_height) {
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
        terminal_cell_at(
            terminal_pixel_rect(&self.layout()),
            x as i32,
            y as i32,
            rows,
            cols,
            CELL_WIDTH as i32,
            CELL_HEIGHT as i32,
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
        let (rows, cols) = self.tabs[position].last_size;
        let Some(gesture) =
            SelectionGesture::prepare(tab_id, TerminalPoint { row, col }, rows, cols)
        else {
            return false;
        };
        self.terminal_selection = gesture.selection();
        self.terminal_selection_gesture = Some(gesture);
        self.set_focus_surface_internal(UnixFocusSurface::Terminal, "selection");
        self.request_redraw();
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
        let Some((col, row)) = terminal_cell_at(
            terminal,
            clamped_x,
            clamped_y,
            rows,
            cols,
            CELL_WIDTH as i32,
            CELL_HEIGHT as i32,
        ) else {
            return;
        };
        let updated = gesture.drag_to(TerminalPoint { row, col }, rows, cols);
        self.terminal_selection = updated.selection();
        self.terminal_selection_gesture = Some(updated);
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
        if changed {
            self.request_redraw();
        }
        changed
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

    fn mouse_wheel(&mut self, x: f64, y: f64, delta: MouseScrollDelta) {
        if self.settings_open {
            return;
        }
        let terminal = terminal_pixel_rect(&self.layout());
        if !terminal.contains(x as i32, y as i32) {
            return;
        }
        let units = match delta {
            MouseScrollDelta::LineDelta(_, lines) => wheel_delta_units(f64::from(lines), true),
            MouseScrollDelta::PixelDelta(pos) => wheel_delta_units(pos.y, false),
        };
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
        let (cols, rows) =
            grid_dimensions_for_pixels(size.width, size.height, sidebar_width, COMPOSER_HEIGHT);
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
        let (cols, rows) =
            grid_dimensions_for_pixels(size.width, size.height, sidebar_width, COMPOSER_HEIGHT);
        if let Some(grid) = self.grid.as_mut() {
            grid.resize(cols, rows);
        }
        if let Some(position) = self.active_position() {
            self.tabs[position].resize(rows, cols);
        }
    }

    fn handle_ipc(&mut self, envelope: IpcEnvelope) {
        let command = envelope.request.args.first().map(String::as_str);
        let response = match dispatch_shared_command(self, &envelope.request.args) {
            Some(response) => response,
            None if command == Some("ui-action") => {
                let action = envelope
                    .request
                    .args
                    .get(1)
                    .map(String::as_str)
                    .unwrap_or("");
                IpcResponse::failure(format!("unknown UI action: {action}"))
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

        let sidebar_rows = self.sidebar_rows();
        let palette = self.palette();
        let layout = self.layout();
        let sidebar_width = self.sidebar_width();
        let content_height = size.height.saturating_sub(COMPOSER_HEIGHT);
        let scrollbar = self.active_position().map(|position| {
            let visible_rows = usize::from(self.tabs[position].last_size.0);
            let (offset, maximum) = self.tabs[position].scrollback_bounds();
            scrollbar_view_from_geometry(scrollbar_geometry(&layout, visible_rows, offset, maximum))
        });
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
                terminal: TerminalPaint {
                    grid,
                    selection: self
                        .terminal_selection
                        .filter(|selection| self.active == Some(selection.tab_id)),
                },
                sidebar_rows: &sidebar_rows,
                composer: ComposerView {
                    text: &self.composer_buffer,
                    focused: self.focus_surface == UnixFocusSurface::Composer,
                },
                scrollbar,
                settings,
            },
        );
        let _ = buffer.present();
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
            "resolved_font_family": "bitmap-8x8",
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
        if self.settings_open {
            self.close_settings(false)?;
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
            WindowEvent::CloseRequested => event_loop.exit(),
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
                if self.settings_open {
                    if let Key::Named(NamedKey::Escape) = event.logical_key {
                        let _ = self.close_settings(false);
                    }
                    return;
                }
                if matches!(event.logical_key, Key::Named(NamedKey::Escape))
                    && self.cancel_terminal_selection(true)
                {
                    return;
                }
                if self.focus_surface == UnixFocusSurface::Composer {
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
                            if self.note_edit_target.is_some() {
                                let _ = self.complete_tab_editor(false);
                            } else {
                                self.sync_composer_buffer_to_tab();
                                self.set_focus_surface_internal(
                                    UnixFocusSurface::Terminal,
                                    "composer-escape",
                                );
                            }
                        }
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
                if let Some(bytes) = input::key_event_to_bytes(&event) {
                    let _ = self.cancel_terminal_selection(true);
                    self.queue_pty_input(bytes);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.last_cursor = (position.x, position.y);
                if self.scroll_drag.is_some() {
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
                let window_height = self
                    .window
                    .as_ref()
                    .map(|window| window.inner_size().height)
                    .unwrap_or(INITIAL_HEIGHT);
                if x < f64::from(self.sidebar_width()) {
                    let _ = self.cancel_terminal_selection(true);
                    self.handle_sidebar_click(x, y);
                } else {
                    self.handle_content_click(x, y, window_height);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if self.scroll_drag.is_some() {
                    self.end_scroll_drag();
                } else {
                    self.complete_terminal_selection();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.drain_wake_and_pty()
            && let Some(window) = self.window.as_ref()
        {
            window.request_redraw();
        }
        if self.close_requested {
            event_loop.exit();
        }
    }
}
