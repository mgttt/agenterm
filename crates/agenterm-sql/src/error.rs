//! Typed error envelope, structurally aligned with `agenterm_qjs::QjsError`
//! (see `crates/agenterm-qjs/src/error.rs`) and, further back,
//! `agenterm_rh::RhError` — same shape (parse vs. other-check failure
//! classes), not the same variants. sql has no execute/runtime phase at all
//! yet (see `lib.rs`'s module doc and `eval.rs`), so this enum only needs to
//! cover the two things that exist today: parse failures and everything
//! else (I/O, manifest shape, CLI argv).

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlError {
    /// Source failed to parse as SQL (sqlparser tokenizer/parser error).
    Parse(String),
    /// Source parsed (or wasn't reached) but failed some other check/host-side
    /// step (I/O, manifest shape, CLI argv, project-root resolution, ...).
    Check(String),
}

impl fmt::Display for SqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "sql parse error: {message}"),
            Self::Check(message) => write!(f, "sql check error: {message}"),
        }
    }
}

impl std::error::Error for SqlError {}
