//! Platform-neutral console line-editor contract.

use std::borrow::Cow;

/// A single key press normalized across Windows and Unix consoles.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ConsoleKey {
    /// A printable character produced by the key press.
    Char(char),
    /// Enter / Return.
    Enter,
    /// Backspace (left-delete).
    Backspace,
    /// Delete (forward-delete under the cursor).
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// End-of-input (Windows Ctrl+Z / Unix Ctrl+D on an empty line).
    Eof,
    /// A key the adapter recognized without an editor mapping.
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConsoleLineEditorError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        native_code: Option<i64>,
        message: String,
    },
}

impl ConsoleLineEditorError {
    pub(crate) fn unsupported(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }

    pub(crate) fn failed(
        code: impl Into<Cow<'static, str>>,
        native_code: Option<i64>,
        message: impl Into<String>,
    ) -> Self {
        Self::Failed {
            code: code.into(),
            native_code,
            message: message.into(),
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Unsupported { reason } => Some(reason),
            Self::Failed { .. } => None,
        }
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Unsupported { .. } => None,
            Self::Failed { code, .. } => Some(code),
        }
    }

    pub const fn native_code(&self) -> Option<i64> {
        match self {
            Self::Unsupported { .. } => None,
            Self::Failed { native_code, .. } => *native_code,
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Unsupported { .. } => None,
            Self::Failed { message, .. } => Some(message),
        }
    }
}

impl std::fmt::Display for ConsoleLineEditorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { reason } => {
                write!(formatter, "console line editor unsupported: {reason}")
            }
            Self::Failed {
                code,
                native_code: Some(native_code),
                message,
            } => write!(
                formatter,
                "console line editor failed ({code}, native {native_code}): {message}"
            ),
            Self::Failed {
                code,
                native_code: None,
                message,
            } => write!(formatter, "console line editor failed ({code}): {message}"),
        }
    }
}

impl std::error::Error for ConsoleLineEditorError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_preserve_typed_status_and_native_evidence() {
        let unsupported = ConsoleLineEditorError::unsupported("host-unavailable");
        assert_eq!(unsupported.reason(), Some("host-unavailable"));
        assert_eq!(unsupported.code(), None);

        let failed = ConsoleLineEditorError::failed("enter", Some(5), "native failure");
        assert_eq!(failed.code(), Some("enter"));
        assert_eq!(failed.native_code(), Some(5));
        assert_eq!(failed.message(), Some("native failure"));
    }
}
