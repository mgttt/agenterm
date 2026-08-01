use crate::contract::process_window::*;

const fn unsupported(message: &'static str) -> ProcessWindowError {
    ProcessWindowError::new("process_window_unsupported", message, Some("unsupported"))
}

pub(crate) fn facts(_: u32) -> ProcessWindowFacts {
    ProcessWindowFacts {
        supported: false,
        present: false,
        window_id: 0,
        title: String::new(),
        foreground_window_id: 0,
        is_foreground: false,
    }
}
pub(crate) fn key(_: u32, _: ProcessWindowKey) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child-window input is not implemented on this platform",
    ))
}
pub(crate) fn pointer(
    _: u32,
    _: ProcessWindowPointerAction,
    _: i32,
    _: i32,
) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child-window input is not implemented on this platform",
    ))
}
pub(crate) fn message(_: u32, _: ProcessWindowMessage) -> Result<isize, ProcessWindowError> {
    Err(unsupported(
        "native child-window messaging is not implemented on this platform",
    ))
}
pub(crate) fn rect(_: u32, _: bool) -> Result<ProcessWindowRect, ProcessWindowError> {
    Err(unsupported(
        "native child-window bounds are not implemented on this platform",
    ))
}
pub(crate) fn resize(_: u32, _: i32, _: i32) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child-window resize is not implemented on this platform",
    ))
}
pub(crate) fn control_exists(_: u32, _: i32) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child controls are not implemented on this platform",
    ))
}
pub(crate) fn control_visible(_: u32, _: i32) -> Result<bool, ProcessWindowError> {
    Err(unsupported(
        "native child controls are not implemented on this platform",
    ))
}
pub(crate) fn control_text(_: u32, _: i32) -> Result<String, ProcessWindowError> {
    Err(unsupported(
        "native child controls are not implemented on this platform",
    ))
}
pub(crate) fn control_set_text(_: u32, _: i32, _: &str) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child controls are not implemented on this platform",
    ))
}
pub(crate) fn control_click(_: u32, _: i32) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child controls are not implemented on this platform",
    ))
}
