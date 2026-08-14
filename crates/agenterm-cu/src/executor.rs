//! `current` transport: in-process execution through the shared libagenterm
//! dynamic library (`mechanism` + `dynlib`) only.

use std::{
    thread,
    time::{Duration, Instant},
};

use crate::mechanism::window_enumerate::WindowInfo;

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

        if required == Grant::Actuate
            && let Err(error) = self.audit_before(command)
        {
            return CuReply::err(command, error);
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
                let windows = mechanism::window_enumerate::enumerate_top_level()
                    .map_err(map_mechanism_err)?;
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
            Command::Copy {
                window, name, role, ..
            } => copy(*window, name.as_deref(), role.as_deref()),
            Command::Paste {
                text,
                window,
                name,
                role,
                ..
            } => paste(text.as_deref(), *window, name.as_deref(), role.as_deref()),
            Command::SendKeys {
                keys,
                window,
                name,
                role,
                ..
            } => send_keys(keys, *window, name.as_deref(), role.as_deref()),
            Command::Scroll {
                window, name, role, ..
            } => scroll(*window, name.as_deref(), role.as_deref()),
            Command::GetExtents {
                window, name, role, ..
            } => get_extents(*window, name.as_deref(), role.as_deref()),
            Command::Select {
                start,
                end,
                window,
                name,
                role,
                ..
            } => select(*window, name.as_deref(), role.as_deref(), *start, *end),
            Command::GetSelection {
                window, name, role, ..
            } => get_selection(*window, name.as_deref(), role.as_deref()),
            Command::SetCaret {
                offset,
                window,
                name,
                role,
                ..
            } => set_caret(*window, name.as_deref(), role.as_deref(), *offset),
            Command::GetCaret {
                window, name, role, ..
            } => get_caret(*window, name.as_deref(), role.as_deref()),
            Command::GetText {
                window, name, role, ..
            } => get_text(*window, name.as_deref(), role.as_deref()),
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
    let status = |capability: mechanism::Capability| {
        format!("{:?}", mechanism::capability_status(capability))
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
            "windows": status(mechanism::Capability::WindowEnumerate),
            "tree": tree_status,
            "screenshot": status(mechanism::Capability::Screenshot),
            "input": status(mechanism::Capability::InputInject),
            "window_place": status(mechanism::Capability::WindowOp),
            "desktop_host": status(mechanism::Capability::DesktopHost),
        },
        "mapping": {
            "windows": "libagenterm agt_window_enumerate",
            "tree": "libagenterm agt_a11y_* → Windows UIA / Linux AT-SPI2 / macOS AX",
            "window_place": "Spectacle catalog via libagenterm agt_native_window_*",
        },
        "gaps": {
            "windows": "none — shared agenterm.dll (milestone 46)",
            "screenshot": "none — shared agenterm.dll (milestone 46)",
            "input_degraded": "none — shared agenterm.dll (milestone 46)",
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
    let raw = window.unwrap_or(0);
    if raw == 0 {
        return Err(CuError::new(
            "invalid_input",
            "screenshot window handle must be non-zero",
        ));
    }
    let result = mechanism::screenshot::capture_native_window_png(raw, std::path::Path::new(path))
        .map_err(map_mechanism_err)?;
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
        PointerButton::Left => mechanism::input_inject::PointerButton::Left,
        PointerButton::Right => mechanism::input_inject::PointerButton::Right,
        PointerButton::Middle => mechanism::input_inject::PointerButton::Middle,
    };
    mechanism::input_inject::pointer_click(x, y, inject_button, clicks)
        .map_err(map_mechanism_err)?;
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

/// `send-text` with `--name` writes through native AT-SPI
/// `EditableText` (`SetTextContents` / `InsertText`) or, when the named
/// showing node exposes `Text` + `editable` but not `EditableText`
/// (Chrome 151, WebKitGTK/Reasonix `<textarea>`), through AT-SPI `Text`
/// plus the toolkit set-value. Success is confirmed by `Text.GetText`.
/// The WebKit eval helper's `OK` and `last_text_write_via` are write-path
/// reports; `wait --text-equals` must poll GetText again. A named showing
/// node with no writeable text interface typed-fails
/// (`a11y_text_unavailable`) and never falls through to XTest /
/// `input_inject::type_text`.
///
/// `--window` without `--name` writes that same path on the showing
/// focused node — the same innermost `Text.GetText` candidate
/// `get-text --window` reads — so `focus --name X` then
/// `send-text --window H TEXT` then `get-text --window H` closes the
/// loop on Chrome `GetTextField` and the Reasonix composer
/// (`Message Reasonix…` under `scripts/reasonix-desktop-a11y.sh`).
/// WebKit 2.52 still has no `EditableText`; the write is AT-SPI `Text`
/// plus the eval-helper set-value (`id=composer-input`). Never XTest
/// when `--window` is set. Without `--window` it stays the plain
/// "type into whatever is focused" inject.
fn send_text(
    text: &str,
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    if let Some(resolved) = resolve_actuation_node(window, None, name, role, "send-text")? {
        return send_text_to_node(text, window, resolved);
    }
    if role.filter(|value| !value.is_empty()).is_some() {
        return Err(CuError::new(
            "invalid_input",
            "send-text --role requires --name <pattern>",
        ));
    }
    if window.is_some() {
        let (resolved, _current) = get_text_focused(window)?;
        return send_text_to_node(text, window, resolved);
    }
    mechanism::input_inject::type_text(text).map_err(map_mechanism_err)?;
    Ok(serde_json::json!({ "typed": text }))
}

fn send_text_to_node(
    text: &str,
    window: Option<isize>,
    resolved: ResolvedNode,
) -> Result<serde_json::Value, CuError> {
    mechanism::set_node_text(window, &resolved.node_id, text).map_err(map_mechanism_err)?;
    let _ = mechanism::accessibility_tree::drain_bus();
    let via = mechanism::accessibility_tree::last_text_write_via().unwrap_or_default();
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "send-text",
        "typed": text,
        "via": via,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `copy --name` reads AT-SPI `Text.GetText` (`agt_a11y_node_get_text`)
/// from the unique showing named node and publishes that UTF-8 onto the
/// native clipboard (`agt_clipboard_set_text`). On Linux X11 the owner
/// process stays in the `SetSelectionOwner` event loop so a later
/// `paste --name` (no `--text`) can `ConvertSelection`. A named showing
/// node with no Text interface typed-fails (`a11y_text_unavailable`) and
/// never falls through to XTest / `--coords` / screenshot. `--name` is
/// required. `matched.text` is the resolve-time snapshot; the copied
/// payload is independent GetText. Live close-the-circuit includes Chrome
/// fixture fields and the Reasonix composer (`Message Reasonix…`): paste
/// after copy still uses the WebKit eval-helper set-value path; only
/// `wait --text-equals` GetText proves the restore.
fn copy(
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "copy requires --window <handle> --name <pattern>",
        )
    })?;
    let resolved =
        resolve_actuation_node(window, None, Some(name), role, "copy")?.ok_or_else(|| {
            CuError::new(
                "invalid_input",
                "copy requires --window <handle> --name <pattern>",
            )
        })?;
    let text = mechanism::get_node_text(window, &resolved.node_id).map_err(map_mechanism_err)?;
    mechanism::clipboard::publish_text(&text).map_err(map_mechanism_err)?;
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "copy",
        "text": text,
        "via": "gettext",
        "clipboard": true,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `paste --name` writes clipboard text into the unique showing named
/// field through the same native AT-SPI `EditableText` / `Text` path as
/// named `send-text`. `--text` only seeds the clipboard; the field write
/// always reads `agt_clipboard_get_text` first. A named showing node with
/// no writeable text interface typed-fails (`a11y_text_unavailable`) and
/// never falls through to XTest / `--coords` / screenshot. `--name` is
/// required: there is no "paste into whatever is focused" verb. A miss or
/// an ambiguous name writes nothing. `matched.text` is the resolve-time
/// snapshot; `wait --text-equals` must poll `Text.GetText` independently.
fn paste(
    seed: Option<&str>,
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "paste requires --window <handle> --name <pattern>",
        )
    })?;
    let resolved =
        resolve_actuation_node(window, None, Some(name), role, "paste")?.ok_or_else(|| {
            CuError::new(
                "invalid_input",
                "paste requires --window <handle> --name <pattern>",
            )
        })?;
    if let Some(seed) = seed {
        mechanism::clipboard::set_text(seed).map_err(map_mechanism_err)?;
    }
    let pasted = mechanism::clipboard::get_text().map_err(map_mechanism_err)?;
    mechanism::set_node_text(window, &resolved.node_id, &pasted).map_err(map_mechanism_err)?;
    let _ = mechanism::accessibility_tree::drain_bus();
    let via = mechanism::accessibility_tree::last_text_write_via().unwrap_or_default();
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "paste",
        "typed": pasted,
        "via": via,
        "clipboard": true,
        "seeded": seed.is_some(),
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `send-keys` with `--name` delivers the chord through native AT-SPI
/// Device/key events (`DeviceEventListener.NotifyEvent`). A named showing
/// node with no key interface typed-fails (`a11y_key_unavailable`) and
/// never falls through to XTest / `input_inject::send_keys`. Without
/// `--name` it stays the plain "send to whatever is focused" verb.
fn send_keys(
    keys: &str,
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let Some(resolved) = resolve_actuation_node(window, None, name, role, "send-keys")? else {
        mechanism::input_inject::send_keys(keys).map_err(map_mechanism_err)?;
        return Ok(serde_json::json!({ "keys": keys }));
    };
    mechanism::send_node_keys(window, &resolved.node_id, keys).map_err(map_mechanism_err)?;
    let _ = mechanism::accessibility_tree::drain_bus();
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "send-keys",
        "keys": keys,
        "via": "device-event",
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `scroll --name` is one-shot AT-SPI `Component.ScrollTo(TopEdge)`
/// (`agt_a11y_node_scroll`). Missing / false / `UnknownMethod` typed-fails
/// (`a11y_scroll_unavailable`). Never Action `scroll*`, XTest wheel,
/// `GenerateMouseEvent`, or `--coords`. `matched.extents` / snapshot
/// bounds do not count as proof.
fn scroll(
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "scroll requires --window <handle> --name <pattern>",
        )
    })?;
    let resolved =
        resolve_actuation_node(window, None, Some(name), role, "scroll")?.ok_or_else(|| {
            CuError::new(
                "invalid_input",
                "scroll requires --window <handle> --name <pattern>",
            )
        })?;
    mechanism::scroll_node(window, &resolved.node_id).map_err(map_mechanism_err)?;
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "scroll",
        "via": "scroll-to",
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `get-extents --name` reads independent AT-SPI `Component.GetExtents(Screen)`
/// (`agt_a11y_node_get_extents`). Snapshot `node.bounds` do not count.
/// Empty extents typed-fail (`a11y_extents_unavailable`).
fn get_extents(
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "get-extents requires --window <handle> --name <pattern>",
        )
    })?;
    let resolved = resolve_actuation_node(window, None, Some(name), role, "get-extents")?
        .ok_or_else(|| {
            CuError::new(
                "invalid_input",
                "get-extents requires --window <handle> --name <pattern>",
            )
        })?;
    let extents =
        mechanism::get_node_extents(window, &resolved.node_id).map_err(map_mechanism_err)?;
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "get-extents",
        "via": "get-extents",
        "extents": {
            "x": extents.x,
            "y": extents.y,
            "width": extents.width,
            "height": extents.height,
        },
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `select --name` is one-shot AT-SPI `Text.SetSelection`
/// (`agt_a11y_node_set_selection`). Missing Text / `UnknownMethod`
/// typed-fails (`a11y_selection_unavailable`). SetSelection false
/// typed-fails (`a11y_selection_no_effect`). Never XTest, mouse-drag,
/// `--coords`, or screenshot. The reply is not proof — `get-selection`
/// is the independent `GetNSelections` / `GetSelection` readback.
fn select(
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
    start: i32,
    end: i32,
) -> Result<serde_json::Value, CuError> {
    if start < 0 || end < start {
        return Err(CuError::new(
            "invalid_input",
            format!("select requires 0 <= --start <= --end; got {start}..{end}"),
        ));
    }
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "select requires --window <handle> --name <pattern> --start N --end M",
        )
    })?;
    let resolved =
        resolve_actuation_node(window, None, Some(name), role, "select")?.ok_or_else(|| {
            CuError::new(
                "invalid_input",
                "select requires --window <handle> --name <pattern> --start N --end M",
            )
        })?;
    mechanism::set_node_selection(window, &resolved.node_id, start, end)
        .map_err(map_mechanism_err)?;
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "select",
        "via": "set-selection",
        "start": start,
        "end": end,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `get-selection --name` reads independent AT-SPI `Text.GetNSelections`
/// + `GetSelection(0)` (`agt_a11y_node_get_selection`). The `select`
///
/// The reply payload does not count. Missing Text typed-fails
/// (`a11y_selection_unavailable`). `n == 0` is empty success.
fn get_selection(
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "get-selection requires --window <handle> --name <pattern>",
        )
    })?;
    let resolved = resolve_actuation_node(window, None, Some(name), role, "get-selection")?
        .ok_or_else(|| {
            CuError::new(
                "invalid_input",
                "get-selection requires --window <handle> --name <pattern>",
            )
        })?;
    let selection =
        mechanism::get_node_selection(window, &resolved.node_id).map_err(map_mechanism_err)?;
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "get-selection",
        "via": "get-selection",
        "n": selection.n,
        "start": selection.start,
        "end": selection.end,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `set-caret --name` is one-shot AT-SPI `Text.SetCaretOffset`
/// (`agt_a11y_node_set_caret_offset`). Missing Text / `UnknownMethod`
/// typed-fails (`a11y_caret_unavailable`). SetCaretOffset false
/// typed-fails (`a11y_caret_no_effect`). Never XTest, `--coords`, or
/// screenshot. The reply is not proof — `get-caret` is the independent
/// `CaretOffset` readback.
fn set_caret(
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
    offset: i32,
) -> Result<serde_json::Value, CuError> {
    if offset < 0 {
        return Err(CuError::new(
            "invalid_input",
            format!("set-caret requires --offset >= 0; got {offset}"),
        ));
    }
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "set-caret requires --window <handle> --name <pattern> --offset N",
        )
    })?;
    let resolved = resolve_actuation_node(window, None, Some(name), role, "set-caret")?
        .ok_or_else(|| {
            CuError::new(
                "invalid_input",
                "set-caret requires --window <handle> --name <pattern> --offset N",
            )
        })?;
    mechanism::set_node_caret_offset(window, &resolved.node_id, offset)
        .map_err(map_mechanism_err)?;
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "set-caret",
        "via": "set-caret-offset",
        "offset": offset,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `get-caret --name` reads independent AT-SPI `Text.CaretOffset`
/// (`agt_a11y_node_get_caret_offset`). The `set-caret` reply payload
/// does not count. Missing Text typed-fails (`a11y_caret_unavailable`).
fn get_caret(
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        CuError::new(
            "invalid_input",
            "get-caret requires --window <handle> --name <pattern>",
        )
    })?;
    let resolved = resolve_actuation_node(window, None, Some(name), role, "get-caret")?
        .ok_or_else(|| {
            CuError::new(
                "invalid_input",
                "get-caret requires --window <handle> --name <pattern>",
            )
        })?;
    let offset =
        mechanism::get_node_caret_offset(window, &resolved.node_id).map_err(map_mechanism_err)?;
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "get-caret",
        "via": "get-caret-offset",
        "offset": offset,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

/// `get-text --name` reads independent AT-SPI `Text.GetText`
/// (`agt_a11y_node_get_text`) once for the unique showing named node.
/// Without `--name` it reads the focused node instead: toolkits may mark
/// a whole ancestor chain `focused` (Reasonix marks a container that has
/// no Text interface), so the candidates are every showing node carrying
/// the AT-SPI `focused` state, probed innermost-first, and the winner is
/// the innermost one that actually exposes `Text.GetText`. So
/// `focus --name X` then `get-text --window H` closes the loop on
/// whatever holds focus. This is the same text authority
/// `wait --text-equals` polls, exposed as a first-class one-shot readback
/// so an independent observation does not need a wait timeout. Not
/// `send-text` / `paste` / `copy` `matched.text`, `last_text_write_via`,
/// the WebKit eval helper's queued-job `OK`, or a tree snapshot `text`.
/// No focused candidate with Text typed-fails (`a11y_text_unavailable`).
/// Never XTest / `--coords` / screenshot.
fn get_text(
    window: Option<isize>,
    name: Option<&str>,
    role: Option<&str>,
) -> Result<serde_json::Value, CuError> {
    let name = name.filter(|value| !value.is_empty());
    let (resolved, text) = match name {
        Some(name) => {
            let resolved = resolve_actuation_node(window, None, Some(name), role, "get-text")?
                .ok_or_else(|| {
                    CuError::new(
                        "invalid_input",
                        "get-text requires --window <handle> [--name <pattern>]",
                    )
                })?;
            let text =
                mechanism::get_node_text(window, &resolved.node_id).map_err(map_mechanism_err)?;
            (resolved, text)
        }
        None => {
            if role.is_some() {
                return Err(CuError::new(
                    "invalid_input",
                    "get-text --role requires --name <pattern>",
                ));
            }
            get_text_focused(window)?
        }
    };
    let mut payload = serde_json::json!({
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "node": resolved.node_id,
        "window": window,
        "action": "get-text",
        "via": "gettext",
        "text": text,
    });
    attach_name_match(&mut payload, &resolved);
    Ok(payload)
}

struct ResolvedNode {
    node_id: String,
    matched: Option<mechanism::A11yNode>,
    backend: Option<String>,
}

/// Focused-node text readback: no name pattern, no coordinates — the
/// toolkit's own focus report picks the node. Probes every showing
/// `focused` node innermost-first with independent `Text.GetText` and
/// returns the first that exposes it. A `focused` ancestor without the
/// Text interface (`a11y_text_unavailable`) falls through to the next
/// candidate; any other mechanism failure aborts. All candidates missing
/// Text re-raises the innermost candidate's `a11y_text_unavailable`.
fn get_text_focused(window: Option<isize>) -> Result<(ResolvedNode, String), CuError> {
    let Some(handle) = window else {
        return Err(CuError::new(
            "invalid_input",
            "get-text without --name requires --window <handle>",
        ));
    };
    let tree = mechanism::tree_for_window(Some(handle)).map_err(map_mechanism_err)?;
    let candidates = focused_candidates_innermost_first(&tree.nodes);
    if candidates.is_empty() {
        return Err(CuError::new(
            "a11y_node_not_found",
            "no showing focused accessibility node in window tree",
        ));
    }
    let mut text_unavailable: Option<CuError> = None;
    for node in candidates {
        match mechanism::get_node_text(window, &node.id) {
            Ok(text) => {
                let resolved = ResolvedNode {
                    node_id: node.id.clone(),
                    matched: Some(node.clone()),
                    backend: Some(tree.backend.clone()),
                };
                return Ok((resolved, text));
            }
            Err(mechanism::MechanismError::Failed { code, message })
                if code == "a11y_text_unavailable" =>
            {
                text_unavailable.get_or_insert(CuError::new(code, message));
            }
            Err(other) => return Err(map_mechanism_err(other)),
        }
    }
    Err(text_unavailable.expect("non-empty candidates yield Ok or a stored error"))
}

/// Every showing node carrying the AT-SPI `focused` state, deepest child
/// path first, so an innermost real widget wins over a `focused` ancestor
/// container. Depth is the child-index path length; the stable sort keeps
/// snapshot pre-order between equal depths.
fn focused_candidates_innermost_first(nodes: &[mechanism::A11yNode]) -> Vec<&mechanism::A11yNode> {
    let mut candidates: Vec<&mechanism::A11yNode> = nodes
        .iter()
        .filter(|node| node_is_showing(node))
        .filter(|node| node.states.iter().any(|state| state == "focused"))
        .collect();
    candidates.sort_by_key(|node| std::cmp::Reverse(node.id.matches('/').count()));
    candidates
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
    let windows = mechanism::window_enumerate::enumerate_top_level().map_err(map_mechanism_err)?;
    let screens = mechanism::window_enumerate::list_screens().map_err(map_mechanism_err)?;
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
        crate::place::apply_rect(apply_handle, after_target, visible).map_err(map_mechanism_err)?;
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
    match condition {
        WaitCondition::NodeNameContains {
            pattern,
            role,
            window,
        } => return wait_node(timeout_ms, pattern, role.as_deref(), *window),
        WaitCondition::NodeTextEquals {
            expected,
            name,
            role,
            window,
        } => {
            return wait_node_text(
                timeout_ms,
                expected,
                name,
                role.as_deref(),
                *window,
                NodeTextMatch::Equals,
            );
        }
        WaitCondition::NodeTextContains {
            substring,
            name,
            role,
            window,
        } => {
            return wait_node_text(
                timeout_ms,
                substring,
                name,
                role.as_deref(),
                *window,
                NodeTextMatch::Contains,
            );
        }
        _ => {}
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(120_000));
    let poll = Duration::from_millis(50);
    let mut last_observation = serde_json::json!({ "windows": [] });

    while Instant::now() < deadline {
        let windows =
            mechanism::window_enumerate::enumerate_top_level().map_err(map_mechanism_err)?;
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
        WaitCondition::NodeNameContains { .. }
        | WaitCondition::NodeTextEquals { .. }
        | WaitCondition::NodeTextContains { .. } => false,
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
            Err(mechanism::MechanismError::Unsupported { .. }) => {
                return Err(map_mechanism_err(mechanism::MechanismError::Unsupported {
                    reason: "accessibility-tree mechanism unavailable".to_owned(),
                }));
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NodeTextMatch {
    Equals,
    Contains,
}

impl NodeTextMatch {
    fn flag(self) -> &'static str {
        match self {
            Self::Equals => "--text-equals",
            Self::Contains => "--text-contains",
        }
    }

    fn matches(self, text: &str, expected: &str) -> bool {
        match self {
            Self::Equals => text == expected,
            Self::Contains => text.contains(expected),
        }
    }

    fn timeout_verb(self) -> &'static str {
        match self {
            Self::Equals => "did not reach text",
            Self::Contains => "did not contain",
        }
    }
}

/// Polls AT-SPI `Text.GetText` (`agt_a11y_node_get_text`) on the unique
/// showing node addressed by `name` until that independent text equals
/// `expected` (`--text-equals`) or contains it (`--text-contains`). The
/// tree snapshot `node.text`, a prior `send-text` / `paste` / `copy`
/// `matched.text`, `last_text_write_via`, and the WebKit eval helper's
/// queued-job `OK` (Reasonix composer) are not this predicate. Timeout
/// is typed so loop-until callers break on `ok:false`.
fn wait_node_text(
    timeout_ms: u64,
    expected: &str,
    name: &str,
    role: Option<&str>,
    window: Option<isize>,
    match_kind: NodeTextMatch,
) -> Result<serde_json::Value, CuError> {
    if window.is_none() {
        return Err(CuError::new(
            "invalid_input",
            format!("wait {} requires --window <handle>", match_kind.flag()),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.min(120_000));
    let poll = Duration::from_millis(50);
    let mut polls = 0usize;
    let mut last_node_count = 0usize;
    let mut last_text: Option<String> = None;
    let mut last_error: Option<CuError> = None;

    loop {
        polls += 1;
        match mechanism::tree_for_window(window) {
            Ok(tree) => {
                last_node_count = tree.nodes.len();
                last_error.take();
                let matches = showing_name_matches(&tree.nodes, name, role);
                match matches.len() {
                    0 => {}
                    1 => match mechanism::get_node_text(window, &matches[0].id) {
                        Ok(text) => {
                            last_text = Some(text.clone());
                            if match_kind.matches(&text, expected) {
                                return Ok(text_equals_success(
                                    &tree.backend,
                                    window,
                                    polls,
                                    matches[0],
                                    &text,
                                    last_node_count,
                                ));
                            }
                        }
                        Err(error @ mechanism::MechanismError::Unsupported { .. }) => {
                            return Err(map_mechanism_err(error));
                        }
                        Err(error) => last_error = Some(map_mechanism_err(error)),
                    },
                    count => return Err(name_match_error(name, role, count)),
                }
            }
            Err(error @ mechanism::MechanismError::Unsupported { .. }) => {
                return Err(map_mechanism_err(error));
            }
            Err(error) => last_error = Some(map_mechanism_err(error)),
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(poll);
    }

    Err(CuError::new(
        "timeout",
        format!(
            "accessibility node with {} {} {expected:?} after {timeout_ms}ms ({polls} polls, {})",
            name_scope(name, role),
            match_kind.timeout_verb(),
            text_equals_timeout_detail(last_text.as_deref(), last_error.as_ref(), last_node_count,)
        ),
    ))
}

/// Success payload for `--text-equals` / `--text-contains`. `gettext` is
/// the only text authority: snapshot `node.text` is overwritten so a
/// sidecar tree walk or `send-text` / `paste` `matched.text` cannot be
/// mistaken for the hit. Published `text` is the full independent GetText.
fn text_equals_success(
    backend: &str,
    window: Option<isize>,
    polls: usize,
    node: &mechanism::A11yNode,
    gettext: &str,
    node_count: usize,
) -> serde_json::Value {
    let mut node = node.clone();
    node.text = Some(gettext.to_owned());
    serde_json::json!({
        "met": true,
        "addressing": "accessibility-tree",
        "mechanism": "libagenterm",
        "backend": backend,
        "window": window,
        "polls": polls,
        "node": node,
        "text": gettext,
        "via": "gettext",
        "observation": {
            "node_count": node_count,
            "text": gettext,
        },
    })
}

fn text_equals_timeout_detail(
    last_text: Option<&str>,
    last_error: Option<&CuError>,
    last_node_count: usize,
) -> String {
    match (last_text, last_error) {
        (Some(text), _) => format!("last GetText={text:?}"),
        (None, Some(error)) => {
            format!("last GetText failed: {} ({})", error.message, error.code)
        }
        (None, None) => format!("last tree read had {last_node_count} nodes"),
    }
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

fn map_mechanism_err(error: mechanism::MechanismError) -> CuError {
    match error {
        mechanism::MechanismError::Unsupported { reason } => CuError::new("unsupported", reason),
        mechanism::MechanismError::Failed { code, message } => CuError::new(code, message),
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
                "a11y_invalid_node_id"
                    | "a11y_node_not_found"
                    | "a11y_backend_failed"
                    | "dylib_load"
                    | "unsupported"
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

    #[test]
    fn node_text_equals_timeout_is_a_typed_failure() {
        let auth = Authorization::new([Grant::Observe].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::Wait {
            target: TargetRef::Current,
            timeout_ms: 1,
            condition: WaitCondition::NodeTextEquals {
                expected: "agenterm-no-such-text".into(),
                name: "agenterm-no-such-node".into(),
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
    fn text_equals_success_publishes_gettext_not_snapshot_text() {
        let mut snapshot = node("Message Reasonix…", "text", &["showing", "editable"]);
        snapshot.text = Some("stale-snapshot".into());
        snapshot.id = "/0/0/0/0/0/0/0/0/8/1/0".into();
        let payload =
            text_equals_success("at-spi2", Some(4194318), 2, &snapshot, "RXWAIT-TYPED", 130);
        assert_eq!(payload["via"], "gettext");
        assert_eq!(payload["text"], "RXWAIT-TYPED");
        assert_eq!(payload["observation"]["text"], "RXWAIT-TYPED");
        assert_eq!(payload["node"]["text"], "RXWAIT-TYPED");
        assert_ne!(payload["via"], "text");
        assert_ne!(payload["node"]["text"], "stale-snapshot");
    }

    #[test]
    fn focused_candidates_order_innermost_widget_before_focused_ancestor() {
        // Reasonix shape: a focused container without Text sits above the
        // focused composer textarea; the composer must be probed first.
        let panel = node_at("/0/0/0/0/0/0/0", "", "filler", &["showing", "focused"]);
        let composer = node_at(
            "/0/0/0/0/0/0/0/0/5/1/0",
            "Message Reasonix…",
            "text",
            &["showing", "editable", "focused"],
        );
        let hidden = node_at("/0/0/0/0/0/0/0/0/9", "", "text", &["focused"]);
        let unfocused = node_at("/0/1", "Send", "push button", &["showing"]);
        let nodes = vec![panel.clone(), composer.clone(), hidden, unfocused];
        let candidates = focused_candidates_innermost_first(&nodes);
        let ids: Vec<&str> = candidates.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(ids, vec![composer.id.as_str(), panel.id.as_str()]);
    }

    #[test]
    fn text_equals_timeout_reports_last_gettext() {
        assert_eq!(
            text_equals_timeout_detail(Some("RXWAIT-TYPED"), None, 130),
            "last GetText=\"RXWAIT-TYPED\""
        );
        let failed = CuError::new("a11y_text_unavailable", "no Text.GetText");
        assert_eq!(
            text_equals_timeout_detail(None, Some(&failed), 130),
            "last GetText failed: no Text.GetText (a11y_text_unavailable)"
        );
    }

    #[test]
    fn node_text_equals_requires_window() {
        let auth = Authorization::new([Grant::Observe].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::Wait {
            target: TargetRef::Current,
            timeout_ms: 1,
            condition: WaitCondition::NodeTextEquals {
                expected: "x".into(),
                name: "FixtureField".into(),
                role: None,
                window: None,
            },
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn node_text_contains_timeout_is_a_typed_failure() {
        let auth = Authorization::new([Grant::Observe].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::Wait {
            target: TargetRef::Current,
            timeout_ms: 1,
            condition: WaitCondition::NodeTextContains {
                substring: "agenterm-no-such-sub".into(),
                name: "agenterm-no-such-node".into(),
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
    fn node_text_contains_requires_window() {
        let auth = Authorization::new([Grant::Observe].into_iter().collect());
        let executor = Executor::new(auth);
        let command = Command::Wait {
            target: TargetRef::Current,
            timeout_ms: 1,
            condition: WaitCondition::NodeTextContains {
                substring: "GATE".into(),
                name: "FixtureField".into(),
                role: None,
                window: None,
            },
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
        assert!(
            reply
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("--text-contains"),
            "missing-window message should name the flag"
        );
    }

    #[test]
    fn text_contains_matches_substring_of_independent_gettext() {
        assert!(NodeTextMatch::Contains.matches("34aGATEXXXX", "GATE"));
        assert!(!NodeTextMatch::Contains.matches("34aGATEXXXX", "NOPE"));
        assert!(!NodeTextMatch::Equals.matches("34aGATEXXXX", "GATE"));
        assert!(NodeTextMatch::Equals.matches("34aGATEXXXX", "34aGATEXXXX"));
    }

    #[test]
    fn text_contains_success_publishes_full_gettext_not_substring() {
        let mut snapshot = node("FixtureField", "entry", &["showing", "editable"]);
        snapshot.text = Some("stale-snapshot".into());
        let payload =
            text_equals_success("at-spi2", Some(4194318), 2, &snapshot, "34aGATEXXXX", 12);
        assert_eq!(payload["via"], "gettext");
        assert_eq!(payload["text"], "34aGATEXXXX");
        assert!(payload["text"].as_str().unwrap().contains("GATE"));
        assert_ne!(payload["text"], "GATE");
        assert_ne!(payload["node"]["text"], "stale-snapshot");
    }

    fn actuate_executor() -> Executor {
        Executor::new(Authorization::new(
            [Grant::Observe, Grant::Actuate].into_iter().collect(),
        ))
    }

    fn observe_executor() -> Executor {
        Executor::new(Authorization::new([Grant::Observe].into_iter().collect()))
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
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
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
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
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
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
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
    fn send_text_role_without_name_is_typed() {
        let command = Command::SendText {
            target: TargetRef::Current,
            text: "hello".into(),
            window: Some(1),
            name: None,
            role: Some("entry".into()),
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
        assert!(
            reply
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("--role requires --name"),
            "role-without-name message should name the addressing contract"
        );
    }

    #[test]
    fn send_text_window_without_name_does_not_xtest() {
        // A synthetic window must take the focused AT-SPI path, not
        // input_inject::type_text. Success here would mean XTest spray.
        let command = Command::SendText {
            target: TargetRef::Current,
            text: "hello".into(),
            window: Some(-1),
            name: None,
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(
            !reply.ok,
            "send-text --window without --name must not fall through to XTest"
        );
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(
                code,
                "a11y_node_not_found"
                    | "a11y_tree_empty"
                    | "a11y_text_unavailable"
                    | "a11y_backend_failed"
                    | "unsupported"
                    | "failed"
            ),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn name_copy_requires_name() {
        let command = Command::Copy {
            target: TargetRef::Current,
            window: Some(1),
            name: None,
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_copy_requires_window() {
        let command = Command::Copy {
            target: TargetRef::Current,
            window: None,
            name: Some("FixtureSource".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_copy_missing_node_is_typed_and_copies_nothing() {
        let command = Command::Copy {
            target: TargetRef::Current,
            window: Some(-1),
            name: Some("agenterm-no-such-node".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok, "missing name must not seed the clipboard");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn copy_without_grant_is_refused() {
        let auth = Authorization::new(Default::default());
        let executor = Executor::new(auth);
        let command = Command::Copy {
            target: TargetRef::Current,
            window: Some(1),
            name: Some("FixtureSource".into()),
            role: None,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "refused");
    }

    #[test]
    fn name_paste_requires_name() {
        let command = Command::Paste {
            target: TargetRef::Current,
            text: Some("hello".into()),
            window: Some(1),
            name: None,
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_paste_requires_window() {
        let command = Command::Paste {
            target: TargetRef::Current,
            text: Some("hello".into()),
            window: None,
            name: Some("FixtureField".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_paste_missing_node_is_typed_and_writes_nothing() {
        let command = Command::Paste {
            target: TargetRef::Current,
            text: Some("hello".into()),
            window: Some(-1),
            name: Some("agenterm-no-such-node".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(
            !reply.ok,
            "missing name must not paste into the wrong place"
        );
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn paste_without_grant_is_refused() {
        let auth = Authorization::new(Default::default());
        let executor = Executor::new(auth);
        let command = Command::Paste {
            target: TargetRef::Current,
            text: Some("hello".into()),
            window: Some(1),
            name: Some("FixtureField".into()),
            role: None,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "refused");
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
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
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

    #[test]
    fn name_scroll_requires_name() {
        let command = Command::Scroll {
            target: TargetRef::Current,
            window: Some(1),
            name: None,
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_scroll_requires_window() {
        let command = Command::Scroll {
            target: TargetRef::Current,
            window: None,
            name: Some("OffscreenField".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_scroll_missing_node_is_typed_and_scrolls_nothing() {
        let command = Command::Scroll {
            target: TargetRef::Current,
            window: Some(-1),
            name: Some("agenterm-no-such-node".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok, "missing name must not scroll");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn scroll_without_grant_is_refused() {
        let auth = Authorization::new(Default::default());
        let executor = Executor::new(auth);
        let command = Command::Scroll {
            target: TargetRef::Current,
            window: Some(1),
            name: Some("OffscreenField".into()),
            role: None,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "refused");
    }

    #[test]
    fn name_get_extents_requires_name() {
        let command = Command::GetExtents {
            target: TargetRef::Current,
            window: Some(1),
            name: None,
            role: None,
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_get_extents_requires_window() {
        let command = Command::GetExtents {
            target: TargetRef::Current,
            window: None,
            name: Some("OffscreenField".into()),
            role: None,
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_get_extents_missing_node_is_typed() {
        let command = Command::GetExtents {
            target: TargetRef::Current,
            window: Some(-1),
            name: Some("agenterm-no-such-node".into()),
            role: None,
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok, "missing name must not invent extents");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn scroll_and_get_extents_verbs_are_named() {
        let scroll = Command::Scroll {
            target: TargetRef::Current,
            window: Some(1),
            name: Some("OffscreenField".into()),
            role: None,
        };
        let extents = Command::GetExtents {
            target: TargetRef::Current,
            window: Some(1),
            name: Some("ScrollViewport".into()),
            role: None,
        };
        assert_eq!(scroll.verb(), "scroll");
        assert_eq!(extents.verb(), "get-extents");
        assert_eq!(scroll.required_grant(), Grant::Actuate);
        assert_eq!(extents.required_grant(), Grant::Observe);
    }

    #[test]
    fn name_select_requires_name() {
        let command = Command::Select {
            target: TargetRef::Current,
            start: 0,
            end: 4,
            window: Some(1),
            name: None,
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_select_requires_window() {
        let command = Command::Select {
            target: TargetRef::Current,
            start: 0,
            end: 4,
            window: None,
            name: Some("SelectField".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_select_rejects_inverted_range() {
        let command = Command::Select {
            target: TargetRef::Current,
            start: 4,
            end: 0,
            window: Some(1),
            name: Some("SelectField".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_select_missing_node_is_typed_and_selects_nothing() {
        let command = Command::Select {
            target: TargetRef::Current,
            start: 0,
            end: 4,
            window: Some(-1),
            name: Some("agenterm-no-such-node".into()),
            role: None,
        };
        let reply = actuate_executor().execute(&command);
        assert!(!reply.ok, "missing name must not select");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn select_without_grant_is_refused() {
        let auth = Authorization::new(Default::default());
        let executor = Executor::new(auth);
        let command = Command::Select {
            target: TargetRef::Current,
            start: 0,
            end: 4,
            window: Some(1),
            name: Some("SelectField".into()),
            role: None,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "refused");
    }

    #[test]
    fn name_get_selection_requires_name() {
        let command = Command::GetSelection {
            target: TargetRef::Current,
            window: Some(1),
            name: None,
            role: None,
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_get_selection_requires_window() {
        let command = Command::GetSelection {
            target: TargetRef::Current,
            window: None,
            name: Some("SelectField".into()),
            role: None,
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn name_get_selection_missing_node_is_typed() {
        let command = Command::GetSelection {
            target: TargetRef::Current,
            window: Some(-1),
            name: Some("agenterm-no-such-node".into()),
            role: None,
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok, "missing name must not invent a selection");
        let code = reply.error.as_ref().unwrap().code.as_str();
        assert!(
            matches!(
                code,
                "a11y_node_not_found" | "a11y_tree_empty" | "a11y_backend_failed" | "unsupported"
            ),
            "unexpected code: {code}"
        );
    }

    #[test]
    fn select_and_get_selection_verbs_are_named() {
        let select = Command::Select {
            target: TargetRef::Current,
            start: 0,
            end: 4,
            window: Some(1),
            name: Some("SelectField".into()),
            role: None,
        };
        let get_selection = Command::GetSelection {
            target: TargetRef::Current,
            window: Some(1),
            name: Some("SelectField".into()),
            role: None,
        };
        assert_eq!(select.verb(), "select");
        assert_eq!(get_selection.verb(), "get-selection");
        assert_eq!(select.required_grant(), Grant::Actuate);
        assert_eq!(get_selection.required_grant(), Grant::Observe);
    }

    #[test]
    fn set_caret_without_grant_is_refused() {
        let auth = Authorization::new(Default::default());
        let executor = Executor::new(auth);
        let command = Command::SetCaret {
            target: TargetRef::Current,
            offset: 2,
            window: Some(1),
            name: Some("Command".into()),
            role: None,
        };
        let reply = executor.execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "refused");
    }

    #[test]
    fn name_get_caret_requires_name() {
        let command = Command::GetCaret {
            target: TargetRef::Current,
            window: Some(1),
            name: None,
            role: None,
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
    }

    #[test]
    fn set_caret_and_get_caret_verbs_are_named() {
        let set_caret = Command::SetCaret {
            target: TargetRef::Current,
            offset: 2,
            window: Some(1),
            name: Some("Command".into()),
            role: None,
        };
        let get_caret = Command::GetCaret {
            target: TargetRef::Current,
            window: Some(1),
            name: Some("Command".into()),
            role: None,
        };
        assert_eq!(set_caret.verb(), "set-caret");
        assert_eq!(get_caret.verb(), "get-caret");
        assert_eq!(set_caret.required_grant(), Grant::Actuate);
        assert_eq!(get_caret.required_grant(), Grant::Observe);
    }

    #[test]
    fn get_text_without_name_requires_window() {
        let command = Command::GetText {
            target: TargetRef::Current,
            window: None,
            name: None,
            role: None,
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
        assert!(
            reply
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("requires --window <handle>"),
            "missing-window message should name the addressing contract"
        );
    }

    #[test]
    fn get_text_role_without_name_is_typed() {
        let command = Command::GetText {
            target: TargetRef::Current,
            window: Some(1),
            name: None,
            role: Some("text".into()),
        };
        let reply = observe_executor().execute(&command);
        assert!(!reply.ok);
        assert_eq!(reply.error.as_ref().unwrap().code, "invalid_input");
        assert!(
            reply
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("--role requires --name"),
            "role-without-name message should name the addressing contract"
        );
    }

    #[test]
    fn get_text_verb_is_named_and_observe() {
        let get_text = Command::GetText {
            target: TargetRef::Current,
            window: Some(1),
            name: Some("Command".into()),
            role: None,
        };
        assert_eq!(get_text.verb(), "get-text");
        assert_eq!(get_text.required_grant(), Grant::Observe);
    }
}
