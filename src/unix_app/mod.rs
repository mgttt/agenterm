mod font;
mod input;
mod render;

use std::{
    env,
    rc::Rc,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    window::{Window, WindowAttributes, WindowId},
};

use crate::{
    client::no_activate_from_environment,
    gui_wake::install_unix_wake,
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
    let event_loop = EventLoop::new()?;
    let context = Context::new(event_loop.owned_display_handle())
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let wake_signal = Arc::new(WakeSignal::new());
    let (wake_tx, wake_rx) = mpsc::sync_channel(64);
    install_unix_wake(wake_tx);

    let ipc_receiver = start_ipc_server(0, Arc::clone(&wake_signal))?;
    let session_name = format!("agenterm-{}", std::process::id());
    let _instance = register_instance(&crate::ipc_address(), &workspace_path(), &session_name)?;

    let mut app = UnixApp::new(
        title,
        no_activate,
        context,
        wake_signal,
        wake_rx,
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
    context: Context<OwnedDisplayHandle>,
    wake_signal: Arc<WakeSignal>,
    wake_rx: Receiver<()>,
    ipc_receiver: Receiver<IpcEnvelope>,
    session_name: String,
    window: Option<Rc<Window>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    grid: Option<TerminalGrid>,
    tab: Option<TerminalTab>,
    next_tab_id: u64,
}

impl UnixApp {
    fn new(
        title: String,
        no_activate: bool,
        context: Context<OwnedDisplayHandle>,
        wake_signal: Arc<WakeSignal>,
        wake_rx: Receiver<()>,
        ipc_receiver: Receiver<IpcEnvelope>,
        session_name: String,
    ) -> Self {
        Self {
            title,
            no_activate,
            context,
            wake_signal,
            wake_rx,
            ipc_receiver,
            session_name,
            window: None,
            surface: None,
            grid: None,
            tab: None,
            next_tab_id: 1,
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
        self.tab = Some(tab);
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
        if let Some(tab) = self.tab.as_mut() {
            tab.resize(rows, cols);
        }
    }

    fn drain_wake_and_pty(&mut self) -> bool {
        self.wake_signal.begin_drain();
        while self.wake_rx.try_recv().is_ok() {}

        let mut changed = false;
        while let Ok(envelope) = self.ipc_receiver.try_recv() {
            changed = true;
            let _ = envelope.respond_to.send(IpcResponse::typed_failure(
                "Unix GUI control plane is still integrating; window and PTY are live",
                "unix_gui_partial",
                "availability",
                true,
            ));
        }

        if let Some(tab) = self.tab.as_mut()
            && tab.poll()
        {
            changed = true;
        }
        if changed {
            self.sync_grid_from_tab();
        }
        changed
    }

    fn sync_grid_from_tab(&mut self) {
        let Some(tab) = self.tab.as_ref() else {
            return;
        };
        let Some(grid) = self.grid.as_mut() else {
            return;
        };
        grid.sync_from_screen(tab.parser.screen());
    }

    fn queue_pty_input(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Some(tab) = self.tab.as_mut() {
            let _ = tab.send(&bytes);
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

impl ApplicationHandler for UnixApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.ensure_window(event_loop) {
            eprintln!("AgenTerm GUI failed to create window: {error:#}");
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
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(bytes) = input::key_event_to_bytes(&event) {
                    self.queue_pty_input(bytes);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let mut woke = false;
        while self.wake_rx.try_recv().is_ok() {
            woke = true;
        }
        if (woke || self.drain_wake_and_pty())
            && let Some(window) = self.window.as_ref()
        {
            window.request_redraw();
        }
    }
}
