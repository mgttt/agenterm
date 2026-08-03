//! macOS console line-editor adapter.

#[path = "../unix/console_line_editor.rs"]
mod unix;

pub(crate) use unix::Editor;
