//! Host function injection, architecturally aligned with `agenterm_lua`'s
//! `LuaHostFunctions`/`inject_host_table` (see
//! `crates/agenterm-lua/src/lib.rs`) — same low-level primitive shape
//! (`fleet_call`, `args_len`, `arg`, `print`) bound under the *same*
//! `__host` global name, so `scripts/qjs/lib/fleet.js` can be a
//! near-line-for-line port of `scripts/lua/lib/fleet.lua` rather than a
//! reinvention. This is a deliberate alignment decision, not a coincidence:
//! lua is the only other independent implementation of this contract so
//! far — matching its already-shipped, tested design directly mitigates
//! the "parallel-spec drift" risk tracked in the PRD "qjs execution
//! backend" risk list, instead of each engine inventing its own shape for
//! the same host boundary.
//!
//! rh's fleet binding works differently in kind (native-codegen calls a C
//! ABI host function, see `crates/agenterm-rh/src/fleet.rs` +
//! `host_api.rs`) — that's rh's AOT-specific mechanism, not part of what
//! "capability alignment" requires (see PRD "capability alignment" scope
//! note). What both interpreted engines (lua, qjs) need instead is a
//! same-shape *in-process callback* binding, which is what this module is.

use std::sync::{Arc, Mutex};

use rquickjs::{Ctx, Exception, Function, Object};

use crate::error::QjsError;

pub type FleetCallFn = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;
pub type ArgFn = Arc<dyn Fn(i64) -> Result<String, String> + Send + Sync>;

/// Host functions injectable into qjs scripts as `__host.*` globals — same
/// shape as `agenterm_lua::LuaHostFunctions`.
#[derive(Default, Clone)]
pub struct QjsHostFunctions {
    /// Called as `__host.fleet_call(operation_id, params_json) -> result_json`.
    pub fleet_call: Option<FleetCallFn>,
    /// Called as `__host.args_len() -> int`.
    pub args_len: Option<Arc<dyn Fn() -> i64 + Send + Sync>>,
    /// Called as `__host.arg(index) -> string`.
    pub arg: Option<ArgFn>,
}

/// Bind `host`'s functions plus a stdout-capturing `print` under
/// `globalThis.__host`, and override the global `print` too — mirroring
/// lua's `inject_host_table` (which injects both `__host.print` and
/// overrides the global `print`).
pub fn inject_host<'js>(
    ctx: &Ctx<'js>,
    host: &QjsHostFunctions,
    stdout: Arc<Mutex<String>>,
) -> Result<(), QjsError> {
    let host_object = Object::new(ctx.clone()).map_err(js_err)?;

    if let Some(fleet_call) = host.fleet_call.clone() {
        let bound_ctx = ctx.clone();
        let func = Function::new(
            ctx.clone(),
            move |op_id: String, params: String| -> rquickjs::Result<String> {
                fleet_call(&op_id, &params)
                    .map_err(|err| Exception::throw_message(&bound_ctx, &err))
            },
        )
        .map_err(js_err)?;
        host_object.set("fleet_call", func).map_err(js_err)?;
    }

    if let Some(args_len) = host.args_len.clone() {
        let func = Function::new(ctx.clone(), move || -> i64 { args_len() }).map_err(js_err)?;
        host_object.set("args_len", func).map_err(js_err)?;
    }

    if let Some(arg) = host.arg.clone() {
        let bound_ctx = ctx.clone();
        let func = Function::new(
            ctx.clone(),
            move |index: i64| -> rquickjs::Result<String> {
                arg(index).map_err(|err| Exception::throw_message(&bound_ctx, &err))
            },
        )
        .map_err(js_err)?;
        host_object.set("arg", func).map_err(js_err)?;
    }

    let print_buffer = Arc::clone(&stdout);
    let print_func = Function::new(ctx.clone(), move |text: String| {
        if let Ok(mut buffer) = print_buffer.lock() {
            buffer.push_str(&text);
            buffer.push('\n');
        }
    })
    .map_err(js_err)?;
    host_object
        .set("print", print_func.clone())
        .map_err(js_err)?;

    ctx.globals().set("__host", host_object).map_err(js_err)?;
    // Override the global `print` too (scripts commonly call `print(...)`
    // directly, same convention as lua's global `print` override).
    ctx.globals().set("print", print_func).map_err(js_err)?;

    Ok(())
}

fn js_err(err: rquickjs::Error) -> QjsError {
    QjsError::Check(err.to_string())
}
