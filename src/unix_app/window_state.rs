//! Window semantic state and `ui-action` window control for the Unix embedded GUI.
//!
//! Actual winit window calls live behind [`UnixAppWindowHandle`]; `mod.rs` wires a
//! [`WinitWindowHandle`] adapter and calls [`apply_ui_action`] from the `ui-action` IPC path.
//!
//! ## Window-close modal mapping (implemented in `mod.rs`, not this module)
//!
//! | `ui-action`              | `WindowCloseChoice`     |
//! |--------------------------|-------------------------|
//! | `keep-server-running`    | `KeepServerRunning`     |
//! | `stop-server-and-exit`   | `StopServerAndExit`     |
//! | `cancel` (when pending)  | `Cancel`                |
//!
//! Snapshot fields emitted via [`window_snapshot_json`] match Win `wait-ui --window-state`:
//! `window.minimized`, `window.state` (`minimized` | `maximized` | `restored`), plus
//! `visible`, `detached`, `client_width`, `client_height`, and `title`.

use winit::{dpi::PhysicalSize, event::WindowEvent, window::Window};

use crate::commands::option_value;

pub const MIN_CLIENT_WIDTH: u32 = 320;
pub const MIN_CLIENT_HEIGHT: u32 = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowSemanticState {
    Minimized,
    Maximized,
    Restored,
}

impl WindowSemanticState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minimized => "minimized",
            Self::Maximized => "maximized",
            Self::Restored => "restored",
        }
    }

    pub const fn is_minimized(self) -> bool {
        matches!(self, Self::Minimized)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowStateTracker {
    semantic: WindowSemanticState,
}

impl WindowStateTracker {
    pub const fn new() -> Self {
        Self {
            semantic: WindowSemanticState::Restored,
        }
    }

    pub const fn semantic_state(self) -> WindowSemanticState {
        self.semantic
    }

    pub const fn snapshot_minimized(self) -> bool {
        self.semantic.is_minimized()
    }

    pub const fn snapshot_state_str(self) -> &'static str {
        self.semantic.as_str()
    }

    pub fn set_semantic_state(&mut self, state: WindowSemanticState) {
        self.semantic = state;
    }

    /// Reconcile tracked semantics with the live winit window when the platform exposes state.
    pub fn sync_from_window(&mut self, window: &Window) {
        if window.is_minimized().unwrap_or(false) {
            self.semantic = WindowSemanticState::Minimized;
        } else if window.is_maximized() {
            self.semantic = WindowSemanticState::Maximized;
        } else {
            self.semantic = WindowSemanticState::Restored;
        }
    }
}

/// Window manipulation surface for the Unix embedded GUI.
pub trait UnixAppWindowHandle {
    fn minimize_window(&self);
    fn maximize_window(&self);
    fn restore_window(&self);
    fn resize_client(&self, width: u32, height: u32);
    fn client_size(&self) -> (u32, u32);
    fn is_visible(&self) -> bool;
    fn window_title(&self) -> &str;
}

pub struct WinitWindowHandle<'a> {
    pub window: &'a Window,
    pub title: &'a str,
}

impl UnixAppWindowHandle for WinitWindowHandle<'_> {
    fn minimize_window(&self) {
        self.window.set_minimized(true);
    }

    fn maximize_window(&self) {
        self.window.set_maximized(true);
    }

    fn restore_window(&self) {
        self.window.set_minimized(false);
        self.window.set_maximized(false);
    }

    fn resize_client(&self, width: u32, height: u32) {
        let _ = self
            .window
            .request_inner_size(PhysicalSize::new(width, height));
    }

    fn client_size(&self) -> (u32, u32) {
        let size = self.window.inner_size();
        (size.width, size.height)
    }

    fn is_visible(&self) -> bool {
        self.window.is_visible().unwrap_or(true)
    }

    fn window_title(&self) -> &str {
        self.title
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowUiActionResult {
    Applied,
    NotHandled,
    Invalid(String),
}

/// Apply a window-control `ui-action` (`window-minimize`, `window-maximize`,
/// `window-restore`, `window-resize`). Other actions return [`WindowUiActionResult::NotHandled`].
pub fn apply_ui_action(
    action: &str,
    args: &[String],
    handle: &impl UnixAppWindowHandle,
    tracker: &mut WindowStateTracker,
) -> WindowUiActionResult {
    match action {
        "window-minimize" => {
            handle.minimize_window();
            tracker.set_semantic_state(WindowSemanticState::Minimized);
            WindowUiActionResult::Applied
        }
        "window-maximize" => {
            handle.maximize_window();
            tracker.set_semantic_state(WindowSemanticState::Maximized);
            WindowUiActionResult::Applied
        }
        "window-restore" => {
            handle.restore_window();
            tracker.set_semantic_state(WindowSemanticState::Restored);
            WindowUiActionResult::Applied
        }
        "window-resize" => {
            let width = option_value(args, "--width")
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|value| *value >= MIN_CLIENT_WIDTH);
            let height = option_value(args, "--height")
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|value| *value >= MIN_CLIENT_HEIGHT);
            match (width, height) {
                (Some(width), Some(height)) => {
                    handle.resize_client(width, height);
                    if tracker.semantic_state() != WindowSemanticState::Minimized {
                        tracker.set_semantic_state(WindowSemanticState::Restored);
                    }
                    WindowUiActionResult::Applied
                }
                (None, _) => WindowUiActionResult::Invalid(
                    "window-resize requires --width of at least 320".to_string(),
                ),
                (_, None) => WindowUiActionResult::Invalid(
                    "window-resize requires --height of at least 240".to_string(),
                ),
            }
        }
        _ => WindowUiActionResult::NotHandled,
    }
}

/// Keep semantic state aligned when the user resizes or changes window state outside IPC.
pub fn absorb_window_event(event: &WindowEvent, window: &Window, tracker: &mut WindowStateTracker) {
    match event {
        WindowEvent::Resized(_) | WindowEvent::Focused(_) => tracker.sync_from_window(window),
        _ => {}
    }
}

pub fn window_snapshot_json(
    handle: &impl UnixAppWindowHandle,
    tracker: &WindowStateTracker,
) -> serde_json::Value {
    let (client_width, client_height) = handle.client_size();
    crate::ui_snapshot::embedded_window_json_with_state(
        handle.window_title(),
        client_width,
        client_height,
        handle.is_visible(),
        tracker.snapshot_minimized(),
        tracker.snapshot_state_str(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockHandle {
        title: String,
        client_width: u32,
        client_height: u32,
        visible: bool,
        minimized: bool,
        maximized: bool,
    }

    impl UnixAppWindowHandle for MockHandle {
        fn minimize_window(&self) {
            // tracked externally in tests
        }

        fn maximize_window(&self) {
            // tracked externally in tests
        }

        fn restore_window(&self) {
            // tracked externally in tests
        }

        fn resize_client(&self, width: u32, height: u32) {
            let _ = (width, height);
        }

        fn client_size(&self) -> (u32, u32) {
            (self.client_width, self.client_height)
        }

        fn is_visible(&self) -> bool {
            self.visible
        }

        fn window_title(&self) -> &str {
            self.title.as_str()
        }
    }

    fn mock_handle() -> MockHandle {
        MockHandle {
            title: String::from("AgenTerm"),
            client_width: 960,
            client_height: 600,
            visible: true,
            minimized: false,
            maximized: false,
        }
    }

    #[test]
    fn semantic_state_strings_match_wait_ui() {
        assert_eq!(WindowSemanticState::Minimized.as_str(), "minimized");
        assert_eq!(WindowSemanticState::Maximized.as_str(), "maximized");
        assert_eq!(WindowSemanticState::Restored.as_str(), "restored");
    }

    #[test]
    fn apply_ui_action_updates_tracker() {
        let handle = mock_handle();
        let mut tracker = WindowStateTracker::new();
        assert_eq!(
            apply_ui_action("window-maximize", &[], &handle, &mut tracker),
            WindowUiActionResult::Applied
        );
        assert_eq!(tracker.semantic_state(), WindowSemanticState::Maximized);
        assert_eq!(
            apply_ui_action("window-minimize", &[], &handle, &mut tracker),
            WindowUiActionResult::Applied
        );
        assert_eq!(tracker.semantic_state(), WindowSemanticState::Minimized);
        assert_eq!(
            apply_ui_action("window-restore", &[], &handle, &mut tracker),
            WindowUiActionResult::Applied
        );
        assert_eq!(tracker.semantic_state(), WindowSemanticState::Restored);
    }

    #[test]
    fn window_resize_requires_minimum_client_size() {
        let handle = mock_handle();
        let mut tracker = WindowStateTracker::new();
        let args = vec![
            String::from("ui-action"),
            String::from("window-resize"),
            String::from("--width"),
            String::from("100"),
            String::from("--height"),
            String::from("600"),
        ];
        let result = apply_ui_action("window-resize", &args, &handle, &mut tracker);
        assert_eq!(
            result,
            WindowUiActionResult::Invalid(
                "window-resize requires --width of at least 320".to_string()
            )
        );

        let args = vec![
            String::from("ui-action"),
            String::from("window-resize"),
            String::from("--width"),
            String::from("640"),
            String::from("--height"),
            String::from("100"),
        ];
        let result = apply_ui_action("window-resize", &args, &handle, &mut tracker);
        assert_eq!(
            result,
            WindowUiActionResult::Invalid(
                "window-resize requires --height of at least 240".to_string()
            )
        );

        let args = vec![
            String::from("ui-action"),
            String::from("window-resize"),
            String::from("--width"),
            String::from("640"),
            String::from("--height"),
            String::from("480"),
        ];
        assert_eq!(
            apply_ui_action("window-resize", &args, &handle, &mut tracker),
            WindowUiActionResult::Applied
        );
        assert_eq!(tracker.semantic_state(), WindowSemanticState::Restored);
    }

    #[test]
    fn window_snapshot_json_matches_win_window_block() {
        let handle = mock_handle();
        let mut tracker = WindowStateTracker::new();
        tracker.set_semantic_state(WindowSemanticState::Maximized);
        let json = window_snapshot_json(&handle, &tracker);
        assert_eq!(json["title"], "AgenTerm");
        assert_eq!(json["client_width"], 960);
        assert_eq!(json["client_height"], 600);
        assert_eq!(json["visible"], true);
        assert_eq!(json["detached"], false);
        assert_eq!(json["minimized"], false);
        assert_eq!(json["state"], "maximized");
    }

    #[test]
    fn unknown_action_is_not_handled() {
        let handle = mock_handle();
        let mut tracker = WindowStateTracker::new();
        assert_eq!(
            apply_ui_action("close-window", &[], &handle, &mut tracker),
            WindowUiActionResult::NotHandled
        );
    }
}
