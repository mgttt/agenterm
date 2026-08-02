// Keep legacy path compatibility for callers that still import
// `platform::services::frontend`.
pub(crate) use crate::frontend::{
    attempt_gui_handoff,
    gui_launch_argument_error,
    gui_help_result,
    parse_gui_launch_target,
    request_gui_wake,
    request_gui_wake_best_effort,
    GuiHandoffResult,
    GuiLaunchResult,
    GuiWakeResult,
    run_gui_entry,
    UNIX_GUI_LAUNCH_POLICY,
    UNIX_GUI_USAGE,
    WINDOWS_GUI_LAUNCH_POLICY,
    WINDOWS_GUI_USAGE,
};
