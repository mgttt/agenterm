//! Selected process-window automation facade.

pub use crate::contract::process_window::{
    ProcessWindowError, ProcessWindowFacts, ProcessWindowKey, ProcessWindowMessage,
    ProcessWindowPointerAction, ProcessWindowRect,
};

use crate::selected::process_window as adapter;

pub fn facts(process_id: u32) -> ProcessWindowFacts {
    adapter::facts(process_id)
}
pub fn key(process_id: u32, key: ProcessWindowKey) -> Result<(), ProcessWindowError> {
    adapter::key(process_id, key)
}
pub fn pointer(
    process_id: u32,
    action: ProcessWindowPointerAction,
    x: i32,
    y: i32,
) -> Result<(), ProcessWindowError> {
    adapter::pointer(process_id, action, x, y)
}
pub fn message(
    process_id: u32,
    message: ProcessWindowMessage,
) -> Result<isize, ProcessWindowError> {
    adapter::message(process_id, message)
}
pub fn rect(process_id: u32, client: bool) -> Result<ProcessWindowRect, ProcessWindowError> {
    adapter::rect(process_id, client)
}
pub fn resize(process_id: u32, width: i32, height: i32) -> Result<(), ProcessWindowError> {
    adapter::resize(process_id, width, height)
}
pub fn control_exists(process_id: u32, id: i32) -> Result<(), ProcessWindowError> {
    adapter::control_exists(process_id, id)
}
pub fn control_visible(process_id: u32, id: i32) -> Result<bool, ProcessWindowError> {
    adapter::control_visible(process_id, id)
}
pub fn control_text(process_id: u32, id: i32) -> Result<String, ProcessWindowError> {
    adapter::control_text(process_id, id)
}
pub fn control_set_text(process_id: u32, id: i32, text: &str) -> Result<(), ProcessWindowError> {
    adapter::control_set_text(process_id, id, text)
}
pub fn control_click(process_id: u32, id: i32) -> Result<(), ProcessWindowError> {
    adapter::control_click(process_id, id)
}
