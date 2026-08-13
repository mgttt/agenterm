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
    mechanism,
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
        audit.record_actuation(command.target(), command, Grant::Actuate, "attempt", None)
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
                reply.error.as_ref().map(
                    |error| serde_json::json!({ "code": error.code, "message": error.message }),
                )
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
            Command::Tree { window, .. } => tree_payload(*window),
            Command::Screenshot { path, window, .. } => screenshot(path, *window),
            Command::Click {
                window,
                node,
                coords,
                degraded,
                clicks,
                button,
                ..
            } => click(
                *window,
                node.as_deref(),
                *coords,
                *degraded,
                *clicks,
                *button,
            ),
            Command::Focus { window, node, .. } => focus(*window, node),
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
    let tree_status = if mechanism::accessibility_tree_available() {
        "Available"
    } else {
        "Unsupported"
    };
    serde_json::json!({
        "target": "current",
        "mechanism": "libagenterm",
        "capabilities": {
            "windows": status(agenterm_platform::Capability::WindowEnumerate),
            "tree": tree_status,
            "screenshot": status(agenterm_platform::Capability::Screenshot),
            "input": status(agenterm_platform::Capability::InputInject),
        },
        "mapping": {
            "windows": "Win32 EnumWindows / Linux X11 _NET_CLIENT_LIST / macOS AX (planned)",
            "tree": "libagenterm agt_a11y_* → Windows UIA / Linux AT-SPI2 / macOS AX",
        },
        "gaps": {
            "windows": "still agenterm-platform until agt_window_enumerate ships",
            "screenshot": "still agenterm-platform until unified ABI path",
            "input_degraded": "still agenterm-platform until agt_input_inject ships",
        }
    })
}

fn tree_payload(window: Option<isize>) -> Result<serde_json::Value, CuError> {
    let tree = mechanism::tree_for_window(window).map_err(map_mechanism_err)?;
    Ok(serde_json::json!({
        "degraded": false,
        "backend": tree.backend,
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "window": tree.window_handle,
        "root_id": tree.root_id,
        "nodes": tree.nodes,
    }))
}

fn screenshot(path: &str, window: Option<isize>) -> Result<serde_json::Value, CuError> {
    if path.is_empty() {
        return Err(CuError::new("invalid_input", "screenshot path is required"));
    }
    let raw = window.unwrap_or(0) as isize;
    let handle = unsafe { agenterm_platform::screenshot::ScreenshotWindowHandle::from_raw(raw) }
        .ok_or_else(|| {
            CuError::new("invalid_input", "screenshot window handle must be non-zero")
        })?;
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
        for _ in 0..clicks.max(1) {
            mechanism::perform_node_action(window, node_id, mechanism::NodeAction::Click)
                .map_err(map_mechanism_err)?;
        }
        return Ok(serde_json::json!({
            "addressing": "accessibility-tree",
            "mechanism": "libagenterm",
            "node": node_id,
            "window": window,
            "action": "click",
            "clicks": clicks,
            "button": button,
        }));
    }
    let Some([x, y]) = coords else {
        return Err(CuError::new(
            "invalid_input",
            "click requires --window + --node for structured AT-SPI actuation, or --coords with --degraded",
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

fn focus(window: Option<isize>, node_id: &str) -> Result<serde_json::Value, CuError> {
    mechanism::perform_node_action(window, node_id, mechanism::NodeAction::Focus)
        .map_err(map_mechanism_err)?;
    Ok(serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": node_id,
        "window": window,
        "action": "focus",
    }))
}

fn wait(timeout_ms: u64, condition: &WaitCondition) -> Result<serde_json::Value, CuError> {
    if let WaitCondition::NodeNameContains {
        pattern,
        role,
        window,
    } = condition
    {
        return wait_node(timeout_ms, pattern, role.as_deref(), *window);
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(120_000));
    let poll = Duration::from_millis(50);
    let mut last_observation = serde_json::json!({ "windows": [] });

    while Instant::now() < deadline {
        let windows =
            agenterm_platform::window_enumerate::enumerate_top_level().map_err(map_enum_err)?;
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
        // Polled against the accessibility tree, not the window list.
        WaitCondition::NodeNameContains { .. } => false,
    }
}

/// Polls `tree` until a showing node whose name contains `pattern` (and whose
/// role matches `role`, when given) appears. Timeout is a typed failure so
/// loop-until callers break on `ok:false` instead of retrying blind.
fn wait_node(
    timeout_ms: u64,
    pattern: &str,
    role: Option<&str>,
    window: Option<isize>,
) -> Result<serde_json::Value, CuError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(120_000));
    let poll = Duration::from_millis(50);
    let name_pat = pattern.to_ascii_lowercase();
    let role_pat = role.map(str::to_ascii_lowercase);
    let mut polls = 0usize;
    let mut last_node_count = 0usize;
    let mut last_error: Option<CuError> = None;

    loop {
        polls += 1;
        match mechanism::tree_for_window(window) {
            Ok(tree) => {
                last_node_count = tree.nodes.len();
                last_error.take();
                if let Some(node) = tree
                    .nodes
                    .iter()
                    .find(|node| node_matches(node, &name_pat, role_pat.as_deref()))
                {
                    return Ok(serde_json::json!({
                        "met": true,
                        "addressing": "accessibility-tree",
                        "mechanism": "libagenterm",
                        "backend": tree.backend,
                        "window": window,
                        "polls": polls,
                        "node": node,
                        "observation": { "node_count": last_node_count },
                    }));
                }
            }
            // The tree can be missing outright; that is not something more
            // polling will fix.
            Err(mechanism::MechanismError::Unsupported) => {
                return Err(map_mechanism_err(mechanism::MechanismError::Unsupported));
            }
            // A scoped window may not have an AT-SPI root yet — keep polling and
            // report the last failure if we run out of time.
            Err(error) => last_error = Some(map_mechanism_err(error)),
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(poll);
    }

    let detail = match last_error {
        Some(error) => format!("last tree read failed: {} ({})", error.message, error.code),
        None => format!("last tree read had {last_node_count} nodes"),
    };
    let scope = match role {
        Some(role) => format!("name contains '{pattern}' and role '{role}'"),
        None => format!("name contains '{pattern}'"),
    };
    Err(CuError::new(
        "timeout",
        format!(
            "no showing accessibility node with {scope} after {timeout_ms}ms ({polls} polls, {detail})"
        ),
    ))
}

fn node_matches(node: &mechanism::A11yNode, name_pat: &str, role_pat: Option<&str>) -> bool {
    if !node_is_showing(node) {
        return false;
    }
    if !node.name.to_ascii_lowercase().contains(name_pat) {
        return false;
    }
    match role_pat {
        Some(role) => node.role.to_ascii_lowercase().contains(role),
        None => true,
    }
}

fn node_is_showing(node: &mechanism::A11yNode) -> bool {
    node.states
        .iter()
        .any(|state| state.eq_ignore_ascii_case("showing") || state.eq_ignore_ascii_case("visible"))
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

fn map_mechanism_err(error: mechanism::MechanismError) -> CuError {
    match error {
        mechanism::MechanismError::Unsupported => {
            CuError::new("unsupported", "accessibility-tree mechanism unavailable")
        }
        mechanism::MechanismError::Failed { code, message } => CuError::new(code, message),
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
    fn node_click_uses_accessibility_tree_when_node_is_set() {
        let auth = Authorization::new([Grant::Actuate].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::Click {
            target: TargetRef::Current,
            window: None,
            node: Some("/0/999999".into()),
            coords: None,
            degraded: false,
            clicks: 1,
            button: PointerButton::Left,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(
                code,
                "a11y_node_not_found" | "a11y_backend_failed" | "unsupported"
            ),
            "unexpected code: {code}"
        );
    }

    fn node(name: &str, role: &str, states: &[&str]) -> mechanism::A11yNode {
        mechanism::A11yNode {
            id: "/0/1".into(),
            parent_id: Some("/0".into()),
            role: role.into(),
            name: name.into(),
            states: states.iter().map(|state| (*state).to_owned()).collect(),
            bounds: mechanism::A11yBounds {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            actions: Vec::new(),
            text: None,
        }
    }

    #[test]
    fn node_match_is_case_insensitive_and_requires_showing() {
        let shown = node("Reload this page", "push button", &["showing", "enabled"]);
        assert!(node_matches(&shown, "reload", None));
        assert!(node_matches(&shown, "reload", Some("push button")));
        assert!(!node_matches(&shown, "reload", Some("entry")));
        assert!(!node_matches(&shown, "bookmark", None));

        let hidden = node("Reload this page", "push button", &["enabled"]);
        assert!(!node_matches(&hidden, "reload", None));
    }

    #[test]
    fn node_wait_timeout_is_a_typed_failure() {
        let auth = Authorization::new([Grant::Observe].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::Wait {
            target: TargetRef::Current,
            timeout_ms: 1,
            condition: WaitCondition::NodeNameContains {
                pattern: "agenterm-no-such-node".into(),
                role: None,
                window: Some(-1),
            },
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok, "timeout must not report success");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(code, "timeout" | "unsupported"),
            "unexpected code: {code}"
        );
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
