//! Windows UI Automation accessibility-tree client.
//!
//! The SDK's `windows-sys` bindings expose UIA constants and the provider-side
//! C API, but not the client COM interfaces. This module therefore owns a
//! deliberately small, layout-tested subset of those vtables. Every pointer,
//! BSTR, SAFEARRAY, VARIANT, and COM apartment has one RAII owner. No COM
//! pointer crosses a thread or survives its apartment.

use std::cell::Cell;
use std::collections::{HashSet, VecDeque};
use std::ffi::c_void;
use std::fmt::Write as _;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};
use std::rc::Rc;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CO_E_OBJNOTCONNECTED, E_ACCESSDENIED, E_FAIL, E_NOINTERFACE, HWND, RPC_E_CALL_REJECTED,
    RPC_E_CHANGED_MODE, RPC_E_DISCONNECTED, RPC_E_SERVERCALL_RETRYLATER, SysAllocStringLen,
    SysFreeString, SysStringLen,
};
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    SAFEARRAY,
};
use windows_sys::Win32::System::Ole::{
    SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
};
use windows_sys::Win32::System::Variant::{
    VARIANT, VT_ARRAY, VT_BOOL, VT_BSTR, VT_EMPTY, VT_I4, VT_R8, VariantClear,
};
use windows_sys::Win32::UI::Accessibility::{
    CUIAutomation8, UIA_AppBarControlTypeId, UIA_BoundingRectanglePropertyId,
    UIA_ButtonControlTypeId, UIA_CalendarControlTypeId, UIA_CheckBoxControlTypeId,
    UIA_ComboBoxControlTypeId, UIA_ControlTypePropertyId, UIA_CustomControlTypeId,
    UIA_DataGridControlTypeId, UIA_DataItemControlTypeId, UIA_DocumentControlTypeId,
    UIA_E_ELEMENTNOTAVAILABLE, UIA_E_ELEMENTNOTENABLED, UIA_E_INVALIDOPERATION, UIA_E_NOTSUPPORTED,
    UIA_E_TIMEOUT, UIA_EditControlTypeId, UIA_GroupControlTypeId, UIA_HasKeyboardFocusPropertyId,
    UIA_HeaderControlTypeId, UIA_HeaderItemControlTypeId, UIA_HyperlinkControlTypeId,
    UIA_ImageControlTypeId, UIA_InvokePatternId, UIA_IsContentElementPropertyId,
    UIA_IsControlElementPropertyId, UIA_IsEnabledPropertyId, UIA_IsKeyboardFocusablePropertyId,
    UIA_IsOffscreenPropertyId, UIA_IsPasswordPropertyId, UIA_LegacyIAccessiblePatternId,
    UIA_ListControlTypeId, UIA_ListItemControlTypeId, UIA_MenuBarControlTypeId,
    UIA_MenuControlTypeId, UIA_MenuItemControlTypeId, UIA_NamePropertyId, UIA_PaneControlTypeId,
    UIA_ProgressBarControlTypeId, UIA_RadioButtonControlTypeId, UIA_ScrollBarControlTypeId,
    UIA_SelectionItemPatternId, UIA_SemanticZoomControlTypeId, UIA_SeparatorControlTypeId,
    UIA_SliderControlTypeId, UIA_SpinnerControlTypeId, UIA_SplitButtonControlTypeId,
    UIA_StatusBarControlTypeId, UIA_TabControlTypeId, UIA_TabItemControlTypeId,
    UIA_TableControlTypeId, UIA_TextControlTypeId, UIA_TextPatternId, UIA_ThumbControlTypeId,
    UIA_TitleBarControlTypeId, UIA_TogglePatternId, UIA_ToolBarControlTypeId,
    UIA_ToolTipControlTypeId, UIA_TreeControlTypeId, UIA_TreeItemControlTypeId, UIA_ValuePatternId,
    UIA_WindowControlTypeId,
};
use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;
use windows_sys::core::{BSTR, GUID, HRESULT};

use crate::CapabilityStatus;
use crate::contract::accessibility_tree::{
    AccessibilityBounds, AccessibilityNode, AccessibilityNodeAction, AccessibilitySelection,
    AccessibilityTree, AccessibilityTreeError,
};
use crate::contract::input_inject::InputInjectError;

const MAX_NODES: usize = 1_000;
const MAX_DEPTH: usize = 32;
const MAX_SIBLINGS_PER_LEVEL: usize = 1_000;
const MAX_RUNTIME_ID_PARTS: usize = 32;
const MAX_NODE_ID_BYTES: usize = 4_096;
const MAX_STRING_BYTES: usize = 16 * 1024;
const MAX_TOTAL_STRING_BYTES: usize = 2 * 1024 * 1024;
const MAX_TEXT_UTF16_UNITS: usize = 8_192;
const MAX_SET_TEXT_BYTES: usize = 64 * 1024;
const MAX_KEYS_BYTES: usize = 256;
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
const ACTION_TIMEOUT: Duration = Duration::from_secs(2);
const COM_CONNECTION_TIMEOUT_MS: u32 = 500;
const COM_TRANSACTION_TIMEOUT_MS: u32 = 250;
const SEND_NODE_KEYS_VIA: &str = "uia-focus+send-input";

const IID_IUIAUTOMATION2: GUID = GUID::from_u128(0x34723aff_0c9d_49d0_9896_7ab52df8cd8a);
const IID_INVOKE_PATTERN: GUID = GUID::from_u128(0xfb377fbe_8ea6_46d5_9c73_6499642d3059);
const IID_LEGACY_PATTERN: GUID = GUID::from_u128(0x828055ad_355b_4435_86d5_3b51c14a9b1b);
const IID_SELECTION_ITEM_PATTERN: GUID = GUID::from_u128(0xa8efa66a_0fda_421a_9194_38021f3578ea);
const IID_TEXT_PATTERN: GUID = GUID::from_u128(0x32eba289_3583_42c9_9c59_3b6d9a1e9b6a);
const IID_TOGGLE_PATTERN: GUID = GUID::from_u128(0x94cf8058_9b8d_4ab9_8bfd_4cd0a33c8c70);
const IID_VALUE_PATTERN: GUID = GUID::from_u128(0xa94cd8b1_0844_4cd6_9d2d_640537ab39e9);

// IUIAutomation contributes 55 methods after IUnknown. IUIAutomation2 then
// adds AutoSetFocus, SetAutoSetFocus, ConnectionTimeout,
// SetConnectionTimeout, TransactionTimeout, SetTransactionTimeout.
const IUIAUTOMATION2_SET_CONNECTION_TIMEOUT_SLOT: usize = 61;
const IUIAUTOMATION2_SET_TRANSACTION_TIMEOUT_SLOT: usize = 63;
const IUIAUTOMATION2_SET_AUTO_SET_FOCUS_SLOT: usize = 59;

thread_local! {
    static LAST_TEXT_WRITE_VIA: Cell<&'static str> = const { Cell::new("value-pattern") };
}

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

/// `None` deliberately means the desktop ControlView root, not an unbounded
/// machine-wide crawl. It has the same node, depth, string, per-COM-call, and
/// wall-clock limits as a window-scoped snapshot.
pub(crate) fn tree_for_window(
    window_handle: Option<isize>,
) -> Result<AccessibilityTree, AccessibilityTreeError> {
    let mut budget = Budget::new(
        SNAPSHOT_TIMEOUT,
        "a11y_tree_timeout",
        "UI Automation tree snapshot",
    );
    let session = UiaSession::new(&budget)?;
    let root = session.root_element(window_handle, &budget)?;
    let root_segment = root_segment(window_handle);
    let root_id = format!("/{root_segment}");
    budget.account_string(&root_id)?;

    let mut nodes = Vec::new();
    let mut queue = VecDeque::new();
    let mut seen_ids = HashSet::new();
    seen_ids.insert(root_id.clone());
    queue.push_back((root, root_id.clone(), None, 0usize));

    while let Some((element, id, parent_id, depth)) = queue.pop_front() {
        budget.check()?;
        if nodes.len() >= MAX_NODES {
            return Err(limit_error(
                "a11y_node_limit",
                format!("UI Automation tree exceeds {MAX_NODES} nodes"),
            ));
        }

        let node = match session.read_node(&element, id.clone(), parent_id.clone(), &mut budget) {
            Ok(node) => node,
            Err(error) if parent_id.is_some() && is_snapshot_branch_loss(&error) => continue,
            Err(error) => return Err(error),
        };
        budget.account_node(&node)?;
        nodes.push(node);

        let first_child = session.first_child(&element, &budget)?;
        if depth >= MAX_DEPTH {
            if first_child.is_some() {
                return Err(limit_error(
                    "a11y_depth_limit",
                    format!("UI Automation tree exceeds depth {MAX_DEPTH}"),
                ));
            }
            continue;
        }

        let mut current = first_child;
        let mut sibling_count = 0usize;
        while let Some(child) = current {
            budget.check()?;
            sibling_count += 1;
            if sibling_count > MAX_SIBLINGS_PER_LEVEL
                || nodes.len().saturating_add(queue.len()) >= MAX_NODES
            {
                return Err(limit_error(
                    "a11y_node_limit",
                    format!("UI Automation tree exceeds {MAX_NODES} nodes"),
                ));
            }
            let next = session.next_sibling(&child, &budget)?;
            let runtime_id = match session.runtime_id(&child, &budget) {
                Ok(runtime_id) => runtime_id,
                Err(error) if is_snapshot_branch_loss(&error) => {
                    current = next;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let segment = match runtime_segment(&runtime_id) {
                Ok(segment) => segment,
                Err(error) if is_snapshot_branch_loss(&error) => {
                    current = next;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let child_id = format!("{id}/{segment}");
            if child_id.len() > MAX_NODE_ID_BYTES {
                return Err(limit_error(
                    "a11y_node_id_limit",
                    format!("UI Automation node id exceeds {MAX_NODE_ID_BYTES} bytes"),
                ));
            }
            if !seen_ids.insert(child_id.clone()) {
                return Err(AccessibilityTreeError::failed(
                    "a11y_runtime_id_duplicate",
                    format!("UI Automation returned duplicate runtime path {child_id}"),
                ));
            }
            queue.push_back((child, child_id, Some(id.clone()), depth + 1));
            current = next;
        }
    }

    if nodes.is_empty() {
        return Err(AccessibilityTreeError::failed(
            "a11y_tree_empty",
            "UI Automation returned no nodes",
        ));
    }

    Ok(AccessibilityTree {
        backend: "uia",
        window_handle,
        root_id,
        nodes,
    })
}

pub(crate) fn drain_bus() {}

pub(crate) fn perform_node_action(
    window_handle: Option<isize>,
    node_id: &str,
    action: AccessibilityNodeAction,
) -> Result<(), AccessibilityTreeError> {
    let budget = Budget::new(
        ACTION_TIMEOUT,
        "a11y_action_timeout",
        "UI Automation node action",
    );
    let session = UiaSession::new(&budget)?;
    let element = session.resolve_node(window_handle, node_id, &budget)?;
    match action {
        AccessibilityNodeAction::Focus => session.set_focus(&element, &budget),
        AccessibilityNodeAction::Click => session.click(&element, &budget),
    }
}

pub(crate) fn set_node_text(
    window_handle: Option<isize>,
    node_id: &str,
    text: &str,
) -> Result<(), AccessibilityTreeError> {
    if text.len() > MAX_SET_TEXT_BYTES {
        return Err(limit_error(
            "a11y_text_limit",
            format!("text exceeds {MAX_SET_TEXT_BYTES} UTF-8 bytes"),
        ));
    }
    let budget = Budget::new(
        ACTION_TIMEOUT,
        "a11y_text_timeout",
        "UI Automation text write",
    );
    let session = UiaSession::new(&budget)?;
    let element = session.resolve_node(window_handle, node_id, &budget)?;
    session.set_text(&element, text, &budget)?;
    LAST_TEXT_WRITE_VIA.with(|via| via.set("value-pattern"));
    Ok(())
}

pub(crate) fn get_node_text(
    window_handle: Option<isize>,
    node_id: &str,
) -> Result<String, AccessibilityTreeError> {
    let budget = Budget::new(
        ACTION_TIMEOUT,
        "a11y_text_timeout",
        "UI Automation text read",
    );
    let session = UiaSession::new(&budget)?;
    let element = session.resolve_node(window_handle, node_id, &budget)?;
    session.get_text(&element, &budget)
}

pub(crate) fn last_text_write_via() -> &'static str {
    LAST_TEXT_WRITE_VIA.with(Cell::get)
}

/// Resolves and focuses through UIA, then delegates only the actual key chord
/// to the selected platform input mechanism. The externally meaningful route
/// is `uia-focus+send-input`; no pointer or coordinate fallback exists.
pub(crate) fn send_node_keys(
    window_handle: Option<isize>,
    node_id: &str,
    keys: &str,
) -> Result<(), AccessibilityTreeError> {
    if keys.len() > MAX_KEYS_BYTES {
        return Err(limit_error(
            "a11y_key_limit",
            format!("key chord exceeds {MAX_KEYS_BYTES} UTF-8 bytes"),
        ));
    }
    let budget = Budget::new(
        ACTION_TIMEOUT,
        "a11y_key_timeout",
        "UI Automation key delivery",
    );
    let session = UiaSession::new(&budget)?;
    let element = session.resolve_node(window_handle, node_id, &budget)?;
    session.set_focus(&element, &budget)?;
    crate::selected::input_inject::send_keys(keys).map_err(map_input_error)
}

pub(crate) fn scroll_node(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "AT-SPI Component.ScrollTo is unavailable through Windows UIA".into(),
    })
}

pub(crate) fn get_node_extents(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<AccessibilityBounds, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "independent AT-SPI Component.GetExtents is unavailable through Windows UIA".into(),
    })
}

pub(crate) fn set_node_selection(
    _window_handle: Option<isize>,
    _node_id: &str,
    _start: i32,
    _end: i32,
) -> Result<(), AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "AT-SPI Text.SetSelection is unavailable through Windows UIA".into(),
    })
}

pub(crate) fn get_node_selection(
    _window_handle: Option<isize>,
    _node_id: &str,
) -> Result<AccessibilitySelection, AccessibilityTreeError> {
    Err(AccessibilityTreeError::Unsupported {
        reason: "AT-SPI Text.GetSelection is unavailable through Windows UIA".into(),
    })
}

struct Budget {
    deadline: Instant,
    timeout_code: &'static str,
    operation: &'static str,
    total_string_bytes: usize,
}

impl Budget {
    fn new(duration: Duration, timeout_code: &'static str, operation: &'static str) -> Self {
        Self {
            deadline: Instant::now() + duration,
            timeout_code,
            operation,
            total_string_bytes: 0,
        }
    }

    fn check(&self) -> Result<(), AccessibilityTreeError> {
        if Instant::now() >= self.deadline {
            return Err(AccessibilityTreeError::failed(
                self.timeout_code,
                format!("{} exceeded its wall-clock deadline", self.operation),
            ));
        }
        Ok(())
    }

    fn account_string(&mut self, value: &str) -> Result<(), AccessibilityTreeError> {
        if value.len() > MAX_STRING_BYTES && value.len() > MAX_NODE_ID_BYTES {
            return Err(limit_error(
                "a11y_string_limit",
                format!("UI Automation string exceeds {MAX_STRING_BYTES} UTF-8 bytes"),
            ));
        }
        self.total_string_bytes = self
            .total_string_bytes
            .checked_add(value.len())
            .ok_or_else(|| limit_error("a11y_string_limit", "string-byte budget overflow"))?;
        if self.total_string_bytes > MAX_TOTAL_STRING_BYTES {
            return Err(limit_error(
                "a11y_string_limit",
                format!(
                    "UI Automation tree exceeds {MAX_TOTAL_STRING_BYTES} aggregate string bytes"
                ),
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

struct UiaSession {
    // Field order is intentional: interface pointers release before COM is
    // uninitialized by `_apartment`.
    automation: ComPtr,
    walker: ComPtr,
    _apartment: ComApartment,
}

impl UiaSession {
    fn new(budget: &Budget) -> Result<Self, AccessibilityTreeError> {
        budget.check()?;
        let apartment = ComApartment::initialize()?;
        let mut raw = ptr::null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &CUIAutomation8,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IUIAUTOMATION2,
                &mut raw,
            )
        };
        hresult(hr, "CoCreateInstance(CUIAutomation8)")?;
        let automation = unsafe { ComPtr::from_raw(raw, "IUIAutomation2")? };
        unsafe {
            set_automation_timeout(
                &automation,
                IUIAUTOMATION2_SET_AUTO_SET_FOCUS_SLOT,
                0,
                "IUIAutomation2.SetAutoSetFocus(FALSE)",
            )?;
            set_automation_timeout(
                &automation,
                IUIAUTOMATION2_SET_CONNECTION_TIMEOUT_SLOT,
                COM_CONNECTION_TIMEOUT_MS,
                "IUIAutomation2.SetConnectionTimeout",
            )?;
            set_automation_timeout(
                &automation,
                IUIAUTOMATION2_SET_TRANSACTION_TIMEOUT_SLOT,
                COM_TRANSACTION_TIMEOUT_MS,
                "IUIAutomation2.SetTransactionTimeout",
            )?;
        }
        budget.check()?;
        let walker = automation_control_view_walker(&automation)?;
        Ok(Self {
            automation,
            walker,
            _apartment: apartment,
        })
    }

    fn root_element(
        &self,
        window_handle: Option<isize>,
        budget: &Budget,
    ) -> Result<ComPtr, AccessibilityTreeError> {
        budget.check()?;
        let mut raw = ptr::null_mut();
        let vtable = unsafe { automation_vtable(&self.automation) };
        let hr = match window_handle {
            Some(handle) => {
                let hwnd = validate_window(handle)?;
                unsafe { ((*vtable).element_from_handle)(self.automation.as_ptr(), hwnd, &mut raw) }
            }
            None => unsafe { ((*vtable).get_root_element)(self.automation.as_ptr(), &mut raw) },
        };
        if hr < 0 {
            if let Some(handle) = window_handle
                && unsafe { IsWindow(handle as HWND) } == 0
            {
                return Err(window_gone(handle));
            }
            return Err(map_hresult("resolve UI Automation root", hr));
        }
        unsafe { ComPtr::from_raw(raw, "IUIAutomationElement root") }
    }

    fn runtime_id(
        &self,
        element: &ComPtr,
        budget: &Budget,
    ) -> Result<Vec<i32>, AccessibilityTreeError> {
        for attempt in 0..8 {
            budget.check()?;
            let mut raw = ptr::null_mut();
            let hr =
                unsafe { ((*element_vtable(element)).get_runtime_id)(element.as_ptr(), &mut raw) };
            hresult(hr, "IUIAutomationElement.GetRuntimeId")?;
            let array = unsafe { OwnedSafeArray::from_raw(raw, "UIA runtime id")? };
            let values = array.i32_values(MAX_RUNTIME_ID_PARTS)?;
            if !values.is_empty() || attempt == 7 {
                return Ok(values);
            }
            // Native proxies can transiently publish an empty SAFEARRAY just
            // after a window appears. Re-read within the snapshot deadline;
            // never manufacture an id that cannot be resolved later.
            std::thread::sleep(Duration::from_millis(1));
        }
        unreachable!("bounded UIA runtime-id loop always returns")
    }

    fn first_child(
        &self,
        element: &ComPtr,
        budget: &Budget,
    ) -> Result<Option<ComPtr>, AccessibilityTreeError> {
        self.walk(element, budget, true)
    }

    fn next_sibling(
        &self,
        element: &ComPtr,
        budget: &Budget,
    ) -> Result<Option<ComPtr>, AccessibilityTreeError> {
        self.walk(element, budget, false)
    }

    fn walk(
        &self,
        element: &ComPtr,
        budget: &Budget,
        first_child: bool,
    ) -> Result<Option<ComPtr>, AccessibilityTreeError> {
        budget.check()?;
        let vtable = unsafe { tree_walker_vtable(&self.walker) };
        for attempt in 0..3 {
            let mut raw = ptr::null_mut();
            let hr = unsafe {
                if first_child {
                    ((*vtable).get_first_child)(self.walker.as_ptr(), element.as_ptr(), &mut raw)
                } else {
                    ((*vtable).get_next_sibling)(self.walker.as_ptr(), element.as_ptr(), &mut raw)
                }
            };
            if hr >= 0 {
                return if raw.is_null() {
                    Ok(None)
                } else {
                    unsafe { ComPtr::from_raw(raw, "IUIAutomationElement child") }.map(Some)
                };
            }
            if !raw.is_null() {
                drop(unsafe { ComPtr::from_raw(raw, "failed UIA navigation result")? });
            }
            if hr != E_FAIL && hr as u32 != UIA_E_TIMEOUT {
                return hresult(hr, "IUIAutomationTreeWalker navigation").map(|()| None);
            }
            budget.check()?;
            if attempt < 2 {
                std::thread::yield_now();
            }
        }
        // A provider can disappear or stop responding between sibling
        // discovery and navigation. After bounded retries that branch is
        // absent from this snapshot rather than failing the whole desktop.
        Ok(None)
    }

    fn resolve_node(
        &self,
        window_handle: Option<isize>,
        node_id: &str,
        budget: &Budget,
    ) -> Result<ComPtr, AccessibilityTreeError> {
        let (root_anchor, path) = parse_runtime_path(node_id)?;
        let mut current = self.root_element(window_handle, budget)?;
        match root_anchor {
            RootAnchor::Desktop if window_handle.is_none() => {}
            RootAnchor::Window(expected) if window_handle == Some(expected) => {}
            RootAnchor::Runtime(expected) if self.runtime_id(&current, budget)? == expected => {}
            _ => return Err(node_recycled(node_id)),
        }

        for wanted in &path {
            let mut candidate = self.first_child(&current, budget)?;
            let mut scanned = 0usize;
            let mut found = None;
            while let Some(child) = candidate {
                scanned += 1;
                if scanned > MAX_SIBLINGS_PER_LEVEL {
                    return Err(limit_error(
                        "a11y_node_limit",
                        format!(
                            "node resolution exceeds {MAX_SIBLINGS_PER_LEVEL} siblings per level"
                        ),
                    ));
                }
                let next = self.next_sibling(&child, budget)?;
                if self.runtime_id(&child, budget)? == *wanted {
                    found = Some(child);
                    break;
                }
                candidate = next;
            }
            current = found.ok_or_else(|| node_recycled(node_id))?;
        }
        Ok(current)
    }

    fn read_node(
        &self,
        element: &ComPtr,
        id: String,
        parent_id: Option<String>,
        budget: &mut Budget,
    ) -> Result<AccessibilityNode, AccessibilityTreeError> {
        let name = self.property_string(element, UIA_NamePropertyId, budget)?;
        let control_type = self
            .property_i32(element, UIA_ControlTypePropertyId, budget)?
            .unwrap_or_default();
        let role = control_type_role(control_type);
        let enabled = self.property_bool(element, UIA_IsEnabledPropertyId, budget)?;
        let focusable = self.property_bool(element, UIA_IsKeyboardFocusablePropertyId, budget)?;
        let focused = self.property_bool(element, UIA_HasKeyboardFocusPropertyId, budget)?;
        let offscreen = self.property_bool(element, UIA_IsOffscreenPropertyId, budget)?;
        let password = self.property_bool(element, UIA_IsPasswordPropertyId, budget)?;
        let is_control = self.property_bool(element, UIA_IsControlElementPropertyId, budget)?;
        let is_content = self.property_bool(element, UIA_IsContentElementPropertyId, budget)?;
        let bounds = self.property_bounds(element, budget)?;

        let patterns = self.patterns(element, budget)?;
        let mut states = Vec::new();
        push_bool_state(&mut states, enabled, "enabled", "disabled");
        push_true_state(&mut states, focusable, "focusable");
        push_true_state(&mut states, focused, "focused");
        push_bool_state(&mut states, offscreen, "offscreen", "showing");
        push_true_state(&mut states, password, "password");
        push_true_state(&mut states, is_control, "control");
        push_true_state(&mut states, is_content, "content");

        if let Some(pattern) = &patterns.selection {
            let selected = pattern_bool(pattern, PatternBool::SelectionSelected)?;
            push_bool_state(&mut states, Some(selected), "selected", "unselected");
        }
        if let Some(pattern) = &patterns.toggle {
            states.push(toggle_state(pattern)?);
        }
        if let Some(pattern) = &patterns.value {
            let read_only = pattern_bool(pattern, PatternBool::ValueReadOnly)?;
            push_bool_state(&mut states, Some(read_only), "read-only", "editable");
        }

        let mut actions = Vec::new();
        if patterns.has_click() {
            actions.push("click".to_owned());
        }
        if focusable.unwrap_or(false) {
            actions.push("focus".to_owned());
            actions.push(SEND_NODE_KEYS_VIA.to_owned());
        }
        if patterns.value.is_some() {
            actions.push("set-text".to_owned());
            actions.push("get-text".to_owned());
        } else if patterns.text.is_some() {
            actions.push("get-text".to_owned());
        }

        let text = if password.unwrap_or(false) {
            None
        } else if let Some(value) = &patterns.value {
            let value = value_pattern_text(value)?;
            (!value.is_empty()).then_some(value)
        } else if let Some(text_pattern) = &patterns.text {
            let value = text_pattern_text(text_pattern)?;
            (!value.is_empty()).then_some(value)
        } else {
            None
        };

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

    fn property(
        &self,
        element: &ComPtr,
        property_id: i32,
        budget: &Budget,
    ) -> Result<OwnedVariant, AccessibilityTreeError> {
        budget.check()?;
        let mut value = OwnedVariant::new();
        let hr = unsafe {
            ((*element_vtable(element)).get_current_property_value)(
                element.as_ptr(),
                property_id,
                value.as_mut_ptr(),
            )
        };
        if hr as u32 == UIA_E_NOTSUPPORTED {
            return Ok(OwnedVariant::new());
        }
        hresult(hr, "IUIAutomationElement.GetCurrentPropertyValue")?;
        Ok(value)
    }

    fn property_string(
        &self,
        element: &ComPtr,
        property_id: i32,
        budget: &Budget,
    ) -> Result<String, AccessibilityTreeError> {
        let value = self.property(element, property_id, budget)?;
        value.string()?.map_or_else(|| Ok(String::new()), Ok)
    }

    fn property_bool(
        &self,
        element: &ComPtr,
        property_id: i32,
        budget: &Budget,
    ) -> Result<Option<bool>, AccessibilityTreeError> {
        self.property(element, property_id, budget)?.boolean()
    }

    fn property_i32(
        &self,
        element: &ComPtr,
        property_id: i32,
        budget: &Budget,
    ) -> Result<Option<i32>, AccessibilityTreeError> {
        self.property(element, property_id, budget)?.integer()
    }

    fn property_bounds(
        &self,
        element: &ComPtr,
        budget: &Budget,
    ) -> Result<AccessibilityBounds, AccessibilityTreeError> {
        self.property(element, UIA_BoundingRectanglePropertyId, budget)?
            .bounds()
    }

    fn patterns(
        &self,
        element: &ComPtr,
        budget: &Budget,
    ) -> Result<NodePatterns, AccessibilityTreeError> {
        Ok(NodePatterns {
            invoke: self.pattern(element, UIA_InvokePatternId, &IID_INVOKE_PATTERN, budget)?,
            selection: self.pattern(
                element,
                UIA_SelectionItemPatternId,
                &IID_SELECTION_ITEM_PATTERN,
                budget,
            )?,
            toggle: self.pattern(element, UIA_TogglePatternId, &IID_TOGGLE_PATTERN, budget)?,
            legacy: self.pattern(
                element,
                UIA_LegacyIAccessiblePatternId,
                &IID_LEGACY_PATTERN,
                budget,
            )?,
            value: self.pattern(element, UIA_ValuePatternId, &IID_VALUE_PATTERN, budget)?,
            text: self.pattern(element, UIA_TextPatternId, &IID_TEXT_PATTERN, budget)?,
        })
    }

    fn pattern(
        &self,
        element: &ComPtr,
        pattern_id: i32,
        iid: &GUID,
        budget: &Budget,
    ) -> Result<Option<ComPtr>, AccessibilityTreeError> {
        budget.check()?;
        let mut raw = ptr::null_mut();
        let hr = unsafe {
            ((*element_vtable(element)).get_current_pattern_as)(
                element.as_ptr(),
                pattern_id,
                iid,
                &mut raw,
            )
        };
        if hr as u32 == UIA_E_NOTSUPPORTED || hr == E_NOINTERFACE || raw.is_null() {
            return Ok(None);
        }
        hresult(hr, "IUIAutomationElement.GetCurrentPatternAs")?;
        unsafe { ComPtr::from_raw(raw, "UI Automation pattern") }.map(Some)
    }

    fn set_focus(&self, element: &ComPtr, budget: &Budget) -> Result<(), AccessibilityTreeError> {
        budget.check()?;
        let hr = unsafe { ((*element_vtable(element)).set_focus)(element.as_ptr()) };
        hresult(hr, "IUIAutomationElement.SetFocus")
    }

    fn click(&self, element: &ComPtr, budget: &Budget) -> Result<(), AccessibilityTreeError> {
        budget.check()?;
        if let Some(pattern) =
            self.pattern(element, UIA_InvokePatternId, &IID_INVOKE_PATTERN, budget)?
        {
            return invoke_pattern(&pattern);
        }
        if let Some(pattern) = self.pattern(
            element,
            UIA_SelectionItemPatternId,
            &IID_SELECTION_ITEM_PATTERN,
            budget,
        )? {
            return select_pattern(&pattern);
        }
        if let Some(pattern) =
            self.pattern(element, UIA_TogglePatternId, &IID_TOGGLE_PATTERN, budget)?
        {
            return toggle_pattern(&pattern);
        }
        if let Some(pattern) = self.pattern(
            element,
            UIA_LegacyIAccessiblePatternId,
            &IID_LEGACY_PATTERN,
            budget,
        )? {
            return legacy_default_action(&pattern);
        }
        Err(AccessibilityTreeError::failed(
            "a11y_action_unavailable",
            "node exposes no Invoke, SelectionItem, Toggle, or LegacyIAccessible default action; coordinate fallback is disabled",
        ))
    }

    fn set_text(
        &self,
        element: &ComPtr,
        text: &str,
        budget: &Budget,
    ) -> Result<(), AccessibilityTreeError> {
        budget.check()?;
        let pattern = self
            .pattern(element, UIA_ValuePatternId, &IID_VALUE_PATTERN, budget)?
            .ok_or_else(|| {
                AccessibilityTreeError::failed(
                    "a11y_text_unavailable",
                    "node exposes no writable UI Automation Value pattern; Text pattern is read-only",
                )
            })?;
        if pattern_bool(&pattern, PatternBool::ValueReadOnly)? {
            return Err(AccessibilityTreeError::failed(
                "a11y_text_read_only",
                "UI Automation Value pattern reports read-only",
            ));
        }
        let value = OwnedBstr::from_str(text)?;
        let hr = unsafe { ((*value_vtable(&pattern)).set_value)(pattern.as_ptr(), value.as_raw()) };
        hresult(hr, "IUIAutomationValuePattern.SetValue")
    }

    fn get_text(
        &self,
        element: &ComPtr,
        budget: &Budget,
    ) -> Result<String, AccessibilityTreeError> {
        budget.check()?;
        if let Some(pattern) =
            self.pattern(element, UIA_ValuePatternId, &IID_VALUE_PATTERN, budget)?
        {
            return value_pattern_text(&pattern);
        }
        if let Some(pattern) =
            self.pattern(element, UIA_TextPatternId, &IID_TEXT_PATTERN, budget)?
        {
            return text_pattern_text(&pattern);
        }
        Err(AccessibilityTreeError::failed(
            "a11y_text_unavailable",
            "node exposes neither UI Automation Value nor Text pattern",
        ))
    }
}

struct NodePatterns {
    invoke: Option<ComPtr>,
    selection: Option<ComPtr>,
    toggle: Option<ComPtr>,
    legacy: Option<ComPtr>,
    value: Option<ComPtr>,
    text: Option<ComPtr>,
}

impl NodePatterns {
    fn has_click(&self) -> bool {
        self.invoke.is_some()
            || self.selection.is_some()
            || self.toggle.is_some()
            || self.legacy.is_some()
    }
}

fn invoke_pattern(pattern: &ComPtr) -> Result<(), AccessibilityTreeError> {
    let hr = unsafe { ((*invoke_vtable(pattern)).invoke)(pattern.as_ptr()) };
    hresult(hr, "IUIAutomationInvokePattern.Invoke")
}

fn select_pattern(pattern: &ComPtr) -> Result<(), AccessibilityTreeError> {
    let hr = unsafe { ((*selection_vtable(pattern)).select)(pattern.as_ptr()) };
    hresult(hr, "IUIAutomationSelectionItemPattern.Select")
}

fn toggle_pattern(pattern: &ComPtr) -> Result<(), AccessibilityTreeError> {
    let hr = unsafe { ((*toggle_vtable(pattern)).toggle)(pattern.as_ptr()) };
    hresult(hr, "IUIAutomationTogglePattern.Toggle")
}

fn legacy_default_action(pattern: &ComPtr) -> Result<(), AccessibilityTreeError> {
    let hr = unsafe { ((*legacy_vtable(pattern)).do_default_action)(pattern.as_ptr()) };
    hresult(hr, "IUIAutomationLegacyIAccessiblePattern.DoDefaultAction")
}

enum PatternBool {
    SelectionSelected,
    ValueReadOnly,
}

fn pattern_bool(pattern: &ComPtr, kind: PatternBool) -> Result<bool, AccessibilityTreeError> {
    let mut value = 0i32;
    let (hr, operation) = unsafe {
        match kind {
            PatternBool::SelectionSelected => (
                ((*selection_vtable(pattern)).current_is_selected)(pattern.as_ptr(), &mut value),
                "IUIAutomationSelectionItemPattern.CurrentIsSelected",
            ),
            PatternBool::ValueReadOnly => (
                ((*value_vtable(pattern)).current_is_read_only)(pattern.as_ptr(), &mut value),
                "IUIAutomationValuePattern.CurrentIsReadOnly",
            ),
        }
    };
    hresult(hr, operation)?;
    Ok(value != 0)
}

fn toggle_state(pattern: &ComPtr) -> Result<String, AccessibilityTreeError> {
    let mut value = 0i32;
    let hr =
        unsafe { ((*toggle_vtable(pattern)).current_toggle_state)(pattern.as_ptr(), &mut value) };
    hresult(hr, "IUIAutomationTogglePattern.CurrentToggleState")?;
    Ok(match value {
        0 => "unchecked",
        1 => "checked",
        2 => "indeterminate",
        _ => "toggle-unknown",
    }
    .to_owned())
}

fn value_pattern_text(pattern: &ComPtr) -> Result<String, AccessibilityTreeError> {
    let mut raw: BSTR = ptr::null();
    let hr = unsafe { ((*value_vtable(pattern)).current_value)(pattern.as_ptr(), &mut raw) };
    hresult(hr, "IUIAutomationValuePattern.CurrentValue")?;
    unsafe { OwnedBstr::from_raw(raw) }.to_string_bounded()
}

fn text_pattern_text(pattern: &ComPtr) -> Result<String, AccessibilityTreeError> {
    let mut range = ptr::null_mut();
    let hr = unsafe { ((*text_vtable(pattern)).document_range)(pattern.as_ptr(), &mut range) };
    hresult(hr, "IUIAutomationTextPattern.DocumentRange")?;
    let range = unsafe { ComPtr::from_raw(range, "IUIAutomationTextRange")? };
    let mut raw: BSTR = ptr::null();
    let hr = unsafe {
        ((*text_range_vtable(&range)).get_text)(
            range.as_ptr(),
            MAX_TEXT_UTF16_UNITS as i32,
            &mut raw,
        )
    };
    hresult(hr, "IUIAutomationTextRange.GetText")?;
    unsafe { OwnedBstr::from_raw(raw) }.to_string_bounded()
}

fn push_bool_state(states: &mut Vec<String>, value: Option<bool>, yes: &str, no: &str) {
    if let Some(value) = value {
        states.push(if value { yes } else { no }.to_owned());
    }
}

fn push_true_state(states: &mut Vec<String>, value: Option<bool>, label: &str) {
    if value == Some(true) {
        states.push(label.to_owned());
    }
}

#[allow(non_upper_case_globals)] // Windows SDK constant spellings are generated verbatim.
fn control_type_role(control_type: i32) -> String {
    let role = match control_type {
        UIA_ButtonControlTypeId => "button",
        UIA_CalendarControlTypeId => "calendar",
        UIA_CheckBoxControlTypeId => "check box",
        UIA_ComboBoxControlTypeId => "combo box",
        UIA_EditControlTypeId => "edit",
        UIA_HyperlinkControlTypeId => "link",
        UIA_ImageControlTypeId => "image",
        UIA_ListItemControlTypeId => "list item",
        UIA_ListControlTypeId => "list",
        UIA_MenuControlTypeId => "menu",
        UIA_MenuBarControlTypeId => "menu bar",
        UIA_MenuItemControlTypeId => "menu item",
        UIA_ProgressBarControlTypeId => "progress bar",
        UIA_RadioButtonControlTypeId => "radio button",
        UIA_ScrollBarControlTypeId => "scroll bar",
        UIA_SliderControlTypeId => "slider",
        UIA_SpinnerControlTypeId => "spin button",
        UIA_StatusBarControlTypeId => "status bar",
        UIA_TabControlTypeId => "page tab list",
        UIA_TabItemControlTypeId => "page tab",
        UIA_TextControlTypeId => "text",
        UIA_ToolBarControlTypeId => "tool bar",
        UIA_ToolTipControlTypeId => "tool tip",
        UIA_TreeControlTypeId => "tree",
        UIA_TreeItemControlTypeId => "tree item",
        UIA_CustomControlTypeId => "custom",
        UIA_GroupControlTypeId => "group",
        UIA_ThumbControlTypeId => "thumb",
        UIA_DataGridControlTypeId => "data grid",
        UIA_DataItemControlTypeId => "data item",
        UIA_DocumentControlTypeId => "document",
        UIA_SplitButtonControlTypeId => "split button",
        UIA_WindowControlTypeId => "window",
        UIA_PaneControlTypeId => "panel",
        UIA_HeaderControlTypeId => "header",
        UIA_HeaderItemControlTypeId => "header item",
        UIA_TableControlTypeId => "table",
        UIA_TitleBarControlTypeId => "title bar",
        UIA_SeparatorControlTypeId => "separator",
        UIA_SemanticZoomControlTypeId => "semantic zoom",
        UIA_AppBarControlTypeId => "app bar",
        0 => "unknown",
        _ => return format!("control-type-{control_type}"),
    };
    role.to_owned()
}

fn runtime_segment(runtime_id: &[i32]) -> Result<String, AccessibilityTreeError> {
    if runtime_id.is_empty() || runtime_id.len() > MAX_RUNTIME_ID_PARTS {
        return Err(AccessibilityTreeError::failed(
            "a11y_runtime_id_invalid",
            format!(
                "UI Automation runtime id has {} parts; expected 1..={MAX_RUNTIME_ID_PARTS}",
                runtime_id.len()
            ),
        ));
    }
    let mut segment = String::from('r');
    for (index, part) in runtime_id.iter().enumerate() {
        if index != 0 {
            segment.push('.');
        }
        let _ = write!(segment, "{:08x}", *part as u32);
    }
    Ok(segment)
}

#[derive(Debug, PartialEq, Eq)]
enum RootAnchor {
    Desktop,
    Window(isize),
    Runtime(Vec<i32>),
}

fn root_segment(window_handle: Option<isize>) -> String {
    window_handle.map_or_else(
        || "d".to_owned(),
        |handle| format!("w{:016x}", handle as u64),
    )
}

fn parse_runtime_path(
    node_id: &str,
) -> Result<(RootAnchor, Vec<Vec<i32>>), AccessibilityTreeError> {
    if node_id.len() > MAX_NODE_ID_BYTES || !node_id.starts_with('/') {
        return Err(invalid_node_id(
            "node id must be a bounded slash-separated UIA runtime path",
        ));
    }
    let raw_segments = node_id.strip_prefix('/').unwrap_or_default();
    if raw_segments.is_empty() || raw_segments.contains("//") {
        return Err(invalid_node_id("node id contains an empty path segment"));
    }
    let segments: Vec<&str> = raw_segments.split('/').collect();
    if segments.len() > MAX_DEPTH + 1 {
        return Err(invalid_node_id("node id exceeds the UIA depth limit"));
    }
    let root = match segments[0] {
        "d" => RootAnchor::Desktop,
        segment if segment.starts_with('w') => {
            let encoded = &segment[1..];
            if encoded.len() != 16 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(invalid_node_id(
                    "UIA window root must contain exactly sixteen hexadecimal digits",
                ));
            }
            let handle = u64::from_str_radix(encoded, 16)
                .map_err(|_| invalid_node_id("UIA window root is invalid"))?;
            RootAnchor::Window(handle as isize)
        }
        segment => RootAnchor::Runtime(parse_runtime_segment(segment)?),
    };
    let descendants = segments
        .into_iter()
        .skip(1)
        .map(parse_runtime_segment)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((root, descendants))
}

fn parse_runtime_segment(segment: &str) -> Result<Vec<i32>, AccessibilityTreeError> {
    let encoded = segment
        .strip_prefix('r')
        .ok_or_else(|| invalid_node_id("UIA path segment must start with 'r'"))?;
    if encoded.is_empty() {
        return Err(invalid_node_id("UIA runtime-id segment is empty"));
    }
    let parts: Vec<&str> = encoded.split('.').collect();
    if parts.len() > MAX_RUNTIME_ID_PARTS {
        return Err(invalid_node_id("UIA runtime-id segment has too many parts"));
    }
    parts
        .into_iter()
        .map(|part| {
            if part.len() != 8 || !part.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(invalid_node_id(
                    "UIA runtime-id parts must be exactly eight hexadecimal digits",
                ));
            }
            u32::from_str_radix(part, 16)
                .map(|value| value as i32)
                .map_err(|_| invalid_node_id("UIA runtime-id part is invalid"))
        })
        .collect()
}

fn invalid_node_id(message: impl ToString) -> AccessibilityTreeError {
    AccessibilityTreeError::failed("a11y_invalid_node_id", message)
}

fn validate_window(handle: isize) -> Result<HWND, AccessibilityTreeError> {
    let hwnd = handle as HWND;
    if handle == 0 || unsafe { IsWindow(hwnd) } == 0 {
        return Err(window_gone(handle));
    }
    Ok(hwnd)
}

fn window_gone(handle: isize) -> AccessibilityTreeError {
    AccessibilityTreeError::failed(
        "a11y_window_gone",
        format!("window handle {handle} no longer identifies a live HWND"),
    )
}

fn node_recycled(node_id: &str) -> AccessibilityTreeError {
    AccessibilityTreeError::failed(
        "a11y_node_recycled",
        format!("UI Automation node {node_id} disappeared or its runtime id was recycled"),
    )
}

fn is_snapshot_branch_loss(error: &AccessibilityTreeError) -> bool {
    matches!(
        error,
        AccessibilityTreeError::Failed { code, .. }
            if code == "a11y_node_recycled"
                || code == "a11y_timeout"
                || code == "a11y_runtime_id_invalid"
    )
}

fn limit_error(code: &'static str, message: impl ToString) -> AccessibilityTreeError {
    AccessibilityTreeError::failed(code, message)
}

fn map_input_error(error: InputInjectError) -> AccessibilityTreeError {
    match error {
        InputInjectError::Unsupported { reason } => AccessibilityTreeError::failed(
            "a11y_key_unavailable",
            format!("via={SEND_NODE_KEYS_VIA}: {reason}"),
        ),
        InputInjectError::Failed { code, message } => AccessibilityTreeError::failed(
            "a11y_key_injection_failed",
            format!("via={SEND_NODE_KEYS_VIA}: {code}: {message}"),
        ),
    }
}

fn hresult(hr: HRESULT, operation: &'static str) -> Result<(), AccessibilityTreeError> {
    if hr >= 0 {
        Ok(())
    } else {
        Err(map_hresult(operation, hr))
    }
}

fn map_hresult(operation: &'static str, hr: HRESULT) -> AccessibilityTreeError {
    let raw = hr as u32;
    let (code, detail) = if hr == E_ACCESSDENIED {
        ("a11y_access_denied", "access was denied")
    } else if raw == UIA_E_TIMEOUT || hr == RPC_E_CALL_REJECTED || hr == RPC_E_SERVERCALL_RETRYLATER
    {
        (
            "a11y_timeout",
            "the provider exceeded its bounded call deadline",
        )
    } else if raw == UIA_E_ELEMENTNOTAVAILABLE
        || hr == CO_E_OBJNOTCONNECTED
        || hr == RPC_E_DISCONNECTED
    {
        (
            "a11y_node_recycled",
            "the element disappeared or was recycled",
        )
    } else if raw == UIA_E_ELEMENTNOTENABLED {
        ("a11y_node_disabled", "the element is disabled")
    } else if raw == UIA_E_NOTSUPPORTED || hr == E_NOINTERFACE {
        (
            "a11y_pattern_unsupported",
            "the requested UIA pattern is unavailable",
        )
    } else if raw == UIA_E_INVALIDOPERATION {
        (
            "a11y_invalid_operation",
            "the provider rejected the operation",
        )
    } else {
        ("a11y_uia_failed", "the UI Automation call failed")
    };
    AccessibilityTreeError::failed(code, format!("{operation}: {detail} (HRESULT 0x{raw:08X})"))
}

struct ComApartment {
    uninitialize: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

impl ComApartment {
    fn initialize() -> Result<Self, AccessibilityTreeError> {
        let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_MULTITHREADED as u32) };
        if hr >= 0 {
            return Ok(Self {
                uninitialize: true,
                _thread_bound: PhantomData,
            });
        }
        if hr == RPC_E_CHANGED_MODE {
            // The caller already owns a different apartment on this thread.
            // Borrow it without balancing CoUninitialize; UIA pointers remain
            // local to this call and its configured transaction deadline.
            return Ok(Self {
                uninitialize: false,
                _thread_bound: PhantomData,
            });
        }
        Err(map_hresult("CoInitializeEx", hr))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

struct ComPtr {
    raw: NonNull<c_void>,
    _thread_bound: PhantomData<Rc<()>>,
}

impl ComPtr {
    unsafe fn from_raw(
        raw: *mut c_void,
        interface: &'static str,
    ) -> Result<Self, AccessibilityTreeError> {
        let raw = NonNull::new(raw).ok_or_else(|| {
            AccessibilityTreeError::failed(
                "a11y_null_interface",
                format!("UI Automation returned null {interface}"),
            )
        })?;
        Ok(Self {
            raw,
            _thread_bound: PhantomData,
        })
    }

    fn as_ptr(&self) -> *mut c_void {
        self.raw.as_ptr()
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        unsafe {
            let vtable = *(self.as_ptr() as *const *const IUnknownVtable);
            ((*vtable).release)(self.as_ptr());
        }
    }
}

struct OwnedBstr(BSTR);

impl OwnedBstr {
    fn from_str(value: &str) -> Result<Self, AccessibilityTreeError> {
        let wide: Vec<u16> = value.encode_utf16().collect();
        if wide.len() > MAX_TEXT_UTF16_UNITS * 8 {
            return Err(limit_error(
                "a11y_text_limit",
                "text exceeds the bounded UI Automation BSTR size",
            ));
        }
        let raw = unsafe {
            SysAllocStringLen(
                if wide.is_empty() {
                    ptr::null()
                } else {
                    wide.as_ptr()
                },
                wide.len() as u32,
            )
        };
        if raw.is_null() && !wide.is_empty() {
            return Err(AccessibilityTreeError::failed(
                "a11y_allocation_failed",
                "SysAllocStringLen failed",
            ));
        }
        Ok(Self(raw))
    }

    unsafe fn from_raw(raw: BSTR) -> Self {
        Self(raw)
    }

    fn as_raw(&self) -> BSTR {
        self.0
    }

    fn to_string_bounded(&self) -> Result<String, AccessibilityTreeError> {
        bstr_to_string(self.0)
    }
}

impl Drop for OwnedBstr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { SysFreeString(self.0) };
        }
    }
}

fn bstr_to_string(raw: BSTR) -> Result<String, AccessibilityTreeError> {
    if raw.is_null() {
        return Ok(String::new());
    }
    let len = unsafe { SysStringLen(raw) } as usize;
    if len > MAX_TEXT_UTF16_UNITS {
        return Err(limit_error(
            "a11y_string_limit",
            format!("UI Automation BSTR exceeds {MAX_TEXT_UTF16_UNITS} UTF-16 units"),
        ));
    }
    let wide = unsafe { std::slice::from_raw_parts(raw, len) };
    let value = String::from_utf16_lossy(wide);
    if value.len() > MAX_STRING_BYTES {
        return Err(limit_error(
            "a11y_string_limit",
            format!("UI Automation string exceeds {MAX_STRING_BYTES} UTF-8 bytes"),
        ));
    }
    Ok(value)
}

struct OwnedSafeArray(NonNull<SAFEARRAY>);

impl OwnedSafeArray {
    unsafe fn from_raw(
        raw: *mut SAFEARRAY,
        description: &'static str,
    ) -> Result<Self, AccessibilityTreeError> {
        NonNull::new(raw).map(Self).ok_or_else(|| {
            AccessibilityTreeError::failed(
                "a11y_invalid_safearray",
                format!("UI Automation returned null {description} SAFEARRAY"),
            )
        })
    }

    fn i32_values(&self, max: usize) -> Result<Vec<i32>, AccessibilityTreeError> {
        safearray_values(self.0.as_ptr(), max, |array, index| {
            let mut value = 0i32;
            let hr = unsafe { SafeArrayGetElement(array, &index, (&mut value as *mut i32).cast()) };
            hresult(hr, "SafeArrayGetElement(i32)")?;
            Ok(value)
        })
    }
}

impl Drop for OwnedSafeArray {
    fn drop(&mut self) {
        unsafe {
            SafeArrayDestroy(self.0.as_ptr());
        }
    }
}

struct OwnedVariant(VARIANT);

impl OwnedVariant {
    fn new() -> Self {
        Self(unsafe { std::mem::zeroed() })
    }

    fn as_mut_ptr(&mut self) -> *mut VARIANT {
        &mut self.0
    }

    fn variant_type(&self) -> u16 {
        unsafe { self.0.Anonymous.Anonymous.vt }
    }

    fn string(&self) -> Result<Option<String>, AccessibilityTreeError> {
        match self.variant_type() {
            VT_EMPTY => Ok(None),
            VT_BSTR => {
                let raw = unsafe { self.0.Anonymous.Anonymous.Anonymous.bstrVal };
                bstr_to_string(raw).map(Some)
            }
            other => Err(invalid_variant("BSTR", other)),
        }
    }

    fn boolean(&self) -> Result<Option<bool>, AccessibilityTreeError> {
        match self.variant_type() {
            VT_EMPTY => Ok(None),
            VT_BOOL => Ok(Some(unsafe {
                self.0.Anonymous.Anonymous.Anonymous.boolVal != 0
            })),
            other => Err(invalid_variant("BOOL", other)),
        }
    }

    fn integer(&self) -> Result<Option<i32>, AccessibilityTreeError> {
        match self.variant_type() {
            VT_EMPTY => Ok(None),
            VT_I4 => Ok(Some(unsafe { self.0.Anonymous.Anonymous.Anonymous.lVal })),
            other => Err(invalid_variant("I4", other)),
        }
    }

    fn bounds(&self) -> Result<AccessibilityBounds, AccessibilityTreeError> {
        if self.variant_type() == VT_EMPTY {
            return Ok(AccessibilityBounds {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            });
        }
        let expected = VT_ARRAY | VT_R8;
        if self.variant_type() != expected {
            return Err(invalid_variant("VT_ARRAY | VT_R8", self.variant_type()));
        }
        let array = unsafe { self.0.Anonymous.Anonymous.Anonymous.parray };
        if array.is_null() {
            return Err(AccessibilityTreeError::failed(
                "a11y_invalid_safearray",
                "UIA bounding rectangle returned a null SAFEARRAY",
            ));
        }
        let values = safearray_values(array, 4, |array, index| {
            let mut value = 0f64;
            let hr = unsafe { SafeArrayGetElement(array, &index, (&mut value as *mut f64).cast()) };
            hresult(hr, "SafeArrayGetElement(f64)")?;
            Ok(value)
        })?;
        if values.len() != 4 {
            return Err(AccessibilityTreeError::failed(
                "a11y_property_invalid",
                format!(
                    "UIA bounding rectangle has {} values, expected 4",
                    values.len()
                ),
            ));
        }
        Ok(bounds_from_f64(values[0], values[1], values[2], values[3]))
    }
}

impl Drop for OwnedVariant {
    fn drop(&mut self) {
        unsafe {
            VariantClear(&mut self.0);
        }
    }
}

fn invalid_variant(expected: &str, actual: u16) -> AccessibilityTreeError {
    AccessibilityTreeError::failed(
        "a11y_property_invalid",
        format!("UI Automation property has VARTYPE {actual}, expected {expected}"),
    )
}

fn safearray_values<T>(
    array: *mut SAFEARRAY,
    max: usize,
    mut read: impl FnMut(*mut SAFEARRAY, i32) -> Result<T, AccessibilityTreeError>,
) -> Result<Vec<T>, AccessibilityTreeError> {
    if unsafe { SafeArrayGetDim(array) } != 1 {
        return Err(AccessibilityTreeError::failed(
            "a11y_invalid_safearray",
            "UI Automation SAFEARRAY must have exactly one dimension",
        ));
    }
    let mut lower = 0i32;
    let mut upper = -1i32;
    hresult(
        unsafe { SafeArrayGetLBound(array, 1, &mut lower) },
        "SafeArrayGetLBound",
    )?;
    hresult(
        unsafe { SafeArrayGetUBound(array, 1, &mut upper) },
        "SafeArrayGetUBound",
    )?;
    if upper < lower {
        return Ok(Vec::new());
    }
    let count = i64::from(upper) - i64::from(lower) + 1;
    let count = usize::try_from(count).map_err(|_| {
        AccessibilityTreeError::failed("a11y_invalid_safearray", "SAFEARRAY length overflow")
    })?;
    if count > max {
        return Err(limit_error(
            "a11y_safearray_limit",
            format!("UI Automation SAFEARRAY has {count} elements; limit is {max}"),
        ));
    }
    let mut values = Vec::with_capacity(count);
    for index in lower..=upper {
        values.push(read(array, index)?);
    }
    Ok(values)
}

fn bounds_from_f64(x: f64, y: f64, width: f64, height: f64) -> AccessibilityBounds {
    AccessibilityBounds {
        x: saturating_i32(x),
        y: saturating_i32(y),
        width: saturating_i32(width.max(0.0)),
        height: saturating_i32(height.max(0.0)),
    }
}

fn saturating_i32(value: f64) -> i32 {
    if value.is_nan() {
        0
    } else if value <= i32::MIN as f64 {
        i32::MIN
    } else if value >= i32::MAX as f64 {
        i32::MAX
    } else {
        value.round() as i32
    }
}

unsafe fn set_automation_timeout(
    automation: &ComPtr,
    slot: usize,
    timeout_ms: u32,
    operation: &'static str,
) -> Result<(), AccessibilityTreeError> {
    let raw_slot = unsafe { vtable_slot(automation, slot) };
    let function: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT =
        unsafe { std::mem::transmute(raw_slot) };
    let hr = unsafe { function(automation.as_ptr(), timeout_ms) };
    hresult(hr, operation)
}

unsafe fn vtable_slot(interface: &ComPtr, slot: usize) -> *const c_void {
    let vtable = unsafe { *(interface.as_ptr() as *const *const *const c_void) };
    unsafe { *vtable.add(slot) }
}

fn automation_control_view_walker(automation: &ComPtr) -> Result<ComPtr, AccessibilityTreeError> {
    let mut raw = ptr::null_mut();
    let hr = unsafe {
        ((*automation_vtable(automation)).control_view_walker)(automation.as_ptr(), &mut raw)
    };
    hresult(hr, "IUIAutomation.ControlViewWalker")?;
    unsafe { ComPtr::from_raw(raw, "IUIAutomationTreeWalker") }
}

unsafe fn automation_vtable(interface: &ComPtr) -> *const IUIAutomationVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationVtable) }
}

unsafe fn element_vtable(interface: &ComPtr) -> *const IUIAutomationElementVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationElementVtable) }
}

unsafe fn tree_walker_vtable(interface: &ComPtr) -> *const IUIAutomationTreeWalkerVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationTreeWalkerVtable) }
}

unsafe fn invoke_vtable(interface: &ComPtr) -> *const IUIAutomationInvokePatternVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationInvokePatternVtable) }
}

unsafe fn selection_vtable(interface: &ComPtr) -> *const IUIAutomationSelectionItemPatternVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationSelectionItemPatternVtable) }
}

unsafe fn toggle_vtable(interface: &ComPtr) -> *const IUIAutomationTogglePatternVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationTogglePatternVtable) }
}

unsafe fn legacy_vtable(interface: &ComPtr) -> *const IUIAutomationLegacyPatternVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationLegacyPatternVtable) }
}

unsafe fn value_vtable(interface: &ComPtr) -> *const IUIAutomationValuePatternVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationValuePatternVtable) }
}

unsafe fn text_vtable(interface: &ComPtr) -> *const IUIAutomationTextPatternVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationTextPatternVtable) }
}

unsafe fn text_range_vtable(interface: &ComPtr) -> *const IUIAutomationTextRangeVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationTextRangeVtable) }
}

#[repr(C)]
struct IUnknownVtable {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct IUIAutomationVtable {
    base: IUnknownVtable,
    compare_elements: usize,
    compare_runtime_ids: usize,
    get_root_element: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    element_from_handle: unsafe extern "system" fn(*mut c_void, HWND, *mut *mut c_void) -> HRESULT,
    element_from_point: usize,
    get_focused_element: usize,
    get_root_element_build_cache: usize,
    element_from_handle_build_cache: usize,
    element_from_point_build_cache: usize,
    get_focused_element_build_cache: usize,
    create_tree_walker: usize,
    control_view_walker: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationElementVtable {
    base: IUnknownVtable,
    set_focus: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    get_runtime_id: unsafe extern "system" fn(*mut c_void, *mut *mut SAFEARRAY) -> HRESULT,
    find_first: usize,
    find_all: usize,
    find_first_build_cache: usize,
    find_all_build_cache: usize,
    build_updated_cache: usize,
    get_current_property_value:
        unsafe extern "system" fn(*mut c_void, i32, *mut VARIANT) -> HRESULT,
    get_current_property_value_ex: usize,
    get_cached_property_value: usize,
    get_cached_property_value_ex: usize,
    get_current_pattern_as:
        unsafe extern "system" fn(*mut c_void, i32, *const GUID, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationTreeWalkerVtable {
    base: IUnknownVtable,
    get_parent: usize,
    get_first_child:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT,
    get_last_child: usize,
    get_next_sibling:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationInvokePatternVtable {
    base: IUnknownVtable,
    invoke: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationSelectionItemPatternVtable {
    base: IUnknownVtable,
    select: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    add_to_selection: usize,
    remove_from_selection: usize,
    current_is_selected: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationTogglePatternVtable {
    base: IUnknownVtable,
    toggle: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    current_toggle_state: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationLegacyPatternVtable {
    base: IUnknownVtable,
    select: usize,
    do_default_action: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationValuePatternVtable {
    base: IUnknownVtable,
    set_value: unsafe extern "system" fn(*mut c_void, BSTR) -> HRESULT,
    current_value: unsafe extern "system" fn(*mut c_void, *mut BSTR) -> HRESULT,
    current_is_read_only: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationTextPatternVtable {
    base: IUnknownVtable,
    range_from_point: usize,
    range_from_child: usize,
    get_selection: usize,
    get_visible_ranges: usize,
    document_range: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationTextRangeVtable {
    base: IUnknownVtable,
    clone_range: usize,
    compare: usize,
    compare_endpoints: usize,
    expand_to_enclosing_unit: usize,
    find_attribute: usize,
    find_text: usize,
    get_attribute_value: usize,
    get_bounding_rectangles: usize,
    get_enclosing_element: usize,
    get_text: unsafe extern "system" fn(*mut c_void, i32, *mut BSTR) -> HRESULT,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn raw_vtable_prefix_offsets_match_windows_sdk_slots() {
        let pointer = size_of::<usize>();
        assert_eq!(
            offset_of!(IUIAutomationVtable, get_root_element),
            5 * pointer
        );
        assert_eq!(
            offset_of!(IUIAutomationVtable, element_from_handle),
            6 * pointer
        );
        assert_eq!(
            offset_of!(IUIAutomationVtable, control_view_walker),
            14 * pointer
        );
        assert_eq!(
            offset_of!(IUIAutomationElementVtable, set_focus),
            3 * pointer
        );
        assert_eq!(
            offset_of!(IUIAutomationElementVtable, get_runtime_id),
            4 * pointer
        );
        assert_eq!(
            offset_of!(IUIAutomationElementVtable, get_current_property_value),
            10 * pointer
        );
        assert_eq!(
            offset_of!(IUIAutomationElementVtable, get_current_pattern_as),
            14 * pointer
        );
        assert_eq!(
            offset_of!(IUIAutomationTreeWalkerVtable, get_first_child),
            4 * pointer
        );
        assert_eq!(
            offset_of!(IUIAutomationTreeWalkerVtable, get_next_sibling),
            6 * pointer
        );
        assert_eq!(IUIAUTOMATION2_SET_CONNECTION_TIMEOUT_SLOT, 61);
        assert_eq!(IUIAUTOMATION2_SET_TRANSACTION_TIMEOUT_SLOT, 63);
    }

    #[test]
    fn runtime_paths_are_stable_bounded_and_round_trip_signed_parts() {
        let root = runtime_segment(&[42, -1, i32::MIN]).expect("root id");
        let child = runtime_segment(&[42, 7]).expect("child id");
        let path = format!("/{root}/{child}");
        assert_eq!(
            parse_runtime_path(&path).unwrap(),
            (
                RootAnchor::Runtime(vec![42, -1, i32::MIN]),
                vec![vec![42, 7]]
            )
        );
        assert_eq!(root_segment(None), "d");
        assert_eq!(root_segment(Some(0x42)), "w0000000000000042");
        assert_eq!(
            parse_runtime_path("/w0000000000000042/r00000007").unwrap(),
            (RootAnchor::Window(0x42), vec![vec![7]])
        );
        assert!(matches!(
            parse_runtime_path("/0/1"),
            Err(AccessibilityTreeError::Failed { code, .. }) if code == "a11y_invalid_node_id"
        ));
        assert!(matches!(
            parse_runtime_path("/r00000001//r00000002"),
            Err(AccessibilityTreeError::Failed { code, .. }) if code == "a11y_invalid_node_id"
        ));
        let empty_runtime_id = runtime_segment(&[]).expect_err("empty runtime id");
        assert!(is_snapshot_branch_loss(&empty_runtime_id));
    }

    #[test]
    fn hresults_map_to_stable_failure_classes() {
        let cases = [
            (E_ACCESSDENIED, "a11y_access_denied"),
            (UIA_E_TIMEOUT as i32, "a11y_timeout"),
            (UIA_E_ELEMENTNOTAVAILABLE as i32, "a11y_node_recycled"),
            (UIA_E_ELEMENTNOTENABLED as i32, "a11y_node_disabled"),
            (UIA_E_NOTSUPPORTED as i32, "a11y_pattern_unsupported"),
        ];
        for (hr, expected) in cases {
            let AccessibilityTreeError::Failed { code, .. } = map_hresult("test", hr) else {
                panic!("expected typed failure");
            };
            assert_eq!(code, expected);
        }
    }

    #[test]
    fn bounds_saturate_nan_overflow_and_negative_extent() {
        assert_eq!(
            bounds_from_f64(f64::NAN, f64::NEG_INFINITY, f64::INFINITY, -10.0),
            AccessibilityBounds {
                x: 0,
                y: i32::MIN,
                width: i32::MAX,
                height: 0,
            }
        );
    }

    #[test]
    fn roles_and_key_delivery_route_are_explicit() {
        assert_eq!(control_type_role(UIA_ButtonControlTypeId), "button");
        assert_eq!(control_type_role(UIA_EditControlTypeId), "edit");
        assert_eq!(control_type_role(123), "control-type-123");
        assert_eq!(SEND_NODE_KEYS_VIA, "uia-focus+send-input");
    }
}
