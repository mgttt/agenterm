use crate::platform::contract::script_window::*;
const fn unsupported(message: &'static str) -> ScriptWindowError {
    ScriptWindowError::new(
        "process_window_input_unsupported",
        message,
        Some("unsupported"),
    )
}
const INPUT: ScriptWindowError =
    unsupported("native child-window input is not implemented on this platform");
const MESSAGE: ScriptWindowError =
    unsupported("native child-window messaging is not implemented on this platform");
const BOUNDS: ScriptWindowError =
    unsupported("native child-window bounds are not implemented on this platform");
const CLIENT_BOUNDS: ScriptWindowError =
    unsupported("native child-window client bounds are not implemented on this platform");
const RESIZE: ScriptWindowError =
    unsupported("native child-window resize is not implemented on this platform");
const CONTROLS: ScriptWindowError =
    unsupported("native child controls are not implemented on this platform");
pub(crate) fn facts(_: u32) -> ScriptWindowFacts {
    ScriptWindowFacts {
        supported: false,
        present: false,
        window_id: 0,
        title: String::new(),
        foreground_window_id: 0,
        is_foreground: false,
    }
}
pub(crate) fn key(_: u32, _: ScriptWindowKey) -> Result<(), ScriptWindowError> {
    Err(INPUT)
}
pub(crate) fn pointer(
    _: u32,
    _: ScriptWindowPointerAction,
    _: i32,
    _: i32,
) -> Result<(), ScriptWindowError> {
    Err(INPUT)
}
pub(crate) fn message(_: u32, _: ScriptWindowMessage) -> Result<isize, ScriptWindowError> {
    Err(MESSAGE)
}
pub(crate) fn rect(_: u32, client: bool) -> Result<ScriptWindowRect, ScriptWindowError> {
    Err(if client { CLIENT_BOUNDS } else { BOUNDS })
}
pub(crate) fn resize(_: u32, _: i32, _: i32) -> Result<(), ScriptWindowError> {
    Err(RESIZE)
}
pub(crate) fn control_exists(_: u32, _: i32) -> Result<(), ScriptWindowError> {
    Err(CONTROLS)
}
pub(crate) fn control_visible(_: u32, _: i32) -> Result<bool, ScriptWindowError> {
    Err(CONTROLS)
}
pub(crate) fn control_text(_: u32, _: i32) -> Result<String, ScriptWindowError> {
    Err(CONTROLS)
}
pub(crate) fn control_set_text(_: u32, _: i32, _: &str) -> Result<(), ScriptWindowError> {
    Err(CONTROLS)
}
pub(crate) fn control_click(_: u32, _: i32) -> Result<(), ScriptWindowError> {
    Err(CONTROLS)
}
