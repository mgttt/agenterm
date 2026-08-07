//! agenterm-qjs — QuickJS-backed script engine, capability-aligned with `agenterm-rh`.
//!
//! QJS-M0 (skeleton): pick a QuickJS binding, prove a minimal eval round-trip.
//! Not yet wired into the root workspace (see Cargo.toml header comment) and
//! not yet capability-aligned (no L2 facade / CLI verb parity) — that's
//! QJS-M1/M2, tracked in plan/plan-v0.1.16.md §1 "Rh. 脚本引擎矩阵".

use rquickjs::{CatchResultExt, Context, Runtime};

/// Evaluate a JS source string and return its result rendered as a string.
///
/// Placeholder API shape only — will grow into something structurally
/// parallel to `agenterm_rh`'s `check`/`compile_native`/`load_and_call_entry`
/// once QJS-M1 (CLI verb parity) starts.
pub fn eval_to_string(source: &str) -> Result<String, String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    context.with(|ctx| {
        let value: rquickjs::Value = ctx.eval(source).catch(&ctx).map_err(|e| e.to_string())?;
        Ok(format!("{value:?}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evals_arithmetic() {
        let out = eval_to_string("1 + 2").expect("eval should succeed");
        assert!(out.contains('3'), "expected 3 in output, got {out}");
    }

    #[test]
    fn evals_string_concat() {
        let out = eval_to_string("'agenterm-' + 'qjs'").expect("eval should succeed");
        assert!(out.contains("agenterm-qjs"), "got {out}");
    }

    #[test]
    fn reports_syntax_errors_without_panicking() {
        let out = eval_to_string("this is not valid js (((");
        assert!(out.is_err(), "expected an error, got {out:?}");
    }
}
