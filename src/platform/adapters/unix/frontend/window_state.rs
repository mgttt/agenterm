//! Window semantic state and `ui-action` control for the Unix frontend adapter.
//!
//! Native window calls live behind [`UnixAppWindowHandle`]; `mod.rs` wires the
//! selected platform window and calls [`apply_ui_action`] from the `ui-action` IPC path.
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

use crate::commands::option_value;
use crate::platform::window::ClientSize;
pub(crate) use crate::platform::window::WindowSemanticState;

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

    /// Reconcile tracked semantics with selected-adapter native state facts.
    pub fn sync_from_native_flags(&mut self, minimized: bool, maximized: bool) {
        self.semantic = WindowSemanticState::from_native_flags(minimized, maximized);
    }
}

/// Window manipulation surface for the Unix embedded GUI.
pub trait UnixAppWindowHandle {
    fn focus_window(&self);
    fn minimize_window(&self);
    fn maximize_window(&self);
    fn restore_window(&self);
    fn resize_client(&self, width: u32, height: u32) -> Result<(), String>;
    fn client_size(&self) -> (u32, u32);
    fn is_visible(&self) -> bool;
    fn window_title(&self) -> &str;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowUiActionResult {
    Applied,
    ActivationRequested,
    NotHandled,
    Invalid(String),
}

/// Apply a window-control `ui-action` (`window-activate`, `window-minimize`,
/// `window-maximize`, `window-restore`, `window-resize`). Other actions return
/// [`WindowUiActionResult::NotHandled`].
pub fn apply_ui_action(
    action: &str,
    args: &[String],
    handle: &impl UnixAppWindowHandle,
    tracker: &mut WindowStateTracker,
) -> WindowUiActionResult {
    match action {
        "window-activate" => {
            handle.focus_window();
            WindowUiActionResult::ActivationRequested
        }
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
            match ClientSize::parse(
                option_value(args, "--width"),
                option_value(args, "--height"),
            ) {
                Ok(size) => {
                    if let Err(error) = handle.resize_client(size.width, size.height) {
                        return WindowUiActionResult::Invalid(error);
                    }
                    if tracker.semantic_state() != WindowSemanticState::Minimized {
                        tracker.set_semantic_state(WindowSemanticState::Restored);
                    }
                    WindowUiActionResult::Applied
                }
                Err(error) => {
                    WindowUiActionResult::Invalid(format!("{} ({})", error.message(), error.code()))
                }
            }
        }
        _ => WindowUiActionResult::NotHandled,
    }
}

pub fn window_snapshot_json(
    handle: &impl UnixAppWindowHandle,
    tracker: &WindowStateTracker,
) -> serde_json::Value {
    let (client_width, client_height) = handle.client_size();
    super::ui_snapshot::embedded_window_json_with_state(
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
        resize_error: Option<String>,
    }

    impl UnixAppWindowHandle for MockHandle {
        fn focus_window(&self) {
            // invocation is covered by the dedicated recording handle below
        }

        fn minimize_window(&self) {
            // tracked externally in tests
        }

        fn maximize_window(&self) {
            // tracked externally in tests
        }

        fn restore_window(&self) {
            // tracked externally in tests
        }

        fn resize_client(&self, width: u32, height: u32) -> Result<(), String> {
            let _ = (width, height);
            self.resize_error.clone().map_or(Ok(()), Err)
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
            resize_error: None,
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
    fn window_activate_calls_the_live_handle_without_changing_semantic_state() {
        use std::cell::Cell;

        struct RecordingHandle<'a> {
            focused: &'a Cell<bool>,
        }

        impl UnixAppWindowHandle for RecordingHandle<'_> {
            fn focus_window(&self) {
                self.focused.set(true);
            }

            fn minimize_window(&self) {}
            fn maximize_window(&self) {}
            fn restore_window(&self) {}

            fn resize_client(&self, _width: u32, _height: u32) -> Result<(), String> {
                Ok(())
            }

            fn client_size(&self) -> (u32, u32) {
                (960, 600)
            }

            fn is_visible(&self) -> bool {
                true
            }

            fn window_title(&self) -> &str {
                "AgenTerm"
            }
        }

        let focused = Cell::new(false);
        let handle = RecordingHandle { focused: &focused };
        let mut tracker = WindowStateTracker::new();
        tracker.set_semantic_state(WindowSemanticState::Maximized);

        assert_eq!(
            apply_ui_action("window-activate", &[], &handle, &mut tracker),
            WindowUiActionResult::ActivationRequested
        );
        assert!(focused.get());
        assert_eq!(tracker.semantic_state(), WindowSemanticState::Maximized);
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
                "window-resize requires --width of at least 320 (window_width_invalid)".to_string()
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
                "window-resize requires --height of at least 240 (window_height_invalid)"
                    .to_string()
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
    fn window_resize_reports_adapter_failure_without_updating_state() {
        let mut handle = mock_handle();
        handle.resize_error = Some("pixel_window_resize_failed: denied".to_owned());
        let mut tracker = WindowStateTracker::new();
        tracker.set_semantic_state(WindowSemanticState::Maximized);
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
            WindowUiActionResult::Invalid("pixel_window_resize_failed: denied".to_owned())
        );
        assert_eq!(tracker.semantic_state(), WindowSemanticState::Maximized);
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
