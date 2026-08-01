//! Compatibility projection for the platform PTY facade.

pub(crate) use crate::platform::services::pty::{
    ChildCommand, NativeInputOwnership, NativeTerminalKey, PtyChild, PtyError, PtyMaster,
    PtyResult, TerminalSize,
};
