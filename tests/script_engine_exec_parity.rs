//! Execution-level parity across rh/lua/qjs/sql through the unified
//! `ScriptEngineBackend` trait (`src/script_engine.rs`).
//!
//! Whole-file gate: parity across all four engines only exists when all
//! four are compiled in — the engine features default off, so a plain
//! `cargo test` skips this file rather than failing to name gated variants.
#![cfg(all(feature = "script-lua", feature = "script-qjs", feature = "script-sql"))]
//!
//! `tests/script_engine_parity.rs` already locks check-many-level parity.
//! Nothing locked EXECUTION-level parity — what each engine actually
//! returns for equivalent trivial programs through `check`/`execute` — until
//! this file. The goal here is not to force false uniformity across three
//! (now four) independently-designed engines; it is to produce a precise,
//! asserted map of where their execution envelopes agree and where they
//! genuinely diverge (a divergence caught and asserted here is a *success*
//! for this file, not a bug to paper over).
//!
//! ## `agenterm-sql`'s place in this file (M1, `execute` now real — see
//! `plan/design-sql-execution-target.md` §4/§5)
//!
//! sql's `check` was always real, so it already joined
//! `check_accepts_valid_rejects_broken` and `disabled_backend_errors`
//! (neither needs a working `execute`). As of M1, `execute` is ALSO real
//! (`agenterm_sql::execute_entry`, backed by an ephemeral in-memory SQLite
//! database — see `crates/agenterm-sql/src/eval.rs`'s module doc), but sql's
//! declarative "batch of statements, last result set wins" execution model
//! is different enough from rh/lua/qjs's "single command program, one
//! value" model (design doc §1.2) that most of the trivial-program scenarios
//! below still don't enroll sql *directly* — per the design doc §4 table,
//! each gets sql's own scenario instead, asserting sql's actual documented
//! shape rather than forcing scalar uniformity:
//!
//! | scenario | sql treatment | why |
//! |---|---|---|
//! | `trivial_entry_value` | NOT enrolled; see `sql_trivial_select_value` | sql's `value` for a trivial program is `Some(json!([{"1": 1}]))` (an array of row objects), not a bare scalar `Some(json!(42))` — mixing it into this test would break its "three engines agree on a scalar" assertion |
//! | `stdout_capture` | NOT enrolled; see `sql_stdout_is_empty` | SQL has no `print()`/`NOTICE` concept in M1 — `stdout` is unconditionally `""`, not a print-and-capture result |
//! | `execute_missing_entry_fails_closed` | NOT enrolled; see `sql_execute_no_result_set_is_none_not_error` | sql has no `entry()` concept at all, so "missing entry" doesn't apply; sql's closest analogue (a script with no result-producing statement) is a documented fail-*open*-to-`None` contract, not a fail-closed error |
//! | `error_not_panic` | **enrolled directly** below | sql's execution-time-error-not-panic contract is structurally identical to rh/lua/qjs's — no new scenario needed |
//! | `disabled_backend_errors` | already enrolled | unaffected by M1 — the `enabled()` gate runs before `execute` is attempted either way |
//! | `check_accepts_valid_rejects_broken` | already enrolled | `check()` is unaffected by M1 |
//!
//! `sql_execute_placeholder_contract` (the "execute always fails closed with
//! a not-implemented error" pin) is DELETED in this pass — that contract no
//! longer holds now that `execute` is real, exactly as the design doc §4
//! predicted it would need to be ("这个测试断言的是...M1 落地后这个契约不再
//! 成立，必须删除或改写").

use agenterm::script_backend::ScriptBackend;
use agenterm::script_engine::{ScriptEngineBackend, ScriptInvocationOptions, engine_for};

// ---------------------------------------------------------------------
// AGENTERM_SCRIPT_BACKEND env guard
//
// Mirrors `src/script_engine.rs`'s own `#[cfg(test)]` `ENV_LOCK`/`EnvGuard`
// pattern. This integration test binary runs in its own process (separate
// from `cargo test`'s lib-test process), but every `#[test]` fn *within
// this file* still shares that one process, so races between our own tests
// over the same env var are real and must be serialized here.
// ---------------------------------------------------------------------

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    prior: Option<String>,
}

impl EnvGuard {
    fn set(value: &str) -> Self {
        let prior = std::env::var("AGENTERM_SCRIPT_BACKEND").ok();
        unsafe {
            std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
        }
        Self { prior }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prior {
            Some(value) => unsafe {
                std::env::set_var("AGENTERM_SCRIPT_BACKEND", value);
            },
            None => unsafe {
                std::env::remove_var("AGENTERM_SCRIPT_BACKEND");
            },
        }
    }
}

const ENGINES: [ScriptBackend; 4] = [
    ScriptBackend::Rh,
    ScriptBackend::Lua,
    ScriptBackend::Qjs,
    ScriptBackend::Sql,
];

/// Same "not enabled" format `src/script_engine.rs`'s private
/// `not_enabled_error` produces (`format!("{} backend not enabled", ...)`)
/// — reconstructed here since that helper isn't `pub`.
fn not_enabled_message(backend: ScriptBackend) -> String {
    format!("{} backend not enabled", backend.as_str())
}

// ---------------------------------------------------------------------
// 1. trivial_entry_value
// ---------------------------------------------------------------------

#[test]
fn trivial_entry_value() {
    let _guard = ENV_LOCK.lock().expect("lock");

    // rh: `fn entry() { 42 }` — verified fixture from src/script_engine.rs tests.
    let rh_result = {
        let _env = EnvGuard::set("rh");
        engine_for(ScriptBackend::Rh)
            .execute(
                "fn entry() { 42 }",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect("rh execute should succeed")
    };
    assert_eq!(
        rh_result.value,
        Some(serde_json::json!(42)),
        "rh: fn entry() {{ 42 }} should yield Some(42)"
    );

    // lua: `return 42` — lua's native i64 result is widened via
    // `serde_json::Value::from` by `LuaEngineBackend::execute`.
    let lua_result = {
        let _env = EnvGuard::set("lua");
        engine_for(ScriptBackend::Lua)
            .execute("return 42", &ScriptInvocationOptions::default(), None)
            .expect("lua execute should succeed")
    };
    assert_eq!(
        lua_result.value,
        Some(serde_json::json!(42)),
        "lua: return 42 should yield Some(42)"
    );

    // qjs: `function entry() { return 42; }`
    let qjs_result = {
        let _env = EnvGuard::set("qjs");
        engine_for(ScriptBackend::Qjs)
            .execute(
                "function entry() { return 42; }",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect("qjs execute should succeed")
    };
    assert_eq!(
        qjs_result.value,
        Some(serde_json::json!(42)),
        "qjs: function entry() {{ return 42; }} should yield Some(42)"
    );

    // PARITY FINDING: all three engines agree — the trivial 42-program
    // returns `Some(json!(42))` through the unified trait layer for rh,
    // lua, and qjs alike. No divergence on this dimension.
}

// ---------------------------------------------------------------------
// 2. stdout_capture
// ---------------------------------------------------------------------

#[test]
fn stdout_capture() {
    let _guard = ENV_LOCK.lock().expect("lock");

    // rh: `print(...)` is a real builtin (`rh_print` -> host utility
    // `RH_HOST_UTILITY_PRINT` -> `RhOutputCapture::push_line`, which
    // *always* appends a trailing '\n' per call — see
    // src/script_rh_run.rs:31-45). This is distinct from rh's *other*
    // stdout source, `cc_lines()` (an AOT compile-time constant-folded
    // export appended by `try_execute_rh_invocation` after the capture) —
    // not exercised here since it isn't a "print mechanism" in the other
    // two engines' sense.
    let rh = {
        let _env = EnvGuard::set("rh");
        engine_for(ScriptBackend::Rh)
            .execute(
                "fn entry() { print(\"hi\"); 0 }",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect("rh execute should succeed")
    };
    assert_eq!(
        rh.stdout, "hi\n",
        "rh: print(\"hi\") should capture \"hi\\n\""
    );

    // lua: native `print` is overridden to append to the captured buffer,
    // one '\n' appended per call (crates/agenterm-lua/src/lib.rs:281-296).
    let lua = {
        let _env = EnvGuard::set("lua");
        engine_for(ScriptBackend::Lua)
            .execute(
                "print('hi') return 0",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect("lua execute should succeed")
    };
    assert_eq!(
        lua.stdout, "hi\n",
        "lua: print('hi') should capture \"hi\\n\""
    );

    // qjs: `print()` is host-bound and captured, newline-terminated per
    // call per crates/agenterm-qjs/src/eval.rs's `EvalOutcome::stdout` doc.
    let qjs = {
        let _env = EnvGuard::set("qjs");
        engine_for(ScriptBackend::Qjs)
            .execute(
                "function entry() { print('hi'); return 0; }",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect("qjs execute should succeed")
    };
    assert_eq!(
        qjs.stdout, "hi\n",
        "qjs: print('hi') should capture \"hi\\n\""
    );

    // PARITY FINDING: all three engines produce byte-identical stdout
    // ("hi\n") for the equivalent one-line-print program — same text, same
    // single trailing-newline convention. No divergence on this dimension.
}

// ---------------------------------------------------------------------
// 3. check_accepts_valid_rejects_broken
// ---------------------------------------------------------------------

#[test]
fn check_accepts_valid_rejects_broken() {
    let _guard = ENV_LOCK.lock().expect("lock");

    struct CheckFixture {
        backend: ScriptBackend,
        valid: &'static str,
        broken: &'static str,
    }

    // Same fixtures as src/script_engine.rs's own `#[cfg(test)]` module
    // (RH_VALID_SOURCE/RH_BROKEN_SOURCE etc.) — copied here since those
    // consts are private to that module.
    let fixtures = [
        CheckFixture {
            backend: ScriptBackend::Rh,
            valid: "fn entry() { 42 }",
            broken: "fn entry() { 1 ",
        },
        CheckFixture {
            backend: ScriptBackend::Lua,
            valid: "return 42",
            broken: "return !!",
        },
        CheckFixture {
            backend: ScriptBackend::Qjs,
            valid: "function entry() { return 42; }",
            broken: "function entry() { return 1 ",
        },
        // sql: same fixtures as `src/script_engine.rs`'s own `#[cfg(test)]`
        // `SQL_VALID_SOURCE`/`SQL_BROKEN_SOURCE` consts. Unlike the other
        // three, sql's `check` needs no `fn`/`function entry()` wrapper at
        // all — it's bare SQL parsed statement-by-statement.
        CheckFixture {
            backend: ScriptBackend::Sql,
            valid: "SELECT 1;",
            broken: "SELEC 1 FORM;",
        },
    ];

    for fixture in fixtures {
        let _env = EnvGuard::set(fixture.backend.as_str());
        let engine = engine_for(fixture.backend);
        let options = ScriptInvocationOptions::default();

        engine
            .check(fixture.valid, &options)
            .unwrap_or_else(|error| {
                panic!(
                    "{:?}: valid source should check clean, got {error}",
                    fixture.backend
                )
            });

        let error = engine.check(fixture.broken, &options).expect_err(&format!(
            "{:?}: broken source should fail check",
            fixture.backend
        ));
        assert!(
            !error.is_empty(),
            "{:?}: broken-source check error should carry a non-empty diagnostic",
            fixture.backend
        );
    }

    // PARITY FINDING: all four engines agree on the check() contract —
    // Ok(()) for valid source, Err(non-empty diagnostic) for broken source.
    // sql joining this scenario is not a coincidence: its check() is real
    // (delegates to `sqlparser`), so it was always going to hold up here —
    // unlike execute(), which is still a placeholder for sql (see this
    // file's top-of-file doc and `sql_execute_placeholder_contract` below).
}

// ---------------------------------------------------------------------
// 4. execute_missing_entry_fails_closed
// ---------------------------------------------------------------------

#[test]
fn execute_missing_entry_fails_closed() {
    let _guard = ENV_LOCK.lock().expect("lock");

    // rh: no `fn entry()` at all. `agenterm_rh::check()` (used by
    // `check()`/`ScriptOperation::Check`) does NOT require `fn entry()` —
    // `tests/script_engine_parity.rs` even uses bare "40 + 2" as rh's
    // check-many *valid* source. But `execute()` goes through
    // `transpile_cdylib_with_mode`, which explicitly requires it
    // (crates/agenterm-rh/src/transpile.rs:267-269:
    // "cdylib pack requires fn entry()"). So rh's fail-closed-ness is
    // execute()-specific, not check()-specific.
    let rh_error = {
        let _env = EnvGuard::set("rh");
        engine_for(ScriptBackend::Rh)
            .execute("40 + 2", &ScriptInvocationOptions::default(), None)
            .expect_err("rh: source without fn entry() should fail closed on execute")
    };
    assert!(
        rh_error.contains("entry"),
        "rh: missing-entry error should mention entry(), got: {rh_error}"
    );

    // lua: DOCUMENTED CONTRACT DIVERGENCE. Lua has no separate "entry
    // function" concept at all — the whole script chunk *is* the entry
    // point, and `LuaEngine::eval`'s `value_to_i64` (crates/agenterm-lua/
    // src/lib.rs:322-335) maps a `nil` result (i.e. no explicit `return`)
    // to `0` rather than erroring. So a lua script with no return value
    // does NOT fail closed the way rh/qjs do — it silently succeeds with
    // value 0. This is real lua behavior, asserted here rather than
    // papered over.
    let lua_result = {
        let _env = EnvGuard::set("lua");
        engine_for(ScriptBackend::Lua)
            .execute(
                "local x = 40 + 2",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect("lua: source without an explicit return should NOT fail closed")
    };
    assert_eq!(
        lua_result.value,
        Some(serde_json::json!(0)),
        "lua: a returnless script coerces to value 0 rather than erroring — this IS lua's actual (fail-open) contract"
    );

    // qjs: no top-level `entry()` function. `eval_entry_with_host` requires
    // it and fails closed (crates/agenterm-qjs/src/eval.rs:66-69:
    // "{label}: no top-level `entry()` function").
    let qjs_error = {
        let _env = EnvGuard::set("qjs");
        engine_for(ScriptBackend::Qjs)
            .execute("40 + 2;", &ScriptInvocationOptions::default(), None)
            .expect_err("qjs: source without entry() should fail closed on execute")
    };
    assert!(
        qjs_error.contains("entry()"),
        "qjs: missing-entry error should mention entry(), got: {qjs_error}"
    );

    // DIVERGENCE FOUND (this dimension's whole point): rh and qjs both
    // fail closed on a missing entry point, each with its own wording
    // ("cdylib pack requires fn entry()" vs "no top-level `entry()`
    // function") — but lua has NO such contract. A lua script with no
    // explicit `return` silently produces `0`. Callers relying on
    // "missing entry point == execute() error" would be correct for
    // rh/qjs and *wrong* for lua.
}

// ---------------------------------------------------------------------
// 5. disabled_backend_errors
// ---------------------------------------------------------------------

#[test]
fn disabled_backend_errors() {
    let _guard = ENV_LOCK.lock().expect("lock");

    for &enabled in &ENGINES {
        let _env = EnvGuard::set(enabled.as_str());
        for &other in &ENGINES {
            if other == enabled {
                continue;
            }
            let error = engine_for(other)
                .execute(
                    "irrelevant source",
                    &ScriptInvocationOptions::default(),
                    None,
                )
                .expect_err(&format!(
                    "{:?} should be disabled while AGENTERM_SCRIPT_BACKEND={}",
                    other,
                    enabled.as_str()
                ));
            assert_eq!(
                error,
                not_enabled_message(other),
                "disabled-backend error message should match exactly"
            );
        }
    }

    // PARITY FINDING: all four engines agree on the "disabled backend"
    // contract — same message shape (`"{backend} backend not enabled"`)
    // for all twelve (enabled, other) pairings, and the check never even
    // reaches parsing the (deliberately garbage) source. This holds for sql
    // too even though its execute() is otherwise a placeholder: the
    // enabled()-gate runs before anything execute-shaped is attempted.
}

// ---------------------------------------------------------------------
// 6. error_not_panic
// ---------------------------------------------------------------------

#[test]
fn error_not_panic() {
    let _guard = ENV_LOCK.lock().expect("lock");

    // rh: calling an undefined function. rh is AOT-compiled to native
    // code with statically-resolved calls, so this is NOT a "runtime
    // exception" the way lua/qjs would surface one — it fails during
    // execute()'s compile step (loaded_pack_for_source_with_project ->
    // transpile), before any native code ever runs. Still: Err, not a
    // panic, and the diagnostic names the offending symbol.
    let rh_error = {
        let _env = EnvGuard::set("rh");
        engine_for(ScriptBackend::Rh)
            .execute(
                "fn entry() { undefined_fn_xyz() }",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect_err("rh: calling an undefined function should error, not panic")
    };
    assert!(
        rh_error.contains("undefined_fn_xyz"),
        "rh: error should name the unresolved symbol, got: {rh_error}"
    );

    // lua: `error('boom')` is a genuine *runtime* error (unlike rh's
    // compile-time rejection above) — LuaEngine::eval turns it into
    // `LuaError::Runtime(e.to_string())`, and mlua's rendering embeds the
    // original message text.
    let lua_error = {
        let _env = EnvGuard::set("lua");
        engine_for(ScriptBackend::Lua)
            .execute("error('boom')", &ScriptInvocationOptions::default(), None)
            .expect_err("lua: error('boom') should error, not panic")
    };
    assert!(
        lua_error.contains("boom"),
        "lua: error message should surface the original 'boom' text, got: {lua_error}"
    );

    // qjs: `throw new Error('boom')` inside entry() is also a genuine
    // *runtime* error — eval_entry_with_host wraps it as
    // "{label}: entry() failed: {err}" (crates/agenterm-qjs/src/eval.rs:74).
    let qjs_error = {
        let _env = EnvGuard::set("qjs");
        engine_for(ScriptBackend::Qjs)
            .execute(
                "function entry() { throw new Error('boom'); }",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect_err("qjs: throw new Error('boom') should error, not panic")
    };
    assert!(
        qjs_error.contains("boom"),
        "qjs: error message should surface the original 'boom' text, got: {qjs_error}"
    );

    // sql: querying a table that doesn't exist. This parses fine (it's
    // syntactically valid SQL) and fails only when SQLite actually tries to
    // resolve `does_not_exist` at execution time — same "error, not panic"
    // contract as the other three, enrolled directly (design doc §4: no new
    // scenario needed for this one).
    let sql_error = {
        let _env = EnvGuard::set("sql");
        engine_for(ScriptBackend::Sql)
            .execute(
                "SELECT * FROM does_not_exist;",
                &ScriptInvocationOptions::default(),
                None,
            )
            .expect_err("sql: querying a nonexistent table should error, not panic")
    };
    assert!(
        sql_error.contains("does_not_exist"),
        "sql: error should name the unresolved table, got: {sql_error}"
    );

    // DIVERGENCE FOUND: none of the four panics — all four fail closed
    // with `Err(String)` carrying a real diagnostic, so the trait-level
    // "error, not panic" contract holds uniformly. But *when* the error
    // occurs differs qualitatively: lua/qjs raise genuine runtime
    // exceptions from a running script, rh's equivalent "calling
    // something that doesn't exist" case is caught at AOT compile time
    // before rh ever executes a single instruction, and sql's is caught by
    // SQLite at statement-execution time (not sqlparser's parse time — the
    // statement is syntactically valid SQL). A caller cannot assume "the
    // program started running" from "execute() returned Err" uniformly
    // across all four engines.
}

// ---------------------------------------------------------------------
// 7. sql's own execution-envelope scenarios (design doc §4's "needs a new
// scenario" rows — see this file's top-of-file doc table)
// ---------------------------------------------------------------------

/// sql's analogue of `trivial_entry_value`: NOT enrolled directly in that
/// test because sql's `value` shape for a trivial program is an array of
/// row objects, not a bare scalar (design doc §2.1/§4).
#[test]
fn sql_trivial_select_value() {
    let _guard = ENV_LOCK.lock().expect("lock");
    let _env = EnvGuard::set("sql");

    let result = engine_for(ScriptBackend::Sql)
        .execute("SELECT 1;", &ScriptInvocationOptions::default(), None)
        .expect("sql execute should succeed");
    assert_eq!(
        result.value,
        Some(serde_json::json!([{"1": 1}])),
        "sql: SELECT 1; should yield an array of one row object, keyed by sqlite's own column \
         name (`\"1\"` for an unaliased literal expression, not PostgreSQL's `?column?`)"
    );
}

/// sql's analogue of `stdout_capture`: NOT enrolled directly in that test
/// because sql has no `print()`/`NOTICE` concept in M1 — `stdout` is
/// unconditionally empty rather than a print-and-capture result (design
/// doc §2.1: "SQL 没有 print() 概念").
#[test]
fn sql_stdout_is_empty() {
    let _guard = ENV_LOCK.lock().expect("lock");
    let _env = EnvGuard::set("sql");

    let result = engine_for(ScriptBackend::Sql)
        .execute(
            "CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1); SELECT * FROM t;",
            &ScriptInvocationOptions::default(),
            None,
        )
        .expect("sql execute should succeed");
    assert_eq!(
        result.stdout, "",
        "sql: stdout should always be empty in M1"
    );
}

/// sql's analogue of `execute_missing_entry_fails_closed`: NOT enrolled
/// directly in that test because sql has no `entry()` concept for
/// "missing" to apply to. The closest analogue — a script with no
/// result-producing statement at all (empty script, or only DDL/DML) — is a
/// DOCUMENTED fail-*open*-to-`None` contract (design doc §4: "建议 M1 决定
/// 空 value 用 None 而不是报错"), the mirror image of rh/qjs's fail-closed
/// missing-entry error and closer in spirit to lua's fail-open-to-0 (but
/// sql uses `None`, not a fabricated `0`, to keep "no result" distinguishable
/// from "a result that happens to be falsy/zero").
#[test]
fn sql_execute_no_result_set_is_none_not_error() {
    let _guard = ENV_LOCK.lock().expect("lock");
    let _env = EnvGuard::set("sql");
    let engine = engine_for(ScriptBackend::Sql);
    let options = ScriptInvocationOptions::default();

    let empty = engine
        .execute("", &options, None)
        .expect("sql: an empty script must not fail closed");
    assert_eq!(
        empty.value, None,
        "sql: an empty script's value should be None"
    );

    let ddl_only = engine
        .execute(
            "CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1);",
            &options,
            None,
        )
        .expect("sql: a script with no result-producing statement must not fail closed");
    assert_eq!(
        ddl_only.value, None,
        "sql: a script with only DDL/DML should yield None, not an error or a fabricated 0"
    );
}
