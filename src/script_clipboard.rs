//! Rh bindings for the typed Script Runtime clipboard facade.

use rhai::{EvalAltResult, Module};

use crate::{
    platform::{contract::script_clipboard::ScriptClipboardError, services::script_clipboard},
    script_error::runtime_error,
};

pub(crate) fn register(rhai_module: &mut Module) {
    let mut clipboard = Module::new();
    clipboard.set_native_fn("get_text", get_text);
    clipboard.set_native_fn("set_text", set_text);
    rhai_module.set_sub_module("clipboard", clipboard);
}

fn get_text() -> Result<String, Box<EvalAltResult>> {
    script_clipboard::get_text().map_err(|error| clipboard_error(error, "rh.clipboard.get_text"))
}

fn set_text(text: &str) -> Result<(), Box<EvalAltResult>> {
    script_clipboard::set_text(text)
        .map_err(|error| clipboard_error(error, "rh.clipboard.set_text"))
}

fn clipboard_error(error: ScriptClipboardError, operation: &'static str) -> Box<EvalAltResult> {
    runtime_error(
        "clipboard",
        error.code,
        operation,
        error.message,
        false,
        "system_clipboard",
        false,
        error.cause,
    )
}
