//! Typed error envelope, structurally aligned with `agenterm_rh::RhError`
//! (see `crates/agenterm-rh/src/error.rs`). Same shape (parse vs. compile
//! failure classes), not the same variants — qjs has no subset/transpile
//! phase, so it only needs a subset of rh's error surface.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QjsError {
    /// Source failed to parse/compile in QuickJS (syntax error).
    Parse(String),
    /// Source parsed but failed some other check/host-side step (I/O,
    /// manifest shape, project-root resolution, ...).
    Check(String),
}

impl fmt::Display for QjsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "qjs parse error: {message}"),
            Self::Check(message) => write!(f, "qjs check error: {message}"),
        }
    }
}

impl std::error::Error for QjsError {}
