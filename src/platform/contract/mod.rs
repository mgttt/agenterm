//! OS-neutral facade contracts and typed failures.

#[cfg(test)]
pub(crate) mod adapter;
pub(crate) mod control_center_shell;
pub(crate) mod ipc;
pub(crate) mod ipc_transport;
#[allow(unused_imports)]
pub(crate) use agenterm_platform::contract::process;
pub(crate) mod pty;
pub(crate) mod runtime;
pub(crate) mod script_clipboard;
pub(crate) mod script_window;
pub(crate) mod supervisor_audit;
pub(crate) mod ui_clipboard;
pub(crate) mod ui_font;
pub(crate) mod ui_screenshot;
pub(crate) mod webview;
