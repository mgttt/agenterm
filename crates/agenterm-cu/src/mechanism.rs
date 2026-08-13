//! libagenterm (`agt_*`) mechanism boundary for structured accessibility-tree
//! observe and node actuation. Product commands (`cu tree`, grants, audit) stay
//! above this layer; windows/screenshot/input-degraded paths still use
//! `agenterm-platform` until their ABI milestones ship.

use agenterm::{
    agt_a11y_node, agt_a11y_node_action_name, agt_a11y_node_get_text, agt_a11y_node_perform,
    agt_a11y_node_send_keys, agt_a11y_node_set_text, agt_a11y_node_string,
    agt_a11y_tree_meta_string, agt_a11y_tree_node, agt_a11y_tree_snapshot, agt_capability,
    agt_capability_query, agt_last_error, agt_status,
};

const AGT_A11Y_META_BACKEND: i32 = 0;
const AGT_A11Y_META_ROOT_ID: i32 = 1;

const AGT_A11Y_STR_ROLE: i32 = 0;
const AGT_A11Y_STR_NAME: i32 = 1;
const AGT_A11Y_STR_TEXT: i32 = 2;
const AGT_A11Y_STR_STATES: i32 = 3;

const AGT_A11Y_ACTION_CLICK: i32 = 0;
const AGT_A11Y_ACTION_FOCUS: i32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct A11yBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct A11yNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub name: String,
    pub states: Vec<String>,
    pub bounds: A11yBounds,
    pub actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct A11yTree {
    pub backend: String,
    pub window_handle: Option<isize>,
    pub root_id: String,
    pub nodes: Vec<A11yNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MechanismError {
    Unsupported,
    Failed { code: String, message: String },
}

pub fn accessibility_tree_available() -> bool {
    agt_capability_query(agt_capability::AGT_CAP_ACCESSIBILITY_TREE) == agt_status::AGT_OK
}

pub fn tree_for_window(window: Option<isize>) -> Result<A11yTree, MechanismError> {
    let handle = window.unwrap_or(0);
    let mut count = 0usize;
    let status = agt_a11y_tree_snapshot(handle, &mut count);
    map_status("agt_a11y_tree_snapshot", status)?;
    let backend = read_meta_string(AGT_A11Y_META_BACKEND)?;
    let root_id = read_meta_string(AGT_A11Y_META_ROOT_ID)?;
    let mut nodes = Vec::with_capacity(count);
    for index in 0..count {
        nodes.push(read_node(index)?);
    }
    Ok(A11yTree {
        backend,
        window_handle: window,
        root_id,
        nodes,
    })
}

pub fn perform_node_action(
    window: Option<isize>,
    node_id: &str,
    action: NodeAction,
) -> Result<(), MechanismError> {
    let handle = window.unwrap_or(0);
    let action_kind = match action {
        NodeAction::Click => AGT_A11Y_ACTION_CLICK,
        NodeAction::Focus => AGT_A11Y_ACTION_FOCUS,
    };
    let node_c = CStringOrStack::new(node_id)?;
    let status = agt_a11y_node_perform(handle, node_c.as_ptr(), action_kind);
    map_status("agt_a11y_node_perform", status)?;
    Ok(())
}

pub fn set_node_text(
    window: Option<isize>,
    node_id: &str,
    text: &str,
) -> Result<(), MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let status = agt_a11y_node_set_text(handle, node_c.as_ptr(), text.as_ptr(), text.len());
    map_status("agt_a11y_node_set_text", status)?;
    Ok(())
}

/// Independent AT-SPI `Text.GetText` for a resolved child-index path.
/// Distinguishes a real mechanism failure from the two-stage empty-payload
/// probe (`buffer_too_small` + required == 0).
pub fn get_node_text(window: Option<isize>, node_id: &str) -> Result<String, MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let mut required = 0usize;
    let status = agt_a11y_node_get_text(
        handle,
        node_c.as_ptr(),
        std::ptr::null_mut(),
        0,
        &mut required,
    );
    if status == agt_status::AGT_UNSUPPORTED {
        return Err(MechanismError::Unsupported);
    }
    if status != agt_status::AGT_FAILED {
        return Err(last_mechanism_error("agt_a11y_node_get_text"));
    }
    let probe = last_mechanism_error("agt_a11y_node_get_text");
    match &probe {
        MechanismError::Failed { code, .. } if code == "buffer_too_small" => {}
        other => return Err(other.clone()),
    }
    if required == 0 {
        return Ok(String::new());
    }
    let mut buf = vec![0u8; required];
    let status = agt_a11y_node_get_text(
        handle,
        node_c.as_ptr(),
        buf.as_mut_ptr(),
        required,
        &mut required,
    );
    map_status("agt_a11y_node_get_text", status)?;
    buf.truncate(required);
    String::from_utf8(buf).map_err(|_| MechanismError::Failed {
        code: "bad_encoding".into(),
        message: "node text is not UTF-8".into(),
    })
}

pub fn send_node_keys(
    window: Option<isize>,
    node_id: &str,
    keys: &str,
) -> Result<(), MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let status = agt_a11y_node_send_keys(handle, node_c.as_ptr(), keys.as_ptr(), keys.len());
    map_status("agt_a11y_node_send_keys", status)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeAction {
    Click,
    Focus,
}

fn read_node(index: usize) -> Result<A11yNode, MechanismError> {
    let mut record = agt_a11y_node {
        bounds_x: 0,
        bounds_y: 0,
        bounds_width: 0,
        bounds_height: 0,
        id: [0u8; 64],
        id_len: 0,
        id_truncated: 0,
        parent_id: [0u8; 64],
        parent_id_len: 0,
        parent_id_truncated: 0,
        has_parent: 0,
        actions_count: 0,
    };
    let status = agt_a11y_tree_node(index, &mut record);
    map_status("agt_a11y_tree_node", status)?;
    let id = fixed_field(&record.id, record.id_len);
    let parent_id = if record.has_parent != 0 {
        Some(fixed_field(&record.parent_id, record.parent_id_len))
    } else {
        None
    };
    let role = read_node_string(index, AGT_A11Y_STR_ROLE)?;
    let name = read_node_string(index, AGT_A11Y_STR_NAME)?;
    let text_raw = read_node_string(index, AGT_A11Y_STR_TEXT)?;
    let text = if text_raw.is_empty() {
        None
    } else {
        Some(text_raw)
    };
    let states_raw = read_node_string(index, AGT_A11Y_STR_STATES)?;
    let states = if states_raw.is_empty() {
        Vec::new()
    } else {
        states_raw.split(',').map(str::to_owned).collect()
    };
    let mut actions = Vec::with_capacity(record.actions_count as usize);
    for action_index in 0..record.actions_count as usize {
        actions.push(read_action_name(index, action_index)?);
    }
    Ok(A11yNode {
        id,
        parent_id,
        role,
        name,
        states,
        bounds: A11yBounds {
            x: record.bounds_x,
            y: record.bounds_y,
            width: record.bounds_width,
            height: record.bounds_height,
        },
        actions,
        text,
    })
}

fn read_meta_string(field: i32) -> Result<String, MechanismError> {
    let bytes =
        read_two_stage(|buf, cap, out_len| agt_a11y_tree_meta_string(field, buf, cap, out_len))?;
    Ok(
        String::from_utf8(bytes).map_err(|_| MechanismError::Failed {
            code: "bad_encoding".into(),
            message: "metadata string is not UTF-8".into(),
        })?,
    )
}

fn read_node_string(node_index: usize, kind: i32) -> Result<String, MechanismError> {
    let bytes = read_two_stage(|buf, cap, out_len| {
        agt_a11y_node_string(node_index, kind, buf, cap, out_len)
    })?;
    Ok(
        String::from_utf8(bytes).map_err(|_| MechanismError::Failed {
            code: "bad_encoding".into(),
            message: "node string is not UTF-8".into(),
        })?,
    )
}

fn read_action_name(node_index: usize, action_index: usize) -> Result<String, MechanismError> {
    let bytes = read_two_stage(|buf, cap, out_len| {
        agt_a11y_node_action_name(node_index, action_index, buf, cap, out_len)
    })?;
    Ok(
        String::from_utf8(bytes).map_err(|_| MechanismError::Failed {
            code: "bad_encoding".into(),
            message: "action name is not UTF-8".into(),
        })?,
    )
}

fn read_two_stage(
    mut probe: impl FnMut(*mut u8, usize, *mut usize) -> agt_status,
) -> Result<Vec<u8>, MechanismError> {
    let mut required = 0usize;
    let status = probe(std::ptr::null_mut(), 0, &mut required);
    if status != agt_status::AGT_FAILED {
        return Err(last_mechanism_error("two_stage_probe"));
    }
    // libagenterm treats cap==0 as a size probe even when the payload is empty.
    // A second call with cap==0 would repeat buffer_too_small, so stop here.
    if required == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; required];
    let status = probe(buf.as_mut_ptr(), required, &mut required);
    map_status("two_stage_read", status)?;
    buf.truncate(required);
    Ok(buf)
}

fn fixed_field(bytes: &[u8], len: u32) -> String {
    String::from_utf8_lossy(&bytes[..len as usize]).into_owned()
}

fn map_status(operation: &str, status: agt_status) -> Result<(), MechanismError> {
    match status {
        agt_status::AGT_OK => Ok(()),
        agt_status::AGT_UNSUPPORTED => Err(MechanismError::Unsupported),
        agt_status::AGT_FAILED => Err(last_mechanism_error(operation)),
    }
}

fn last_mechanism_error(operation: &str) -> MechanismError {
    let mut err = agenterm::agt_error {
        operation: std::ptr::null(),
        code: std::ptr::null(),
        message: std::ptr::null(),
    };
    if agt_last_error(&mut err) != agt_status::AGT_OK {
        return MechanismError::Failed {
            code: "mechanism_failed".into(),
            message: format!("{operation} failed without error detail"),
        };
    }
    let code = cstr_to_string(err.code).unwrap_or_else(|| "mechanism_failed".to_string());
    let message = cstr_to_string(err.message).unwrap_or_default();
    MechanismError::Failed { code, message }
}

fn cstr_to_string(ptr: *const std::ffi::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
    cstr.to_str().ok().map(str::to_owned)
}

/// NUL-terminated UTF-8 for node paths passed into FFI.
struct CStringOrStack(std::ffi::CString);

impl CStringOrStack {
    fn new(s: &str) -> Result<Self, MechanismError> {
        std::ffi::CString::new(s)
            .map_err(|_| MechanismError::Failed {
                code: "invalid_input".into(),
                message: "node id contains an interior NUL byte".into(),
            })
            .map(CStringOrStack)
    }

    fn as_ptr(&self) -> *const std::ffi::c_char {
        self.0.as_ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_stage_empty_payload_does_not_reprobe_with_cap_zero() {
        let mut calls = 0usize;
        let bytes = read_two_stage(|_buf, cap, out_len| {
            calls += 1;
            unsafe {
                *out_len = 0;
            }
            assert_eq!(cap, 0, "empty payload must not make a second cap=0 read");
            agt_status::AGT_FAILED
        })
        .expect("empty two-stage probe should succeed");
        assert!(bytes.is_empty());
        assert_eq!(calls, 1);
    }
}
