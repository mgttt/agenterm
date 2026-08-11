//! Unix winit/softbuffer pixel-window host.

use std::{
    num::NonZeroU32,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

#[cfg(target_os = "macos")]
use std::{cell::Cell, time::Duration};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize as NativeLogicalSize},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

use crate::{
    contract::{
        ime::ImeEvent,
        input::ModifierState,
        window_host::{
            GeometryChange, LogicalPoint, LogicalRect, LogicalSize, PixelWindow,
            PixelWindowApplication, PixelWindowBackend, PixelWindowDirective, PixelWindowError,
            PixelWindowEvent, PixelWindowMetrics, PixelWindowOptions, PointerButton,
            PointerButtonState, WheelDelta, WindowSemanticFlags, WindowWaker, XrgbPixelFrame,
        },
    },
    input::{NativeKeyEventExt as _, NativeModifierStateExt as _},
};

type DisplayHandle = winit::event_loop::OwnedDisplayHandle;
type PixelSurface = Surface<DisplayHandle, Rc<Window>>;

#[cfg(target_os = "macos")]
const MACOS_REACTIVATION_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn run_pixel_window(
    options: PixelWindowOptions,
    application: Box<dyn PixelWindowApplication>,
) -> Result<(), PixelWindowError> {
    let mut builder = EventLoop::<()>::with_user_event();
    configure_event_loop(&mut builder, options.no_activate);
    let event_loop = builder.build().map_err(|error| {
        PixelWindowError::failed("pixel_window_event_loop_create_failed", error)
    })?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let context = Context::new(event_loop.owned_display_handle())
        .map_err(|error| PixelWindowError::failed("pixel_window_surface_context_failed", error))?;
    let alive = Arc::new(AtomicBool::new(true));
    let proxy = event_loop.create_proxy();
    let wake_alive = Arc::clone(&alive);
    let waker = WindowWaker::new(Arc::new(move || {
        if !wake_alive.load(Ordering::Acquire) {
            return Err(event_loop_closed());
        }
        proxy.send_event(()).map_err(|_| event_loop_closed())
    }));
    let mut runner = PixelWindowRunner {
        options,
        application,
        context,
        window: None,
        surface: None,
        surface_size: None,
        waker,
        alive: Arc::clone(&alive),
        modifiers: ModifierState::empty(),
        last_pointer: None,
        failure: None,
        #[cfg(target_os = "macos")]
        detached_for_reopen: Rc::new(Cell::new(false)),
    };
    let run_result = event_loop.run_app(&mut runner);
    alive.store(false, Ordering::Release);
    run_result
        .map_err(|error| PixelWindowError::failed("pixel_window_event_loop_failed", error))?;
    runner.failure.take().map_or(Ok(()), Err)
}

fn configure_event_loop<T: 'static>(
    builder: &mut winit::event_loop::EventLoopBuilder<T>,
    no_activate: bool,
) {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS as _;
        builder.with_activate_ignoring_other_apps(!no_activate);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (builder, no_activate);
}

fn event_loop_closed() -> PixelWindowError {
    PixelWindowError::failed(
        "pixel_window_event_loop_closed",
        "the native pixel-window event loop is no longer running",
    )
}

struct NativeWindowBackend {
    window: Rc<Window>,
    #[cfg(target_os = "macos")]
    detached_for_reopen: Rc<Cell<bool>>,
}

impl PixelWindowBackend for NativeWindowBackend {
    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    fn metrics(&self) -> Result<PixelWindowMetrics, PixelWindowError> {
        native_metrics(&self.window)
    }

    fn semantic_flags(&self) -> WindowSemanticFlags {
        WindowSemanticFlags {
            minimized: self.window.is_minimized().unwrap_or(false),
            maximized: self.window.is_maximized(),
            visible: self.window.is_visible().unwrap_or(true),
        }
    }

    fn set_minimized(&self, minimized: bool) {
        self.window.set_minimized(minimized);
    }

    fn set_maximized(&self, maximized: bool) {
        self.window.set_maximized(maximized);
    }

    fn set_visible(&self, visible: bool) {
        self.window.set_visible(visible);
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSApplication;
            use objc2_foundation::MainThreadMarker;

            self.detached_for_reopen.set(!visible);
            if !visible && let Some(marker) = MainThreadMarker::new() {
                let application = NSApplication::sharedApplication(marker);
                application.hide(None);
            }
        }
    }

    fn focus(&self) {
        self.window.set_minimized(false);
        self.window.focus_window();
    }

    fn set_title(&self, title: &str) {
        self.window.set_title(title);
    }

    fn request_logical_inner_size(&self, size: LogicalSize) -> Result<(), PixelWindowError> {
        if !size.is_valid() {
            return Err(PixelWindowError::failed(
                "pixel_window_invalid_client_size",
                "logical client width and height must be finite and greater than zero",
            ));
        }
        let _ = self
            .window
            .request_inner_size(NativeLogicalSize::new(size.width, size.height));
        Ok(())
    }

    fn set_pointer_capture(&self, _captured: bool) -> Result<(), PixelWindowError> {
        Err(PixelWindowError::unsupported(
            "portable pixel-window pointer capture is not implemented",
        ))
    }

    fn set_ime_allowed(&self, allowed: bool) {
        self.window.set_ime_allowed(allowed);
    }

    fn set_ime_cursor_area(&self, area: LogicalRect) -> Result<(), PixelWindowError> {
        if !area.origin.x.is_finite() || !area.origin.y.is_finite() || !area.size.is_valid() {
            return Err(PixelWindowError::failed(
                "pixel_window_invalid_ime_cursor_area",
                "IME cursor area must contain finite coordinates and positive extents",
            ));
        }
        self.window.set_ime_cursor_area(
            LogicalPosition::new(area.origin.x, area.origin.y),
            NativeLogicalSize::new(area.size.width, area.size.height),
        );
        Ok(())
    }
}

struct PixelWindowRunner {
    options: PixelWindowOptions,
    application: Box<dyn PixelWindowApplication>,
    context: Context<DisplayHandle>,
    window: Option<PixelWindow>,
    surface: Option<PixelSurface>,
    surface_size: Option<(u32, u32)>,
    waker: WindowWaker,
    alive: Arc<AtomicBool>,
    modifiers: ModifierState,
    last_pointer: Option<LogicalPoint>,
    failure: Option<PixelWindowError>,
    #[cfg(target_os = "macos")]
    detached_for_reopen: Rc<Cell<bool>>,
}

impl PixelWindowRunner {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: PixelWindowError) {
        self.failure = Some(error);
        self.alive.store(false, Ordering::Release);
        event_loop.exit();
    }

    fn apply_directive(&mut self, event_loop: &ActiveEventLoop, directive: PixelWindowDirective) {
        match directive {
            PixelWindowDirective::Continue => {}
            PixelWindowDirective::Wait => event_loop.set_control_flow(ControlFlow::Wait),
            PixelWindowDirective::WaitUntil(deadline) => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
            PixelWindowDirective::Exit => {
                self.alive.store(false, Ordering::Release);
                event_loop.exit();
            }
        }
    }

    fn dispatch_event(&mut self, event_loop: &ActiveEventLoop, event: PixelWindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };
        match self.application.event(&window, event) {
            Ok(directive) => self.apply_directive(event_loop, directive),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn dispatch_geometry(&mut self, event_loop: &ActiveEventLoop, change: GeometryChange) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let metrics = match window.metrics() {
            Ok(metrics) => metrics,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        if !metrics.is_drawable() {
            return;
        }
        window.request_redraw();
        self.dispatch_event(
            event_loop,
            PixelWindowEvent::GeometryChanged { change, metrics },
        );
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let metrics = match window.metrics() {
            Ok(metrics) => metrics,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        let (Some(width), Some(height)) = (
            NonZeroU32::new(metrics.physical_width),
            NonZeroU32::new(metrics.physical_height),
        ) else {
            return;
        };
        match self.render_once(&window, metrics, width, height) {
            Ok(directive) => self.apply_directive(event_loop, directive),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn render_once(
        &mut self,
        window: &PixelWindow,
        metrics: PixelWindowMetrics,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        let surface = self.surface.as_mut().ok_or_else(|| {
            PixelWindowError::failed(
                "pixel_window_surface_unavailable",
                "redraw requested before the native surface was created",
            )
        })?;
        if self.surface_size != Some((width.get(), height.get())) {
            surface.resize(width, height).map_err(|error| {
                PixelWindowError::failed("pixel_window_surface_resize_failed", error)
            })?;
            self.surface_size = Some((width.get(), height.get()));
        }
        let mut buffer = surface.buffer_mut().map_err(|error| {
            PixelWindowError::failed("pixel_window_surface_buffer_failed", error)
        })?;
        let frame_width = buffer.width().get();
        let frame_height = buffer.height().get();
        let directive = {
            let mut frame =
                XrgbPixelFrame::new(&mut buffer, frame_width, frame_height, metrics.scale_factor);
            self.application.render(window, &mut frame)?
        };
        buffer.present().map_err(|error| {
            PixelWindowError::failed("pixel_window_surface_present_failed", error)
        })?;
        Ok(directive)
    }
}

impl ApplicationHandler<()> for PixelWindowRunner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title(self.options.title.clone())
            .with_inner_size(NativeLogicalSize::new(
                self.options.initial_logical_size.width,
                self.options.initial_logical_size.height,
            ))
            // Keeps the terminal viewport at a usable size; a shorter window
            // leaves fewer grid rows than the terminal contract supports.
            .with_min_inner_size(NativeLogicalSize::new(320.0, 240.0));
        let attributes = if let Some((width, height, rgba)) = &self.options.window_icon_rgba {
            match winit::window::Icon::from_rgba(rgba.clone(), *width, *height) {
                Ok(icon) => attributes.with_window_icon(Some(icon)),
                Err(_) => attributes,
            }
        } else {
            attributes
        };
        let attributes = configure_window_attributes(attributes, self.options.no_activate);
        let native_window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(error) => {
                self.fail(
                    event_loop,
                    PixelWindowError::failed("pixel_window_create_failed", error),
                );
                return;
            }
        };
        #[cfg(target_os = "linux")]
        if let Err(error) = super::x11_no_activate::reveal_window(
            event_loop,
            &native_window,
            self.options.no_activate,
        ) {
            self.fail(
                event_loop,
                PixelWindowError::failed("pixel_window_no_activate_failed", error),
            );
            return;
        }
        native_window.set_ime_allowed(self.options.ime_allowed);
        let surface = match Surface::new(&self.context, Rc::clone(&native_window)) {
            Ok(surface) => surface,
            Err(error) => {
                self.fail(
                    event_loop,
                    PixelWindowError::failed("pixel_window_surface_create_failed", error),
                );
                return;
            }
        };
        let backend: Rc<dyn PixelWindowBackend> = Rc::new(NativeWindowBackend {
            window: native_window,
            #[cfg(target_os = "macos")]
            detached_for_reopen: Rc::clone(&self.detached_for_reopen),
        });
        let window = PixelWindow::new(backend, self.waker.clone());
        self.surface = Some(surface);
        self.window = Some(window.clone());
        match self.application.opened(&window) {
            Ok(directive) => {
                window.request_redraw();
                self.apply_directive(event_loop, directive);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: ()) {
        self.dispatch_event(event_loop, PixelWindowEvent::Wake);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.dispatch_event(event_loop, PixelWindowEvent::CloseRequested);
            }
            WindowEvent::Resized(_) => self.dispatch_geometry(event_loop, GeometryChange::Resized),
            WindowEvent::ScaleFactorChanged { .. } => {
                self.dispatch_geometry(event_loop, GeometryChange::ScaleFactorChanged);
            }
            WindowEvent::Focused(focused) => {
                self.dispatch_event(event_loop, PixelWindowEvent::FocusChanged(focused));
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state().to_platform_modifiers();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.dispatch_event(
                    event_loop,
                    PixelWindowEvent::Keyboard(event.to_normalized_key_event(self.modifiers)),
                );
            }
            WindowEvent::Ime(event) => {
                let event = match event {
                    Ime::Enabled => ImeEvent::Enabled,
                    Ime::Preedit(text, cursor) => ImeEvent::Preedit { text, cursor },
                    Ime::Commit(text) => ImeEvent::Commit(text),
                    Ime::Disabled => ImeEvent::Disabled,
                };
                self.dispatch_event(event_loop, PixelWindowEvent::Ime(event));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .and_then(|window| window.scale_factor().ok())
                    .unwrap_or(1.0);
                let logical = position.to_logical::<f64>(scale);
                let point = LogicalPoint::new(logical.x, logical.y);
                self.last_pointer = Some(point);
                self.dispatch_event(
                    event_loop,
                    PixelWindowEvent::PointerMoved {
                        position: point,
                        modifiers: self.modifiers,
                    },
                );
            }
            WindowEvent::CursorLeft { .. } => {
                self.last_pointer = None;
                self.dispatch_event(event_loop, PixelWindowEvent::PointerLeft);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => WheelDelta::Lines { x, y },
                    MouseScrollDelta::PixelDelta(position) => {
                        let scale = self
                            .window
                            .as_ref()
                            .and_then(|window| window.scale_factor().ok())
                            .unwrap_or(1.0);
                        let logical = position.to_logical::<f64>(scale);
                        WheelDelta::LogicalPixels {
                            x: logical.x,
                            y: logical.y,
                        }
                    }
                };
                self.dispatch_event(
                    event_loop,
                    PixelWindowEvent::MouseWheel {
                        delta,
                        position: self.last_pointer,
                        modifiers: self.modifiers,
                    },
                );
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let state = match state {
                    ElementState::Pressed => PointerButtonState::Pressed,
                    ElementState::Released => PointerButtonState::Released,
                };
                self.dispatch_event(
                    event_loop,
                    PixelWindowEvent::PointerButton {
                        button: pointer_button(button),
                        state,
                        position: self.last_pointer,
                        modifiers: self.modifiers,
                    },
                );
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        if macos_should_reopen(
            self.window.as_ref().is_some_and(PixelWindow::visible),
            self.detached_for_reopen.get(),
            macos_application_is_hidden(),
        ) {
            self.dispatch_event(event_loop, PixelWindowEvent::Reopen);
        }
        let Some(window) = self.window.clone() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        let now = Instant::now();
        match self.application.about_to_wait(&window, now) {
            Ok(directive) => {
                #[cfg(target_os = "macos")]
                let directive = macos_reactivation_poll_directive(window.visible(), now, directive);
                self.apply_directive(event_loop, directive);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_reactivation_poll_directive(
    window_visible: bool,
    now: Instant,
    directive: PixelWindowDirective,
) -> PixelWindowDirective {
    if window_visible || matches!(directive, PixelWindowDirective::Exit) {
        return directive;
    }

    let poll_at = now + MACOS_REACTIVATION_POLL_INTERVAL;
    match directive {
        PixelWindowDirective::WaitUntil(deadline) => {
            PixelWindowDirective::WaitUntil(deadline.min(poll_at))
        }
        PixelWindowDirective::Continue | PixelWindowDirective::Wait => {
            PixelWindowDirective::WaitUntil(poll_at)
        }
        PixelWindowDirective::Exit => PixelWindowDirective::Exit,
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn hidden_window_keeps_the_event_loop_polling_for_dock_reactivation() {
        let now = Instant::now();
        assert_eq!(
            macos_reactivation_poll_directive(false, now, PixelWindowDirective::Wait),
            PixelWindowDirective::WaitUntil(now + MACOS_REACTIVATION_POLL_INTERVAL)
        );
    }

    #[test]
    fn hidden_window_preserves_an_earlier_application_deadline() {
        let now = Instant::now();
        let earlier = now + Duration::from_millis(25);
        assert_eq!(
            macos_reactivation_poll_directive(false, now, PixelWindowDirective::WaitUntil(earlier),),
            PixelWindowDirective::WaitUntil(earlier)
        );
    }

    #[test]
    fn detached_window_reopens_as_soon_as_the_dock_unhides_the_app() {
        assert!(macos_should_reopen(false, true, false));
        assert!(!macos_should_reopen(false, true, true));
        assert!(!macos_should_reopen(false, false, false));
        assert!(!macos_should_reopen(true, true, false));
    }
}

#[cfg(target_os = "macos")]
const fn macos_should_reopen(
    window_visible: bool,
    detached_for_reopen: bool,
    application_hidden: bool,
) -> bool {
    !window_visible && detached_for_reopen && !application_hidden
}

#[cfg(target_os = "macos")]
fn macos_application_is_hidden() -> bool {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    MainThreadMarker::new().is_some_and(|marker| {
        let application = NSApplication::sharedApplication(marker);
        unsafe { application.isHidden() }
    })
}

impl Drop for PixelWindowRunner {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

fn configure_window_attributes(
    attributes: WindowAttributes,
    no_activate: bool,
) -> WindowAttributes {
    #[cfg(target_os = "linux")]
    return super::x11_no_activate::prepare_window(attributes, no_activate);
    #[cfg(not(target_os = "linux"))]
    attributes.with_active(!no_activate)
}

fn native_metrics(window: &Window) -> Result<PixelWindowMetrics, PixelWindowError> {
    let scale_factor = window.scale_factor();
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err(PixelWindowError::failed(
            "pixel_window_invalid_scale_factor",
            format!("native window returned {scale_factor}"),
        ));
    }
    let physical = window.inner_size();
    Ok(PixelWindowMetrics {
        logical_size: LogicalSize::new(
            f64::from(physical.width) / scale_factor,
            f64::from(physical.height) / scale_factor,
        ),
        physical_width: physical.width,
        physical_height: physical.height,
        scale_factor,
    })
}

fn pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Left,
        MouseButton::Right => PointerButton::Right,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Other(4),
        MouseButton::Forward => PointerButton::Other(5),
        MouseButton::Other(value) => PointerButton::Other(value),
    }
}
