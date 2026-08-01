//! Platform-neutral native control-window contract.

use std::{borrow::Cow, fmt, time::Instant};

use super::input::NormalizedKeyEvent;

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
    TextInput { multiline: bool, password: bool },
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
    Window,
    Control(ControlId),
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum ControlWheelDelta {
    Lines(f32),
    Pixels(i32),
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
    PointerMoved(PixelPoint),
    PointerButton {
        button: PointerButton,
        state: ButtonState,
        position: PixelPoint,
        clicks: u8,
    },
    Wheel {
        delta: ControlWheelDelta,
        position: PixelPoint,
    },
    CaptureChanged(bool),
    Command(ControlId),
    SystemMenu(MenuCommandId),
    Automation {
        name: String,
        argument: Option<String>,
    },
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
    fn close(&self);
    fn focus(&self);
    fn client_size(&self) -> PixelSize;
    fn set_title(&self, title: &str) -> Result<(), ControlWindowError>;
    fn set_control_text(&self, id: ControlId, text: &str) -> Result<(), ControlWindowError>;
    fn control_text(&self, id: ControlId) -> Result<String, ControlWindowError>;
    fn set_control_bounds(
        &self,
        id: ControlId,
        bounds: PixelRect,
    ) -> Result<(), ControlWindowError>;
    fn set_control_enabled(&self, id: ControlId, enabled: bool) -> Result<(), ControlWindowError>;
    fn set_control_visible(&self, id: ControlId, visible: bool) -> Result<(), ControlWindowError>;
    fn focus_control(&self, id: ControlId) -> Result<(), ControlWindowError>;
    fn set_pointer_capture(&self, capture: bool) -> Result<(), ControlWindowError>;
    fn set_cursor(&self, cursor: ControlCursor) -> Result<(), ControlWindowError>;
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
    pub fn close(&self) {
        self.0.close();
    }
    pub fn focus(&self) {
        self.0.focus();
    }
    pub fn client_size(&self) -> PixelSize {
        self.0.client_size()
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
    pub fn focus_control(&self, id: ControlId) -> Result<(), ControlWindowError> {
        self.0.focus_control(id)
    }
    pub fn set_pointer_capture(&self, v: bool) -> Result<(), ControlWindowError> {
        self.0.set_pointer_capture(v)
    }
    pub fn set_cursor(&self, v: ControlCursor) -> Result<(), ControlWindowError> {
        self.0.set_cursor(v)
    }
}

pub trait ControlCanvas {
    fn size(&self) -> PixelSize;
    fn clear(&mut self, color: Rgb8);
    fn fill_rect(&mut self, rect: PixelRect, color: Rgb8);
    fn stroke_rect(&mut self, rect: PixelRect, color: Rgb8, width: u32);
    fn line(&mut self, start: PixelPoint, end: PixelPoint, color: Rgb8, width: u32);
    fn text(&mut self, origin: PixelPoint, text: &str, color: Rgb8);
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
