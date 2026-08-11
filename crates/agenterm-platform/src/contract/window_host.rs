//! Platform-neutral pixel-window host contract.

use std::{borrow::Cow, fmt, rc::Rc, sync::Arc, time::Instant};

use super::{
    ime::ImeEvent,
    input::NormalizedKeyEvent,
    pixel_present::{PixelPresentReceipt, PixelPresentStats},
};
use crate::window::WindowSemanticState;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalSize {
    pub width: f64,
    pub height: f64,
}

impl LogicalSize {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    pub fn is_valid(self) -> bool {
        self.width.is_finite() && self.width > 0.0 && self.height.is_finite() && self.height > 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalPoint {
    pub x: f64,
    pub y: f64,
}

impl LogicalPoint {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRect {
    pub origin: LogicalPoint,
    pub size: LogicalSize,
}

impl LogicalRect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: LogicalPoint::new(x, y),
            size: LogicalSize::new(width, height),
        }
    }
}

/// A host-neutral physical-pixel rectangle.
///
/// Coordinates are half-open: `left <= x < right` and `top <= y < bottom`.
/// This type deliberately contains no product meaning such as tabs, cells,
/// selection, IME, or cursor state. Native adapters may clip or conservatively
/// promote it to a full redraw when their coordinate domain cannot represent
/// it safely.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelRect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl PixelRect {
    pub const fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn empty() -> Self {
        Self::new(0, 0, 0, 0)
    }

    pub const fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    pub const fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    pub const fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }

    /// Clips the rectangle to a physical frame with `width x height` pixels.
    /// Invalid or reversed edges collapse to an empty rectangle rather than
    /// escaping the frame.
    pub fn clip(self, width: u32, height: u32) -> Self {
        let left = self.left.min(width);
        let top = self.top.min(height);
        let right = self.right.min(width).max(left);
        let bottom = self.bottom.min(height).max(top);
        Self::new(left, top, right, bottom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelWindowMetrics {
    pub logical_size: LogicalSize,
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f64,
}

impl PixelWindowMetrics {
    pub fn is_drawable(self) -> bool {
        self.physical_width > 0
            && self.physical_height > 0
            && self.logical_size.is_valid()
            && self.scale_factor.is_finite()
            && self.scale_factor > 0.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowSemanticFlags {
    pub minimized: bool,
    pub maximized: bool,
    pub visible: bool,
}

impl WindowSemanticFlags {
    pub const fn state(self) -> WindowSemanticState {
        WindowSemanticState::from_native_flags(self.minimized, self.maximized)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelWindowOptions {
    pub title: String,
    pub initial_logical_size: LogicalSize,
    pub no_activate: bool,
    pub ime_allowed: bool,
    pub window_icon_rgba: Option<(u32, u32, Vec<u8>)>,
}

impl PixelWindowOptions {
    pub fn new(title: impl Into<String>, initial_logical_size: LogicalSize) -> Self {
        Self {
            title: title.into(),
            initial_logical_size,
            no_activate: false,
            ime_allowed: false,
            window_icon_rgba: None,
        }
    }

    pub const fn with_no_activate(mut self, no_activate: bool) -> Self {
        self.no_activate = no_activate;
        self
    }

    pub const fn with_ime_allowed(mut self, ime_allowed: bool) -> Self {
        self.ime_allowed = ime_allowed;
        self
    }

    pub fn with_window_icon_rgba(mut self, icon: Option<(u32, u32, Vec<u8>)>) -> Self {
        self.window_icon_rgba = icon;
        self
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
pub enum PointerButtonState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum WheelDelta {
    Lines { x: f32, y: f32 },
    LogicalPixels { x: f64, y: f64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum GeometryChange {
    Resized,
    ScaleFactorChanged,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum PixelWindowEvent {
    Wake,
    Reopen,
    CloseRequested,
    GeometryChanged {
        change: GeometryChange,
        metrics: PixelWindowMetrics,
    },
    FocusChanged(bool),
    Keyboard(NormalizedKeyEvent),
    Ime(ImeEvent),
    PointerMoved {
        position: LogicalPoint,
        modifiers: crate::contract::input::ModifierState,
    },
    PointerLeft,
    PointerCaptureLost,
    PointerButton {
        button: PointerButton,
        state: PointerButtonState,
        position: Option<LogicalPoint>,
        modifiers: crate::contract::input::ModifierState,
    },
    MouseWheel {
        delta: WheelDelta,
        position: Option<LogicalPoint>,
        modifiers: crate::contract::input::ModifierState,
    },
}

pub struct XrgbPixelFrame<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
    scale_factor: f64,
}

impl<'a> XrgbPixelFrame<'a> {
    // Selected Unix adapters construct frames; unsupported targets still compile
    // the neutral contract for downstream applications.
    #[allow(dead_code)]
    pub(crate) fn new(pixels: &'a mut [u32], width: u32, height: u32, scale_factor: f64) -> Self {
        Self {
            pixels,
            width,
            height,
            scale_factor,
        }
    }

    pub fn pixels(&self) -> &[u32] {
        self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        self.pixels
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn scale_factor(&self) -> f64 {
        self.scale_factor
    }
}

impl fmt::Debug for XrgbPixelFrame<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("XrgbPixelFrame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("scale_factor", &self.scale_factor)
            .field("pixel_count", &self.pixels.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PixelWindowDirective {
    Continue,
    Wait,
    WaitUntil(Instant),
    Exit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PixelWindowError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl PixelWindowError {
    pub fn unsupported(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }

    pub fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code: Cow::Borrowed(code),
            message: message.to_string(),
        }
    }
}

impl fmt::Display for PixelWindowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "pixel window unsupported: {reason}"),
            Self::Failed { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for PixelWindowError {}

pub(crate) trait PixelWindowBackend {
    fn request_redraw(&self);

    fn present_stats(&self) -> PixelPresentStats {
        PixelPresentStats::default()
    }

    fn last_present(&self) -> Option<PixelPresentReceipt> {
        None
    }

    /// Requests a redraw for a physical-pixel region. Backends without a
    /// partial-present contract conservatively fall back to a full redraw.
    fn request_redraw_rect(&self, rect: PixelRect) {
        if !rect.is_empty() {
            self.request_redraw();
        }
    }

    fn metrics(&self) -> Result<PixelWindowMetrics, PixelWindowError>;
    fn semantic_flags(&self) -> WindowSemanticFlags;
    fn set_minimized(&self, minimized: bool);
    fn set_maximized(&self, maximized: bool);
    fn set_visible(&self, visible: bool);
    fn focus(&self);
    fn set_title(&self, title: &str);
    fn request_logical_inner_size(&self, size: LogicalSize) -> Result<(), PixelWindowError>;
    fn set_pointer_capture(&self, captured: bool) -> Result<(), PixelWindowError>;
    fn set_pointer_cursor(&self, cursor: PixelPointerCursor) -> Result<(), PixelWindowError>;
    fn set_ime_allowed(&self, allowed: bool);
    fn set_ime_cursor_area(&self, area: LogicalRect) -> Result<(), PixelWindowError>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PixelPointerCursor {
    #[default]
    Arrow,
    ResizeHorizontal,
}

#[derive(Clone)]
pub struct PixelWindow {
    backend: Rc<dyn PixelWindowBackend>,
    waker: WindowWaker,
}

impl PixelWindow {
    // Selected Unix adapters construct windows; unsupported targets still compile
    // the neutral contract for downstream applications.
    #[allow(dead_code)]
    pub(crate) fn new(backend: Rc<dyn PixelWindowBackend>, waker: WindowWaker) -> Self {
        Self { backend, waker }
    }

    pub fn waker(&self) -> WindowWaker {
        self.waker.clone()
    }

    pub fn request_redraw(&self) {
        self.backend.request_redraw();
    }

    pub fn request_redraw_rect(&self, rect: PixelRect) {
        self.backend.request_redraw_rect(rect);
    }

    pub fn present_stats(&self) -> PixelPresentStats {
        self.backend.present_stats()
    }

    pub fn last_present(&self) -> Option<PixelPresentReceipt> {
        self.backend.last_present()
    }

    pub fn metrics(&self) -> Result<PixelWindowMetrics, PixelWindowError> {
        self.backend.metrics()
    }

    pub fn set_pointer_cursor(&self, cursor: PixelPointerCursor) -> Result<(), PixelWindowError> {
        self.backend.set_pointer_cursor(cursor)
    }

    pub fn scale_factor(&self) -> Result<f64, PixelWindowError> {
        self.metrics().map(|metrics| metrics.scale_factor)
    }

    pub fn semantic_flags(&self) -> WindowSemanticFlags {
        self.backend.semantic_flags()
    }

    pub fn minimized(&self) -> bool {
        self.semantic_flags().minimized
    }

    pub fn maximized(&self) -> bool {
        self.semantic_flags().maximized
    }

    pub fn visible(&self) -> bool {
        self.semantic_flags().visible
    }

    pub fn set_minimized(&self, minimized: bool) {
        self.backend.set_minimized(minimized);
    }

    pub fn set_maximized(&self, maximized: bool) {
        self.backend.set_maximized(maximized);
    }

    pub fn set_visible(&self, visible: bool) {
        self.backend.set_visible(visible);
    }

    pub fn focus(&self) {
        self.backend.focus();
    }

    pub fn set_title(&self, title: &str) {
        self.backend.set_title(title);
    }

    pub fn request_logical_inner_size(&self, size: LogicalSize) -> Result<(), PixelWindowError> {
        self.backend.request_logical_inner_size(size)
    }

    pub fn set_pointer_capture(&self, captured: bool) -> Result<(), PixelWindowError> {
        self.backend.set_pointer_capture(captured)
    }

    pub fn set_ime_allowed(&self, allowed: bool) {
        self.backend.set_ime_allowed(allowed);
    }

    pub fn set_ime_cursor_area(&self, area: LogicalRect) -> Result<(), PixelWindowError> {
        self.backend.set_ime_cursor_area(area)
    }
}

impl fmt::Debug for PixelWindow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PixelWindow")
            .field("metrics", &self.metrics())
            .field("semantic_flags", &self.semantic_flags())
            .finish_non_exhaustive()
    }
}

type WakeCallback = dyn Fn() -> Result<(), PixelWindowError> + Send + Sync;

#[derive(Clone)]
pub struct WindowWaker {
    callback: Arc<WakeCallback>,
}

impl WindowWaker {
    // Selected Unix adapters construct wakers; unsupported targets still compile
    // the neutral contract for downstream applications.
    #[allow(dead_code)]
    pub(crate) fn new(callback: Arc<WakeCallback>) -> Self {
        Self { callback }
    }

    pub fn wake(&self) -> Result<(), PixelWindowError> {
        (self.callback)()
    }
}

impl fmt::Debug for WindowWaker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowWaker")
            .finish_non_exhaustive()
    }
}

pub trait PixelWindowApplication: 'static {
    fn opened(&mut self, window: &PixelWindow) -> Result<PixelWindowDirective, PixelWindowError>;

    fn event(
        &mut self,
        window: &PixelWindow,
        event: PixelWindowEvent,
    ) -> Result<PixelWindowDirective, PixelWindowError>;

    fn render(
        &mut self,
        window: &PixelWindow,
        frame: &mut XrgbPixelFrame<'_>,
    ) -> Result<PixelWindowDirective, PixelWindowError>;

    fn about_to_wait(
        &mut self,
        window: &PixelWindow,
        now: Instant,
    ) -> Result<PixelWindowDirective, PixelWindowError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_and_drawable_metrics_are_platform_neutral() {
        let options = PixelWindowOptions::new("test", LogicalSize::new(960.0, 600.0))
            .with_no_activate(true)
            .with_ime_allowed(true);
        assert!(options.initial_logical_size.is_valid());
        assert!(options.no_activate);
        assert!(options.ime_allowed);
        assert!(
            PixelWindowMetrics {
                logical_size: options.initial_logical_size,
                physical_width: 1920,
                physical_height: 1200,
                scale_factor: 2.0,
            }
            .is_drawable()
        );
    }

    #[test]
    fn zero_sized_metrics_are_not_drawable() {
        assert!(
            !PixelWindowMetrics {
                logical_size: LogicalSize::new(0.0, 0.0),
                physical_width: 0,
                physical_height: 0,
                scale_factor: 1.0,
            }
            .is_drawable()
        );
    }

    #[test]
    fn typed_failure_codes_are_stable() {
        let error = PixelWindowError::failed("pixel_window_surface_present_failed", "lost");
        assert_eq!(
            error.to_string(),
            "pixel_window_surface_present_failed: lost"
        );
    }

    #[test]
    fn physical_pixel_rect_is_half_open_and_clips_safely() {
        let rect = PixelRect::new(2, 3, 12, 14);
        assert_eq!(rect.width(), 10);
        assert_eq!(rect.height(), 11);
        assert!(!rect.is_empty());
        assert_eq!(rect.clip(8, 9), PixelRect::new(2, 3, 8, 9));
        assert_eq!(
            PixelRect::new(8, 9, 2, 3).clip(20, 20),
            PixelRect::new(8, 9, 8, 9)
        );
    }

    #[test]
    fn zero_area_pixel_rect_is_safe() {
        let rect = PixelRect::new(4, 4, 4, 10);
        assert!(rect.is_empty());
        assert_eq!(rect.clip(0, 0), rect);
    }
}
