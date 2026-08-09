//! Typed error envelope, structurally aligned with `agenterm_rh::RhError`
//! (see `crates/agenterm-rh/src/error.rs`). Same shape (parse vs. compile
//! failure classes), not the same variants — qjs has no subset/transpile
//! phase, so it only needs a subset of rh's error surface.
//!
//! ## Exit-code classification (script vs. usage), aligned 2026-08 with
//! `agenterm_script_common::check_many::CheckManyReport::exit_code()`'s
//! shared `exit_class` taxonomy (`"script"` → 1, `"configuration"` → 2, ...
//! see that module's doc): [`QjsError::Parse`] and [`QjsError::Check`] are
//! both **script-level** (the failure's root cause is the script/pack
//! content — a syntax error, a missing `entry()`, a thrown exception, a
//! tampered pack) and map to exit `1`. [`QjsError::Usage`] is
//! **usage/configuration-level** (bad argv, an unreadable/malformed
//! manifest or pack directory, a `--project-root`/entry-path that doesn't
//! resolve or doesn't confine, an unknown verb) and maps to exit `2` — see
//! `main.rs`'s `main()` for where that mapping is actually applied, and its
//! module doc for the call-site-by-call-site classification this variant
//! set enables.
//!
//! `Check` keeps its original name/shape (not renamed to e.g. `Script`)
//! deliberately: it's still exactly what its own doc always said — "source
//! parsed but failed some other check/host-side step" — the only thing
//! that changed is which exit code that maps to at the top level; call
//! sites that were quietly usage/configuration errors wearing a `Check`
//! label were moved to the new `Usage` variant instead of `Check`'s meaning
//! being redefined out from under its existing call sites.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QjsError {
    /// Source failed to parse/compile in QuickJS (syntax error).
    /// Script-level — exit `1`.
    Parse(String),
    /// Source parsed but failed some other script-level check/execution
    /// step (missing `entry()`, `entry()` threw, module link/eval
    /// failure, a tampered/corrupt pack's content, ...). Script-level —
    /// exit `1`.
    Check(String),
    /// CLI/configuration problem: bad argv, an unknown verb, a missing or
    /// unreadable required flag value, an unreadable/malformed manifest or
    /// pack directory, a `--project-root`/entry-path that fails to
    /// canonicalize or escapes its project root. Usage-level — exit `2`.
    Usage(String),
}

impl fmt::Display for QjsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "qjs parse error: {message}"),
            Self::Check(message) => write!(f, "qjs check error: {message}"),
            Self::Usage(message) => write!(f, "qjs usage error: {message}"),
        }
    }
}

impl std::error::Error for QjsError {}
