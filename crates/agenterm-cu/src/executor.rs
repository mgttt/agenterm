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
            Command::Click { .. } => click_command(command),
            Command::Focus {
                window,
                node,
                name,
                role,
                ..
            } => focus(*window, node.as_deref(), name.as_deref(), role.as_deref()),
            Command::SendText {
                text,
                window,
                name,
                role,
                ..
            } => send_text(text, *window, name.as_deref(), role.as_deref()),
            Command::SendKeys {
                keys,
                window,
                name,
                role,
                ..
            } => send_keys(keys, *window, name.as_deref(), role.as_deref()),
            Command::Wait {
                timeout_ms,
                condition,
                ..
            } => wait(*timeout_ms, condition),
            Command::WindowPlace { action, window, .. } => window_place(action, *window),
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
            "window_place": status(agenterm_platform::Capability::WindowOp),
        },
        "mapping": {
            "windows": "Win32 EnumWindows / Linux X11 _NET_CLIENT_LIST / macOS CGWindowList",
            "tree": "libagenterm agt_a11y_* → Windows UIA / Linux AT-SPI2 / macOS AX",
            "window_place": "Spectacle catalog via platform move_window (Win32 / X11 / macOS AX)",
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

fn click_command(command: &Command) -> Result<serde_json::Value, CuError> {
    let Command::Click {
        window,
        node,
        name,
        role,
        coords,
        degraded,
        clicks,
        button,
        ..
    } = command
    else {
        return Err(CuError::new(
            "invalid_input",
            "internal: expected click command",
        ));
    };
    let window = *window;
    let node = node.as_deref();
    let name = name.as_deref();
    let role = role.as_deref();
    let coords = *coords;
    let degraded = *degraded;
    let clicks = *clicks;
    let button = *button;
    if name.filter(|value| !value.is_empty()).is_some() && coords.is_some() {
        return Err(CuError::new(
            "invalid_input",
            "click --name is accessibility-tree addressing; do not pass --coords",
        ));
    }
    if let Some(resolved) = resolve_actuation_node(window, node, name, role, "click")? {
        for _ in 0..clicks.max(1) {
            mechanism::perform_node_action(window, &resolved.node_id, mechanism::NodeAction::Click)
                .map_err(map_mechanism_err)?;
        }
        return Ok(click_tree_payload(&resolved, window, clicks, button));
    }
    let Some([x, y]) = coords else {
        return Err(CuError::new(
            "invalid_input",
            "click requires --window + --node, --window + --name, or --coords with --degraded",
        ));
    };
    if !degraded {
        return Err(CuError::new(
            "invalid_input",
            "coordinate click requires --degraded so callers can see pixel addressing explicitly",
        ));
    }
    let inject_button = match button {
        PointerButton::Left => agenterm_platform::input_inject::PointerButton::Left,
        PointerButton::Right => agenterm_platform::input_inject::PointerButton::Right,
        PointerButton::Middle => agenterm_platform::input_inject::PointerButton::Middle,
    };
    agenterm_platform::input_inject::pointer_click(
        agenterm_platform::input_inject::PointerPosition { x, y },
        inject_button,
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

fn focus(
    window: Option<isize>,
    node: Option<&str>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let resolved = resolve_actuation_node(window, node, name, role, "focus")?.ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "focus requires --node <path-id> or --window + --name",
        )
    })?;
    mechanism::perform_node_action(window, &resolved.node_id, mechanism::NodeAction::Focus)
        .map_err(map_mechanism_err)?;
    Ok(focus_tree_payload(&resolved, window))
}

/// `send-text` with `--name` focuses the matched node first (same matcher as
/// `focus`), then reuses the existing keyboard-injection path. Without
/// `--name` it stays the plain "type into whatever is focused" verb.
fn send_text(
    text: &str,
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let Some(resolved) = resolve_actuation_node(window, None, name, role, "send-text")? else {
        agenterm_platform::input_inject::type_text(text).map_err(map_inject_err)?;
        return Ok(serde_json::json!({ "typed": text }));
    };
    mechanism::perform_node_action(window, &resolved.node_id, mechanism::NodeAction::Focus)
        .map_err(map_mechanism_err)?;
    agenterm_platform::input_inject::type_text(text).map_err(map_inject_err)?;
    agenterm_platform::accessibility_tree::drain_bus();
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "send-text",
        "typed": text,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `send-keys` with `--name` focuses the matched node first (same matcher as
/// `focus`/`send-text`), then reuses the existing chord-injection path. Without
/// `--name` it stays the plain "send to whatever is focused" verb.
fn send_keys(
    keys: &str,
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let Some(resolved) = resolve_actuation_node(window, None, name, role, "send-keys")? else {
        agenterm_platform::input_inject::send_keys(keys).map_err(map_inject_err)?;
        return Ok(serde_json::json!({ "keys": keys }));
    };
    mechanism::perform_node_action(window, &resolved.node_id, mechanism::NodeAction::Focus)
        .map_err(map_mechanism_err)?;
    agenterm_platform::input_inject::send_keys(keys).map_err(map_inject_err)?;
    agenterm_platform::accessibility_tree::drain_bus();
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "send-keys",
        "keys": keys,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

struct ResolvedNode {
    node_id: String,
    matched: Option<mechanism::A11yNode>,
    backend: Option<String>,
}

/// Shared addressing gate for structured click/focus: `--node` or `--name`,
/// never both, and `--name` never opens a coordinate/screenshot path.
/// `--name` requires exactly one showing/visible match.
fn resolve_actuation_node(
    window: Option<isize>,
    node: Option<&str>,
    name: Option<&str>,
    role: Option<&str>,
    verb: &str,
) -> Result<Option<ResolvedNode>, CuError> {
    let node = node.filter(|value| !value.is_empty());
    let name = name.filter(|value| !value.is_empty());
    if node.is_some() && name.is_some() {
        return Err(CuError::new(
            "invalid_input",
            format!("{verb} accepts --node or --name, not both"),
        ));
    }
    if let Some(pattern) = name {
        let (tree, matched) = resolve_named_node(window, pattern, role)?;
        return Ok(Some(ResolvedNode {
            node_id: matched.id.clone(),
            matched: Some(matched),
            backend: Some(tree.backend),
        }));
    }
    Ok(node.map(|node_id| ResolvedNode {
        node_id: node_id.to_owned(),
        matched: None,
        backend: None,
    }))
}

fn resolve_named_node(
    window: Option<isize>,
    pattern: &str,
    role: Option<&str>,
) -> Result<(mechanism::A11yTree, mechanism::A11yNode), CuError> {
    let Some(window) = window else {
        return Err(CuError::new(
            "invalid_input",
            "name addressing requires --window <handle>",
        ));
    };
    let tree = mechanism::tree_for_window(Some(window)).map_err(map_mechanism_err)?;
    let node = require_unique_showing_node(&tree.nodes, pattern, role)?.clone();
    Ok((tree, node))
}

fn click_tree_payload(
    resolved: &ResolvedNode,
    window: Option<isize>,
    clicks: u32,
    button: PointerButton,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "click",
        "clicks": clicks,
        "button": button,
    });
    attach_name_match(&mut payload, resolved);
    payload
}

fn focus_tree_payload(resolved: &ResolvedNode, window: Option<isize>) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "focus",
    });
    attach_name_match(&mut payload, resolved);
    payload
}

fn attach_name_match(payload: &mut serde_json::Value, resolved: &ResolvedNode) {
    let Some(matched) = &resolved.matched else {
        return;
    };
    if let Some(backend) = &resolved.backend {
        payload["backend"] = serde_json::json!(backend);
    }
    payload["matched"] = serde_json::to_value(matched).unwrap_or(serde_json::Value::Null);
}

fn name_scope(pattern: &str, role: Option<&str>) -> String {
    match role {
        Some(role) => format!("name contains '{pattern}' and role '{role}'"),
        None => format!("name contains '{pattern}'"),
    }
}

fn window_place(action_raw: &str, window: Option<isize>) -> Result<serde_json::Value, CuError> {
    let action = crate::place::PlaceAction::parse(action_raw).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            format!("unknown window-place action '{action_raw}'"),
        )
    })?;
    let windows =
        agenterm_platform::window_enumerate::enumerate_top_level().map_err(map_enum_err)?;
    let screens = agenterm_platform::window_enumerate::list_screens().map_err(map_enum_err)?;
    if screens.is_empty() {
        return Err(CuError::new("failed", "no screens available"));
    }
    let target_window = if let Some(handle) = window {
        windows
            .iter()
            .find(|item| item.handle == handle)
            .ok_or_else(|| CuError::new("failed", format!("window handle {handle} not found")))?
    } else {
        windows
            .iter()
            .find(|item| item.focused)
            .or_else(|| windows.first())
            .ok_or_else(|| CuError::new("failed", "no top-level window to place"))?
    };
    let handle = target_window.handle;
    let app_key = format!("{}:{}", target_window.process_id, target_window.app_name);
    let mut history = crate::place::PlaceHistory::open()
        .map_err(|error| CuError::new("failed", format!("history: {error}")))?;
    let before = crate::place::read_rect(handle)
        .unwrap_or_else(|_| crate::place::rect_from_bounds(target_window.bounds));
    let geo_screens: Vec<_> = screens.iter().map(crate::place::screen_from_info).collect();

    let (after_target, used_history) = if action.is_history() {
        let step = if matches!(action, crate::place::PlaceAction::Undo) {
            history.undo(&app_key)
        } else {
            history.redo(&app_key)
        };
        let Some((hist_handle, rect)) = step else {
            return Err(CuError::new(
                "unsupported",
                format!("{} has no {} history", app_key, action.kebab()),
            ));
        };
        (rect, Some(hist_handle))
    } else {
        let dest = crate::place::place(action, before, &geo_screens)
            .ok_or_else(|| CuError::new("failed", "could not compute destination rectangle"))?;
        (dest, None)
    };

    let apply_handle = used_history.unwrap_or(handle);
    let visible = crate::place::place(crate::place::PlaceAction::Fullscreen, before, &geo_screens)
        .unwrap_or(before);
    let (after, quantized, clamped) =
        crate::place::apply_rect(apply_handle, after_target, visible).map_err(map_op_err)?;
    if !action.is_history() {
        history.record(&app_key, apply_handle, before, after);
    }
    history
        .save()
        .map_err(|error| CuError::new("failed", format!("history save: {error}")))?;

    Ok(serde_json::json!({
        "action": action.kebab(),
        "spectacle_id": action.spectacle_id(),
        "window": apply_handle,
        "app": app_key,
        "before": { "x": before.x, "y": before.y, "width": before.width, "height": before.height },
        "after": { "x": after.x, "y": after.y, "width": after.width, "height": after.height },
        "quantized": quantized,
        "clamped": clamped,
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

/// Polls `tree` until exactly one showing node whose name contains `pattern`
/// (and whose role matches `role`, when given) appears. Two or more showing
/// hits fail typed (`a11y_node_ambiguous`) instead of taking the first.
/// Timeout is a typed failure so loop-until callers break on `ok:false`
/// instead of retrying blind.
fn wait_node(
    timeout_ms: u64,
    pattern: &str,
    role: Option<&str>,
    window: Option<isize>,
) -> Result<serde_json::Value, CuError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(120_000));
    let poll = Duration::from_millis(50);
    let mut polls = 0usize;
    let mut last_node_count = 0usize;
    let mut last_error: Option<CuError> = None;

    loop {
        polls += 1;
        match mechanism::tree_for_window(window) {
            Ok(tree) => {
                last_node_count = tree.nodes.len();
                last_error.take();
                let matches = showing_name_matches(&tree.nodes, pattern, role);
                match matches.len() {
                    0 => {}
                    1 => {
                        return Ok(serde_json::json!({
                            "met": true,
                            "addressing": "accessibility-tree",
                            "mechanism": "libagenterm",
                            "backend": tree.backend,
                            "window": window,
                            "polls": polls,
                            "node": matches[0],
                            "observation": { "node_count": last_node_count },
                        }));
                    }
                    count => return Err(name_match_error(pattern, role, count)),
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
    Err(CuError::new(
        "timeout",
        format!(
            "no showing accessibility node with {} after {timeout_ms}ms ({polls} polls, {detail})",
            name_scope(pattern, role)
        ),
    ))
}

fn showing_name_matches<'a>(
    nodes: &'a [mechanism::A11yNode],
    pattern: &str,
    role: Option<&str>,
) -> Vec<&'a mechanism::A11yNode> {
    let name_pat = pattern.to_ascii_lowercase();
    let role_pat = role.map(str::to_ascii_lowercase);
    nodes
        .iter()
        .filter(|node| node_matches(node, &name_pat, role_pat.as_deref()))
        .collect()
}

fn require_unique_showing_node<'a>(
    nodes: &'a [mechanism::A11yNode],
    pattern: &str,
    role: Option<&str>,
) -> Result<&'a mechanism::A11yNode, CuError> {
    let matches = showing_name_matches(nodes, pattern, role);
    match matches.len() {
        1 => Ok(matches[0]),
        count => Err(name_match_error(pattern, role, count)),
    }
}

fn name_match_error(pattern: &str, role: Option<&str>, count: usize) -> CuError {
    if count == 0 {
        return CuError::new(
            "a11y_node_not_found",
            format!(
                "no showing accessibility node with {}",
                name_scope(pattern, role)
            ),
        );
    }
    CuError::new(
        "a11y_node_ambiguous",
        format!(
            "{count} showing accessibility nodes with {}",
            name_scope(pattern, role)
        ),
    )
    .with_count(count)
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

fn map_op_err(error: agenterm_platform::window_op::WindowOpError) -> CuError {
    match error {
        agenterm_platform::window_op::WindowOpError::Unsupported { reason } => {
            CuError::new("unsupported", reason.to_string())
        }
        agenterm_platform::window_op::WindowOpError::Failed { code, message } => {
            CuError::new(code.to_string(), message)
        }
        _ => CuError::new("unknown", "unknown window-op error"),
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
            name: None,
            role: None,
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
            name: None,
            role: None,
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
        node_at("/0/1", name, role, states)
    }

    fn node_at(id: &str, name: &str, role: &str, states: &[&str]) -> mechanism::A11yNode {
        mechanism::A11yNode {
            id: id.into(),
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

    fn actuate_executor() -> Executor {
        Executor::new(Authorization::new(
            [Grant::Observe, Grant::Actuate].into_iter().collect(),
        ))
    }

    #[test]
    fn name_click_requires_window() {
        let command = Command::Click {
            target: TargetRef::Current,
            window: None,
            node: None,
            name: Some("Reload".into()),
            role: None,
            coords: None,
            degraded: false,
            clicks: 1,
            button: PointerButton::Left,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_and_node_are_exclusive() {
        let command = Command::Click {
            target: TargetRef::Current,
            window: Some(1),
            node: Some("/0/1".into()),
            name: Some("Reload".into()),
            role: None,
            coords: None,
            degraded: false,
            clicks: 1,
            button: PointerButton::Left,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_and_coords_are_exclusive() {
        let command = Command::Click {
            target: TargetRef::Current,
            window: Some(1),
            node: None,
            name: Some("Reload".into()),
            role: None,
            coords: Some([1, 2]),
            degraded: true,
            clicks: 1,
            button: PointerButton::Left,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_click_missing_node_is_typed() {
        let command = Command::Click {
            target: TargetRef::Current,
            window: Some(-1),
            node: None,
            name: Some("agenterm-no-such-node".into()),
            role: None,
            coords: None,
            degraded: false,
            clicks: 1,
            button: PointerButton::Left,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok, "missing name must not report success");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(code, "a11y_node_not_found" | "unsupported"),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn name_focus_missing_node_is_typed() {
        let command = Command::Focus {
            target: TargetRef::Current,
            window: Some(-1),
            node: None,
            name: Some("agenterm-no-such-node".into()),
            role: Some("button".into()),
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(code, "a11y_node_not_found" | "unsupported"),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn find_showing_node_reuses_wait_matcher() {
        let nodes = vec![
            node("hidden Reload", "button", &["enabled"]),
            node("Reload this page", "push button", &["showing", "enabled"]),
        ];
        let matched =
            require_unique_showing_node(&nodes, "reload", Some("button")).expect("shown match");
        assert_eq!(matched.name, "Reload this page");
        let missing = require_unique_showing_node(&nodes, "reload", Some("entry")).unwrap_err();
        assert_eq!(missing.code, "a11y_node_not_found");
        assert_eq!(missing.count, None);
    }

    #[test]
    fn two_showing_nodes_named_alike_are_ambiguous() {
        let nodes = vec![
            node_at("/0/1", "Tab search", "push button", &["showing", "enabled"]),
            node_at("/0/2", "Tab search", "push button", &["visible", "enabled"]),
        ];
        let err = require_unique_showing_node(&nodes, "Tab search", None).unwrap_err();
        assert_eq!(err.code, "a11y_node_ambiguous");
        assert_eq!(err.count, Some(2));
        assert!(
            err.message.contains("2"),
            "ambiguous error must carry the match count: {}",
            err.message
        );

        // A hidden duplicate must not count; only showing/visible nodes do.
        let one_showing = vec![
            node_at("/0/1", "Tab search", "push button", &["showing"]),
            node_at("/0/2", "Tab search", "push button", &["enabled"]),
        ];
        let matched = require_unique_showing_node(&one_showing, "Tab search", None)
            .expect("hidden twin is not a match");
        assert_eq!(matched.id, "/0/1");
    }

    #[test]
    fn actuation_without_grant_is_refused() {
        let auth = Authorization::new(Default::default());
        let executor = Executor::new(auth);
        let command = Command::SendText {
            target: TargetRef::Current,
            text: "hello".into(),
            window: None,
            name: None,
            role: None,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "refused");
    }

    #[test]
    fn window_place_without_grant_is_refused() {
        let auth = Authorization::new(Default::default());
        let executor = Executor::new(auth);
        let command = Command::WindowPlace {
            target: TargetRef::Current,
            action: "left-half".into(),
            window: None,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "refused");
    }

    #[test]
    fn window_place_unknown_action_is_invalid() {
        let auth = Authorization::new([Grant::Actuate].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::WindowPlace {
            target: TargetRef::Current,
            action: "tile-magic".into(),
            window: None,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_send_text_missing_node_is_typed_and_types_nothing() {
        let command = Command::SendText {
            target: TargetRef::Current,
            text: "hello".into(),
            window: Some(-1),
            name: Some("agenterm-no-such-node".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok, "missing name must not type into the wrong place");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(code, "a11y_node_not_found" | "unsupported"),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn name_send_text_requires_window() {
        let command = Command::SendText {
            target: TargetRef::Current,
            text: "hello".into(),
            window: None,
            name: Some("Address and search bar".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_send_keys_requires_window() {
        let command = Command::SendKeys {
            target: TargetRef::Current,
            keys: "enter".into(),
            window: None,
            name: Some("Address and search bar".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_send_keys_missing_node_is_typed_and_sends_nothing() {
        let command = Command::SendKeys {
            target: TargetRef::Current,
            keys: "enter".into(),
            window: Some(-1),
            name: Some("agenterm-no-such-node".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok, "missing name must not send keys somewhere else");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(code, "a11y_node_not_found" | "unsupported"),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn name_send_keys_two_showing_matches_are_ambiguous() {
        // `send-keys --name` resolves through this exact matcher, so two
        // showing hits must abort before any chord reaches the display.
        let nodes = vec![
            node_at("/0/1", "Address and search bar", "entry", &["showing"]),
            node_at("/0/2", "Address and search bar", "entry", &["visible"]),
        ];
        let err = require_unique_showing_node(&nodes, "Address and search bar", None).unwrap_err();
        assert_eq!(err.code, "a11y_node_ambiguous");
        assert_eq!(err.count, Some(2));
    }
}
