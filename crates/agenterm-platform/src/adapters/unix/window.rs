//! Shared winit/softbuffer native text-window implementation for Unix adapters.

use std::{num::NonZeroU32, rc::Rc, time::Duration};

use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopBuilder},
    keyboard::{Key, NamedKey},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{UserAttentionType, Window, WindowAttributes, WindowId},
};

use crate::window::{
    NativeTextFrame, NativeTextInputEvent, NativeTextKey, NativeTextPointerButton,
    NativeTextWindowError, NativeTextWindowFocus, NativeTextWindowHost,
};

type DisplayHandle = winit::event_loop::OwnedDisplayHandle;
type ShellSurface = Surface<DisplayHandle, Rc<Window>>;

pub(crate) fn run_native_text_window<A, E>(
    host: Box<dyn NativeTextWindowHost>,
    no_activate: bool,
    platform_identity: &'static str,
    configure_attributes: A,
    configure_event_loop: E,
) -> Result<(), NativeTextWindowError>
where
    A: Fn(WindowAttributes, bool) -> WindowAttributes + 'static,
    E: Fn(&mut EventLoopBuilder<()>, bool),
{
    let mut builder = EventLoop::<()>::builder();
    configure_event_loop(&mut builder, no_activate);
    let event_loop = builder.build().map_err(|error| {
        NativeTextWindowError::failed("native_text_window_event_loop_create_failed", error)
    })?;
    let context = Context::new(event_loop.owned_display_handle()).map_err(|error| {
        NativeTextWindowError::failed("native_text_window_surface_context_failed", error)
    })?;
    let mut app = App {
        host,
        no_activate,
        platform_identity,
        configure_attributes: Box::new(configure_attributes),
        context,
        window: None,
        surface: None,
        frame: Vec::new(),
        frame_width: 0,
        frame_height: 0,
        scale_factor: 1.0,
        pointer_position: None,
        failure: None,
    };
    event_loop.run_app(&mut app).map_err(|error| {
        NativeTextWindowError::failed("native_text_window_event_loop_failed", error)
    })?;
    app.failure.map_or(Ok(()), Err)
}

struct App {
    host: Box<dyn NativeTextWindowHost>,
    no_activate: bool,
    platform_identity: &'static str,
    configure_attributes: Box<dyn Fn(WindowAttributes, bool) -> WindowAttributes>,
    context: Context<DisplayHandle>,
    window: Option<Rc<Window>>,
    surface: Option<ShellSurface>,
    frame: Vec<u32>,
    frame_width: u32,
    frame_height: u32,
    scale_factor: f64,
    pointer_position: Option<(i32, i32)>,
    failure: Option<NativeTextWindowError>,
}

impl App {
    fn fail(
        &mut self,
        event_loop: &ActiveEventLoop,
        code: &'static str,
        error: impl std::fmt::Display,
    ) {
        self.failure = Some(NativeTextWindowError::failed(code, error));
        event_loop.exit();
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn redraw(&mut self) -> Result<(), NativeTextWindowError> {
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(());
        };
        let Some(surface) = self.surface.as_mut() else {
            return Ok(());
        };
        surface.resize(width, height).map_err(|error| {
            NativeTextWindowError::failed("native_text_window_surface_resize_failed", error)
        })?;
        let mut buffer = surface.buffer_mut().map_err(|error| {
            NativeTextWindowError::failed("native_text_window_surface_buffer_failed", error)
        })?;
        let width = buffer.width().get();
        let height = buffer.height().get();
        render_shell(&mut buffer, width, height, &self.host.lines());
        self.frame.clear();
        self.frame.extend_from_slice(&buffer);
        self.frame_width = width;
        self.frame_height = height;
        self.scale_factor = window.scale_factor();
        buffer.present().map_err(|error| {
            NativeTextWindowError::failed("native_text_window_surface_present_failed", error)
        })
    }

    fn publish_window(&mut self, event_loop: &ActiveEventLoop, window: &Window) -> bool {
        let raw_handle = match native_window_identity(window) {
            Ok(handle) => handle,
            Err(error) => {
                self.fail(event_loop, "native_text_window_handle_failed", error);
                return false;
            }
        };
        if let Err(error) = self.host.publish_native_window(raw_handle) {
            self.failure = Some(error);
            event_loop.exit();
            return false;
        }
        true
    }

    fn service_host(&mut self, event_loop: &ActiveEventLoop) {
        if self.host.close_requested() {
            event_loop.exit();
            return;
        }
        if self.host.poll()
            && let Some(window) = self.window.as_ref()
        {
            window.set_title(&self.host.title());
            window.request_redraw();
        }
        if let Some(request) = self.host.take_focus_request()
            && request == NativeTextWindowFocus::Activate
            && let Some(window) = self.window.as_ref()
        {
            window.set_minimized(false);
            window.request_user_attention(Some(UserAttentionType::Informational));
            window.focus_window();
        }
        let frame = (!self.frame.is_empty()).then_some(NativeTextFrame {
            pixels: &self.frame,
            width: self.frame_width,
            height: self.frame_height,
            scale_factor: self.scale_factor,
        });
        if let Err(error) = self.host.capture_requested_screenshot(frame) {
            self.failure = Some(error);
            event_loop.exit();
        }
    }

    fn dispatch_input(&mut self, event_loop: &ActiveEventLoop, event: NativeTextInputEvent) {
        match self.host.handle_input(event) {
            Ok(true) => self.request_redraw(),
            Ok(false) => {}
            Err(error) => {
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = (self.configure_attributes)(
            WindowAttributes::default()
                .with_title(self.host.title())
                .with_inner_size(LogicalSize::new(760, 480)),
            self.no_activate,
        );
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(error) => {
                self.fail(event_loop, "native_text_window_create_failed", error);
                return;
            }
        };
        #[cfg(target_os = "linux")]
        if let Err(error) =
            super::x11_no_activate::reveal_window(event_loop, &window, self.no_activate)
        {
            self.fail(event_loop, "native_text_window_no_activate_failed", error);
            return;
        }
        if !self.publish_window(event_loop, &window) {
            return;
        }
        match Surface::new(&self.context, Rc::clone(&window)) {
            Ok(surface) => {
                self.surface = Some(surface);
                self.window = Some(window);
                self.request_redraw();
            }
            Err(error) => self.fail(
                event_loop,
                "native_text_window_surface_create_failed",
                format!("{}: {error}", self.platform_identity),
            ),
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
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw() {
                    self.failure = Some(error);
                    event_loop.exit();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer_position = Some((
                    position
                        .x
                        .round()
                        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
                    position
                        .y
                        .round()
                        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
                ));
            }
            WindowEvent::CursorLeft { .. } => self.pointer_position = None,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                let Some(button) = normalize_pointer_button(button) else {
                    return;
                };
                #[cfg(target_os = "macos")]
                let pointer_position = self
                    .window
                    .as_deref()
                    .and_then(macos_current_event_pointer_position)
                    .or(self.pointer_position);
                #[cfg(not(target_os = "macos"))]
                let pointer_position = self.pointer_position;
                let Some((physical_x, physical_y)) = pointer_position else {
                    return;
                };
                let width = self
                    .window
                    .as_ref()
                    .map(|window| window.inner_size().width)
                    .unwrap_or_default();
                self.dispatch_input(
                    event_loop,
                    NativeTextInputEvent::PointerPressed {
                        button,
                        physical_x,
                        physical_y,
                        line: unix_text_line_at(width, physical_y),
                    },
                );
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let Some(key) = normalize_key(&event.logical_key) else {
                    return;
                };
                self.dispatch_input(
                    event_loop,
                    NativeTextInputEvent::KeyPressed {
                        key,
                        repeat: event.repeat,
                    },
                );
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.service_host(event_loop);
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + Duration::from_millis(200),
        ));
    }
}

#[cfg(target_os = "macos")]
fn macos_current_event_pointer_position(window: &Window) -> Option<(i32, i32)> {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    let marker = MainThreadMarker::new()?;
    let event = NSApplication::sharedApplication(marker).currentEvent()?;
    // SAFETY: currentEvent retains the NSEvent while locationInWindow is read
    // on the AppKit main thread handling this MouseInput callback.
    let location = unsafe { event.locationInWindow() };
    macos_physical_pointer_position(
        window.inner_size().height,
        window.scale_factor(),
        location.x,
        location.y,
    )
}

#[cfg(target_os = "macos")]
fn macos_physical_pointer_position(
    physical_height: u32,
    scale_factor: f64,
    cocoa_x: f64,
    cocoa_y_from_bottom: f64,
) -> Option<(i32, i32)> {
    if !scale_factor.is_finite()
        || scale_factor <= 0.0
        || !cocoa_x.is_finite()
        || !cocoa_y_from_bottom.is_finite()
    {
        return None;
    }
    let physical_x = (cocoa_x * scale_factor).round();
    let physical_y = f64::from(physical_height) - (cocoa_y_from_bottom * scale_factor).round();
    if physical_x < 0.0
        || physical_y < 0.0
        || physical_x > f64::from(i32::MAX)
        || physical_y > f64::from(i32::MAX)
        || physical_y >= f64::from(physical_height)
    {
        return None;
    }
    Some((physical_x as i32, physical_y as i32))
}

fn normalize_pointer_button(button: MouseButton) -> Option<NativeTextPointerButton> {
    match button {
        MouseButton::Left => Some(NativeTextPointerButton::Primary),
        MouseButton::Right => Some(NativeTextPointerButton::Secondary),
        MouseButton::Middle => Some(NativeTextPointerButton::Middle),
        MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => None,
    }
}

fn normalize_key(key: &Key) -> Option<NativeTextKey> {
    match key {
        Key::Named(NamedKey::ArrowUp) => Some(NativeTextKey::ArrowUp),
        Key::Named(NamedKey::ArrowDown) => Some(NativeTextKey::ArrowDown),
        Key::Named(NamedKey::Home) => Some(NativeTextKey::Home),
        Key::Named(NamedKey::End) => Some(NativeTextKey::End),
        Key::Named(NamedKey::Enter) => Some(NativeTextKey::Enter),
        Key::Named(NamedKey::Escape) => Some(NativeTextKey::Escape),
        _ => None,
    }
}

fn unix_text_line_at(physical_width: u32, physical_y: i32) -> Option<usize> {
    if (16..64).contains(&physical_y) {
        return Some(0);
    }
    const FIRST_BODY_TOP: i32 = 76;
    if physical_y < FIRST_BODY_TOP {
        return None;
    }
    let body_scale = if physical_width >= 640 { 2 } else { 1 };
    let line_height = 7 * body_scale + 12;
    Some(
        1 + usize::try_from((physical_y - FIRST_BODY_TOP) / line_height)
            .expect("non-negative line offset fits usize"),
    )
}

fn native_window_identity(window: &Window) -> Result<i64, &'static str> {
    let handle = window
        .window_handle()
        .map_err(|_| "window handle unavailable")?;
    match handle.as_raw() {
        RawWindowHandle::Xlib(handle) => {
            i64::try_from(handle.window).map_err(|_| "Xlib window identity exceeds i64")
        }
        RawWindowHandle::Xcb(handle) => Ok(i64::from(handle.window.get())),
        RawWindowHandle::Wayland(handle) => Ok(handle.surface.as_ptr() as isize as i64),
        RawWindowHandle::AppKit(handle) => Ok(handle.ns_view.as_ptr() as isize as i64),
        _ => Err("unexpected Unix window handle"),
    }
}

fn render_shell(pixels: &mut [u32], width: u32, height: u32, lines: &[String]) {
    const BACKGROUND: u32 = 0x00F4_F6F8;
    const HEADER: u32 = 0x001B_2533;
    const TITLE: u32 = 0x00F8_FAFC;
    const BODY: u32 = 0x0020_2937;
    const DIVIDER: u32 = 0x00D9_DFE7;

    pixels.fill(BACKGROUND);
    fill_rect(pixels, width, height, 0, 0, width, 64, HEADER);
    fill_rect(pixels, width, height, 0, 64, width, 1, DIVIDER);
    let body_scale = if width >= 640 { 2 } else { 1 };
    let title_scale = if width >= 420 { 2 } else { 1 };
    if let Some(title) = lines.first() {
        draw_text(pixels, width, height, 24, 24, title_scale, TITLE, title);
    }
    let line_height = 7 * body_scale + 12;
    for (index, line) in lines.iter().skip(1).enumerate() {
        let y = 88_u32.saturating_add(
            u32::try_from(index)
                .unwrap_or(u32::MAX)
                .saturating_mul(line_height),
        );
        if y >= height {
            break;
        }
        draw_text(pixels, width, height, 24, y, body_scale, BODY, line);
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(
    pixels: &mut [u32],
    stride: u32,
    height: u32,
    x: u32,
    y: u32,
    width: u32,
    rect_height: u32,
    color: u32,
) {
    let right = x.saturating_add(width).min(stride);
    let bottom = y.saturating_add(rect_height).min(height);
    for row in y.min(height)..bottom {
        let start = row.saturating_mul(stride).saturating_add(x.min(stride)) as usize;
        let end = row.saturating_mul(stride).saturating_add(right) as usize;
        if let Some(slice) = pixels.get_mut(start..end) {
            slice.fill(color);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    pixels: &mut [u32],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    color: u32,
    text: &str,
) {
    let mut cursor = x;
    for character in text.chars() {
        if cursor >= width {
            break;
        }
        let glyph = glyph(character);
        for (row, bits) in glyph.iter().copied().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    fill_rect(
                        pixels,
                        width,
                        height,
                        cursor + column * scale,
                        y + u32::try_from(row).unwrap_or(u32::MAX) * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
        cursor = cursor.saturating_add(6 * scale);
    }
}

fn glyph(character: char) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [15, 17, 17, 15, 17, 17, 15],
        'C' => [14, 17, 1, 1, 1, 17, 14],
        'D' => [15, 17, 17, 17, 17, 17, 15],
        'E' => [31, 1, 1, 15, 1, 1, 31],
        'F' => [31, 1, 1, 15, 1, 1, 1],
        'G' => [14, 17, 1, 29, 17, 17, 30],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [28, 8, 8, 8, 9, 9, 6],
        'K' => [17, 9, 5, 3, 5, 9, 17],
        'L' => [1, 1, 1, 1, 1, 1, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 19, 21, 21, 25, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [15, 17, 17, 15, 1, 1, 1],
        'Q' => [14, 17, 17, 17, 21, 9, 22],
        'R' => [15, 17, 17, 15, 5, 9, 17],
        'S' => [30, 1, 1, 14, 16, 16, 15],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 16, 8, 4, 2, 1, 31],
        '0' => [14, 17, 25, 21, 19, 17, 14],
        '1' => [4, 6, 4, 4, 4, 4, 14],
        '2' => [14, 17, 16, 8, 4, 2, 31],
        '3' => [15, 16, 16, 14, 16, 16, 15],
        '4' => [8, 12, 10, 9, 31, 8, 8],
        '5' => [31, 1, 1, 15, 16, 16, 15],
        '6' => [14, 1, 1, 15, 17, 17, 14],
        '7' => [31, 16, 8, 4, 2, 2, 2],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 30, 16, 16, 14],
        '-' | '\u{2013}' | '\u{2014}' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        '.' => [0, 0, 0, 0, 0, 6, 6],
        '/' => [16, 8, 8, 4, 2, 2, 1],
        '(' => [8, 4, 2, 2, 2, 4, 8],
        ')' => [2, 4, 8, 8, 8, 4, 2],
        ' ' => [0; 7],
        _ => [14, 17, 16, 8, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_paints_header_and_body_on_bounded_surfaces() {
        let mut pixels = vec![0; 760 * 180];
        render_shell(
            &mut pixels,
            760,
            180,
            &["AgenTerm Control Center".into(), "Cockpit available".into()],
        );
        assert!(pixels.contains(&0x001B_2533));
        assert!(pixels.contains(&0x00F8_FAFC));
        assert!(pixels.contains(&0x0020_2937));

        let mut tiny = vec![0; 7 * 5];
        render_shell(&mut tiny, 7, 5, &["A".into()]);
        assert!(tiny.iter().all(|pixel| *pixel == 0x001B_2533));
    }

    #[test]
    fn text_hit_normalization_matches_renderer_rows() {
        assert_eq!(unix_text_line_at(760, 15), None);
        assert_eq!(unix_text_line_at(760, 24), Some(0));
        assert_eq!(unix_text_line_at(760, 75), None);
        assert_eq!(unix_text_line_at(760, 88), Some(1));
        assert_eq!(unix_text_line_at(760, 114), Some(2));
        assert_eq!(unix_text_line_at(420, 107), Some(2));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn appkit_event_location_becomes_top_left_physical_client_coordinates() {
        assert_eq!(
            macos_physical_pointer_position(936, 2.0, 60.0, 735.5),
            Some((120, 535))
        );
        assert_eq!(
            macos_physical_pointer_position(468, 1.0, 120.0, 200.0),
            Some((120, 268))
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn appkit_event_location_rejects_non_finite_and_outside_coordinates() {
        assert_eq!(
            macos_physical_pointer_position(468, f64::NAN, 0.0, 0.0),
            None
        );
        assert_eq!(macos_physical_pointer_position(468, 1.0, -1.0, 100.0), None);
        assert_eq!(macos_physical_pointer_position(468, 1.0, 10.0, 0.0), None);
    }
}
