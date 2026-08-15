//! Windows UI Automation placement preflight.
//!
//! UIA supplies semantic window/control-pattern metadata. User32 contributes
//! the stable HWND/PID check and bounded `WM_GETMINMAXINFO` numeric limits.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};
use std::rc::Rc;

use windows_sys::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows_sys::Win32::System::Variant::{VARIANT, VT_BOOL, VT_EMPTY, VT_I4, VariantClear};
use windows_sys::Win32::UI::Accessibility::{
    CUIAutomation8, UIA_ControlTypePropertyId, UIA_E_NOTSUPPORTED, UIA_TransformCanMovePropertyId,
    UIA_TransformCanResizePropertyId, UIA_WindowControlTypeId, UIA_WindowIsModalPropertyId,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, IsWindow, MINMAXINFO, SMTO_ABORTIFHUNG, SMTO_BLOCK,
    SendMessageTimeoutW, WM_GETMINMAXINFO,
};
use windows_sys::core::{GUID, HRESULT};

use crate::CapabilityStatus;
use crate::contract::window_placement::{
    PlacementRole, PlacementWindowInfo, SizeConstraints, Support, WindowPlacementError, WindowSize,
};

const IID_IUIAUTOMATION2: GUID = GUID::from_u128(0x34723aff_0c9d_49d0_9896_7ab52df8cd8a);
const IUIAUTOMATION2_SET_CONNECTION_TIMEOUT_SLOT: usize = 61;
const IUIAUTOMATION2_SET_TRANSACTION_TIMEOUT_SLOT: usize = 63;
const IUIAUTOMATION2_SET_AUTO_SET_FOCUS_SLOT: usize = 59;
const UIA_CONNECTION_TIMEOUT_MS: u32 = 500;
const UIA_TRANSACTION_TIMEOUT_MS: u32 = 250;
const MINMAX_TIMEOUT_MS: u32 = 250;

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

pub(crate) fn inspect(
    handle: isize,
    expected_pid: u32,
) -> Result<PlacementWindowInfo, WindowPlacementError> {
    let hwnd = validate_identity(handle, expected_pid)?;
    let uia = UiaWindow::open(hwnd)?;
    let control_type = uia.property_i32(UIA_ControlTypePropertyId)?;
    let is_modal = uia.property_bool(UIA_WindowIsModalPropertyId)?;
    let role = classify_role(control_type, is_modal);
    let movable = support(uia.property_bool(UIA_TransformCanMovePropertyId)?);
    let resizable = support(uia.property_bool(UIA_TransformCanResizePropertyId)?);
    let constraints = match minmax_constraints(hwnd)? {
        Some(explicit) => explicit,
        None if !matches!(resizable, Support::Unknown) => SizeConstraints::ApplicationEnforced,
        None => SizeConstraints::Unknown,
    };
    Ok(PlacementWindowInfo {
        handle,
        process_id: expected_pid,
        role,
        movable,
        resizable,
        constraints,
    })
}

fn validate_identity(handle: isize, expected_pid: u32) -> Result<HWND, WindowPlacementError> {
    let hwnd = handle as HWND;
    if handle == 0 || expected_pid == 0 || unsafe { IsWindow(hwnd) } == 0 {
        return Err(failed(
            "window_identity_invalid",
            "HWND and expected process id must identify a live window",
        ));
    }
    let mut actual_pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut actual_pid) };
    if actual_pid == 0 || actual_pid != expected_pid {
        return Err(failed(
            "window_stale",
            format!("HWND belongs to process {actual_pid}, expected {expected_pid}"),
        ));
    }
    Ok(hwnd)
}

fn classify_role(control_type: Option<i32>, is_modal: Option<bool>) -> PlacementRole {
    if control_type.is_none() {
        return PlacementRole::Unknown;
    }
    if control_type != Some(UIA_WindowControlTypeId) {
        return PlacementRole::Other;
    }
    match is_modal {
        Some(false) => PlacementRole::Standard,
        // UIA exposes modality but no stable system-dialog discriminator.
        // Treating modal as an ordinary application dialog could move a
        // system-wide prompt, so it remains unknown and fails closed.
        Some(true) | None => PlacementRole::Unknown,
    }
}

fn support(value: Option<bool>) -> Support {
    match value {
        Some(true) => Support::Yes,
        Some(false) => Support::No,
        None => Support::Unknown,
    }
}

fn minmax_constraints(hwnd: HWND) -> Result<Option<SizeConstraints>, WindowPlacementError> {
    let mut limits: MINMAXINFO = unsafe { std::mem::zeroed() };
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_GETMINMAXINFO,
            0,
            (&mut limits as *mut MINMAXINFO) as isize,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            MINMAX_TIMEOUT_MS,
            &mut result,
        )
    };
    if sent == 0 {
        // A hung/rejecting provider does not prove that limits are absent.
        // The caller may use ApplicationEnforced only when UIA independently
        // established resize support and final bounds readback is mandatory.
        return Ok(None);
    }
    let min = point_size(limits.ptMinTrackSize.x, limits.ptMinTrackSize.y);
    let max = point_size(limits.ptMaxTrackSize.x, limits.ptMaxTrackSize.y);
    let constraints = SizeConstraints::Explicit {
        min,
        max,
        increment: None,
    };
    constraints.validate()?;
    Ok(Some(constraints))
}

fn point_size(width: i32, height: i32) -> Option<WindowSize> {
    (width > 0 && height > 0).then(|| WindowSize::new(width as u32, height as u32))
}

struct UiaWindow {
    element: ComPtr,
    _automation: ComPtr,
    _apartment: ComApartment,
}

impl UiaWindow {
    fn open(hwnd: HWND) -> Result<Self, WindowPlacementError> {
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
        check_hresult(hr, "CoCreateInstance(CUIAutomation8)")?;
        let automation = unsafe { ComPtr::from_raw(raw, "IUIAutomation2")? };
        unsafe {
            set_automation_u32(
                &automation,
                IUIAUTOMATION2_SET_AUTO_SET_FOCUS_SLOT,
                0,
                "IUIAutomation2.SetAutoSetFocus",
            )?;
            set_automation_u32(
                &automation,
                IUIAUTOMATION2_SET_CONNECTION_TIMEOUT_SLOT,
                UIA_CONNECTION_TIMEOUT_MS,
                "IUIAutomation2.SetConnectionTimeout",
            )?;
            set_automation_u32(
                &automation,
                IUIAUTOMATION2_SET_TRANSACTION_TIMEOUT_SLOT,
                UIA_TRANSACTION_TIMEOUT_MS,
                "IUIAutomation2.SetTransactionTimeout",
            )?;
        }
        let mut element_raw = ptr::null_mut();
        let hr = unsafe {
            ((*automation_vtable(&automation)).element_from_handle)(
                automation.as_ptr(),
                hwnd,
                &mut element_raw,
            )
        };
        check_hresult(hr, "IUIAutomation.ElementFromHandle")?;
        let element = unsafe { ComPtr::from_raw(element_raw, "IUIAutomationElement")? };
        Ok(Self {
            element,
            _automation: automation,
            _apartment: apartment,
        })
    }

    fn property(&self, id: i32) -> Result<OwnedVariant, WindowPlacementError> {
        let mut value = OwnedVariant::new();
        let hr = unsafe {
            ((*element_vtable(&self.element)).get_current_property_value)(
                self.element.as_ptr(),
                id,
                value.as_mut_ptr(),
            )
        };
        if hr as u32 == UIA_E_NOTSUPPORTED {
            return Ok(OwnedVariant::new());
        }
        check_hresult(hr, "IUIAutomationElement.GetCurrentPropertyValue")?;
        Ok(value)
    }

    fn property_bool(&self, id: i32) -> Result<Option<bool>, WindowPlacementError> {
        self.property(id)?.boolean()
    }

    fn property_i32(&self, id: i32) -> Result<Option<i32>, WindowPlacementError> {
        self.property(id)?.integer()
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

    fn boolean(&self) -> Result<Option<bool>, WindowPlacementError> {
        match self.variant_type() {
            VT_EMPTY => Ok(None),
            VT_BOOL => Ok(Some(unsafe {
                self.0.Anonymous.Anonymous.Anonymous.boolVal != 0
            })),
            actual => Err(failed(
                "window_metadata_invalid",
                format!("UIA property has VARTYPE {actual}, expected BOOL"),
            )),
        }
    }

    fn integer(&self) -> Result<Option<i32>, WindowPlacementError> {
        match self.variant_type() {
            VT_EMPTY => Ok(None),
            VT_I4 => Ok(Some(unsafe { self.0.Anonymous.Anonymous.Anonymous.lVal })),
            actual => Err(failed(
                "window_metadata_invalid",
                format!("UIA property has VARTYPE {actual}, expected I4"),
            )),
        }
    }
}

impl Drop for OwnedVariant {
    fn drop(&mut self) {
        unsafe { VariantClear(&mut self.0) };
    }
}

struct ComApartment {
    uninitialize: bool,
    _thread_bound: PhantomData<Rc<()>>,
}

impl ComApartment {
    fn initialize() -> Result<Self, WindowPlacementError> {
        let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_MULTITHREADED as u32) };
        if hr >= 0 {
            Ok(Self {
                uninitialize: true,
                _thread_bound: PhantomData,
            })
        } else if hr == RPC_E_CHANGED_MODE {
            Ok(Self {
                uninitialize: false,
                _thread_bound: PhantomData,
            })
        } else {
            Err(hresult_error("CoInitializeEx", hr))
        }
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
    ) -> Result<Self, WindowPlacementError> {
        let raw = NonNull::new(raw).ok_or_else(|| {
            failed(
                "window_inspect_failed",
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

unsafe fn set_automation_u32(
    automation: &ComPtr,
    slot: usize,
    value: u32,
    operation: &'static str,
) -> Result<(), WindowPlacementError> {
    let vtable = unsafe { *(automation.as_ptr() as *const *const *const c_void) };
    let function: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT =
        unsafe { std::mem::transmute(*vtable.add(slot)) };
    check_hresult(unsafe { function(automation.as_ptr(), value) }, operation)
}

unsafe fn automation_vtable(interface: &ComPtr) -> *const IUIAutomationVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationVtable) }
}

unsafe fn element_vtable(interface: &ComPtr) -> *const IUIAutomationElementVtable {
    unsafe { *(interface.as_ptr() as *const *const IUIAutomationElementVtable) }
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
    get_root_element: usize,
    element_from_handle: unsafe extern "system" fn(*mut c_void, HWND, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationElementVtable {
    base: IUnknownVtable,
    set_focus: usize,
    get_runtime_id: usize,
    find_first: usize,
    find_all: usize,
    find_first_build_cache: usize,
    find_all_build_cache: usize,
    build_updated_cache: usize,
    get_current_property_value:
        unsafe extern "system" fn(*mut c_void, i32, *mut VARIANT) -> HRESULT,
}

fn check_hresult(hr: HRESULT, operation: &'static str) -> Result<(), WindowPlacementError> {
    if hr >= 0 {
        Ok(())
    } else {
        Err(hresult_error(operation, hr))
    }
}

fn hresult_error(operation: &'static str, hr: HRESULT) -> WindowPlacementError {
    failed(
        "window_inspect_failed",
        format!("{operation} failed with HRESULT 0x{:08X}", hr as u32),
    )
}

fn failed(code: &'static str, message: impl ToString) -> WindowPlacementError {
    WindowPlacementError::failed(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uia_modal_window_is_unknown_not_an_ordinary_dialog() {
        assert_eq!(
            classify_role(Some(UIA_WindowControlTypeId), Some(false)),
            PlacementRole::Standard
        );
        assert_eq!(
            classify_role(Some(UIA_WindowControlTypeId), Some(true)),
            PlacementRole::Unknown
        );
        assert_eq!(
            classify_role(Some(UIA_WindowControlTypeId), None),
            PlacementRole::Unknown
        );
        assert_eq!(classify_role(Some(123), Some(false)), PlacementRole::Other);
        assert_eq!(classify_role(None, None), PlacementRole::Unknown);
    }

    #[test]
    fn absent_or_nonpositive_tracking_limits_are_not_fabricated() {
        assert_eq!(point_size(0, 100), None);
        assert_eq!(point_size(100, -1), None);
        assert_eq!(point_size(320, 240), Some(WindowSize::new(320, 240)));
    }
}
