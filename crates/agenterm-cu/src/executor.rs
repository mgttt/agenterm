//! `current` transport: in-process execution through `agenterm-platform` only.

use std::{
    thread,
    time::{Duration, Instant},
};

use agenterm_platform::window_enumerate::WindowInfo;

use crate::{
    audit::AuditLog,
    auth::{Authorization, Grant},
    command::{Command, PointerButton, WaitCondition},
    reply::{CuError, CuReply},
    target::TargetRef,
};

pub struct Executor {
    auth: Authorization,
}

impl Executor {
    pub fn new(auth: Authorization) -> Self {
        Self { auth }
    }

    pub fn execute(&self, command: &Command) -> CuReply {
        let required = command.required_grant();
        if !self.auth.allows(required) {
            return CuReply::err(
                command,
                CuError::new(
                    "refused",
                    format!(
                        "command requires {:?} grant; pass --grant or set AGENTERM_CU_GRANT",
                        required
                    ),
                ),
            );
        }

        if required == Grant::Actuate {
            if let Err(error) = self.audit_before(command) {
                return CuReply::err(command, error);
            }
        }

        let reply = match command.target() {
            TargetRef::Current => self.execute_current(command),
        };

        if required == Grant::Actuate {
            let _ = self.audit_after(command, &reply);
        }

        reply
    }

    fn audit_before(&self, command: &Command) -> Result<(), CuError> {
        let audit = AuditLog::open()?;
        audit.record_actuation(
            command.target(),
            command,
            Grant::Actuate,
            "attempt",
            None,
        )
    }

    fn audit_after(&self, command: &Command, reply: &CuReply) -> Result<(), CuError> {
        let audit = AuditLog::open()?;
        let outcome = if reply.ok { "ok" } else { "failed" };
        audit.record_actuation(
            command.target(),
            command,
            Grant::Actuate,
            outcome,
            reply.data.clone().or_else(|| {
                reply
                    .error
                    .as_ref()
                    .map(|error| serde_json::json!({ "code": error.code, "message": error.message }))
            }),
        )
    }

    fn execute_current(&self, command: &Command) -> CuReply {
        match self.run_current(command) {
            Ok(data) => CuReply::ok(command, data),
            Err(error) => CuReply::err(command, error),
        }
    }

    fn run_current(&self, command: &Command) -> Result<serde_json::Value, CuError> {
        match command {
            Command::Capabilities { .. } => Ok(capabilities_payload()),
            Command::Windows { .. } => {
                let windows = agenterm_platform::window_enumerate::enumerate_top_level()
                    .map_err(map_enum_err)?;
                serde_json::to_value(&windows)
                    .map_err(|error| CuError::new("serialize", error.to_string()))
            }
            Command::Tree { window, .. } => Ok(tree_payload(*window)),
            Command::Screenshot { path, window, .. } => screenshot(path, *window),
            Command::Click {
                window,
                node,
                coords,
                degraded,
                clicks,
                button,
                ..
            } => click(*window, node.as_deref(), *coords, *degraded, *clicks, *button),
            Command::SendText { text, .. } => {
                agenterm_platform::input_inject::type_text(text).map_err(map_inject_err)?;
                Ok(serde_json::json!({ "typed": text }))
            }
            Command::SendKeys { keys, .. } => {
                agenterm_platform::input_inject::send_keys(keys).map_err(map_inject_err)?;
                Ok(serde_json::json!({ "keys": keys }))
            }
            Command::Wait {
                timeout_ms,
                condition,
                ..
            } => wait(*timeout_ms, condition),
        }
    }
}

fn capabilities_payload() -> serde_json::Value {
    let status = |capability: agenterm_platform::Capability| {
        format!("{:?}", agenterm_platform::capability_status(capability))
    };
    serde_json::json!({
        "target": "current",
        "capabilities": {
            "windows": status(agenterm_platform::Capability::WindowEnumerate),
            "tree": "Unsupported",
            "screenshot": status(agenterm_platform::Capability::Screenshot),
            "input": status(agenterm_platform::Capability::InputInject),
        },
        "notes": {
            "tree": "AT-SPI2 control-tree enumeration is not wired in agenterm-platform on Linux; callers receive typed degraded tree responses until it ships."
        }
    })
}

fn tree_payload(window: Option<isize>) -> serde_json::Value {
    serde_json::json!({
        "degraded": true,
        "reason": "control-tree unavailable: AT-SPI2 is not wired in agenterm-platform",
        "addressing": "none",
        "window": window,
        "nodes": []
    })
}

fn screenshot(path: &str, window: Option<isize>) -> Result<serde_json::Value, CuError> {
    if path.is_empty() {
        return Err(CuError::new("invalid_input", "screenshot path is required"));
    }
    let raw = window.unwrap_or(0) as isize;
    let handle = unsafe { agenterm_platform::screenshot::ScreenshotWindowHandle::from_raw(raw) }
        .ok_or_else(|| CuError::new("invalid_input", "screenshot window handle must be non-zero"))?;
    let result = agenterm_platform::screenshot::capture_native_window_png(
        handle,
        std::path::Path::new(path),
        agenterm_platform::screenshot::NativeCaptureArea::Window,
    )
    .map_err(map_screenshot_err)?;
    Ok(serde_json::json!({
        "path": path,
        "window": window,
        "output_width": result.output_width,
        "output_height": result.output_height,
        "output_pixels": result.output_pixels,
    }))
}

fn click(
    window: Option<isize>,
    node: Option<&str>,
    coords: Option<[i32; 2]>,
    degraded: bool,
    clicks: u32,
    button: PointerButton,
) -> Result<serde_json::Value, CuError> {
    if let Some(node_id) = node {
        let _ = (window, node_id);
        return Err(CuError::new(
            "unsupported",
            "structured node click requires a control tree; tree is unavailable on this target",
        ));
    }
    let Some([x, y]) = coords else {
        return Err(CuError::new(
            "invalid_input",
            "click requires --window + --node when a control tree exists, or --coords with --degraded",
        ));
    };
    if !degraded {
        return Err(CuError::new(
            "invalid_input",
            "coordinate click requires --degraded so callers can see pixel addressing explicitly",
        ));
    }
    let button = match button {
        PointerButton::Left => agenterm_platform::input_inject::PointerButton::Left,
        PointerButton::Right => agenterm_platform::input_inject::PointerButton::Right,
        PointerButton::Middle => agenterm_platform::input_inject::PointerButton::Middle,
    };
    agenterm_platform::input_inject::pointer_click(
        agenterm_platform::input_inject::PointerPosition { x, y },
        button,
        clicks,
    )
    .map_err(map_inject_err)?;
    Ok(serde_json::json!({
        "addressing": "degraded-coordinates",
        "coords": [x, y],
        "window": window,
        "button": button,
        "clicks": clicks,
    }))
}

fn wait(timeout_ms: u64, condition: &WaitCondition) -> Result<serde_json::Value, CuError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(120_000));
    let poll = Duration::from_millis(50);
    let mut last_observation = serde_json::json!({ "windows": [] });

    while Instant::now() < deadline {
        let windows = agenterm_platform::window_enumerate::enumerate_top_level()
            .map_err(map_enum_err)?;
        last_observation = serde_json::json!({ "window_count": windows.len(), "windows": windows });
        if condition_met(condition, &windows) {
            return Ok(serde_json::json!({
                "met": true,
                "observation": last_observation,
            }));
        }
        thread::sleep(poll);
    }

    Ok(serde_json::json!({
        "met": false,
        "timeout_ms": timeout_ms,
        "observation": last_observation,
    }))
}

fn condition_met(condition: &WaitCondition, windows: &[WindowInfo]) -> bool {
    match condition {
        WaitCondition::WindowCountGte { count } => windows.len() >= *count,
        WaitCondition::WindowTitleContains { pattern } => {
            let pat = pattern.to_ascii_lowercase();
            windows
                .iter()
                .any(|window| window.title.to_ascii_lowercase().contains(&pat))
        }
        WaitCondition::FocusedHandle { handle } => windows
            .iter()
            .any(|window| window.focused && window.handle == *handle),
    }
}

fn map_enum_err(error: agenterm_platform::window_enumerate::WindowEnumerateError) -> CuError {
    match error {
        agenterm_platform::window_enumerate::WindowEnumerateError::Unsupported { reason } => {
            CuError::new("unsupported", reason.to_string())
        }
        agenterm_platform::window_enumerate::WindowEnumerateError::Failed { code, message } => {
            CuError::new(code.to_string(), message)
        }
        _ => CuError::new("unknown", "unknown window-enumerate error"),
    }
}

fn map_inject_err(error: agenterm_platform::input_inject::InputInjectError) -> CuError {
    match error {
        agenterm_platform::input_inject::InputInjectError::Unsupported { reason } => {
            CuError::new("unsupported", reason.to_string())
        }
        agenterm_platform::input_inject::InputInjectError::Failed { code, message } => {
            CuError::new(code.to_string(), message)
        }
        _ => CuError::new("unknown", "unknown input-inject error"),
    }
}

fn map_screenshot_err(
    error: agenterm_platform::contract::ui_screenshot::UiScreenshotError,
) -> CuError {
    match error {
        agenterm_platform::contract::ui_screenshot::UiScreenshotError::Unsupported { .. } => {
            CuError::new("unsupported", error.message())
        }
        agenterm_platform::contract::ui_screenshot::UiScreenshotError::Failed { .. } => {
            CuError::new(error.code(), error.message())
        }
        _ => CuError::new("unknown", "unknown screenshot error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::target::TargetRef;

    #[test]
    fn coordinate_click_requires_degraded_marker() {
        let auth = Authorization::new([Grant::Observe, Grant::Actuate].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::Click {
            target: TargetRef::Current,
            window: None,
            node: None,
            coords: Some([1, 2]),
            degraded: false,
            clicks: 1,
            button: PointerButton::Left,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn node_click_fails_when_tree_unavailable() {
        let auth = Authorization::new([Grant::Actuate].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::Click {
            target: TargetRef::Current,
            window: Some(42),
            node: Some("btn-1".into()),
            coords: None,
            degraded: false,
            clicks: 1,
            button: PointerButton::Left,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "unsupported");
    }

    #[test]
    fn actuation_without_grant_is_refused() {
        let auth = Authorization::new(Default::default());
        let executor = Executor::new(auth);
        let command = Command::SendText {
            target: TargetRef::Current,
            text: "hello".into(),
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "refused");
    }
}
