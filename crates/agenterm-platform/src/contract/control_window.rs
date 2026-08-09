//! Platform-neutral native control-window contract.

use std::{borrow::Cow, fmt, time::Instant};

use super::input::{ModifierState, NormalizedKeyEvent};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ControlId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MenuCommandId(pub u16);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelPoint {
    pub x: i32,
    pub y: i32,
}

impl PixelPoint {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}

impl PixelSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelRect {
    pub origin: PixelPoint,
    pub size: PixelSize,
}

impl PixelRect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            origin: PixelPoint::new(x, y),
            size: PixelSize::new(width, height),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb8 {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ControlKind {
    Button,
    Label,
    TextInput {
        multiline: bool,
        password: bool,
        vertical_scroll: bool,
        want_return: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlSpec {
    pub id: ControlId,
    pub kind: ControlKind,
    pub text: String,
    pub bounds: PixelRect,
    pub enabled: bool,
    pub visible: bool,
    pub tab_stop: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemMenuItem {
    pub id: MenuCommandId,
    pub text: String,
    pub enabled: bool,
    pub checked: bool,
    pub separator_before: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlWindowOptions {
    pub title: String,
    pub initial_size: PixelSize,
    pub controls: Vec<ControlSpec>,
    pub system_menu: Vec<SystemMenuItem>,
    pub no_activate: bool,
    pub poll_interval_ms: u32,
}

impl ControlWindowOptions {
    pub fn new(title: impl Into<String>, initial_size: PixelSize) -> Self {
        Self {
            title: title.into(),
            initial_size,
            controls: Vec::new(),
            system_menu: Vec::new(),
            no_activate: false,
            poll_interval_ms: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PointerButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FocusTarget {
    None,
    Window,
    Control(ControlId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlWindowState {
    pub minimized: bool,
    pub maximized: bool,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WindowPresentation {
    Minimized,
    Maximized,
    Restored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TextHorizontalAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextOptions {
    pub horizontal: TextHorizontalAlignment,
    pub vertical_center: bool,
    pub single_line: bool,
    pub end_ellipsis: bool,
}

impl TextOptions {
    pub const fn single_line_left() -> Self {
        Self {
            horizontal: TextHorizontalAlignment::Left,
            vertical_center: false,
            single_line: true,
            end_ellipsis: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ControlWindowQuery {
    AutomationFocusSurface,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum ControlWheelDelta {
    Lines(f32),
    Pixels(i32),
}

/// Monotonic native rendering counters used to diagnose repaint storms.
///
/// The values are observations, not promises about a fixed frame rate. Callers
/// should compare two samples taken around a bounded interaction or idle period.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ControlWindowRenderActivity {
    pub redraw_requests: u64,
    pub parent_paints: u64,
    pub control_bounds_updates: u64,
    pub control_bounds_skips: u64,
    pub control_visibility_updates: u64,
    pub control_visibility_skips: u64,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ControlWindowEvent {
    Poll {
        now: Instant,
    },
    Resized {
        size: PixelSize,
        minimized: bool,
    },
    CloseRequested,
    FocusChanged(bool),
    KeyPreview {
        target: FocusTarget,
        event: NormalizedKeyEvent,
    },
    TextInput(String),
    PointerMoved {
        position: PixelPoint,
        modifiers: ModifierState,
    },
    PointerButton {
        button: PointerButton,
        state: ButtonState,
        position: PixelPoint,
        clicks: u8,
        modifiers: ModifierState,
    },
    Wheel {
        delta: ControlWheelDelta,
        position: PixelPoint,
        modifiers: ModifierState,
    },
    CaptureChanged(bool),
    Command(ControlId),
    SystemMenuOpening,
    SystemMenu(MenuCommandId),
    AutomationShortcut {
        key: u32,
        modifiers: ModifierState,
    },
    RenderActivitySample(ControlWindowRenderActivity),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ControlWindowDirective {
    Continue,
    Redraw,
    Consumed,
    ConsumedAndRedraw,
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ControlWindowError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl ControlWindowError {
    pub fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code: Cow::Borrowed(code),
            message: message.to_string(),
        }
    }
}

impl fmt::Display for ControlWindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(f, "control window unsupported: {reason}"),
            Self::Failed { code, message } => write!(f, "{code}: {message}"),
        }
    }
}

impl std::error::Error for ControlWindowError {}

pub(crate) trait ControlWindowBackend {
    fn request_redraw(&self);
    fn render_activity(&self) -> ControlWindowRenderActivity;
    fn close(&self);
    fn focus(&self);
    fn client_size(&self) -> PixelSize;
    fn state(&self) -> ControlWindowState;
    fn focused_target(&self) -> FocusTarget;
    fn control_at(&self, point: PixelPoint) -> Option<ControlId>;
    fn set_client_size(&self, size: PixelSize) -> Result<(), ControlWindowError>;
    fn set_presentation(&self, presentation: WindowPresentation);
    fn show_without_activation(&self);
    fn set_title(&self, title: &str) -> Result<(), ControlWindowError>;
    fn set_control_text(&self, id: ControlId, text: &str) -> Result<(), ControlWindowError>;
    fn control_text(&self, id: ControlId) -> Result<String, ControlWindowError>;
    fn set_control_max_length(&self, id: ControlId, chars: u32) -> Result<(), ControlWindowError>;
    fn control_selection(&self, id: ControlId) -> Result<(u32, u32), ControlWindowError>;
    fn copy_control_selection(&self, id: ControlId) -> Result<(), ControlWindowError>;
    fn paste_control_selection(&self, id: ControlId) -> Result<(), ControlWindowError>;
    fn select_all_control_text(&self, id: ControlId) -> Result<(), ControlWindowError>;
    fn set_control_bounds(
        &self,
        id: ControlId,
        bounds: PixelRect,
    ) -> Result<(), ControlWindowError>;
    fn set_control_enabled(&self, id: ControlId, enabled: bool) -> Result<(), ControlWindowError>;
    fn set_control_visible(&self, id: ControlId, visible: bool) -> Result<(), ControlWindowError>;
    fn set_system_menu_text(&self, id: MenuCommandId, text: &str)
    -> Result<(), ControlWindowError>;
    fn set_system_menu_enabled(
        &self,
        id: MenuCommandId,
        enabled: bool,
    ) -> Result<(), ControlWindowError>;
    fn set_system_menu_checked(
        &self,
        id: MenuCommandId,
        checked: bool,
    ) -> Result<(), ControlWindowError>;
    fn focus_control(&self, id: ControlId) -> Result<(), ControlWindowError>;
    fn set_pointer_capture(&self, capture: bool) -> Result<(), ControlWindowError>;
    fn pointer_capture_owned(&self) -> Result<bool, ControlWindowError>;
    fn set_cursor(&self, cursor: ControlCursor) -> Result<(), ControlWindowError>;
    #[cfg(feature = "font")]
    fn create_font(
        &self,
        request: crate::font::FontRequest<'_>,
    ) -> Result<crate::font::NativeFont, ControlWindowError>;
    #[cfg(feature = "screenshot")]
    fn capture_png(
        &self,
        path: &std::path::Path,
        area: crate::screenshot::NativeCaptureArea,
    ) -> Result<(), ControlWindowError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ControlCursor {
    Arrow,
    Hand,
    Text,
    ResizeHorizontal,
    ResizeVertical,
}

#[derive(Clone)]
pub struct ControlWindow(pub(crate) std::rc::Rc<dyn ControlWindowBackend>);

impl ControlWindow {
    pub fn request_redraw(&self) {
        self.0.request_redraw();
    }
    pub fn render_activity(&self) -> ControlWindowRenderActivity {
        self.0.render_activity()
    }
    pub fn close(&self) {
        self.0.close();
    }
    pub fn focus(&self) {
        self.0.focus();
    }
    pub fn client_size(&self) -> PixelSize {
        self.0.client_size()
    }
    pub fn state(&self) -> ControlWindowState {
        self.0.state()
    }
    pub fn focused_target(&self) -> FocusTarget {
        self.0.focused_target()
    }
    /// Which child control, if any, owns a client-area point.
    ///
    /// This asks the host the same question it answers when routing a real
    /// click; it is not a re-derived hit-test, and it must not become one.
    /// `None` means the window itself receives the pointer event, so a
    /// synthesized gesture reaches the application's own handler. `Some(id)`
    /// means a native control would swallow the click before the application
    /// ever sees it, which is exactly what a synthetic gesture has to detect so
    /// it can refuse out loud instead of doing nothing at all.
    pub fn control_at(&self, point: PixelPoint) -> Option<ControlId> {
        self.0.control_at(point)
    }
    pub fn set_client_size(&self, size: PixelSize) -> Result<(), ControlWindowError> {
        self.0.set_client_size(size)
    }
    pub fn set_presentation(&self, presentation: WindowPresentation) {
        self.0.set_presentation(presentation);
    }
    /// Shows or restores the native window without requesting foreground activation.
    pub fn show_without_activation(&self) {
        self.0.show_without_activation();
    }
    pub fn set_title(&self, v: &str) -> Result<(), ControlWindowError> {
        self.0.set_title(v)
    }
    pub fn set_control_text(&self, id: ControlId, v: &str) -> Result<(), ControlWindowError> {
        self.0.set_control_text(id, v)
    }
    pub fn control_text(&self, id: ControlId) -> Result<String, ControlWindowError> {
        self.0.control_text(id)
    }
    /// Caps how many UTF-16 code units a native text control accepts from the
    /// user. Backends that do not expose native edit limits treat this as a
    /// no-op. The product layer keeps validating byte budgets on save.
    pub fn set_control_max_length(
        &self,
        id: ControlId,
        chars: u32,
    ) -> Result<(), ControlWindowError> {
        self.0.set_control_max_length(id, chars)
    }
    /// Reports the native text control's ordered selection range in UTF-16
    /// code units; a collapsed selection has `start == end` at the caret.
    /// Native edit controls do not expose selection direction, so this is
    /// `(start, end)` with `start <= end`, never anchor/focus.
    pub fn control_selection(&self, id: ControlId) -> Result<(u32, u32), ControlWindowError> {
        self.0.control_selection(id)
    }
    /// Copies the current native text-control selection to the platform clipboard.
    pub fn copy_control_selection(&self, id: ControlId) -> Result<(), ControlWindowError> {
        self.0.copy_control_selection(id)
    }
    /// Pastes the platform clipboard at the current native text-control selection.
    pub fn paste_control_selection(&self, id: ControlId) -> Result<(), ControlWindowError> {
        self.0.paste_control_selection(id)
    }
    /// Selects the whole text of a native text control.
    pub fn select_all_control_text(&self, id: ControlId) -> Result<(), ControlWindowError> {
        self.0.select_all_control_text(id)
    }
    pub fn set_control_bounds(
        &self,
        id: ControlId,
        v: PixelRect,
    ) -> Result<(), ControlWindowError> {
        self.0.set_control_bounds(id, v)
    }
    pub fn set_control_enabled(&self, id: ControlId, v: bool) -> Result<(), ControlWindowError> {
        self.0.set_control_enabled(id, v)
    }
    pub fn set_control_visible(&self, id: ControlId, v: bool) -> Result<(), ControlWindowError> {
        self.0.set_control_visible(id, v)
    }
    pub fn set_system_menu_text(
        &self,
        id: MenuCommandId,
        text: &str,
    ) -> Result<(), ControlWindowError> {
        self.0.set_system_menu_text(id, text)
    }
    pub fn set_system_menu_enabled(
        &self,
        id: MenuCommandId,
        enabled: bool,
    ) -> Result<(), ControlWindowError> {
        self.0.set_system_menu_enabled(id, enabled)
    }
    pub fn set_system_menu_checked(
        &self,
        id: MenuCommandId,
        checked: bool,
    ) -> Result<(), ControlWindowError> {
        self.0.set_system_menu_checked(id, checked)
    }
    pub fn focus_control(&self, id: ControlId) -> Result<(), ControlWindowError> {
        self.0.focus_control(id)
    }
    pub fn set_pointer_capture(&self, v: bool) -> Result<(), ControlWindowError> {
        self.0.set_pointer_capture(v)
    }
    /// Reports whether this window currently owns native pointer capture.
    ///
    /// A successful `false` means that capture is absent or belongs to another
    /// window. Native query failures remain distinguishable as a typed error.
    pub fn pointer_capture_owned(&self) -> Result<bool, ControlWindowError> {
        self.0.pointer_capture_owned()
    }
    pub fn set_cursor(&self, v: ControlCursor) -> Result<(), ControlWindowError> {
        self.0.set_cursor(v)
    }
    #[cfg(feature = "font")]
    pub fn create_font(
        &self,
        request: crate::font::FontRequest<'_>,
    ) -> Result<crate::font::NativeFont, ControlWindowError> {
        self.0.create_font(request)
    }
    #[cfg(feature = "screenshot")]
    pub fn capture_png(
        &self,
        path: &std::path::Path,
        area: crate::screenshot::NativeCaptureArea,
    ) -> Result<(), ControlWindowError> {
        self.0.capture_png(path, area)
    }
}

pub trait ControlCanvas {
    fn size(&self) -> PixelSize;
    fn clear(&mut self, color: Rgb8);
    fn fill_rect(&mut self, rect: PixelRect, color: Rgb8);
    fn stroke_rect(&mut self, rect: PixelRect, color: Rgb8, width: u32);
    fn line(&mut self, start: PixelPoint, end: PixelPoint, color: Rgb8, width: u32);
    fn text(&mut self, origin: PixelPoint, text: &str, color: Rgb8);
    fn text_rect(&mut self, rect: PixelRect, text: &str, color: Rgb8, options: TextOptions);
    #[cfg(feature = "font")]
    fn set_font(&mut self, font: &crate::font::NativeFont) -> Result<(), ControlWindowError>;
}

pub trait ControlWindowApplication: 'static {
    fn opened(
        &mut self,
        window: &ControlWindow,
    ) -> Result<ControlWindowDirective, ControlWindowError>;
    fn event(
        &mut self,
        window: &ControlWindow,
        event: ControlWindowEvent,
    ) -> Result<ControlWindowDirective, ControlWindowError>;
    fn paint(
        &mut self,
        window: &ControlWindow,
        canvas: &mut dyn ControlCanvas,
    ) -> Result<ControlWindowDirective, ControlWindowError>;

    fn query(&self, _window: &ControlWindow, _query: ControlWindowQuery) -> isize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_and_controls_are_native_type_free() {
        let mut options = ControlWindowOptions::new("control host", PixelSize::new(760, 480));
        options.controls.push(ControlSpec {
            id: ControlId(7),
            kind: ControlKind::Button,
            text: "Run".to_owned(),
            bounds: PixelRect::new(8, 12, 96, 28),
            enabled: true,
            visible: true,
            tab_stop: true,
        });
        assert_eq!(options.controls[0].id, ControlId(7));
        assert_eq!(options.initial_size, PixelSize::new(760, 480));
    }

    #[test]
    fn failures_are_typed_and_stable() {
        assert_eq!(
            ControlWindowError::failed("control_window_test_failed", "expected").to_string(),
            "control_window_test_failed: expected"
        );
    }
}
