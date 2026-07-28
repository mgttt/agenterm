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
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

use crate::{
    client::no_activate_from_environment,
    control_dispatch::{ControlHost, dispatch_shared_command},
    gui_wake::{UnixWake, install_unix_wake},
    instances::register_instance,
    ipc_transport::{IpcEnvelope, start_ipc_server},
    protocol::IpcResponse,
    pty::TerminalSize,
    terminal_runtime::{TerminalLaunch, TerminalTab},
    theme::ThemeId,
    wake_signal::WakeSignal,
    workspace::workspace_path,
};

use render::{TerminalGrid, grid_dimensions_for_pixels, render_grid, theme_palette};

const APP_NAME: &str = "AgenTerm";
const INITIAL_WIDTH: u32 = 960;
const INITIAL_HEIGHT: u32 = 600;

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
    window: Option<Rc<Window>>,
    surface: Option<Surface<winit::event_loop::OwnedDisplayHandle, Rc<Window>>>,
    grid: Option<TerminalGrid>,
    tabs: Vec<TerminalTab>,
    active: Option<u64>,
    next_tab_id: u64,
    close_requested: bool,
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
            window: None,
            surface: None,
            grid: None,
            tabs: Vec::new(),
            active: None,
            next_tab_id: 1,
            close_requested: false,
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
        let response = match dispatch_shared_command(self, &envelope.request.args) {
            Some(response) => response,
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
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(grid) = self.grid.as_ref() else {
            return;
        };

        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

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
        let palette = ThemeId::Dark.palette();
        render_grid(&mut buffer, width, width, height, grid, palette);
        let _ = buffer.present();
    }
}

impl ControlHost for UnixApp {
    fn session_name(&self) -> &str {
        &self.session_name
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
        if select {
            self.active = Some(id);
            self.sync_grid_from_tab();
            self.request_redraw();
        }
        Ok(index)
    }

    fn select_tab_at(&mut self, position: usize) -> Result<(), String> {
        let Some(tab) = self.tabs.get(position) else {
            return Err("can't find window".to_owned());
        };
        self.active = Some(tab.id);
        self.sync_grid_from_tab();
        self.request_redraw();
        Ok(())
    }

    fn close_tab_id(&mut self, id: u64) -> Result<bool, String> {
        let Some(position) = self.tabs.iter().position(|tab| tab.id == id) else {
            return Err(format!("can't find window: @{id}"));
        };

        for tab in &mut self.tabs {
            if tab.parent_id == Some(id) {
                tab.parent_id = None;
            }
        }

        let terminal_shutdown_complete = self.tabs[position].close_process();
        self.tabs.remove(position);

        if self.active == Some(id) {
            self.active = self.tabs.first().map(|tab| tab.id);
            self.sync_grid_from_tab();
            self.request_redraw();
        }

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
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(bytes) = input::key_event_to_bytes(&event) {
                    self.queue_pty_input(bytes);
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
