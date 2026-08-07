//! Run a script and produce its result, aligned with `agenterm_rh`'s "eval"
//! entry-point convention (`crates/agenterm-rh/src/main.rs` `eval` command:
//! compile → load → call `entry()` → print the value). rh's `eval` also
//! prints `source_hash`/`native_hash`/`cc_lines` because it goes through the
//! AOT pipeline; qjs has no AOT step (see PRD "qjs execution backend" risk
//! list — no AOT is a deliberate, tracked difference, not an oversight), so
//! `EvalOutcome` only carries what's actually true here: the entry point's
//! result value.
//!
//! Entry-point convention: like rh's current (non-compat) stance — "a
//! top-level script without an explicit `fn entry()` … fails qualification"
//! (see PRD "Shipped baseline") — `eval_entry` requires a top-level
//! `function entry() { ... }` and fails closed if it's missing, rather than
//! guessing at a whole-script completion value. This matches rh's forward
//! direction, not rhai's legacy whole-script compatibility fallback.

use rquickjs::{CatchResultExt, Context, Function, Runtime};
use serde_json::Value as JsonValue;

use crate::error::QjsError;

#[derive(Debug, Clone, PartialEq)]
pub struct EvalOutcome {
    /// `entry()`'s return value, converted through `JSON.stringify` (so it
    /// has the same representable shape as any other JSON-typed contract in
    /// this codebase — see `CheckManyReport` etc.). `None` if `entry()`
    /// returned `undefined` (not JSON-representable, same as rh's `()`/unit
    /// having no numeric i64 encoding today — see rh's native i64 entry ABI
    /// note in the PRD; qjs is not bound to i64-only, so `undefined` is
    /// simply "no value" rather than a forced numeric encoding).
    pub value: Option<JsonValue>,
}

pub fn eval_entry(source: &str, label: &str) -> Result<EvalOutcome, QjsError> {
    let runtime = Runtime::new().map_err(|err| QjsError::Check(err.to_string()))?;
    let context = Context::full(&runtime).map_err(|err| QjsError::Check(err.to_string()))?;
    context.with(|ctx| {
        let mut options = rquickjs::context::EvalOptions::default();
        options.filename = Some(label.to_owned());
        ctx.eval_with_options::<(), _>(source, options)
            .catch(&ctx)
            .map_err(|err| QjsError::Parse(err.to_string()))?;

        let entry: Function = ctx
            .globals()
            .get("entry")
            .map_err(|_| QjsError::Check(format!("{label}: no top-level `entry()` function")))?;

        let result: rquickjs::Value = entry
            .call(())
            .catch(&ctx)
            .map_err(|err| QjsError::Check(format!("{label}: entry() failed: {err}")))?;

        let json_string = ctx
            .json_stringify(result)
            .catch(&ctx)
            .map_err(|err| QjsError::Check(format!("{label}: entry() result: {err}")))?;

        let value =
            match json_string {
                Some(js_string) => {
                    let text = js_string.to_string().map_err(|err| {
                        QjsError::Check(format!("{label}: entry() result: {err}"))
                    })?;
                    Some(serde_json::from_str(&text).map_err(|err| {
                        QjsError::Check(format!("{label}: entry() result: {err}"))
                    })?)
                }
                // JSON.stringify(undefined) is `None` at the JS level (no
                // string produced) — e.g. entry() returned undefined.
                None => None,
            };

        Ok(EvalOutcome { value })
    })
}

#[cfg(test)]
mod tests {
    use super::eval_entry;
    use serde_json::json;

    #[test]
    fn evals_entry_arithmetic() {
        let outcome = eval_entry("function entry() { return 40 + 2; }", "arith.js")
            .expect("eval should succeed");
        assert_eq!(outcome.value, Some(json!(42)));
    }

    #[test]
    fn evals_entry_object_result() {
        let outcome = eval_entry("function entry() { return { ok: true, n: 3 }; }", "obj.js")
            .expect("eval should succeed");
        assert_eq!(outcome.value, Some(json!({"ok": true, "n": 3})));
    }

    #[test]
    fn fails_closed_without_entry() {
        let error = eval_entry("40 + 2;", "no-entry.js").expect_err("missing entry()");
        assert!(matches!(error, super::QjsError::Check(_)));
    }

    #[test]
    fn propagates_entry_exceptions_as_check_errors() {
        let error = eval_entry("function entry() { throw new Error('boom'); }", "throws.js")
            .expect_err("entry() threw");
        let message = error.to_string();
        assert!(message.contains("boom"), "got: {message}");
    }

    #[test]
    fn undefined_result_is_none() {
        let outcome = eval_entry("function entry() {}", "undef.js").expect("eval should succeed");
        assert_eq!(outcome.value, None);
    }
}
