//! Typed error envelope, structurally aligned with `agenterm_qjs::QjsError`
//! (see `crates/agenterm-qjs/src/error.rs`) and, further back,
//! `agenterm_rh::RhError` — same shape (parse vs. other-check failure
//! classes), not the same variants. As of M1 (see `lib.rs`'s module doc and
//! `eval.rs`), sql DOES have a real execute/runtime phase
//! (`eval::execute_entry`), but it still fits this same two-class-plus-usage
//! shape: a `sqlparser` parse failure is [`SqlError::Parse`], and an
//! execution-time failure (SQLite rejects the re-serialized statement, a
//! runtime error, a budget exceeded) is [`SqlError::Check`] — both
//! script-level, both exit `1`. No new variant was needed to land real
//! execution.
//!
//! ## Exit-code classification, mirroring `agenterm_qjs::QjsError`'s (see
//! that file's doc for the full rationale, aligned 2026-08 with
//! `agenterm_script_common::check_many::CheckManyReport::exit_code()`'s
//! shared `exit_class` taxonomy): [`SqlError::Parse`]/[`SqlError::Check`]
//! are script-level (exit `1`); [`SqlError::Usage`] is
//! usage/configuration-level (exit `2`).

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlError {
    /// Source failed to parse as SQL (sqlparser tokenizer/parser error).
    /// Script-level — exit `1`.
    Parse(String),
    /// Source parsed (or wasn't reached) but failed some other
    /// script-level check/execution step. Script-level — exit `1`.
    Check(String),
    /// CLI/configuration problem: bad argv, an unknown verb, a missing or
    /// unreadable required flag value, an unreadable/malformed manifest,
    /// a path that can't be read. Usage-level — exit `2`.
    Usage(String),
}

impl fmt::Display for SqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "sql parse error: {message}"),
            Self::Check(message) => write!(f, "sql check error: {message}"),
            Self::Usage(message) => write!(f, "sql usage error: {message}"),
        }
    }
}

impl std::error::Error for SqlError {}
