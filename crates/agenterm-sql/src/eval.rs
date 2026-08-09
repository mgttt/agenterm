//! Execution — deliberately NOT implemented. See `lib.rs`'s module doc for
//! the full "open design question" writeup; this file is the one place that
//! question turns into actual, fail-closed behavior.
//!
//! Every other engine (`agenterm_rh::eval_entry`/`agenterm_lua::LuaEngine::eval`/
//! `agenterm_qjs::eval_entry`) has an obvious answer to "what does running
//! this source even mean": load/compile it, call its `entry()`, done. SQL
//! has no such obvious answer — a `.sql` file is not a program with an
//! entry point, it's a batch of statements that need *something* to run
//! them against: an embedded engine (e.g. an in-process SQLite/DataFusion),
//! a connection to an external PostgreSQL-compatible database, or the
//! host's own state exposed as virtual tables (fleet inventory, tab state,
//! etc. queried *as SQL* — the most "agenterm-native" option, and the
//! furthest from a solved problem). None of those has been decided, and
//! this crate does not guess: [`eval_entry`] always returns
//! [`SqlError::Check`] rather than silently picking one.
//!
//! `check()` (see `check.rs`) still works today — parsing doesn't need an
//! execution target, only "run" does.

use crate::error::SqlError;

/// `entry()`/`eval`-analog for the other three engines' `eval_entry`.
/// Always fails: there is no decided execution target for sql source yet
/// (embedded engine vs. external DB connection vs. host-state-as-virtual-tables
/// — see this module's doc and `plan/design-script-engine-trait.md` §2.6).
/// `label` is accepted (not `_label`) so the error message can name the
/// source that was rejected, matching the other engines' error ergonomics
/// even though this path can never succeed today.
pub fn eval_entry(_source: &str, label: &str) -> Result<(), SqlError> {
    Err(SqlError::Check(format!(
        "sql_eval_not_implemented: {label}: no execution target is decided for the sql backend \
         yet (embedded engine vs. external DB connection vs. host-state-as-virtual-tables — see \
         crates/agenterm-sql/src/lib.rs's module doc and \
         plan/design-script-engine-trait.md \u{a7}2.6); this is fail-closed by design, not a bug"
    )))
}

#[cfg(test)]
mod tests {
    use super::eval_entry;
    use crate::error::SqlError;

    #[test]
    fn eval_entry_is_fail_closed_not_implemented() {
        let error = eval_entry("SELECT 1;", "ok.sql").expect_err("eval must not be implemented");
        assert!(matches!(error, SqlError::Check(_)));
        assert!(error.to_string().contains("sql_eval_not_implemented"));
    }

    #[test]
    fn eval_entry_fails_even_for_source_that_would_check_clean() {
        // Not a check-vs-eval confusion test: proves eval_entry() doesn't
        // silently delegate to check() and call that "success" — it must
        // fail regardless of whether the source is syntactically valid.
        let error =
            eval_entry("CREATE TABLE widgets (id INTEGER);", "create.sql").expect_err("eval stub");
        assert!(error.to_string().contains("no execution target is decided"));
    }
}
