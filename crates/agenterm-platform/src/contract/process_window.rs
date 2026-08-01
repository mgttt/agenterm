//! Platform-neutral process-window observation and automation contract.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessWindowFacts {
    pub supported: bool,
    pub present: bool,
    pub window_id: i64,
    pub title: String,
    pub foreground_window_id: i64,
    pub is_foreground: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessWindowRect {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProcessWindowKey {
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
#[non_exhaustive]
pub enum ProcessWindowPointerAction {
    Click,
    Down,
    Move,
    MoveHeld,
    Up,
    CaptureChanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessWindowMessage {
    pub message: u32,
    pub wparam: usize,
    pub lparam: isize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessWindowError {
    pub code: &'static str,
    pub message: &'static str,
    pub cause: Option<&'static str>,
}

impl ProcessWindowError {
    pub const fn new(
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

pub(crate) type ScriptWindowFacts = ProcessWindowFacts;
pub(crate) type ScriptWindowRect = ProcessWindowRect;
pub(crate) type ScriptWindowKey = ProcessWindowKey;
pub(crate) type ScriptWindowPointerAction = ProcessWindowPointerAction;
pub(crate) type ScriptWindowMessage = ProcessWindowMessage;
pub(crate) type ScriptWindowError = ProcessWindowError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_error_keeps_stable_receipt_fields() {
        assert_eq!(
            ProcessWindowError::new("process_window_unsupported", "unavailable", Some("unsupported")),
            ProcessWindowError {
                code: "process_window_unsupported",
                message: "unavailable",
                cause: Some("unsupported"),
            }
        );
    }
}
