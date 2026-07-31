//! macOS frontend adapter entry.

#[path = "../unix/frontend/mod.rs"]
mod unix;

pub(crate) use unix::request_gui_wake;
pub use unix::run_gui_entry;
