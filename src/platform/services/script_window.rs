//! Script Runtime child-window service.

use crate::platform::{contract::script_window::*, selected};

pub(crate) fn facts(process_id: u32) -> ScriptWindowFacts {
    selected::script_window::facts(process_id)
}

pub(crate) fn key(process_id: u32, key: ScriptWindowKey) -> Result<(), ScriptWindowError> {
    selected::script_window::key(process_id, key)
}

pub(crate) fn pointer(
    process_id: u32,
    action: ScriptWindowPointerAction,
    x: i32,
    y: i32,
) -> Result<(), ScriptWindowError> {
    selected::script_window::pointer(process_id, action, x, y)
}

pub(crate) fn message(
    process_id: u32,
    message: ScriptWindowMessage,
) -> Result<isize, ScriptWindowError> {
    selected::script_window::message(process_id, message)
}

pub(crate) fn rect(process_id: u32, client: bool) -> Result<ScriptWindowRect, ScriptWindowError> {
    selected::script_window::rect(process_id, client)
}

pub(crate) fn resize(process_id: u32, width: i32, height: i32) -> Result<(), ScriptWindowError> {
    selected::script_window::resize(process_id, width, height)
}

pub(crate) fn control_exists(process_id: u32, id: i32) -> Result<(), ScriptWindowError> {
    selected::script_window::control_exists(process_id, id)
}

pub(crate) fn control_visible(process_id: u32, id: i32) -> Result<bool, ScriptWindowError> {
    selected::script_window::control_visible(process_id, id)
}

pub(crate) fn control_text(process_id: u32, id: i32) -> Result<String, ScriptWindowError> {
    selected::script_window::control_text(process_id, id)
}

pub(crate) fn control_set_text(
    process_id: u32,
    id: i32,
    text: &str,
) -> Result<(), ScriptWindowError> {
    selected::script_window::control_set_text(process_id, id, text)
}

pub(crate) fn control_click(process_id: u32, id: i32) -> Result<(), ScriptWindowError> {
    selected::script_window::control_click(process_id, id)
}
