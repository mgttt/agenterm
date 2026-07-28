mod font;
mod input;
mod render;

use std::{
    env,
    rc::Rc,
    sync::{Arc, mpsc::Receiver},
    time::SystemTime,
};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::ModifiersState,
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
    theme::ThemeId,
    wake_signal::WakeSignal,
    workspace::workspace_path,
};

use render::{
    COMPOSER_HEIGHT, ComposerView, FrameContent, SIDEBAR_WIDTH, SidebarTabRow, TerminalGrid,
    grid_dimensions_for_pixels, render_frame, sidebar_row_at_y, theme_palette,
};

const APP_NAME: &str = "AgenTerm";
const INITIAL_WIDTH: u32 = 960;
const INITIAL_HEIGHT: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnixFocusSurface {
    Terminal,
    Composer,
    Sidebar,
}

impl UnixFocusSurface {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Composer => "composer",
            Self::Sidebar => "sidebar",
        }
    }

    fn from_ipc(value: &str) -> Result<Self, String> {
        match value {
            "terminal" => Ok(Self::Terminal),
            "composer" => Ok(Self::Composer),
            "tabs" | "sidebar" => Ok(Self::Sidebar),
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

    let no_activate = arguments.iter().any(|arg| {
        matches!(arg.as_str(), "--no-activate" | "--not-foreground")
    }) || no_activate_from_environment();

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
            config: load_config(),
            modifiers: ModifiersState::empty(),
        }
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
        x >= f64::from(SIDEBAR_WIDTH)
            && y >= f64::from(window_height.saturating_sub(COMPOSER_HEIGHT))
    }

    fn sidebar_rows(&self) -> Vec<SidebarTabRow> {
        self.all_tree_rows()
            .into_iter()
            .filter_map(|row| {
                let tab = self.tabs.iter().find(|tab| tab.id == row.id)?;
                Some(SidebarTabRow {
                    id: tab.id,
                    depth: row.depth,
                    title: tab.title.clone(),
                    active: self.active == Some(tab.id),
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

    fn build_ui_snapshot_json(&self) -> String {
        let active = self.active;
        let (client_width, client_height) = self.client_size();
        let content_height = client_height.saturating_sub(COMPOSER_HEIGHT);
        let terminal_width = client_width.saturating_sub(SIDEBAR_WIDTH);
        let rows = self.all_tree_rows();
        let tabs = rows
            .iter()
            .filter_map(|row| {
                let tab = self.tabs.iter().find(|tab| tab.id == row.id)?;
                Some(serde_json::json!({
                    "id": format!("@{}", tab.id),
                    "index": tab.index,
                    "name": tab.title,
                    "active": active == Some(tab.id),
                    "state": Self::tab_state(tab),
                    "scrollback_offset": tab.parser.screen().scrollback(),
                    "depth": row.depth,
                }))
            })
            .collect::<Vec<_>>();
        serde_json::to_string_pretty(&serde_json::json!({
            "session": self.session_name,
            "active_window_id": active.map(|id| format!("@{id}")),
            "tabs_visible": self.config.tabs_visible,
            "client": {
                "width": client_width,
                "height": client_height,
            },
            "layout": {
                "sidebar": {
                    "x": 0,
                    "y": 0,
                    "width": SIDEBAR_WIDTH,
                    "height": content_height,
                },
                "terminal": {
                    "x": SIDEBAR_WIDTH,
                    "y": 0,
                    "width": terminal_width,
                    "height": content_height,
                },
                "composer": {
                    "x": SIDEBAR_WIDTH,
                    "y": content_height,
                    "width": terminal_width,
                    "height": COMPOSER_HEIGHT,
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
            "event_position": self.event_journal.position(),
            "tabs": tabs,
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
        if x >= f64::from(SIDEBAR_WIDTH) {
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
        if x < f64::from(SIDEBAR_WIDTH) {
            return;
        }
        if self.composer_region_contains(x, y, window_height) {
            self.set_focus_surface_internal(UnixFocusSurface::Composer, "mouse");
        } else {
            self.set_focus_surface_internal(UnixFocusSurface::Terminal, "mouse");
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
        let (cols, rows) = grid_dimensions_for_pixels(size.width, size.height);
        let grid = TerminalGrid::new(cols, rows, theme_palette());

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
        self.event_journal_mut().commit(
            EventKind::TabSelected,
            Some(id),
            serde_json::json!({}),
        );
        Ok(())
    }

    fn resize_to_window(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        let (cols, rows) = grid_dimensions_for_pixels(size.width, size.height);
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
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        let sidebar_rows = self.sidebar_rows();
        let palette = ThemeId::Dark.palette();
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
                grid,
                sidebar_rows: &sidebar_rows,
                composer: ComposerView {
                    text: &self.composer_buffer,
                    focused: self.focus_surface == UnixFocusSurface::Composer,
                },
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
            return Err(format!("focus surface is unavailable: {}", surface.as_str()));
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
        self.request_redraw();
        Ok(())
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
            WindowEvent::KeyboardInput {
                event,
                ..
            } => {
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
                            self.sync_composer_buffer_to_tab();
                            self.set_focus_surface_internal(
                                UnixFocusSurface::Terminal,
                                "composer-escape",
                            );
                        }
                        input::ComposerKeyAction::Ignored => {}
                    }
                    return;
                }
                if let Some(bytes) = input::key_event_to_bytes(&event) {
                    self.queue_pty_input(bytes);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.last_cursor = (position.x, position.y);
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
                if x < f64::from(SIDEBAR_WIDTH) {
                    self.handle_sidebar_click(x, y);
                } else {
                    self.handle_content_click(x, y, window_height);
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
