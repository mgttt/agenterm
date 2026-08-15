//! macOS Accessibility (AX) tree client — PLACEHOLDER observe path.
//!
//! Ships a real `cfg(macos)` AX walk for `agenterm-cu --target current tree`
//! with typed permission / timeout / bound failures. Live black-box evidence
//! is **not** claimed on non-macOS hosts; a later macOS agent owns the fixture
//! gate (`345AXTREE` + `Fixture Press`, `backend:"ax"`).
//!
//! No click / focus / value actuation, no screenshot, no CGEvent fallback, and
//! no silent reuse of AT-SPI or UIA.

#![cfg(target_os = "macos")]

use std::collections::VecDeque;
use std::ffi::{CStr, c_void};
use std::time::{Duration, Instant};

use crate::CapabilityStatus;
use crate::contract::accessibility_tree::{
    AccessibilityBounds, AccessibilityNode, AccessibilityNodeAction, AccessibilitySelection,
    AccessibilityTree, AccessibilityTreeError,
};

type CfTypeRef = *const c_void;
type CfArrayRef = *const c_void;
type CfDictionaryRef = *const c_void;
type CfStringRef = *const c_void;
type CfIndex = isize;
type AxUiElementRef = *const c_void;
type AxValueRef = *const c_void;
type CgWindowId = u32;

const MAX_NODES: usize = 1_000;
const MAX_DEPTH: usize = 32;
const MAX_SIBLINGS_PER_LEVEL: usize = 1_000;
const MAX_NODE_ID_BYTES: usize = 4_096;
const MAX_STRING_BYTES: usize = 16 * 1024;
const MAX_TOTAL_STRING_BYTES: usize = 2 * 1024 * 1024;
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

const AX_SUCCESS: i32 = 0;
const AX_ERROR_API_DISABLED: i32 = -25211;
const AX_ERROR_INVALID_UI_ELEMENT: i32 = -25202;
const AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;
const AX_ERROR_NOT_IMPLEMENTED: i32 = -25208;
const AX_ERROR_NO_VALUE: i32 = -25212;

const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;

const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CF_NUMBER_SINT32: i32 = 3;
const K_CF_NUMBER_SINT64: i32 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CgSize {
    width: f64,
    height: f64,
}

// One `#[link]` per framework is the documented way to attach several of them
// to a single extern block; clippy reads the repeated attribute name as a
// copy-paste slip. Same false positive as foreign_windows.rs / hotkeys.rs.
#[allow(clippy::duplicated_attributes)]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CfArrayRef;

    fn CFRelease(cf: CfTypeRef);
    fn CFRetain(cf: CfTypeRef) -> CfTypeRef;
    fn CFArrayGetCount(array: CfArrayRef) -> CfIndex;
    fn CFArrayGetValueAtIndex(array: CfArrayRef, idx: CfIndex) -> CfTypeRef;
    fn CFDictionaryGetValue(dict: CfDictionaryRef, key: CfTypeRef) -> CfTypeRef;
    fn CFStringCreateWithCString(alloc: CfTypeRef, c_str: *const i8, encoding: u32) -> CfStringRef;
    fn CFStringGetCStringPtr(s: CfStringRef, encoding: u32) -> *const i8;
    fn CFStringGetCString(s: CfStringRef, buf: *mut i8, size: CfIndex, encoding: u32) -> bool;
    fn CFNumberGetValue(number: CfTypeRef, the_type: CfIndex, value_ptr: *mut c_void) -> bool;
    fn CFGetTypeID(cf: CfTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(boolean: CfTypeRef) -> u8;
    fn CFNumberGetTypeID() -> usize;

    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementCopyAttributeNames(element: AxUiElementRef, names: *mut CfTypeRef) -> i32;
    fn AXValueGetValue(value: AxValueRef, typ: u32, value_ptr: *mut c_void) -> u8;
    fn _AXUIElementGetWindow(element: AxUiElementRef, out: *mut CgWindowId) -> i32;
}

struct CfOwned(CfTypeRef);

impl CfOwned {
    fn from_create(ptr: CfTypeRef) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self(ptr))
        }
    }

    fn retain(ptr: CfTypeRef) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            unsafe {
                CFRetain(ptr);
            }
            Some(Self(ptr))
        }
    }

    fn as_ptr(&self) -> CfTypeRef {
        self.0
    }

    fn as_ax(&self) -> AxUiElementRef {
        self.0 as AxUiElementRef
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CFRelease(self.0);
            }
            self.0 = std::ptr::null();
        }
    }
}

struct Budget {
    deadline: Instant,
    total_string_bytes: usize,
}

impl Budget {
    fn new(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
            total_string_bytes: 0,
        }
    }

    fn check(&self) -> Result<(), AccessibilityTreeError> {
        if Instant::now() >= self.deadline {
            return Err(AccessibilityTreeError::failed(
                "a11y_tree_timeout",
                "AX tree snapshot exceeded its wall-clock deadline",
            ));
        }
        Ok(())
    }

    fn account_string(&mut self, value: &str) -> Result<(), AccessibilityTreeError> {
        if value.len() > MAX_STRING_BYTES {
            return Err(limit_error(
                "a11y_string_limit",
                format!("AX string exceeds {MAX_STRING_BYTES} UTF-8 bytes"),
            ));
        }
        self.total_string_bytes = self
            .total_string_bytes
            .checked_add(value.len())
            .ok_or_else(|| limit_error("a11y_string_limit", "string-byte budget overflow"))?;
        if self.total_string_bytes > MAX_TOTAL_STRING_BYTES {
            return Err(limit_error(
                "a11y_string_limit",
                format!("AX tree exceeds {MAX_TOTAL_STRING_BYTES} aggregate string bytes"),
            ));
        }
        Ok(())
    }

    fn account_node(&mut self, node: &AccessibilityNode) -> Result<(), AccessibilityTreeError> {
        self.account_string(&node.id)?;
        if let Some(parent) = &node.parent_id {
            self.account_string(parent)?;
        }
        self.account_string(&node.role)?;
        self.account_string(&node.name)?;
        for state in &node.states {
            self.account_string(state)?;
        }
        for action in &node.actions {
            self.account_string(action)?;
        }
        if let Some(text) = &node.text {
            self.account_string(text)?;
        }
        Ok(())
    }
}

fn limit_error(code: &'static str, message: impl ToString) -> AccessibilityTreeError {
    AccessibilityTreeError::failed(code, message)
}

fn cfstr(name: &str) -> CfStringRef {
    let c = std::ffi::CString::new(name).expect("AX attribute key must not contain NUL");
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8) }
}

fn cf_string(value: CfTypeRef) -> String {
    if value.is_null() {
        return String::new();
    }
    unsafe {
        if CFGetTypeID(value) != CFStringGetTypeID() {
            return String::new();
        }
        let ptr = CFStringGetCStringPtr(value as CfStringRef, K_CF_STRING_ENCODING_UTF8);
        if !ptr.is_null() {
            return CStr::from_ptr(ptr).to_string_lossy().into_owned();
        }
        let mut buf = [0i8; 4096];
        if CFStringGetCString(
            value as CfStringRef,
            buf.as_mut_ptr(),
            buf.len() as CfIndex,
            K_CF_STRING_ENCODING_UTF8,
        ) {
            return CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned();
        }
    }
    String::new()
}

fn cf_i64(value: CfTypeRef) -> Option<i64> {
    if value.is_null() {
        return None;
    }
    let mut out = 0i64;
    let ok = unsafe {
        CFNumberGetValue(
            value,
            K_CF_NUMBER_SINT64 as CfIndex,
            &mut out as *mut i64 as *mut c_void,
        )
    };
    if ok {
        return Some(out);
    }
    let mut out32 = 0i32;
    let ok = unsafe {
        CFNumberGetValue(
            value,
            K_CF_NUMBER_SINT32 as CfIndex,
            &mut out32 as *mut i32 as *mut c_void,
        )
    };
    if ok {
        Some(i64::from(out32))
    } else {
        None
    }
}

fn dict_get(dict: CfDictionaryRef, key: &str) -> CfTypeRef {
    unsafe {
        let k = cfstr(key);
        let v = CFDictionaryGetValue(dict, k as CfTypeRef);
        CFRelease(k as CfTypeRef);
        v
    }
}

fn map_ax_status(status: i32, operation: &str) -> Result<(), AccessibilityTreeError> {
    if status == AX_SUCCESS {
        return Ok(());
    }
    let (code, detail) = match status {
        AX_ERROR_API_DISABLED => (
            "a11y_permission_denied",
            "Accessibility permission is not granted for this process",
        ),
        AX_ERROR_INVALID_UI_ELEMENT => ("a11y_node_recycled", "AX element is no longer valid"),
        AX_ERROR_ATTRIBUTE_UNSUPPORTED | AX_ERROR_NOT_IMPLEMENTED | AX_ERROR_NO_VALUE => {
            return Err(AccessibilityTreeError::Unsupported {
                reason: format!("{operation}: AX attribute unavailable (status {status})").into(),
            });
        }
        AX_ERROR_CANNOT_COMPLETE => (
            "a11y_tree_timeout",
            "AX could not complete the request within its provider bound",
        ),
        _ => (
            "a11y_backend_failed",
            "AX call failed with an unexpected status",
        ),
    };
    Err(AccessibilityTreeError::failed(
        code,
        format!("{operation}: {detail} (AXError {status})"),
    ))
}

fn require_trusted() -> Result<(), AccessibilityTreeError> {
    if unsafe { AXIsProcessTrusted() } == 0 {
        return Err(AccessibilityTreeError::failed(
            "a11y_permission_denied",
            "AXIsProcessTrusted() is false; grant Accessibility for this process",
        ));
    }
    Ok(())
}

pub(crate) fn capability_status() -> CapabilityStatus {
    // The AX mechanism is compiled into this adapter. Permission is a runtime
    // failure on tree/actuation, not a missing host adapter.
    CapabilityStatus::Available
}

/// `None` walks every on-screen CG window under the same node/depth/string/time
/// bounds as a window-scoped snapshot. `Some(handle)` scopes to that CGWindowID.
pub(crate) fn tree_for_window(
    window_handle: Option<isize>,
) -> Result<AccessibilityTree, AccessibilityTreeError> {
    require_trusted()?;
    let mut budget = Budget::new(SNAPSHOT_TIMEOUT);
    let roots = resolve_roots(window_handle, &mut budget)?;
    if roots.is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_tree_empty",
            match window_handle {
                Some(handle) => format!("no AX window matched CGWindowID {handle}"),
                None => "no on-screen AX windows were found".to_owned(),
            },
        ));
    }

    let mut nodes = Vec::new();
    let mut queue: VecDeque<(CfOwned, String, Option<String>, usize)> = VecDeque::new();
    for (index, root) in roots.into_iter().enumerate() {
        let id = format!("/{index}");
        budget.account_string(&id)?;
        queue.push_back((root, id, None, 0));
    }

    while let Some((element, id, parent_id, depth)) = queue.pop_front() {
        budget.check()?;
        if nodes.len() >= MAX_NODES {
            return Err(limit_error(
                "a11y_node_limit",
                format!("AX tree exceeds {MAX_NODES} nodes"),
            ));
        }
        if id.len() > MAX_NODE_ID_BYTES {
            return Err(limit_error(
                "a11y_node_id_limit",
                format!("AX node id exceeds {MAX_NODE_ID_BYTES} bytes"),
            ));
        }

        let node = match read_node(element.as_ax(), id.clone(), parent_id.clone(), &mut budget) {
            Ok(node) => node,
            Err(error) if parent_id.is_some() && is_snapshot_branch_loss(&error) => continue,
            Err(error) => return Err(error),
        };
        budget.account_node(&node)?;
        nodes.push(node);

        let children = match copy_children(element.as_ax(), &mut budget) {
            Ok(children) => children,
            Err(error) if parent_id.is_some() && is_snapshot_branch_loss(&error) => continue,
            Err(error) => return Err(error),
        };
        if !children.is_empty() && depth >= MAX_DEPTH {
            return Err(limit_error(
                "a11y_depth_limit",
                format!("AX tree exceeds depth {MAX_DEPTH}"),
            ));
        }
        if children.len() > MAX_SIBLINGS_PER_LEVEL {
            return Err(limit_error(
                "a11y_node_limit",
                format!("AX node has more than {MAX_SIBLINGS_PER_LEVEL} children"),
            ));
        }
        for (child_index, child) in children.into_iter().enumerate() {
            budget.check()?;
            if nodes.len().saturating_add(queue.len()).saturating_add(1) > MAX_NODES {
                return Err(limit_error(
                    "a11y_node_limit",
                    format!("AX tree exceeds {MAX_NODES} nodes"),
                ));
            }
            let child_id = format!("{id}/{child_index}");
            queue.push_back((child, child_id, Some(id.clone()), depth + 1));
        }
    }

    if nodes.is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_tree_empty",
            "AX returned no nodes",
        ));
    }

    let root_id = nodes
        .first()
        .map(|node| node.id.clone())
        .unwrap_or_else(|| "/0".to_owned());

    Ok(AccessibilityTree {
        backend: "ax",
        window_handle,
        root_id,
        nodes,
    })
}

pub(crate) fn drain_bus() {}

pub(crate) fn perform_node_action(
    _window_handle: Option<isize>,
    _node_id: &str,
    _action: AccessibilityNodeAction,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX actuation (click/focus) is not implemented in this PLACEHOLDER cut"
            .into(),
    })
}

pub(crate) fn set_node_text(
    _window_handle: Option<isize>,
    _node_id: &str,
    _text: &str,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX set-text is not implemented in this PLACEHOLDER cut".into(),
    })
}

pub(crate) fn get_node_text(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<String, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX get-text is not implemented in this PLACEHOLDER cut".into(),
    })
}

pub(crate) fn last_text_write_via() -> &'static str {
    "ax-value"
}

pub(crate) fn send_node_keys(
    _window_handle: Option<isize>,
    _node_id: &str,
    _keys: &str,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX send-keys is not implemented in this PLACEHOLDER cut".into(),
    })
}

pub(crate) fn scroll_node(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX scroll is not implemented in this PLACEHOLDER cut".into(),
    })
}

pub(crate) fn get_node_extents(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<AccessibilityBounds, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX get-extents is not implemented in this PLACEHOLDER cut".into(),
    })
}

pub(crate) fn set_node_selection(
    _window_handle: Option<isize>,
    _node_id: &str,
    _start: i32,
    _end: i32,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX set-selection is not implemented in this PLACEHOLDER cut".into(),
    })
}

pub(crate) fn get_node_selection(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<AccessibilitySelection, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX get-selection is not implemented in this PLACEHOLDER cut".into(),
    })
}

pub(crate) fn set_node_caret_offset(
    _window_handle: Option<isize>,
    _node_id: &str,
    _offset: i32,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX set-caret is not implemented in this PLACEHOLDER cut".into(),
    })
}

pub(crate) fn get_node_caret_offset(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<i32, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "macOS AX get-caret is not implemented in this PLACEHOLDER cut".into(),
    })
}

fn resolve_roots(
    window_handle: Option<isize>,
    budget: &mut Budget,
) -> Result<Vec<CfOwned>, AccessibilityTreeError> {
    match window_handle {
        Some(handle) => {
            let element = ax_element_for_handle(handle, budget)?;
            Ok(vec![element])
        }
        None => all_on_screen_window_roots(budget),
    }
}

fn all_on_screen_window_roots(
    budget: &mut Budget,
) -> Result<Vec<CfOwned>, AccessibilityTreeError> {
    let windows = enumerate_cg_windows()?;
    let mut roots = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for window in windows {
        budget.check()?;
        if !seen.insert(window.id) {
            continue;
        }
        match ax_element_for_handle(window.id as isize, budget) {
            Ok(element) => roots.push(element),
            Err(error) if is_snapshot_branch_loss(&error) => continue,
            Err(AccessibilityTreeError::Failed { code, .. })
                if code == "a11y_permission_denied" =>
            {
                return Err(AccessibilityTreeError::failed(
                    "a11y_permission_denied",
                    "Accessibility permission is not granted for this process",
                ));
            }
            Err(_) => continue,
        }
        if roots.len() >= MAX_NODES {
            return Err(limit_error(
                "a11y_node_limit",
                format!("AX tree exceeds {MAX_NODES} window roots"),
            ));
        }
    }
    Ok(roots)
}

struct CgWindow {
    id: u32,
    pid: u32,
}

fn enumerate_cg_windows() -> Result<Vec<CgWindow>, AccessibilityTreeError> {
    unsafe {
        let array = CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            0,
        );
        let Some(array) = CfOwned::from_create(array) else {
            return Err(AccessibilityTreeError::failed(
                "a11y_backend_failed",
                "CGWindowListCopyWindowInfo returned null",
            ));
        };
        let count = CFArrayGetCount(array.as_ptr() as CfArrayRef);
        let mut out = Vec::new();
        for i in 0..count {
            let item = CFArrayGetValueAtIndex(array.as_ptr() as CfArrayRef, i);
            if item.is_null() {
                continue;
            }
            let dict = item as CfDictionaryRef;
            let layer = cf_i64(dict_get(dict, "kCGWindowLayer")).unwrap_or(0);
            if layer != 0 {
                continue;
            }
            let id = cf_i64(dict_get(dict, "kCGWindowNumber")).unwrap_or(0);
            if id == 0 {
                continue;
            }
            let pid = cf_i64(dict_get(dict, "kCGWindowOwnerPID")).unwrap_or(0);
            if pid <= 0 {
                continue;
            }
            out.push(CgWindow {
                id: id as u32,
                pid: pid as u32,
            });
        }
        Ok(out)
    }
}

fn owner_pid(handle: isize) -> Result<u32, AccessibilityTreeError> {
    let target = u32::try_from(handle).map_err(|_| {
        AccessibilityTreeError::failed(
            "a11y_window_gone",
            format!("window handle {handle} is not a CGWindowID"),
        )
    })?;
    for window in enumerate_cg_windows()? {
        if window.id == target {
            return Ok(window.pid);
        }
    }
    Err(AccessibilityTreeError::failed(
        "a11y_window_gone",
        format!("no on-screen window for CGWindowID {target}"),
    ))
}

fn ax_element_for_handle(
    handle: isize,
    budget: &mut Budget,
) -> Result<CfOwned, AccessibilityTreeError> {
    budget.check()?;
    let pid = owner_pid(handle)?;
    let target = handle as u32;
    unsafe {
        let app = AXUIElementCreateApplication(pid as i32);
        let Some(app) = CfOwned::from_create(app as CfTypeRef) else {
            return Err(AccessibilityTreeError::failed(
                "a11y_backend_failed",
                "AXUIElementCreateApplication returned null",
            ));
        };
        let windows_key = cfstr("AXWindows");
        let mut windows: CfTypeRef = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(app.as_ax(), windows_key, &mut windows);
        CFRelease(windows_key as CfTypeRef);
        if status == AX_ERROR_API_DISABLED {
            return Err(AccessibilityTreeError::failed(
                "a11y_permission_denied",
                "AXWindows denied: Accessibility permission is not granted",
            ));
        }
        map_ax_status(status, "AXWindows")?;
        let Some(windows) = CfOwned::from_create(windows) else {
            return Err(AccessibilityTreeError::failed(
                "a11y_tree_empty",
                format!("AXWindows was null for pid {pid}"),
            ));
        };
        let count = CFArrayGetCount(windows.as_ptr() as CfArrayRef);
        for i in 0..count {
            budget.check()?;
            let el = CFArrayGetValueAtIndex(windows.as_ptr() as CfArrayRef, i);
            if el.is_null() {
                continue;
            }
            let mut id = 0u32;
            if _AXUIElementGetWindow(el as AxUiElementRef, &mut id) == AX_SUCCESS && id == target {
                return CfOwned::retain(el).ok_or_else(|| {
                    AccessibilityTreeError::failed(
                        "a11y_backend_failed",
                        "failed to retain AX window element",
                    )
                });
            }
        }
    }
    Err(AccessibilityTreeError::failed(
        "a11y_window_gone",
        format!("no AX window for CGWindowID {handle}"),
    ))
}

fn copy_attribute(
    element: AxUiElementRef,
    name: &str,
    budget: &mut Budget,
) -> Result<Option<CfOwned>, AccessibilityTreeError> {
    budget.check()?;
    unsafe {
        let key = cfstr(name);
        let mut value: CfTypeRef = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(element, key, &mut value);
        CFRelease(key as CfTypeRef);
        if status == AX_ERROR_API_DISABLED {
            return Err(AccessibilityTreeError::failed(
                "a11y_permission_denied",
                format!("{name}: Accessibility permission is not granted"),
            ));
        }
        if status == AX_ERROR_ATTRIBUTE_UNSUPPORTED
            || status == AX_ERROR_NO_VALUE
            || status == AX_ERROR_NOT_IMPLEMENTED
        {
            return Ok(None);
        }
        if status == AX_ERROR_INVALID_UI_ELEMENT {
            return Err(AccessibilityTreeError::failed(
                "a11y_node_recycled",
                format!("{name}: AX element disappeared"),
            ));
        }
        if status == AX_ERROR_CANNOT_COMPLETE {
            return Err(AccessibilityTreeError::failed(
                "a11y_tree_timeout",
                format!("{name}: AX could not complete"),
            ));
        }
        if status != AX_SUCCESS {
            return Err(AccessibilityTreeError::failed(
                "a11y_backend_failed",
                format!("{name}: AXError {status}"),
            ));
        }
        Ok(CfOwned::from_create(value))
    }
}

fn copy_children(
    element: AxUiElementRef,
    budget: &mut Budget,
) -> Result<Vec<CfOwned>, AccessibilityTreeError> {
    let Some(array) = copy_attribute(element, "AXChildren", budget)? else {
        return Ok(Vec::new());
    };
    unsafe {
        let count = CFArrayGetCount(array.as_ptr() as CfArrayRef);
        if count < 0 {
            return Ok(Vec::new());
        }
        if count as usize > MAX_SIBLINGS_PER_LEVEL {
            return Err(limit_error(
                "a11y_node_limit",
                format!("AXChildren count exceeds {MAX_SIBLINGS_PER_LEVEL}"),
            ));
        }
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            budget.check()?;
            let child = CFArrayGetValueAtIndex(array.as_ptr() as CfArrayRef, i);
            if child.is_null() {
                continue;
            }
            if let Some(owned) = CfOwned::retain(child) {
                out.push(owned);
            }
        }
        Ok(out)
    }
}

fn read_node(
    element: AxUiElementRef,
    id: String,
    parent_id: Option<String>,
    budget: &mut Budget,
) -> Result<AccessibilityNode, AccessibilityTreeError> {
    budget.check()?;
    let role = attribute_string(element, "AXRole", budget)?.unwrap_or_default();
    let role = normalize_role(&role);
    let name = attribute_string(element, "AXTitle", budget)?
        .filter(|s| !s.is_empty())
        .or(attribute_string(element, "AXDescription", budget)?)
        .or(attribute_string(element, "AXIdentifier", budget)?)
        .unwrap_or_default();
    let text = attribute_string(element, "AXValue", budget)?.filter(|s| !s.is_empty());
    let bounds = read_bounds(element, budget)?;
    let states = read_states(element, budget, &bounds)?;
    let actions = read_actions(element, budget)?;

    Ok(AccessibilityNode {
        id,
        parent_id,
        role,
        name,
        states,
        bounds,
        actions,
        text,
    })
}

fn attribute_string(
    element: AxUiElementRef,
    name: &str,
    budget: &mut Budget,
) -> Result<Option<String>, AccessibilityTreeError> {
    let Some(value) = copy_attribute(element, name, budget)? else {
        return Ok(None);
    };
    let text = cf_string(value.as_ptr());
    if text.len() > MAX_STRING_BYTES {
        return Err(limit_error(
            "a11y_string_limit",
            format!("{name} exceeds {MAX_STRING_BYTES} UTF-8 bytes"),
        ));
    }
    // Number-typed AXValue (e.g. sliders) is not text content.
    if text.is_empty() {
        unsafe {
            if CFGetTypeID(value.as_ptr()) == CFNumberGetTypeID() {
                return Ok(None);
            }
        }
    }
    Ok(Some(text))
}

fn read_bounds(
    element: AxUiElementRef,
    budget: &mut Budget,
) -> Result<AccessibilityBounds, AccessibilityTreeError> {
    let pos = copy_attribute(element, "AXPosition", budget)?;
    let size = copy_attribute(element, "AXSize", budget)?;
    let (Some(pos), Some(size)) = (pos, size) else {
        return Ok(AccessibilityBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
    };
    let mut point = CgPoint { x: 0.0, y: 0.0 };
    let mut cg_size = CgSize {
        width: 0.0,
        height: 0.0,
    };
    let pok = unsafe {
        AXValueGetValue(
            pos.as_ptr() as AxValueRef,
            AX_VALUE_CGPOINT,
            &mut point as *mut CgPoint as *mut c_void,
        )
    };
    let sok = unsafe {
        AXValueGetValue(
            size.as_ptr() as AxValueRef,
            AX_VALUE_CGSIZE,
            &mut cg_size as *mut CgSize as *mut c_void,
        )
    };
    if pok == 0 || sok == 0 {
        return Ok(AccessibilityBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
    }
    Ok(AccessibilityBounds {
        x: point.x.round() as i32,
        y: point.y.round() as i32,
        width: cg_size.width.round().max(0.0) as i32,
        height: cg_size.height.round().max(0.0) as i32,
    })
}

fn read_states(
    element: AxUiElementRef,
    budget: &mut Budget,
    bounds: &AccessibilityBounds,
) -> Result<Vec<String>, AccessibilityTreeError> {
    let mut states = Vec::new();
    if attribute_bool(element, "AXEnabled", budget)?.unwrap_or(true) {
        states.push("enabled".to_owned());
    }
    // Presence of AXFocused means the element participates in focus; the
    // boolean value is the current focus state.
    match attribute_bool(element, "AXFocused", budget)? {
        Some(true) => {
            states.push("focusable".to_owned());
            states.push("focused".to_owned());
        }
        Some(false) => states.push("focusable".to_owned()),
        None => {}
    }
    if !attribute_bool(element, "AXHidden", budget)?.unwrap_or(false)
        && bounds.width > 0
        && bounds.height > 0
    {
        states.push("showing".to_owned());
        states.push("visible".to_owned());
    }
    if attribute_bool(element, "AXSelected", budget)?.unwrap_or(false) {
        states.push("selected".to_owned());
    }
    Ok(states)
}

fn attribute_bool(
    element: AxUiElementRef,
    name: &str,
    budget: &mut Budget,
) -> Result<Option<bool>, AccessibilityTreeError> {
    let Some(value) = copy_attribute(element, name, budget)? else {
        return Ok(None);
    };
    unsafe {
        if CFGetTypeID(value.as_ptr()) == CFBooleanGetTypeID() {
            return Ok(Some(CFBooleanGetValue(value.as_ptr()) != 0));
        }
    }
    Ok(None)
}

fn read_actions(
    element: AxUiElementRef,
    budget: &mut Budget,
) -> Result<Vec<String>, AccessibilityTreeError> {
    // Prefer AXActions when present; fall back to scanning action-like names.
    if let Some(array) = copy_attribute(element, "AXActions", budget)? {
        return actions_from_array(array.as_ptr() as CfArrayRef, budget);
    }
    // Some elements only expose press via performable action names query.
    let _ = element;
    Ok(Vec::new())
}

fn actions_from_array(
    array: CfArrayRef,
    budget: &mut Budget,
) -> Result<Vec<String>, AccessibilityTreeError> {
    unsafe {
        let count = CFArrayGetCount(array);
        let mut out = Vec::new();
        for i in 0..count {
            budget.check()?;
            let item = CFArrayGetValueAtIndex(array, i);
            if item.is_null() {
                continue;
            }
            let raw = cf_string(item);
            if raw.is_empty() {
                continue;
            }
            if let Some(normalized) = normalize_action(&raw) {
                if !out.iter().any(|existing| existing == &normalized) {
                    budget.account_string(&normalized)?;
                    out.push(normalized);
                }
            }
        }
        Ok(out)
    }
}

fn normalize_role(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_prefix = trimmed.strip_prefix("AX").unwrap_or(trimmed);
    if without_prefix.is_empty() {
        return "unknown".to_owned();
    }
    // AXStaticText -> statictext; keep a short stable token.
    without_prefix
        .chars()
        .flat_map(|ch| {
            if ch.is_uppercase() {
                vec!['-', ch.to_ascii_lowercase()]
            } else {
                vec![ch.to_ascii_lowercase()]
            }
        })
        .collect::<String>()
        .trim_start_matches('-')
        .to_owned()
}

fn normalize_action(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mapped = match trimmed {
        "AXPress" | "press" | "click" => "click",
        "AXRaise" | "raise" | "focus" => "focus",
        "AXConfirm" | "confirm" => "confirm",
        "AXCancel" | "cancel" => "cancel",
        "AXShowMenu" | "showMenu" => "show-menu",
        "AXPick" | "pick" => "pick",
        other => {
            let stripped = other.strip_prefix("AX").unwrap_or(other);
            return Some(
                stripped
                    .chars()
                    .flat_map(|ch| {
                        if ch.is_uppercase() {
                            vec!['-', ch.to_ascii_lowercase()]
                        } else {
                            vec![ch.to_ascii_lowercase()]
                        }
                    })
                    .collect::<String>()
                    .trim_start_matches('-')
                    .to_owned(),
            );
        }
    };
    Some(mapped.to_owned())
}

fn is_snapshot_branch_loss(error: &AccessibilityTreeError) -> bool {
    matches!(
        error,
        AccessibilityTreeError::Failed { code, .. }
            if code == "a11y_node_recycled"
                || code == "a11y_window_gone"
                || code == "a11y_tree_timeout"
    )
}

// Silence unused-import noise if attribute-name probing is added later.
#[allow(dead_code)]
fn copy_attribute_names(
    element: AxUiElementRef,
    budget: &mut Budget,
) -> Result<Vec<String>, AccessibilityTreeError> {
    budget.check()?;
    unsafe {
        let mut names: CfTypeRef = std::ptr::null();
        let status = AXUIElementCopyAttributeNames(element, &mut names);
        if status == AX_ERROR_API_DISABLED {
            return Err(AccessibilityTreeError::failed(
                "a11y_permission_denied",
                "AX attribute names denied",
            ));
        }
        if status != AX_SUCCESS || names.is_null() {
            return Ok(Vec::new());
        }
        let names = CfOwned::from_create(names).unwrap();
        let count = CFArrayGetCount(names.as_ptr() as CfArrayRef);
        let mut out = Vec::new();
        for i in 0..count {
            let item = CFArrayGetValueAtIndex(names.as_ptr() as CfArrayRef, i);
            let s = cf_string(item);
            if !s.is_empty() {
                out.push(s);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_action, normalize_role};

    #[test]
    fn normalizes_ax_roles() {
        assert_eq!(normalize_role("AXButton"), "button");
        assert_eq!(normalize_role("AXStaticText"), "static-text");
        assert_eq!(normalize_role("AXTextField"), "text-field");
        assert_eq!(normalize_role(""), "unknown");
    }

    #[test]
    fn normalizes_ax_actions() {
        assert_eq!(normalize_action("AXPress").as_deref(), Some("click"));
        assert_eq!(normalize_action("AXRaise").as_deref(), Some("focus"));
        assert_eq!(
            normalize_action("AXShowMenu").as_deref(),
            Some("show-menu")
        );
    }
}
