//! PTY-neutral scalar types shared by native session adapters.

use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PtyError {
    Unsupported {
        operation: &'static str,
        reason: String,
    },
    Failed {
        operation: &'static str,
        code: &'static str,
        message: String,
    },
}

impl PtyError {
    pub fn unsupported(operation: &'static str, reason: impl fmt::Display) -> Self {
        Self::Unsupported {
            operation,
            reason: reason.to_string(),
        }
    }

    pub fn failed(operation: &'static str, code: &'static str, error: impl fmt::Display) -> Self {
        Self::Failed {
            operation,
            code,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for PtyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { operation, reason } => {
                write!(formatter, "PTY {operation} unsupported: {reason}")
            }
            Self::Failed {
                operation,
                code,
                message,
            } => write!(formatter, "PTY {operation} failed ({code}): {message}"),
        }
    }
}

impl std::error::Error for PtyError {}

pub type PtyResult<T> = Result<T, PtyError>;

/// A platform-neutral terminal key whose native console semantics cannot
/// always be represented faithfully as bytes written to a PTY stream.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum NativeTerminalKey {
    /// The terminal cursor-up key.
    Up,
    /// The terminal cursor-down key.
    Down,
}

/// The input protocol currently owned by the program attached to a PTY.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum NativeInputOwnership {
    /// The operating system's line-oriented console editor owns input.
    Cooked,
    /// The program consumes virtual-terminal input sequences.
    RawVt,
    /// The program consumes native console input records.
    RawNative,
}

#[allow(dead_code)] // Consumed by the Unix PTY adapter only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

#[allow(dead_code)] // Consumed by the Unix PTY adapter only.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ProcessId(u32);

#[allow(dead_code)] // Consumed by the Unix PTY adapter only.
impl ProcessId {
    pub fn new(raw: u32) -> Result<Self, InvalidProcessId> {
        if raw == 0 {
            return Err(InvalidProcessId(raw));
        }
        Ok(Self(raw))
    }
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[allow(dead_code)] // Consumed by the Unix PTY adapter only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidProcessId(u32);

impl std::fmt::Display for InvalidProcessId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid process id: {}", self.0)
    }
}
impl std::error::Error for InvalidProcessId {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_failures_distinguish_unsupported_and_failed() {
        let unsupported = PtyError::unsupported("spawn", "backend unavailable");
        let failed = PtyError::failed("resize", "pty_resize_failed", "invalid dimensions");

        assert!(matches!(unsupported, PtyError::Unsupported { .. }));
        assert!(unsupported.to_string().contains("spawn unsupported"));
        assert!(matches!(failed, PtyError::Failed { .. }));
        assert!(failed.to_string().contains("pty_resize_failed"));
    }

    #[test]
    fn native_terminal_keys_are_platform_neutral_values() {
        assert_ne!(NativeTerminalKey::Up, NativeTerminalKey::Down);
        assert_eq!(NativeTerminalKey::Up, NativeTerminalKey::Up);
    }

    #[test]
    fn native_input_ownership_has_no_unknown_state() {
        let states = [
            NativeInputOwnership::Cooked,
            NativeInputOwnership::RawVt,
            NativeInputOwnership::RawNative,
        ];

        assert_eq!(states.len(), 3);
        assert_ne!(states[0], states[1]);
        assert_ne!(states[1], states[2]);
    }
}
