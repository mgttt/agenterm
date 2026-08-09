//! Parse-only validation — the one genuinely real piece of this scaffold
//! (see `lib.rs`'s module doc for what's real vs. placeholder in this
//! crate overall). "ok" means `sqlparser` can tokenize and parse `source`
//! into a `Vec<Statement>` AST; nothing is executed, planned, or resolved
//! against a schema — there is no schema, no catalog, no runtime here at
//! all (see `eval.rs`).
//!
//! ## Dialect choice
//!
//! `sqlparser` ships several `Dialect` impls, including `PostgreSqlDialect`
//! and `AnsiDialect` (SQL-92-flavored). This module parses with
//! [`PostgreSqlDialect`] rather than `AnsiDialect`, deliberately:
//!
//! - The task brief's benchmark targets are **SQL-92 and PostgreSQL**, and
//!   PostgreSQL's own grammar is a superset of SQL-92 for the constructs
//!   `sqlparser` actually accepts (see `sqlparser`'s own dialect docs: PG
//!   dialect is one of the most permissive/complete dialects it ships).
//!   Parsing with the *more* permissive superset dialect means valid SQL-92
//!   source also parses clean here — the reverse isn't true (some
//!   PostgreSQL-only syntax would be rejected by `AnsiDialect`).
//! - This is a **pragmatic, single-dialect choice for a placeholder crate**,
//!   not a claim that source is validated against the actual SQL-92
//!   standard's constraint set. A future real implementation could run both
//!   dialects and report which one(s) a given source parses under, or add a
//!   `--dialect` flag; neither exists yet — tracked, not silently assumed
//!   done. See `plan/design-script-engine-trait.md` §2.6 for the SSOT on
//!   this crate's scope.
//!
//! Multi-statement sources (statements separated by `;`) parse cleanly:
//! `Parser::parse_sql` already splits on top-level `;` and parses each
//! statement, returning one `Vec<Statement>` for the whole source (verified
//! in the tests below, not assumed).

use sqlparser::ast::Statement;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use crate::error::SqlError;

/// Parse `source` into its `;`-separated statements without executing or
/// planning anything, returning the AST `Vec<Statement>` sqlparser produces.
/// Shared by [`check`] (which discards the AST, keeping only "did it
/// parse?") and `eval::execute_entry` (which re-serializes each `Statement`
/// via its `Display` impl and runs it against SQLite) — one parse call, one
/// dialect choice, per design doc `plan/design-sql-execution-target.md` §5
/// M1's explicit "复用，不重新分词" instruction.
pub(crate) fn parse_statements(source: &str, label: &str) -> Result<Vec<Statement>, SqlError> {
    let dialect = PostgreSqlDialect {};
    Parser::parse_sql(&dialect, source).map_err(|err| SqlError::Parse(format!("{label}: {err}")))
}

/// Parse `source` as one or more `;`-separated SQL statements without
/// executing or planning anything. `label` is accepted for symmetry with
/// `agenterm_qjs::check`/`agenterm_rh::check`'s signatures (and so future
/// error messages can carry a filename) but is not currently interpolated
/// into `sqlparser`'s own error messages — `sqlparser`'s `ParserError`
/// carries its own line/column location, not a caller-supplied label.
pub fn check(source: &str, label: &str) -> Result<(), SqlError> {
    parse_statements(source, label).map(|_statements| ())
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::error::SqlError;

    #[test]
    fn accepts_a_valid_select() {
        check("SELECT 1;", "ok.sql").expect("valid SELECT should check clean");
    }

    #[test]
    fn accepts_a_valid_insert() {
        check(
            "INSERT INTO widgets (id, name) VALUES (1, 'gizmo');",
            "insert.sql",
        )
        .expect("valid INSERT should check clean");
    }

    #[test]
    fn accepts_a_valid_create_table() {
        check(
            "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
            "create.sql",
        )
        .expect("valid CREATE TABLE should check clean");
    }

    #[test]
    fn accepts_multiple_statements_separated_by_semicolons() {
        check(
            "CREATE TABLE widgets (id INTEGER);\n\
             INSERT INTO widgets (id) VALUES (1);\n\
             SELECT * FROM widgets;",
            "multi.sql",
        )
        .expect("a multi-statement source should check clean");
    }

    #[test]
    fn rejects_syntax_errors() {
        let error = check("SELEC 1 FORM;", "bad.sql").expect_err("syntax error");
        assert!(matches!(error, SqlError::Parse(_)));
    }

    #[test]
    fn rejects_an_unterminated_statement() {
        let error =
            check("CREATE TABLE widgets (id INTEGER", "unterminated.sql").expect_err("truncated");
        assert!(matches!(error, SqlError::Parse(_)));
    }

    #[test]
    fn a_syntax_error_in_the_second_statement_of_a_multi_statement_source_is_caught() {
        let error = check(
            "SELECT 1; SELEC 2 FORM;",
            "second_bad.sql",
        )
        .expect_err("syntax error in second statement should fail the whole check");
        assert!(matches!(error, SqlError::Parse(_)));
    }
}
