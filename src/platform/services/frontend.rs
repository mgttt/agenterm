// Keep legacy path compatibility for callers that still import
// `platform::services::frontend`.
pub(crate) use crate::frontend::{
    request_gui_wake,
    request_gui_wake_best_effort,
    run_gui_entry,
};
