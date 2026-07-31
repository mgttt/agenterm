//! OS-neutral Script Runtime child-window facts, inputs, and typed failures.

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScriptWindowFacts {
    pub(crate) supported: bool,
    pub(crate) present: bool,
    pub(crate) window_id: i64,
    pub(crate) title: String,
    pub(crate) foreground_window_id: i64,
    pub(crate) is_foreground: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScriptWindowRect {
    pub(crate) left: i64,
    pub(crate) top: i64,
    pub(crate) right: i64,
    pub(crate) bottom: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScriptWindowKey {
    Backspace,
    Delete,
    Down,
    End,
    Enter,
    Escape,
    F2,
    Home,
    Left,
    Right,
    Tab,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScriptWindowPointerAction {
    Click,
    Down,
    Move,
    MoveHeld,
    Up,
    CaptureChanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScriptWindowMessage {
    pub(crate) message: u32,
    pub(crate) wparam: usize,
    pub(crate) lparam: isize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScriptWindowError {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
    pub(crate) cause: Option<&'static str>,
}

impl ScriptWindowError {
    pub(crate) const fn new(
        code: &'static str,
        message: &'static str,
        cause: Option<&'static str>,
    ) -> Self {
        Self {
            code,
            message,
            cause,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_error_keeps_public_receipt_fields() {
        assert_eq!(
            ScriptWindowError::new(
                "process_window_input_unsupported",
                "unavailable",
                Some("unsupported")
            ),
            ScriptWindowError {
                code: "process_window_input_unsupported",
                message: "unavailable",
                cause: Some("unsupported"),
            }
        );
    }
}
