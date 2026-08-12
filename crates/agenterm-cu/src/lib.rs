//! `agenterm-cu` — computer-use foundation (PRD_02_28/29/30).
//!
//! Layering (outer may depend on inner only):
//!
//! ```text
//! native primitive     agenterm-platform (owned there, consumed here)
//!     ↑
//! abstract command     this crate: target-agnostic `Command` set + typed results
//!     ↑
//! current transport    in-process executor for the local `current` tier
//!     ↑
//! shell command        `cu` binary (src/bin/cu.rs)
//! ```
//!
//! The current tier is the local degenerate case of the `ssh`/`rdp`/`vnc`
//! family: transport is in-process, commands stay identical.

use agenterm_platform::window_enumerate::WindowInfo;
use serde::{Deserialize, Serialize};

/// Abstract, target-agnostic command set (PRD_02_29).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "verb", rename_all = "kebab-case")]
pub enum Command {
    /// Declare what the target supports (never discovered by failure).
    Capabilities,
    WindowList,
    WindowFind {
        pattern: String,
    },
    WindowShow {
        handle: i64,
        state: WindowShowState,
    },
    WindowMove {
        handle: i64,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    WindowTopmost {
        handle: i64,
        topmost: bool,
    },
    WindowClose {
        handle: i64,
    },
    PointerMove {
        x: i32,
        y: i32,
    },
    PointerClick {
        x: i32,
        y: i32,
        button: PointerButton,
        clicks: u32,
    },
    TypeText {
        text: String,
    },
    Keys {
        shortcut: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowShowState {
    Hide,
    Show,
    Minimize,
    Maximize,
    Restore,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

/// Typed failure (PRD_02_30: unsupported is declared, failures are typed).
#[derive(Clone, Debug, Serialize)]
pub struct CuError {
    pub code: String,
    pub message: String,
}

impl CuError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Machine-readable reply; the CLI renders this JSON as-is.
#[derive(Clone, Debug, Serialize)]
pub struct CuReply {
    pub ok: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CuError>,
}

impl CuReply {
    fn ok(command: &Command, data: serde_json::Value) -> Self {
        Self {
            ok: true,
            command: command.verb(),
            data: Some(data),
            error: None,
        }
    }

    fn err(command: &Command, error: CuError) -> Self {
        Self {
            ok: false,
            command: command.verb(),
            data: None,
            error: Some(error),
        }
    }
}

impl Command {
    pub fn verb(&self) -> String {
        match self {
            Command::Capabilities => "capabilities".into(),
            Command::WindowList => "window-list".into(),
            Command::WindowFind { .. } => "window-find".into(),
            Command::WindowShow { .. } => "window-show".into(),
            Command::WindowMove { .. } => "window-move".into(),
            Command::WindowTopmost { .. } => "window-topmost".into(),
            Command::WindowClose { .. } => "window-close".into(),
            Command::PointerMove { .. } => "pointer-move".into(),
            Command::PointerClick { .. } => "pointer-click".into(),
            Command::TypeText { .. } => "type-text".into(),
            Command::Keys { .. } => "keys".into(),
        }
    }
}

/// `current` transport: executes commands in-process against the local host
/// through `agenterm-platform` only (PRD_02_28: cu never opens raw OS APIs).
pub struct CurrentExecutor;

impl CurrentExecutor {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(&self, command: &Command) -> CuReply {
        match self.run(command) {
            Ok(data) => CuReply::ok(command, data),
            Err(error) => CuReply::err(command, error),
        }
    }

    fn run(&self, command: &Command) -> Result<serde_json::Value, CuError> {
        match command {
            Command::Capabilities => {
                let status = |c: agenterm_platform::Capability| {
                    format!("{:?}", agenterm_platform::capability_status(c))
                };
                Ok(serde_json::json!({
                    "target": "current",
                    "capabilities": {
                        "window-enum": status(agenterm_platform::Capability::WindowEnumerate),
                        "window-op": status(agenterm_platform::Capability::WindowOp),
                        "input-inject": status(agenterm_platform::Capability::InputInject),
                        "screenshot": status(agenterm_platform::Capability::Screenshot),
                    },
                }))
            }
            Command::WindowList => {
                let windows = agenterm_platform::window_enumerate::enumerate_top_level()
                    .map_err(|e| map_enum_err(e))?;
                serde_json::to_value(&windows)
                    .map_err(|e| CuError::new("serialize", e.to_string()))
            }
            Command::WindowFind { pattern } => {
                let windows = agenterm_platform::window_enumerate::enumerate_top_level()
                    .map_err(|e| map_enum_err(e))?;
                let pat = pattern.to_ascii_lowercase();
                let hits: Vec<&WindowInfo> = windows
                    .iter()
                    .filter(|w| {
                        w.title.to_ascii_lowercase().contains(&pat)
                            || w.app_name.to_ascii_lowercase().contains(&pat)
                            || (pattern.starts_with("pid:")
                                && w.process_id.to_string() == pattern[4..])
                    })
                    .collect();
                serde_json::to_value(&hits)
                    .map_err(|e| CuError::new("serialize", e.to_string()))
            }
            Command::WindowShow { handle, state } => {
                let state = match state {
                    WindowShowState::Hide => agenterm_platform::window_op::WindowShowState::Hide,
                    WindowShowState::Show => agenterm_platform::window_op::WindowShowState::Show,
                    WindowShowState::Minimize => {
                        agenterm_platform::window_op::WindowShowState::Minimize
                    }
                    WindowShowState::Maximize => {
                        agenterm_platform::window_op::WindowShowState::Maximize
                    }
                    WindowShowState::Restore => {
                        agenterm_platform::window_op::WindowShowState::Restore
                    }
                };
                agenterm_platform::window_op::show(*handle as isize, state)
                    .map_err(|e| map_op_err(e))?;
                Ok(serde_json::json!({ "handle": handle, "state": state }))
            }
            Command::WindowMove {
                handle,
                x,
                y,
                width,
                height,
            } => {
                agenterm_platform::window_op::move_window(*handle as isize, *x, *y, *width, *height)
                    .map_err(|e| map_op_err(e))?;
                Ok(serde_json::json!({ "handle": handle, "x": x, "y": y, "width": width, "height": height }))
            }
            Command::WindowTopmost { handle, topmost } => {
                agenterm_platform::window_op::set_topmost(*handle as isize, *topmost)
                    .map_err(|e| map_op_err(e))?;
                Ok(serde_json::json!({ "handle": handle, "topmost": topmost }))
            }
            Command::WindowClose { handle } => {
                agenterm_platform::window_op::close(*handle as isize).map_err(|e| map_op_err(e))?;
                Ok(serde_json::json!({ "handle": handle }))
            }
            Command::PointerMove { x, y } => {
                agenterm_platform::input_inject::pointer_move(
                    agenterm_platform::input_inject::PointerPosition { x: *x, y: *y },
                )
                .map_err(|e| map_inject_err(e))?;
                Ok(serde_json::json!({ "x": x, "y": y }))
            }
            Command::PointerClick {
                x,
                y,
                button,
                clicks,
            } => {
                let button = match button {
                    PointerButton::Left => agenterm_platform::input_inject::PointerButton::Left,
                    PointerButton::Right => {
                        agenterm_platform::input_inject::PointerButton::Right
                    }
                    PointerButton::Middle => {
                        agenterm_platform::input_inject::PointerButton::Middle
                    }
                };
                agenterm_platform::input_inject::pointer_click(
                    agenterm_platform::input_inject::PointerPosition { x: *x, y: *y },
                    button,
                    *clicks,
                )
                .map_err(|e| map_inject_err(e))?;
                Ok(serde_json::json!({ "x": x, "y": y, "button": button, "clicks": clicks }))
            }
            Command::TypeText { text } => {
                agenterm_platform::input_inject::type_text(text).map_err(|e| map_inject_err(e))?;
                Ok(serde_json::json!({ "typed": text }))
            }
            Command::Keys { shortcut } => {
                agenterm_platform::input_inject::send_keys(shortcut).map_err(|e| map_inject_err(e))?;
                Ok(serde_json::json!({ "shortcut": shortcut }))
            }
        }
    }
}

fn map_enum_err(e: agenterm_platform::window_enumerate::WindowEnumerateError) -> CuError {
    match e {
        agenterm_platform::window_enumerate::WindowEnumerateError::Unsupported { reason } => {
            CuError::new("unsupported", reason.to_string())
        }
        agenterm_platform::window_enumerate::WindowEnumerateError::Failed { code, message } => {
            CuError::new(code.to_string(), message)
        }
        _ => CuError::new("unknown", "unknown window-enumerate error"),
    }
}

fn map_op_err(e: agenterm_platform::window_op::WindowOpError) -> CuError {
    match e {
        agenterm_platform::window_op::WindowOpError::Unsupported { reason } => {
            CuError::new("unsupported", reason.to_string())
        }
        agenterm_platform::window_op::WindowOpError::Failed { code, message } => {
            CuError::new(code.to_string(), message)
        }
        _ => CuError::new("unknown", "unknown window-op error"),
    }
}

fn map_inject_err(e: agenterm_platform::input_inject::InputInjectError) -> CuError {
    match e {
        agenterm_platform::input_inject::InputInjectError::Unsupported { reason } => {
            CuError::new("unsupported", reason.to_string())
        }
        agenterm_platform::input_inject::InputInjectError::Failed { code, message } => {
            CuError::new(code.to_string(), message)
        }
        _ => CuError::new("unknown", "unknown input-inject error"),
    }
}
