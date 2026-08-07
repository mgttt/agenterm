//! Run a script and produce its result, aligned with `agenterm_rh`'s "eval"
//! entry-point convention (`crates/agenterm-rh/src/main.rs` `eval` command:
//! compile → load → call `entry()` → print the value). rh's `eval` also
//! prints `source_hash`/`native_hash`/`cc_lines` because it goes through the
//! AOT pipeline; qjs has no AOT step (see PRD "qjs execution backend" risk
//! list — no AOT is a deliberate, tracked difference, not an oversight), so
//! `EvalOutcome` only carries what's actually true here: the entry point's
//! result value plus captured `print()` output (matching
//! `agenterm_lua::LuaEvalResult`'s `value`/`stdout` shape).
//!
//! Entry-point convention: like rh's current (non-compat) stance — "a
//! top-level script without an explicit `fn entry()` … fails qualification"
//! (see PRD "Shipped baseline") — `eval_entry` requires a top-level
//! `function entry() { ... }` and fails closed if it's missing, rather than
//! guessing at a whole-script completion value. This matches rh's forward
//! direction, not rhai's legacy whole-script compatibility fallback.

use std::sync::{Arc, Mutex};

use rquickjs::{CatchResultExt, Context, Function, Runtime};
use serde_json::Value as JsonValue;

use crate::error::QjsError;
use crate::host::{QjsHostFunctions, inject_host};

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
    /// Captured `print()`/`__host.print()` output, newline-terminated per
    /// call — mirrors `agenterm_lua::LuaEvalResult::stdout`.
    pub stdout: String,
}

/// Evaluate `source` and call its top-level `entry()`, with no host
/// functions bound (`fleet`/`args` calls will throw `ReferenceError:
/// __host is not defined`). Use [`eval_entry_with_host`] to bind
/// `fleet_call`/`args_len`/`arg` for real task/fleet integration.
pub fn eval_entry(source: &str, label: &str) -> Result<EvalOutcome, QjsError> {
    eval_entry_with_host(source, label, &QjsHostFunctions::default())
}

pub fn eval_entry_with_host(
    source: &str,
    label: &str,
    host: &QjsHostFunctions,
) -> Result<EvalOutcome, QjsError> {
    let runtime = Runtime::new().map_err(|err| QjsError::Check(err.to_string()))?;
    let context = Context::full(&runtime).map_err(|err| QjsError::Check(err.to_string()))?;
    let stdout = Arc::new(Mutex::new(String::new()));
    let value = context.with(|ctx| {
        inject_host(&ctx, host, Arc::clone(&stdout))?;

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

        match json_string {
            Some(js_string) => {
                let text = js_string
                    .to_string()
                    .map_err(|err| QjsError::Check(format!("{label}: entry() result: {err}")))?;
                Ok(Some(serde_json::from_str(&text).map_err(|err| {
                    QjsError::Check(format!("{label}: entry() result: {err}"))
                })?))
            }
            // JSON.stringify(undefined) is `None` at the JS level (no
            // string produced) — e.g. entry() returned undefined.
            None => Ok(None),
        }
    })?;

    let stdout = stdout
        .lock()
        .map_err(|err| QjsError::Check(err.to_string()))?
        .clone();
    Ok(EvalOutcome { value, stdout })
}

#[cfg(test)]
mod tests {
    use super::{eval_entry, eval_entry_with_host};
    use crate::host::QjsHostFunctions;
    use serde_json::json;
    use std::sync::Arc;

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

    #[test]
    fn captures_print_output() {
        let outcome = eval_entry(
            "function entry() { print('hello'); print('world'); return 1; }",
            "print.js",
        )
        .expect("eval should succeed");
        assert_eq!(outcome.stdout, "hello\nworld\n");
        assert_eq!(outcome.value, Some(json!(1)));
    }

    #[test]
    fn calls_host_fleet() {
        let host = QjsHostFunctions {
            fleet_call: Some(Arc::new(|op, params| {
                if op == "test" && params == "{}" {
                    Ok("{\"ok\":true}".to_owned())
                } else {
                    Err("unknown".to_owned())
                }
            })),
            ..Default::default()
        };
        let outcome = eval_entry_with_host(
            "function entry() { return __host.fleet_call('test', '{}'); }",
            "fleet.js",
            &host,
        )
        .expect("eval should succeed");
        assert_eq!(outcome.value, Some(json!("{\"ok\":true}")));
    }

    #[test]
    fn calls_host_args() {
        let host = QjsHostFunctions {
            args_len: Some(Arc::new(|| 2)),
            arg: Some(Arc::new(|i| {
                if i == 0 {
                    Ok("first".to_owned())
                } else {
                    Ok("second".to_owned())
                }
            })),
            ..Default::default()
        };
        let outcome = eval_entry_with_host(
            "function entry() { return __host.args_len() + '/' + __host.arg(0); }",
            "args.js",
            &host,
        )
        .expect("eval should succeed");
        assert_eq!(outcome.value, Some(json!("2/first")));
    }

    #[test]
    fn fleet_call_error_surfaces_as_js_exception() {
        let host = QjsHostFunctions {
            fleet_call: Some(Arc::new(|_op, _params| Err("boom".to_owned()))),
            ..Default::default()
        };
        let error = eval_entry_with_host(
            "function entry() { return __host.fleet_call('x', '{}'); }",
            "fleet-err.js",
            &host,
        )
        .expect_err("fleet_call should throw");
        assert!(error.to_string().contains("boom"), "got: {error}");
    }
}

#[cfg(test)]
mod isolate_probe {
    use crate::eval::eval_entry_with_host;
    use crate::host::QjsHostFunctions;
    use std::sync::Arc;

    #[test]
    fn args_len_only() {
        let host = QjsHostFunctions {
            args_len: Some(Arc::new(|| 2)),
            ..Default::default()
        };
        let outcome = eval_entry_with_host(
            "function entry() { return __host.args_len(); }",
            "a.js",
            &host,
        )
        .expect("eval should succeed");
        assert_eq!(outcome.value, Some(serde_json::json!(2)));
    }

    #[test]
    fn arg_only() {
        let host = QjsHostFunctions {
            arg: Some(Arc::new(|_i| Ok("x".to_owned()))),
            ..Default::default()
        };
        let outcome = eval_entry_with_host(
            "function entry() { return __host.arg(0); }",
            "b.js",
            &host,
        )
        .expect("eval should succeed");
        assert_eq!(outcome.value, Some(serde_json::json!("x")));
    }
}

#[cfg(test)]
mod isolate_probe2 {
    use rquickjs::{Context, Function, Runtime};

    #[test]
    fn one_arg_i64_to_string_no_ctx_capture() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let func = Function::new(ctx.clone(), move |index: i64| -> String {
                format!("got {index}")
            })
            .unwrap();
            ctx.globals().set("f", func).unwrap();
            let result: String = ctx.eval("f(0)").unwrap();
            assert_eq!(result, "got 0");
        });
    }

    #[test]
    fn one_arg_i64_to_result_string_no_ctx_capture() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let func = Function::new(ctx.clone(), move |index: i64| -> rquickjs::Result<String> {
                Ok(format!("got {index}"))
            })
            .unwrap();
            ctx.globals().set("f", func).unwrap();
            let result: String = ctx.eval("f(0)").unwrap();
            assert_eq!(result, "got 0");
        });
    }
}

#[cfg(test)]
mod isolate_probe3 {
    use rquickjs::{Context, Exception, Function, Runtime};

    #[test]
    fn one_arg_with_ctx_capture_success_path() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let bound_ctx = ctx.clone();
            let func = Function::new(ctx.clone(), move |index: i64| -> rquickjs::Result<String> {
                if index < 0 {
                    return Err(Exception::throw_message(&bound_ctx, "neg"));
                }
                Ok(format!("got {index}"))
            })
            .unwrap();
            ctx.globals().set("f", func).unwrap();
            let result: String = ctx.eval("f(0)").unwrap();
            assert_eq!(result, "got 0");
        });
    }
}

#[cfg(test)]
mod isolate_probe4 {
    use rquickjs::{Ctx, Context, Exception, Function, Runtime};

    #[test]
    fn one_arg_with_ctx_as_param_not_captured() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let func = Function::new(ctx.clone(), move |ctx: Ctx<'_>, index: i64| -> rquickjs::Result<String> {
                if index < 0 {
                    return Err(Exception::throw_message(&ctx, "neg"));
                }
                Ok(format!("got {index}"))
            })
            .unwrap();
            ctx.globals().set("f", func).unwrap();
            let result: String = ctx.eval("f(0)").unwrap();
            assert_eq!(result, "got 0");
        });
    }
}
