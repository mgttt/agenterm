//! OS-neutral Platform Facade services.

pub(crate) mod control_center;
pub(crate) mod control_center_shell;
pub(crate) mod ipc;
pub(crate) mod paths;
#[allow(unused_imports)]
pub(crate) use agenterm_platform::process;
#[allow(unused_imports)]
pub(crate) use agenterm_platform::pty;
#[allow(unused_imports)]
pub(crate) use agenterm_platform::runtime;
pub(crate) mod script_clipboard;
pub(crate) mod script_files;
pub(crate) mod script_host;
pub(crate) mod script_http;
pub(crate) mod script_stream;
pub(crate) mod script_window;
pub(crate) mod supervisor_audit;
pub(crate) mod ui_clipboard;
#[allow(unused_imports)]
pub(crate) use agenterm_platform::font as ui_font;
pub(crate) mod ui_screenshot;
