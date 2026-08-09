//! Execution — REAL as of M1 (`plan/design-sql-execution-target.md` §5).
//! Was a deliberately fail-closed placeholder (`SqlError::Check` naming an
//! undecided execution target) before this pass; see git history / the
//! design doc §0 for the "why embedded SQLite (`rusqlite`, `bundled`)"
//! decision this module now implements.
//!
//! [`execute_entry`] opens a **private, in-process, ephemeral**
//! (`:memory:`) SQLite database per call (design doc §2.1's "打开一个新的
//! in-memory 数据库句柄 // 每次调用全新，不跨调用持久"), executes every
//! `;`-separated statement in `source` against it in order, and returns the
//! LAST result-producing statement's result set (or `None`, if no statement
//! in the script produced one) encoded as `Some(Value::Array(...))` — one
//! JSON object per row, keyed by column name.
//!
//! `check()` (see `check.rs`) still works exactly as before — parsing
//! doesn't need an execution target, only `execute_entry` does, and both
//! now share `check::parse_statements` for the actual `sqlparser` call.
//!
//! ## Known gap, deliberately NOT resolved in M1: `check()` parses
//! PostgreSQL dialect, `execute_entry` runs SQLite semantics (design doc §6
//! risk 1 / §5 M1's "不做" list doesn't include this — it's explicitly
//! flagged as a documented-not-fixed skew)
//!
//! `check()` and `execute_entry` both parse `source` with `sqlparser`'s
//! `PostgreSqlDialect` (via the shared `check::parse_statements`), but each
//! parsed `Statement` is re-serialized through its `Display` impl and run
//! through `rusqlite` — i.e. against **SQLite's** grammar/runtime, not
//! PostgreSQL's. So `check()` accepting a source as valid PostgreSQL-ish
//! syntax does NOT guarantee `execute_entry` can run it: PostgreSQL-only
//! constructs (`RETURNING`, certain cast syntax, dollar-quoting, ...) may
//! check clean and then fail here with a SQLite parse/runtime error. This
//! is real and tracked, not silently papered over — see
//! `execute_entry`'s own error messages, which name this exact skew.
//!
//! ## Value semantics decided in this pass (design doc §2.1/§4, "不定字节级
//! 细节" left these to the implementer)
//!
//! - `NULL` -> JSON `null`.
//! - `INTEGER` -> JSON number (via `serde_json::Number::from(i64)` — exact,
//!   no precision loss for any `i64`).
//! - `REAL` -> JSON number (via `Number::from_f64`); the vanishingly rare
//!   case of a SQLite `REAL` that's NaN/Infinite (which `serde_json::Number`
//!   cannot represent) falls back to JSON `null` rather than erroring the
//!   whole statement.
//! - `TEXT` -> JSON string (lossy UTF-8 decode — SQLite text columns are
//!   conventionally valid UTF-8, but this doesn't panic on the rare
//!   non-UTF-8 case).
//! - `BLOB` -> JSON string, hex-encoded with a `\x` prefix (e.g. `\x0102ff`)
//!   — the same convention PostgreSQL's own `bytea` hex output format uses,
//!   chosen because this crate's benchmark targets are SQL-92/PostgreSQL
//!   (`lib.rs`'s module doc), not because SQLite itself has this convention.
//!   No base64 dependency needed.
//! - A statement whose `column_count() == 0` (DDL/DML with no result set —
//!   `CREATE TABLE`, `INSERT`, ...) does not update the tracked "last
//!   result"; if EVERY statement in the script is like this (or the script
//!   is empty), [`ExecuteOutcome::value`] is `None`, not an error and not a
//!   fabricated rows-affected number — design doc §4's row for sql's
//!   `execute_missing_entry_fails_closed` analogue: "空 value 用 None 而不是
//!   报错", explicitly preferred over `0`/rows-affected.
//! - `stdout` is always `""`: SQL has no `print()`/`NOTICE` concept and the
//!   design doc doesn't ask M1 to invent one (§2.1: "初期可以留空").

use std::time::{Duration, Instant};

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use serde_json::{Map, Number, Value};

use crate::check::parse_statements;
use crate::error::SqlError;

/// M1's enforced budget subset (design doc §5 M1: "至少落地 wall_time_ms
/// 和 output_bytes/string_bytes 两项"). `agenterm-sql` cannot depend on the
/// root `agenterm` crate's `ScriptBudgets` (dependency direction is root ->
/// `agenterm-sql`, not the reverse — see `Cargo.toml`), so this is a small,
/// crate-local mirror; `src/script_engine.rs`'s `SqlEngineBackend::execute`
/// converts `ScriptBudgets` into this shape before calling
/// [`execute_entry`].
///
/// Deliberately NOT covering `ScriptBudgets`'s other ten fields
/// (`source_bytes`/`operations`/`call_depth`/`expression_depth`/
/// `broker_requests`/`broker_return_bytes`/`capture_bytes`/`event_items`/
/// `wait_time_ms`, and `string_bytes` is folded into `output_bytes` below,
/// not separately enforced) — deferred to M3 per the design doc's M1
/// scope, recorded here rather than silently ignored.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecuteBudgets {
    /// Wall-clock milliseconds before the running query is interrupted, via
    /// `rusqlite::Connection::progress_handler` (checked roughly every 1000
    /// SQLite VM instructions — see `execute_entry`). `0` = unenforced.
    pub wall_time_ms: u64,
    /// Max rows retained while collecting ANY single statement's result
    /// set, checked per-row during collection (not just after) so a
    /// runaway `SELECT` is interrupted mid-collection. Named after
    /// `ScriptBudgets::collection_items`. `0` = unenforced.
    pub collection_items: usize,
    /// Max UTF-8 byte length of the FINAL JSON-encoded `value`, checked
    /// once after encoding. Stands in for both `ScriptBudgets::output_bytes`
    /// and `ScriptBudgets::string_bytes` (sql has no separate host-visible
    /// "string" concept the way rh/lua/qjs scripts do — a result set's
    /// encoded size is the one output-shaped quantity that exists here).
    /// Does NOT bound intermediate (non-final) result sets that get
    /// discarded when a later statement also produces rows — a large
    /// early `SELECT` followed by a tiny final one is not caught by this
    /// check (design doc §6 risk 3, explicitly left for M3). `0` =
    /// unenforced.
    pub output_bytes: usize,
}

/// `execute()`'s outcome. Crate-local mirror of (not the same type as) the
/// root `agenterm::script_engine::ScriptInvocationResult` — same two-field
/// shape every `ScriptEngineBackend::execute` returns, so
/// `SqlEngineBackend::execute` can map 1:1 without any lossy conversion.
#[derive(Debug, Default, PartialEq)]
pub struct ExecuteOutcome {
    /// Always `""` in M1 — see this module's doc.
    pub stdout: String,
    /// `None` if no statement in `source` produced a result set; otherwise
    /// `Some(Value::Array(rows))`, the LAST result-producing statement's
    /// rows (one JSON object per row, keyed by column name) — see this
    /// module's doc for the full value-mapping table.
    pub value: Option<Value>,
}

/// Real `execute()` entry point (design doc §5 M1's acceptance criterion:
/// `SELECT 1;` returns `[{"1": 1}]`-shaped output, not a placeholder
/// error). Opens a private, in-process, ephemeral `:memory:` SQLite
/// connection, executes every `;`-separated statement in `source` against
/// it in order — so an earlier `CREATE TABLE`/`INSERT`'s side effects are
/// visible to a later statement in the SAME call (design doc §2.1) — and
/// returns the last result-producing statement's rows (see
/// [`ExecuteOutcome::value`]'s doc for the `None` case).
///
/// `label` is used only for error messages (matching `check`'s existing
/// convention, not interpolated into `sqlparser`'s own diagnostics).
/// `budgets` is `None` when the caller has no budget to enforce (matching
/// `ScriptInvocationOptions.budgets: Option<ScriptBudgets>`'s own
/// optionality) — in that case wall-time/collection-size/output-size are
/// all unenforced, same as passing an all-zero [`ExecuteBudgets`].
pub fn execute_entry(
    source: &str,
    label: &str,
    budgets: Option<&ExecuteBudgets>,
) -> Result<ExecuteOutcome, SqlError> {
    let statements = parse_statements(source, label)?;

    let conn = Connection::open_in_memory().map_err(|err| {
        SqlError::Check(format!(
            "{label}: failed to open the private in-memory sqlite database: {err}"
        ))
    })?;

    if let Some(budgets) = budgets
        && budgets.wall_time_ms > 0
    {
        let deadline = Instant::now() + Duration::from_millis(budgets.wall_time_ms);
        // num_ops=1000: checked roughly every 1000 SQLite VM
        // instructions, not every statement — cheap enough to not
        // meaningfully slow down normal-sized scripts, frequent enough
        // that a single runaway statement is still bounded close to
        // the requested budget rather than running far past it.
        conn.progress_handler(1000, Some(move || Instant::now() >= deadline))
            .map_err(|err| {
                SqlError::Check(format!(
                    "{label}: failed to install the wall_time_ms budget: {err}"
                ))
            })?;
    }

    let collection_items_budget = budgets.map(|b| b.collection_items).unwrap_or(0);
    let mut last_result: Option<Vec<Value>> = None;

    for statement in &statements {
        // Re-serialize the PostgreSQL-dialect-parsed AST node back to SQL
        // text and run THAT against SQLite — see this module's doc for the
        // known dialect skew this implies.
        let sql_text = statement.to_string();

        let mut stmt = conn.prepare(&sql_text).map_err(|err| {
            SqlError::Check(format!(
                "{label}: sqlite rejected a statement that parsed as valid PostgreSQL-dialect \
                 SQL (see this crate's eval.rs module doc for the known check()-vs-execute() \
                 dialect skew) — statement `{sql_text}`: {err}"
            ))
        })?;

        if stmt.column_count() == 0 {
            // DDL/DML with no result set (CREATE TABLE, INSERT, ...): run
            // it for its side effect, do not touch `last_result`.
            stmt.execute([]).map_err(|err| {
                SqlError::Check(format!(
                    "{label}: execute failed for statement `{sql_text}`: {err}"
                ))
            })?;
            continue;
        }

        let column_names: Vec<String> =
            stmt.column_names().into_iter().map(str::to_owned).collect();
        let mut rows = stmt.query([]).map_err(|err| {
            SqlError::Check(format!(
                "{label}: query failed for statement `{sql_text}`: {err}"
            ))
        })?;

        let mut collected: Vec<Value> = Vec::new();
        while let Some(row) = rows.next().map_err(|err| {
            SqlError::Check(format!(
                "{label}: row fetch failed for statement `{sql_text}`: {err}"
            ))
        })? {
            if collection_items_budget > 0 && collected.len() >= collection_items_budget {
                return Err(SqlError::Check(format!(
                    "{label}: collection_items budget ({collection_items_budget}) exceeded \
                     while reading statement `{sql_text}`"
                )));
            }
            let mut object = Map::with_capacity(column_names.len());
            for (index, name) in column_names.iter().enumerate() {
                let value_ref = row.get_ref(index).map_err(|err| {
                    SqlError::Check(format!(
                        "{label}: column read failed for `{name}` in statement `{sql_text}`: {err}"
                    ))
                })?;
                object.insert(name.clone(), value_ref_to_json(value_ref));
            }
            collected.push(Value::Object(object));
        }
        // Overwrite, not accumulate: design doc §2.1's "只有最后一条的结果
        // 被采用" — the LAST result-producing statement wins, regardless of
        // how many earlier statements also produced (now-discarded) rows.
        last_result = Some(collected);
    }

    let value = last_result.map(Value::Array);

    if let (Some(budgets), Some(value)) = (budgets, value.as_ref())
        && budgets.output_bytes > 0
    {
        let encoded_len = serde_json::to_string(value)
            .map(|encoded| encoded.len())
            .unwrap_or(0);
        if encoded_len > budgets.output_bytes {
            return Err(SqlError::Check(format!(
                "{label}: output_bytes budget ({}) exceeded: encoded result is {encoded_len} \
                 bytes",
                budgets.output_bytes
            )));
        }
    }

    Ok(ExecuteOutcome {
        stdout: String::new(),
        value,
    })
}

/// See this module's doc for the full NULL/INTEGER/REAL/TEXT/BLOB mapping
/// rationale — this function is just the match arms.
fn value_ref_to_json(value_ref: ValueRef<'_>) -> Value {
    match value_ref {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Number(Number::from(i)),
        ValueRef::Real(f) => Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(bytes) => Value::String(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => Value::String(format!("\\x{}", hex_encode(bytes))),
    }
}

/// Dependency-free hex encoder (avoids adding a `hex`/`base64` crate for
/// one call site) — see this module's doc for why `\x`-prefixed hex was
/// chosen over base64 for BLOB values.
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> ExecuteOutcome {
        execute_entry(source, "test.sql", None).expect("execute should succeed")
    }

    #[test]
    fn select_1_returns_one_row_one_column() {
        let outcome = run("SELECT 1;");
        assert_eq!(outcome.stdout, "");
        assert_eq!(outcome.value, Some(serde_json::json!([{"1": 1}])));
    }

    #[test]
    fn select_literal_expressions_map_types_correctly() {
        // NULL / INTEGER / REAL / TEXT in one row — pins the type-mapping
        // table in this module's doc.
        let outcome = run("SELECT NULL AS n, 42 AS i, 3.5 AS r, 'hi' AS t;");
        assert_eq!(
            outcome.value,
            Some(serde_json::json!([{"n": null, "i": 42, "r": 3.5, "t": "hi"}]))
        );
    }

    #[test]
    fn blob_values_are_hex_encoded_with_backslash_x_prefix() {
        let outcome = run("SELECT x'0102ff' AS b;");
        assert_eq!(outcome.value, Some(serde_json::json!([{"b": "\\x0102ff"}])));
    }

    #[test]
    fn multi_statement_create_insert_select_sees_earlier_side_effects() {
        let outcome = run("CREATE TABLE widgets (id INTEGER, name TEXT); \
             INSERT INTO widgets (id, name) VALUES (1, 'gizmo'); \
             SELECT id, name FROM widgets;");
        assert_eq!(
            outcome.value,
            Some(serde_json::json!([{"id": 1, "name": "gizmo"}]))
        );
    }

    #[test]
    fn multiple_select_statements_keep_only_the_last_ones_rows() {
        let outcome = run("SELECT 1; SELECT 2; SELECT 3;");
        assert_eq!(outcome.value, Some(serde_json::json!([{"3": 3}])));
    }

    #[test]
    fn multi_row_select_returns_all_rows_in_order() {
        let outcome = run("CREATE TABLE t (n INTEGER); \
             INSERT INTO t VALUES (1), (2), (3); \
             SELECT n FROM t ORDER BY n;");
        assert_eq!(
            outcome.value,
            Some(serde_json::json!([{"n": 1}, {"n": 2}, {"n": 3}]))
        );
    }

    #[test]
    fn empty_script_yields_none_value_not_an_error() {
        let outcome = run("");
        assert_eq!(outcome.value, None);
        assert_eq!(outcome.stdout, "");
    }

    #[test]
    fn statement_without_a_result_set_yields_none_value() {
        // Only DDL/DML, no SELECT anywhere — design doc §4's "空 value 用
        // None 而不是报错" decision, pinned here.
        let outcome = run("CREATE TABLE widgets (id INTEGER); INSERT INTO widgets VALUES (1);");
        assert_eq!(outcome.value, None);
    }

    #[test]
    fn a_trailing_ddl_statement_does_not_clear_an_earlier_selects_result() {
        // The LAST statement (CREATE TABLE) has no result set, but an
        // earlier one (SELECT 1) did — design doc §2.1 says "最后一条产出
        // 结果集的语句" (the last RESULT-PRODUCING statement), not "the
        // literal last statement in the script", so `value` should still
        // carry SELECT 1's row rather than being cleared to None by the
        // trailing DDL. Pins that `last_result` is overwrite-on-result-set,
        // not overwrite-on-every-statement.
        let outcome = run("SELECT 1; CREATE TABLE t (id INTEGER);");
        assert_eq!(outcome.value, Some(serde_json::json!([{"1": 1}])));
    }

    #[test]
    fn a_syntax_error_at_parse_time_is_a_check_error_not_a_panic() {
        let error = execute_entry("SELEC 1 FORM;", "bad.sql", None)
            .expect_err("broken sql should not execute");
        assert!(matches!(error, SqlError::Parse(_)));
    }

    #[test]
    fn a_runtime_error_at_execute_time_is_a_check_error_not_a_panic() {
        // Parses fine (it's syntactically valid SELECT-from-table syntax);
        // fails at SQLite execution time because the table doesn't exist —
        // this is the "execution-time syntax/semantic error is still
        // script-class" case the task brief calls out.
        let error = execute_entry("SELECT * FROM does_not_exist;", "missing_table.sql", None)
            .expect_err("querying a nonexistent table should error, not panic");
        assert!(matches!(error, SqlError::Check(_)));
        assert!(error.to_string().contains("does_not_exist"));
    }

    #[test]
    fn wall_time_ms_budget_of_zero_means_unenforced() {
        let budgets = ExecuteBudgets {
            wall_time_ms: 0,
            ..Default::default()
        };
        let outcome = execute_entry("SELECT 1;", "ok.sql", Some(&budgets))
            .expect("a zero wall_time_ms budget must not be treated as an immediate timeout");
        assert_eq!(outcome.value, Some(serde_json::json!([{"1": 1}])));
    }

    #[test]
    fn collection_items_budget_rejects_a_result_set_that_is_too_big() {
        let budgets = ExecuteBudgets {
            collection_items: 2,
            ..Default::default()
        };
        let error = execute_entry(
            "CREATE TABLE t (n INTEGER); \
             INSERT INTO t VALUES (1), (2), (3); \
             SELECT n FROM t;",
            "too_many_rows.sql",
            Some(&budgets),
        )
        .expect_err("a 3-row result set should exceed a collection_items budget of 2");
        assert!(error.to_string().contains("collection_items"));
    }

    #[test]
    fn output_bytes_budget_rejects_a_result_that_is_too_big() {
        let budgets = ExecuteBudgets {
            output_bytes: 4,
            ..Default::default()
        };
        let error = execute_entry(
            "SELECT 'this is a long string' AS s;",
            "too_big.sql",
            Some(&budgets),
        )
        .expect_err("an encoded result larger than the output_bytes budget should error");
        assert!(error.to_string().contains("output_bytes"));
    }

    #[test]
    fn wall_time_ms_budget_interrupts_a_long_running_query() {
        // An effectively-unbounded recursive CTE, given a 1ms budget: must
        // be interrupted rather than run to completion (or hang the test
        // suite). The exact error text SQLite/rusqlite surfaces for an
        // interrupted query isn't pinned here (that's an implementation
        // detail of the progress-handler interrupt path, not this crate's
        // contract) — what's pinned is "it errors, script-class, instead
        // of completing or panicking".
        let budgets = ExecuteBudgets {
            wall_time_ms: 1,
            ..Default::default()
        };
        let error = execute_entry(
            "WITH RECURSIVE cnt(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM cnt WHERE x < 100000000) \
             SELECT count(*) FROM cnt;",
            "slow.sql",
            Some(&budgets),
        )
        .expect_err("a 1ms wall_time_ms budget should interrupt an effectively-unbounded query");
        assert!(matches!(error, SqlError::Check(_)));
    }
}
