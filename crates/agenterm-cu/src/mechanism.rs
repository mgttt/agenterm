//! libagenterm (`agt_*`) mechanism boundary: every call goes through the
//! shared runtime dynamic library (`crate::dynlib`) — no `agenterm-platform`
//! or `agenterm-abi` static linking. Product commands (`cu tree`, `windows`,
//! `window-place`, …) stay above this layer.
//!
//! Public API shape is deliberately small and typed: callers get `WindowInfo`
//! / `A11yTree` / `Rect`-style values or a [`MechanismError`], never raw FFI
//! pointers.

use std::ffi::{CStr, CString};

use crate::dynlib::{self, agt_a11y_node, agt_error};

// ---------------------------------------------------------------------------
// Structured accessibility tree (unchanged public API).
// ---------------------------------------------------------------------------

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeAction {
    Click,
    Focus,
}

/// Typed failure for every mechanism call. `Unsupported` carries a human
/// reason; `Failed` mirrors the library's `{code, message}` error record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MechanismError {
    Unsupported { reason: String },
    Failed { code: String, message: String },
}

// ---------------------------------------------------------------------------
// Capability negotiation (discovery/metadata only).
// ---------------------------------------------------------------------------

/// Capabilities `cu` reports on. Values are the ABI capability ids.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    WindowEnumerate,
    WindowOp,
    Screenshot,
    InputInject,
    AccessibilityTree,
    DesktopHost,
}

impl Capability {
    fn abi_id(self) -> i32 {
        match self {
            Capability::WindowEnumerate => dynlib::AGT_CAP_WINDOW_ENUMERATE,
            Capability::WindowOp => dynlib::AGT_CAP_WINDOW_OP,
            Capability::Screenshot => dynlib::AGT_CAP_SCREENSHOT,
            Capability::InputInject => dynlib::AGT_CAP_INPUT_INJECT,
            Capability::AccessibilityTree => dynlib::AGT_CAP_ACCESSIBILITY_TREE,
            Capability::DesktopHost => dynlib::AGT_CAP_DESKTOP_HOST,
        }
    }
}

/// Debug shape matches the old `agenterm-platform` `CapabilityStatus` so
/// `agenterm-cu capabilities` output stays stable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityStatus {
    Available,
    Unsupported { reason: String },
    Failed { code: String, message: String },
}

/// Query one capability through `agt_capability_query`. `AGT_UNSUPPORTED`
/// becomes `Unsupported` with a stable reason (the ABI carries no reason
/// text); any unexpected status becomes `Failed`; a library load / symbol
/// resolution failure is reported verbatim so `agenterm-cu capabilities` stays
/// truthful when the dynamic library is missing.
pub fn capability_status(capability: Capability) -> CapabilityStatus {
    let query = match call_sym::<CapabilityQuery>(b"agt_capability_query") {
        Ok(f) => f,
        Err(error) => {
            return CapabilityStatus::Failed {
                code: error_code(&error),
                message: error_message(&error),
            };
        }
    };
    match unsafe { query(capability.abi_id()) } {
        dynlib::AGT_OK => CapabilityStatus::Available,
        dynlib::AGT_UNSUPPORTED => CapabilityStatus::Unsupported {
            reason: "host adapter unavailable".to_owned(),
        },
        _ => CapabilityStatus::Failed {
            code: "capability_failed".to_owned(),
            message: "agt_capability_query failed".to_owned(),
        },
    }
}

fn error_code(error: &MechanismError) -> String {
    match error {
        MechanismError::Failed { code, .. } => code.clone(),
        MechanismError::Unsupported { .. } => "unsupported".to_owned(),
    }
}

fn error_message(error: &MechanismError) -> String {
    match error {
        MechanismError::Failed { message, .. } => message.clone(),
        MechanismError::Unsupported { reason } => reason.clone(),
    }
}

pub fn accessibility_tree_available() -> bool {
    matches!(
        capability_status(Capability::AccessibilityTree),
        CapabilityStatus::Available
    )
}

// ---------------------------------------------------------------------------
// Window enumeration + screens.
// ---------------------------------------------------------------------------

pub mod window_enumerate {
    use crate::dynlib;

    use super::{MechanismError, call_sym, fixed_field, map_status};

    /// Bounds of a top-level window in physical screen pixels (top-origin).
    #[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
    pub struct WindowBounds {
        pub x: i32,
        pub y: i32,
        pub width: u32,
        pub height: u32,
    }

    /// One display in top-origin coordinates (same space as [`WindowBounds`]).
    #[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
    pub struct ScreenInfo {
        pub frame: WindowBounds,
        pub visible: WindowBounds,
        pub primary: bool,
    }

    /// A snapshot of one visible top-level window.
    #[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
    pub struct WindowInfo {
        /// Native window handle (HWND on Windows), valid for the observation
        /// instant.
        pub handle: isize,
        pub title: String,
        pub process_id: u32,
        pub app_name: String,
        pub bounds: WindowBounds,
        pub focused: bool,
        pub minimized: bool,
    }

    /// `agt_window_enumerate`: two-stage (probe, allocate, fetch).
    pub fn enumerate_top_level() -> Result<Vec<WindowInfo>, MechanismError> {
        let f = call_sym::<super::WindowEnumerate>(b"agt_window_enumerate")?;
        let mut needed = 0usize;
        let status = unsafe { f(std::ptr::null_mut(), 0, &mut needed) };
        match status {
            dynlib::AGT_UNSUPPORTED => Err(MechanismError::Unsupported {
                reason: "window enumeration is unavailable on this host".to_owned(),
            }),
            dynlib::AGT_FAILED if needed == 0 => Ok(Vec::new()),
            dynlib::AGT_FAILED => {
                let mut capacity = needed;
                for _ in 0..4 {
                    let mut buf = vec![dynlib::agt_window_info::default(); capacity];
                    let mut got = 0usize;
                    let status = unsafe { f(buf.as_mut_ptr(), capacity, &mut got) };
                    if status == dynlib::AGT_OK {
                        buf.truncate(got);
                        return Ok(buf.iter().map(record_to_info).collect());
                    }
                    if let Some(grown) = retry_capacity(status, capacity, got) {
                        capacity = grown;
                        continue;
                    }
                    map_status("agt_window_enumerate fetch", status)?;
                }
                Err(MechanismError::Failed {
                    code: "window_churn".to_owned(),
                    message: "window count did not stabilize after bounded retries".to_owned(),
                })
            }
            other => Err(MechanismError::Failed {
                code: "unexpected_status".to_owned(),
                message: format!(
                    "agt_window_enumerate probe: expected AGT_FAILED (buffer_too_small), got {other}"
                ),
            }),
        }
    }

    /// `agt_screen_list`: two-stage, same shape as [`enumerate_top_level`].
    pub fn list_screens() -> Result<Vec<ScreenInfo>, MechanismError> {
        let f = call_sym::<super::ScreenList>(b"agt_screen_list")?;
        let mut needed = 0usize;
        let status = unsafe { f(std::ptr::null_mut(), 0, &mut needed) };
        match status {
            dynlib::AGT_UNSUPPORTED => Err(MechanismError::Unsupported {
                reason: "screen enumeration is unavailable on this host".to_owned(),
            }),
            dynlib::AGT_FAILED if needed == 0 => Ok(Vec::new()),
            dynlib::AGT_FAILED => {
                let mut capacity = needed;
                for _ in 0..4 {
                    let mut buf = vec![dynlib::agt_screen_info::default(); capacity];
                    let mut got = 0usize;
                    let status = unsafe { f(buf.as_mut_ptr(), capacity, &mut got) };
                    if status == dynlib::AGT_OK {
                        buf.truncate(got);
                        return Ok(buf.iter().map(record_to_screen).collect());
                    }
                    if let Some(grown) = retry_capacity(status, capacity, got) {
                        capacity = grown;
                        continue;
                    }
                    map_status("agt_screen_list fetch", status)?;
                }
                Err(MechanismError::Failed {
                    code: "screen_churn".to_owned(),
                    message: "screen count did not stabilize after bounded retries".to_owned(),
                })
            }
            other => Err(MechanismError::Failed {
                code: "unexpected_status".to_owned(),
                message: format!(
                    "agt_screen_list probe: expected AGT_FAILED (buffer_too_small), got {other}"
                ),
            }),
        }
    }

    pub(super) fn retry_capacity(status: i32, capacity: usize, required: usize) -> Option<usize> {
        (status == dynlib::AGT_FAILED && required > capacity).then_some(required)
    }

    fn record_to_info(record: &dynlib::agt_window_info) -> WindowInfo {
        WindowInfo {
            handle: record.handle,
            title: fixed_field(&record.title, record.title_len),
            process_id: record.process_id,
            app_name: fixed_field(&record.app_name, record.app_name_len),
            bounds: WindowBounds {
                x: record.x,
                y: record.y,
                width: record.width,
                height: record.height,
            },
            focused: record.focused != 0,
            minimized: record.minimized != 0,
        }
    }

    fn record_to_screen(record: &dynlib::agt_screen_info) -> ScreenInfo {
        ScreenInfo {
            frame: WindowBounds {
                x: record.frame_x,
                y: record.frame_y,
                width: record.frame_width,
                height: record.frame_height,
            },
            visible: WindowBounds {
                x: record.visible_x,
                y: record.visible_y,
                width: record.visible_width,
                height: record.visible_height,
            },
            primary: record.primary != 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Native window operations.
// ---------------------------------------------------------------------------

pub mod window_op {
    use super::{MechanismError, map_status};

    /// Show/hide/minimize/maximize/restore a native window handle.
    pub fn show(handle: isize, state: i32) -> Result<(), MechanismError> {
        let f = super::call_sym::<super::WindowShow>(b"agt_native_window_show")?;
        let status = unsafe { f(handle, state) };
        map_status("agt_native_window_show", status)
    }

    /// Move/resize a native window handle.
    pub fn move_window(
        handle: isize,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), MechanismError> {
        let f = super::call_sym::<super::WindowMove>(b"agt_native_window_move")?;
        let status = unsafe { f(handle, x, y, width, height) };
        map_status("agt_native_window_move", status)
    }

    /// Read a native window handle's rectangle (physical pixels, top-origin).
    pub fn window_rect(
        handle: isize,
    ) -> Result<super::window_enumerate::WindowBounds, MechanismError> {
        let f = super::call_sym::<super::WindowRect>(b"agt_native_window_rect")?;
        let mut x = 0i32;
        let mut y = 0i32;
        let mut w = 0u32;
        let mut h = 0u32;
        let status = unsafe { f(handle, &mut x, &mut y, &mut w, &mut h) };
        map_status("agt_native_window_rect", status)?;
        Ok(super::window_enumerate::WindowBounds {
            x,
            y,
            width: w,
            height: h,
        })
    }

    /// Pin/unpin a native window handle above other windows.
    pub fn set_topmost(handle: isize, topmost: bool) -> Result<(), MechanismError> {
        let f = super::call_sym::<super::WindowSetTopmost>(b"agt_native_window_set_topmost")?;
        let status = unsafe { f(handle, i32::from(topmost)) };
        map_status("agt_native_window_set_topmost", status)
    }

    /// Close a **native** window handle (distinct from the ABI's own
    /// `agt_window_close`).
    pub fn close(handle: isize) -> Result<(), MechanismError> {
        let f = super::call_sym::<super::WindowClose>(b"agt_native_window_close")?;
        let status = unsafe { f(handle) };
        map_status("agt_native_window_close", status)
    }
}

// ---------------------------------------------------------------------------
// Input injection.
// ---------------------------------------------------------------------------

pub mod input_inject {
    use super::{MechanismError, map_status};
    use crate::dynlib;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum PointerButton {
        Left,
        Right,
        Middle,
    }

    impl PointerButton {
        fn abi_id(self) -> i32 {
            match self {
                PointerButton::Left => dynlib::AGT_INPUT_BUTTON_LEFT,
                PointerButton::Right => dynlib::AGT_INPUT_BUTTON_RIGHT,
                PointerButton::Middle => dynlib::AGT_INPUT_BUTTON_MIDDLE,
            }
        }
    }

    /// Move the pointer to absolute screen coordinates.
    pub fn pointer_move(x: i32, y: i32) -> Result<(), MechanismError> {
        let f = super::call_sym::<super::PointerMove>(b"agt_input_pointer_move")?;
        let status = unsafe { f(x, y) };
        map_status("agt_input_pointer_move", status)
    }

    /// Click a pointer button at absolute screen coordinates.
    pub fn pointer_click(
        x: i32,
        y: i32,
        button: PointerButton,
        clicks: u32,
    ) -> Result<(), MechanismError> {
        let f = super::call_sym::<super::PointerClick>(b"agt_input_pointer_click")?;
        let status = unsafe { f(x, y, button.abi_id(), clicks) };
        map_status("agt_input_pointer_click", status)
    }

    /// Type UTF-8 text into the focused control.
    pub fn type_text(text: &str) -> Result<(), MechanismError> {
        let f = super::call_sym::<super::InputTypeText>(b"agt_input_type_text")?;
        let status = unsafe { f(text.as_ptr(), text.len()) };
        map_status("agt_input_type_text", status)
    }

    /// Send a hotkey chord such as `ctrl+s`, `alt+f4` or `enter`.
    pub fn send_keys(keys: &str) -> Result<(), MechanismError> {
        let f = super::call_sym::<super::InputSendKeys>(b"agt_input_send_keys")?;
        let status = unsafe { f(keys.as_ptr(), keys.len()) };
        map_status("agt_input_send_keys", status)
    }
}

// ---------------------------------------------------------------------------
// Screenshots.
// ---------------------------------------------------------------------------

pub mod screenshot {
    use std::ffi::CString;
    use std::path::Path;

    use super::{MechanismError, map_status};
    use crate::dynlib;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct ScreenshotWriteResult {
        pub frame_width: u32,
        pub frame_height: u32,
        pub output_width: u32,
        pub output_height: u32,
        pub output_pixels: usize,
    }

    /// Capture a native window to a PNG at `path` (whole-window area).
    /// The ABI export writes the file itself; output dimensions are read back
    /// from the PNG header so the result shape matches the previous
    /// `agenterm-platform` path.
    pub fn capture_native_window_png(
        native_window: isize,
        path: &Path,
    ) -> Result<ScreenshotWriteResult, MechanismError> {
        if native_window == 0 {
            return Err(MechanismError::Failed {
                code: "bad_handle".to_owned(),
                message: "native_window is 0".to_owned(),
            });
        }
        let path_c = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| {
            MechanismError::Failed {
                code: "bad_path".to_owned(),
                message: "path contains an interior NUL byte".to_owned(),
            }
        })?;
        let f = super::call_sym::<super::CaptureWindow>(b"agt_screenshot_capture_window")?;
        let status = unsafe {
            f(
                native_window,
                path_c.as_ptr(),
                dynlib::AGT_SCREENSHOT_AREA_WINDOW,
                0,
                0,
                0,
                0,
            )
        };
        map_status("agt_screenshot_capture_window", status)?;
        let (output_width, output_height) = png_dimensions(path);
        let output_pixels = output_width as usize * output_height as usize;
        Ok(ScreenshotWriteResult {
            frame_width: output_width,
            frame_height: output_height,
            output_width,
            output_height,
            output_pixels,
        })
    }

    /// Read the width/height from a PNG file's IHDR (fixed offset 16..24).
    /// Returns `(0, 0)` when the header cannot be read — the capture itself
    /// already succeeded, so a broken header must not fail the command.
    fn png_dimensions(path: &Path) -> (u32, u32) {
        let mut header = [0u8; 24];
        let Ok(mut file) = std::fs::File::open(path) else {
            return (0, 0);
        };
        use std::io::Read;
        if file.read_exact(&mut header).is_err() {
            return (0, 0);
        }
        // PNG signature (8) + IHDR length/type (8) + width/height (4 + 4).
        if &header[0..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
            return (0, 0);
        }
        let width = u32::from_be_bytes(header[16..20].try_into().unwrap_or([0; 4]));
        let height = u32::from_be_bytes(header[20..24].try_into().unwrap_or([0; 4]));
        (width, height)
    }
}

// ---------------------------------------------------------------------------
// Accessibility-tree extras (event bus / write route diagnostics).
// ---------------------------------------------------------------------------

pub mod accessibility_tree {
    use super::{MechanismError, call_sym, map_status, read_two_stage};

    /// Drain the accessibility event bus (no user-visible side effects).
    pub fn drain_bus() -> Result<(), MechanismError> {
        let f = call_sym::<super::DrainBus>(b"agt_a11y_drain_bus")?;
        let status = unsafe { f() };
        map_status("agt_a11y_drain_bus", status)
    }

    /// Route of the last successful text write on this thread (diagnostic
    /// string).
    pub fn last_text_write_via() -> Result<String, MechanismError> {
        let bytes = read_two_stage(|buf, cap, out_len| {
            let f = call_sym::<super::LastTextWriteVia>(b"agt_a11y_last_text_write_via")?;
            let status = unsafe { f(buf, cap, out_len) };
            Ok::<i32, MechanismError>(status)
        })?;
        String::from_utf8(bytes).map_err(|_| MechanismError::Failed {
            code: "bad_encoding".into(),
            message: "write-route string is not UTF-8".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Accessibility tree (public API unchanged).
// ---------------------------------------------------------------------------

pub fn tree_for_window(window: Option<isize>) -> Result<A11yTree, MechanismError> {
    let handle = window.unwrap_or(0);
    let mut count = 0usize;
    let f = call_sym::<TreeSnapshot>(b"agt_a11y_tree_snapshot")?;
    let status = unsafe { f(handle, &mut count) };
    map_status("agt_a11y_tree_snapshot", status)?;
    let backend = read_meta_string(dynlib::AGT_A11Y_META_BACKEND)?;
    let root_id = read_meta_string(dynlib::AGT_A11Y_META_ROOT_ID)?;
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
        NodeAction::Click => dynlib::AGT_A11Y_ACTION_CLICK,
        NodeAction::Focus => dynlib::AGT_A11Y_ACTION_FOCUS,
    };
    let node_c = CStringOrStack::new(node_id)?;
    let f = call_sym::<NodePerform>(b"agt_a11y_node_perform")?;
    let status = unsafe { f(handle, node_c.as_ptr(), action_kind) };
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
    let f = call_sym::<NodeSetText>(b"agt_a11y_node_set_text")?;
    let status = unsafe { f(handle, node_c.as_ptr(), text.as_ptr(), text.len()) };
    map_status("agt_a11y_node_set_text", status)?;
    Ok(())
}

/// Independent AT-SPI `Text.GetText` for a resolved child-index path.
/// Does not reuse `set_node_text` confirmation, `last_text_write_via`, or
/// a tree snapshot `text` field. Distinguishes a real mechanism failure
/// from the two-stage empty-payload probe (`buffer_too_small` + required
/// == 0).
pub fn get_node_text(window: Option<isize>, node_id: &str) -> Result<String, MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let f = call_sym::<NodeGetText>(b"agt_a11y_node_get_text")?;
    let mut required = 0usize;
    let status = unsafe {
        f(
            handle,
            node_c.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut required,
        )
    };
    if status == dynlib::AGT_UNSUPPORTED {
        return Err(MechanismError::Unsupported {
            reason: "agt_a11y_node_get_text: mechanism unavailable on this host".to_owned(),
        });
    }
    if status != dynlib::AGT_FAILED {
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
    let status = unsafe {
        f(
            handle,
            node_c.as_ptr(),
            buf.as_mut_ptr(),
            required,
            &mut required,
        )
    };
    map_status("agt_a11y_node_get_text", status)?;
    buf.truncate(required);
    String::from_utf8(buf).map_err(|_| MechanismError::Failed {
        code: "bad_encoding".into(),
        message: "node text is not UTF-8".into(),
    })
}

/// One-shot AT-SPI `Component.ScrollTo(TopEdge)` for a resolved child-index
/// path. Missing / false / `UnknownMethod` is `a11y_scroll_unavailable`.
pub fn scroll_node(window: Option<isize>, node_id: &str) -> Result<(), MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let f = call_sym::<NodeScroll>(b"agt_a11y_node_scroll")?;
    let status = unsafe { f(handle, node_c.as_ptr()) };
    map_status("agt_a11y_node_scroll", status)?;
    Ok(())
}

/// Independent AT-SPI `Component.GetExtents(Screen)` for a resolved
/// child-index path. Not a tree-snapshot `bounds` field.
pub fn get_node_extents(
    window: Option<isize>,
    node_id: &str,
) -> Result<A11yBounds, MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let f = call_sym::<NodeGetExtents>(b"agt_a11y_node_get_extents")?;
    let mut x = 0i32;
    let mut y = 0i32;
    let mut width = 0i32;
    let mut height = 0i32;
    let status = unsafe {
        f(
            handle,
            node_c.as_ptr(),
            &mut x,
            &mut y,
            &mut width,
            &mut height,
        )
    };
    map_status("agt_a11y_node_get_extents", status)?;
    if width <= 0 || height <= 0 {
        return Err(MechanismError::Failed {
            code: "a11y_extents_unavailable".into(),
            message: format!("Component.GetExtents returned empty rect {width}x{height}"),
        });
    }
    Ok(A11yBounds {
        x,
        y,
        width,
        height,
    })
}

/// Independent AT-SPI `Text` selection (`GetNSelections` + `GetSelection`).
/// `n == 0` is empty (start/end stay 0).
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct A11ySelection {
    pub n: i32,
    pub start: i32,
    pub end: i32,
}

/// One-shot AT-SPI `Text.SetSelection(0, start, end)` for a resolved
/// child-index path. Missing Text / `UnknownMethod` is
/// `a11y_selection_unavailable`. SetSelection false is
/// `a11y_selection_no_effect`.
pub fn set_node_selection(
    window: Option<isize>,
    node_id: &str,
    start: i32,
    end: i32,
) -> Result<(), MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let f = call_sym::<NodeSetSelection>(b"agt_a11y_node_set_selection")?;
    let status = unsafe { f(handle, node_c.as_ptr(), start, end) };
    map_status("agt_a11y_node_set_selection", status)?;
    Ok(())
}

/// Independent AT-SPI `Text.GetNSelections` + `GetSelection(0)` for a
/// resolved child-index path. Not the set-selection reply payload.
pub fn get_node_selection(
    window: Option<isize>,
    node_id: &str,
) -> Result<A11ySelection, MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let f = call_sym::<NodeGetSelection>(b"agt_a11y_node_get_selection")?;
    let mut n = 0i32;
    let mut start = 0i32;
    let mut end = 0i32;
    let status = unsafe { f(handle, node_c.as_ptr(), &mut n, &mut start, &mut end) };
    map_status("agt_a11y_node_get_selection", status)?;
    Ok(A11ySelection { n, start, end })
/// Resident menu and global-shortcut host through libagenterm.
pub mod desktop_host {
    use std::ffi::c_void;
    use std::ptr::null;

    use super::{MechanismError, call_sym, map_status};
    use crate::dynlib;

    #[derive(Clone, Copy, Debug)]
    pub struct ActionSpec<'a> {
        pub action_id: u32,
        pub label: &'a str,
        pub shortcut: Option<&'a str>,
    }

    pub struct DesktopHost {
        raw: *mut c_void,
    }

    impl DesktopHost {
        pub fn open(actions: &[ActionSpec<'_>]) -> Result<Self, MechanismError> {
            let records: Vec<_> = actions
                .iter()
                .map(|action| dynlib::agt_desktop_action {
                    action_id: action.action_id,
                    label: action.label.as_ptr(),
                    label_len: action.label.len(),
                    shortcut: action.shortcut.map_or(null(), str::as_ptr),
                    shortcut_len: action.shortcut.map_or(0, str::len),
                })
                .collect();
            let open = call_sym::<super::DesktopHostOpen>(b"agt_desktop_host_open")?;
            let mut raw = std::ptr::null_mut();
            let status = unsafe { open(records.as_ptr(), records.len(), &mut raw) };
            map_status("agt_desktop_host_open", status)?;
            if raw.is_null() {
                return Err(MechanismError::Failed {
                    code: "desktop_host_null".into(),
                    message: "agt_desktop_host_open succeeded without a host".into(),
                });
            }
            Ok(Self { raw })
        }

        pub fn poll(&mut self, timeout_ms: u32) -> Result<Option<u32>, MechanismError> {
            let poll = call_sym::<super::DesktopHostPoll>(b"agt_desktop_host_poll")?;
            let mut action_id = 0;
            let status = unsafe { poll(self.raw, timeout_ms, &mut action_id) };
            map_status("agt_desktop_host_poll", status)?;
            Ok((action_id != 0).then_some(action_id))
        }

        pub fn close(mut self) -> Result<(), MechanismError> {
            let result = self.close_inner();
            if result.is_ok() {
                self.raw = std::ptr::null_mut();
            }
            result
        }

        fn close_inner(&mut self) -> Result<(), MechanismError> {
            if self.raw.is_null() {
                return Ok(());
            }
            let close = call_sym::<super::DesktopHostClose>(b"agt_desktop_host_close")?;
            map_status("agt_desktop_host_close", unsafe { close(self.raw) })
        }
    }

    impl Drop for DesktopHost {
        fn drop(&mut self) {
            if self.close_inner().is_ok() {
                self.raw = std::ptr::null_mut();
            }
        }
    }
}

/// Clipboard through libagenterm (`agt_clipboard_*`). Named `paste` seeds
/// and reads here; named `copy` publishes GetText here. Neither injects
/// Ctrl+V or XTest.
pub mod clipboard {
    use super::{MechanismError, call_sym, last_mechanism_error, map_status};
    use crate::dynlib;

    /// Hidden `cu` argv that owns CLIPBOARD after `SetSelectionOwner` so a
    /// later process can `ConvertSelection`. Not a public command.
    pub const X11_CLIPBOARD_OWNER_ARG: &str = "__agenterm-internal-x11-clipboard-own";

    /// Env the owner process sets so `agt_clipboard_set_text` stays in the
    /// X11 selection event loop instead of returning and dropping the owner.
    pub const X11_CLIPBOARD_SERVE_ENV: &str = "PLATFORM_X11_CLIPBOARD_SERVE";

    pub fn set_text(text: &str) -> Result<(), MechanismError> {
        let f = call_sym::<super::ClipboardSetText>(b"agt_clipboard_set_text")?;
        let status = unsafe { f(text.as_ptr(), text.len()) };
        map_status("agt_clipboard_set_text", status)
    }

    /// Publish UTF-8 so a later `cu` process can read it. On Linux X11 the
    /// CLIPBOARD owner must outlive this caller; a detached `cu` owner
    /// answers `SelectionRequest`. Other hosts use in-process `set_text`.
    pub fn publish_text(text: &str) -> Result<(), MechanismError> {
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("DISPLAY").is_some() {
                return publish_x11_clipboard(text);
            }
        }
        set_text(text)
    }

    #[cfg(target_os = "linux")]
    fn publish_x11_clipboard(text: &str) -> Result<(), MechanismError> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        let exe = std::env::current_exe().map_err(|error| MechanismError::Failed {
            code: "clipboard_failed".into(),
            message: format!("could not locate agenterm-cu to persist CLIPBOARD: {error}"),
        })?;
        let exe_name = exe.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if exe_name != "agenterm-cu" && exe_name != "agenterm-cu.exe" {
            return set_text(text);
        }
        let mut child = Command::new(&exe);
        child
            .arg(X11_CLIPBOARD_OWNER_ARG)
            .env(X11_CLIPBOARD_SERVE_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            child.process_group(0);
        }
        let mut child = child.spawn().map_err(|error| MechanismError::Failed {
            code: "clipboard_failed".into(),
            message: format!("could not start CLIPBOARD owner: {error}"),
        })?;
        match child.stdin.take() {
            Some(mut stdin) => stdin.write_all(text.as_bytes()).map_err(|error| {
                let _ = child.kill();
                MechanismError::Failed {
                    code: "clipboard_failed".into(),
                    message: format!("could not send CLIPBOARD payload: {error}"),
                }
            })?,
            None => {
                let _ = child.kill();
                return Err(MechanismError::Failed {
                    code: "clipboard_failed".into(),
                    message: "CLIPBOARD owner stdin is missing".into(),
                });
            }
        }
        let deadline = Instant::now() + Duration::from_millis(2_000);
        loop {
            match get_text() {
                Ok(got) if got == text => return Ok(()),
                Ok(_) | Err(_) => {
                    if let Ok(Some(status)) = child.try_wait() {
                        return Err(MechanismError::Failed {
                            code: "clipboard_failed".into(),
                            message: format!("CLIPBOARD owner exited before serving ({status})"),
                        });
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        return Err(MechanismError::Failed {
                            code: "clipboard_failed".into(),
                            message: "CLIPBOARD owner did not become readable in time".into(),
                        });
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }

    /// Owner-process entry: publish `text` and (on X11 with the serve env)
    /// block in the selection loop until replaced.
    pub fn own_text(text: &str) -> Result<(), MechanismError> {
        set_text(text)
    }

    /// Two-stage `agt_clipboard_get_text`. No Unicode text is an empty
    /// success (`AGT_OK`, `out_len == 0`), not `buffer_too_small`.
    pub fn get_text() -> Result<String, MechanismError> {
        let f = call_sym::<super::ClipboardGetText>(b"agt_clipboard_get_text")?;
        let mut required = 0usize;
        let status = unsafe { f(std::ptr::null_mut(), 0, &mut required) };
        if status == dynlib::AGT_UNSUPPORTED {
            return Err(MechanismError::Unsupported {
                reason: "agt_clipboard_get_text: mechanism unavailable on this host".to_owned(),
            });
        }
        if status == dynlib::AGT_OK {
            return Ok(String::new());
        }
        if status != dynlib::AGT_FAILED {
            return Err(last_mechanism_error("agt_clipboard_get_text"));
        }
        let probe = last_mechanism_error("agt_clipboard_get_text");
        match &probe {
            MechanismError::Failed { code, .. } if code == "buffer_too_small" => {}
            other => return Err(other.clone()),
        }
        if required == 0 {
            return Ok(String::new());
        }
        let mut buf = vec![0u8; required];
        let status = unsafe { f(buf.as_mut_ptr(), required, &mut required) };
        map_status("agt_clipboard_get_text", status)?;
        buf.truncate(required);
        String::from_utf8(buf).map_err(|_| MechanismError::Failed {
            code: "bad_encoding".into(),
            message: "clipboard text is not UTF-8".into(),
        })
    }
}

pub fn send_node_keys(
    window: Option<isize>,
    node_id: &str,
    keys: &str,
) -> Result<(), MechanismError> {
    let handle = window.unwrap_or(0);
    let node_c = CStringOrStack::new(node_id)?;
    let f = call_sym::<NodeSendKeys>(b"agt_a11y_node_send_keys")?;
    let status = unsafe { f(handle, node_c.as_ptr(), keys.as_ptr(), keys.len()) };
    map_status("agt_a11y_node_send_keys", status)?;
    Ok(())
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
    let f = call_sym::<TreeNode>(b"agt_a11y_tree_node")?;
    let status = unsafe { f(index, &mut record) };
    map_status("agt_a11y_tree_node", status)?;
    let id = fixed_field(&record.id, record.id_len);
    let parent_id = if record.has_parent != 0 {
        Some(fixed_field(&record.parent_id, record.parent_id_len))
    } else {
        None
    };
    let role = read_node_string(index, dynlib::AGT_A11Y_STR_ROLE)?;
    let name = read_node_string(index, dynlib::AGT_A11Y_STR_NAME)?;
    let text_raw = read_node_string(index, dynlib::AGT_A11Y_STR_TEXT)?;
    let text = if text_raw.is_empty() {
        None
    } else {
        Some(text_raw)
    };
    let states_raw = read_node_string(index, dynlib::AGT_A11Y_STR_STATES)?;
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
    let bytes = read_two_stage(|buf, cap, out_len| {
        let f = call_sym::<MetaString>(b"agt_a11y_tree_meta_string")?;
        let status = unsafe { f(field, buf, cap, out_len) };
        Ok::<i32, MechanismError>(status)
    })?;
    String::from_utf8(bytes).map_err(|_| MechanismError::Failed {
        code: "bad_encoding".into(),
        message: "metadata string is not UTF-8".into(),
    })
}

fn read_node_string(node_index: usize, kind: i32) -> Result<String, MechanismError> {
    let bytes = read_two_stage(|buf, cap, out_len| {
        let f = call_sym::<NodeString>(b"agt_a11y_node_string")?;
        let status = unsafe { f(node_index, kind, buf, cap, out_len) };
        Ok::<i32, MechanismError>(status)
    })?;
    String::from_utf8(bytes).map_err(|_| MechanismError::Failed {
        code: "bad_encoding".into(),
        message: "node string is not UTF-8".into(),
    })
}

fn read_action_name(node_index: usize, action_index: usize) -> Result<String, MechanismError> {
    let bytes = read_two_stage(|buf, cap, out_len| {
        let f = call_sym::<NodeActionName>(b"agt_a11y_node_action_name")?;
        let status = unsafe { f(node_index, action_index, buf, cap, out_len) };
        Ok::<i32, MechanismError>(status)
    })?;
    String::from_utf8(bytes).map_err(|_| MechanismError::Failed {
        code: "bad_encoding".into(),
        message: "action name is not UTF-8".into(),
    })
}

fn read_two_stage(
    mut probe: impl FnMut(*mut u8, usize, *mut usize) -> Result<i32, MechanismError>,
) -> Result<Vec<u8>, MechanismError> {
    let mut required = 0usize;
    let status = probe(std::ptr::null_mut(), 0, &mut required)?;
    if status != dynlib::AGT_FAILED {
        return Err(last_mechanism_error("two_stage_probe"));
    }
    // libagenterm treats cap==0 as a size probe even when the payload is empty.
    // A second call with cap==0 would repeat buffer_too_small, so stop here.
    if required == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; required];
    let status = probe(buf.as_mut_ptr(), required, &mut required)?;
    map_status("two_stage_read", status)?;
    buf.truncate(required);
    Ok(buf)
}

fn fixed_field(bytes: &[u8], len: u32) -> String {
    String::from_utf8_lossy(&bytes[..len as usize]).into_owned()
}

fn map_status(operation: &str, status: i32) -> Result<(), MechanismError> {
    match status {
        dynlib::AGT_OK => Ok(()),
        dynlib::AGT_UNSUPPORTED => Err(MechanismError::Unsupported {
            reason: format!("{operation}: mechanism unavailable on this host"),
        }),
        _ => Err(last_mechanism_error(operation)),
    }
}

fn last_mechanism_error(operation: &str) -> MechanismError {
    let mut err = agt_error {
        operation: std::ptr::null(),
        code: std::ptr::null(),
        message: std::ptr::null(),
    };
    let read = call_sym::<LastError>(b"agt_last_error")
        .ok()
        .map(|f| unsafe { f(&mut err) })
        .unwrap_or(dynlib::AGT_FAILED);
    if read != dynlib::AGT_OK {
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
    let cstr = unsafe { CStr::from_ptr(ptr) };
    cstr.to_str().ok().map(str::to_owned)
}

/// Resolve one exported symbol from the cached library, mapping a load or
/// symbol-resolution failure into a typed [`MechanismError`].
fn call_sym<T>(name: &[u8]) -> Result<libloading::Symbol<'static, T>, MechanismError> {
    let lib = dynlib::load().map_err(|error| MechanismError::Failed {
        code: "dylib_load".into(),
        message: format!(
            "{}; tried: {}",
            error.message,
            error
                .tried
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    })?;
    unsafe { lib.sym(name) }.map_err(|message| MechanismError::Failed {
        code: "dylib_symbol".into(),
        message,
    })
}

/// NUL-terminated UTF-8 for node paths passed into FFI.
struct CStringOrStack(CString);

impl CStringOrStack {
    fn new(s: &str) -> Result<Self, MechanismError> {
        CString::new(s)
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

// ---------------------------------------------------------------------------
// Export signatures — identical to crates/agenterm-abi's exports.
// ---------------------------------------------------------------------------

type CapabilityQuery = unsafe extern "C" fn(i32) -> i32;
type DesktopHostOpen = unsafe extern "C" fn(
    *const dynlib::agt_desktop_action,
    usize,
    *mut *mut std::ffi::c_void,
) -> i32;
type DesktopHostPoll = unsafe extern "C" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32;
type DesktopHostClose = unsafe extern "C" fn(*mut std::ffi::c_void) -> i32;
type WindowEnumerate = unsafe extern "C" fn(*mut dynlib::agt_window_info, usize, *mut usize) -> i32;
type ScreenList = unsafe extern "C" fn(*mut dynlib::agt_screen_info, usize, *mut usize) -> i32;
type WindowShow = unsafe extern "C" fn(isize, i32) -> i32;
type WindowMove = unsafe extern "C" fn(isize, i32, i32, u32, u32) -> i32;
type WindowRect = unsafe extern "C" fn(isize, *mut i32, *mut i32, *mut u32, *mut u32) -> i32;
type WindowSetTopmost = unsafe extern "C" fn(isize, i32) -> i32;
type WindowClose = unsafe extern "C" fn(isize) -> i32;
type PointerMove = unsafe extern "C" fn(i32, i32) -> i32;
type PointerClick = unsafe extern "C" fn(i32, i32, i32, u32) -> i32;
type InputTypeText = unsafe extern "C" fn(*const u8, usize) -> i32;
type InputSendKeys = unsafe extern "C" fn(*const u8, usize) -> i32;
type CaptureWindow =
    unsafe extern "C" fn(isize, *const std::ffi::c_char, i32, i32, i32, i32, i32) -> i32;
type DrainBus = unsafe extern "C" fn() -> i32;
type LastTextWriteVia = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type TreeSnapshot = unsafe extern "C" fn(isize, *mut usize) -> i32;
type MetaString = unsafe extern "C" fn(i32, *mut u8, usize, *mut usize) -> i32;
type TreeNode = unsafe extern "C" fn(usize, *mut agt_a11y_node) -> i32;
type NodeString = unsafe extern "C" fn(usize, i32, *mut u8, usize, *mut usize) -> i32;
type NodeActionName = unsafe extern "C" fn(usize, usize, *mut u8, usize, *mut usize) -> i32;
type NodePerform = unsafe extern "C" fn(isize, *const std::ffi::c_char, i32) -> i32;
type NodeSetText = unsafe extern "C" fn(isize, *const std::ffi::c_char, *const u8, usize) -> i32;
type NodeGetText =
    unsafe extern "C" fn(isize, *const std::ffi::c_char, *mut u8, usize, *mut usize) -> i32;
type NodeSendKeys = unsafe extern "C" fn(isize, *const std::ffi::c_char, *const u8, usize) -> i32;
type NodeScroll = unsafe extern "C" fn(isize, *const std::ffi::c_char) -> i32;
type NodeGetExtents = unsafe extern "C" fn(
    isize,
    *const std::ffi::c_char,
    *mut i32,
    *mut i32,
    *mut i32,
    *mut i32,
) -> i32;
type NodeSetSelection = unsafe extern "C" fn(isize, *const std::ffi::c_char, i32, i32) -> i32;
type NodeGetSelection =
    unsafe extern "C" fn(isize, *const std::ffi::c_char, *mut i32, *mut i32, *mut i32) -> i32;
type ClipboardSetText = unsafe extern "C" fn(*const u8, usize) -> i32;
type ClipboardGetText = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;

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
            Ok::<i32, MechanismError>(dynlib::AGT_FAILED)
        })
        .expect("empty two-stage probe should succeed");
        assert!(bytes.is_empty());
        assert_eq!(calls, 1);
    }

    #[test]
    fn two_stage_record_fetch_retries_only_for_observed_growth() {
        assert_eq!(
            window_enumerate::retry_capacity(dynlib::AGT_FAILED, 8, 11),
            Some(11)
        );
        assert_eq!(
            window_enumerate::retry_capacity(dynlib::AGT_FAILED, 8, 8),
            None
        );
        assert_eq!(
            window_enumerate::retry_capacity(dynlib::AGT_OK, 8, 11),
            None
        );
    }
}
