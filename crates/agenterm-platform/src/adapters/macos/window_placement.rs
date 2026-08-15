//! macOS Accessibility placement preflight for foreign CGWindowID handles.

#![cfg(target_os = "macos")]

use std::ffi::{CStr, CString, c_void};

use crate::CapabilityStatus;
use crate::contract::window_placement::{
    PlacementRole, PlacementWindowInfo, SizeConstraints, Support, WindowPlacementError,
};

type CfTypeRef = *const c_void;
type CfArrayRef = *const c_void;
type CfStringRef = *const c_void;
type CfIndex = isize;
type AxUiElementRef = *const c_void;

const AX_SUCCESS: i32 = 0;
const AX_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
const AX_NO_VALUE: i32 = -25212;
const AX_API_DISABLED: i32 = -25211;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[allow(clippy::duplicated_attributes)]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CfTypeRef);
    fn CFArrayGetCount(array: CfArrayRef) -> CfIndex;
    fn CFArrayGetValueAtIndex(array: CfArrayRef, index: CfIndex) -> CfTypeRef;
    fn CFStringCreateWithCString(
        allocator: CfTypeRef,
        text: *const i8,
        encoding: u32,
    ) -> CfStringRef;
    fn CFStringGetCStringPtr(text: CfStringRef, encoding: u32) -> *const i8;
    fn CFStringGetCString(
        text: CfStringRef,
        buffer: *mut i8,
        capacity: CfIndex,
        encoding: u32,
    ) -> bool;
    fn CFGetTypeID(value: CfTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;

    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementIsAttributeSettable(
        element: AxUiElementRef,
        attribute: CfStringRef,
        settable: *mut u8,
    ) -> i32;
    fn _AXUIElementGetWindow(element: AxUiElementRef, window: *mut u32) -> i32;
}

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

pub(crate) fn inspect(
    handle: isize,
    expected_pid: u32,
) -> Result<PlacementWindowInfo, WindowPlacementError> {
    if handle == 0 || expected_pid == 0 || u32::try_from(handle).is_err() {
        return Err(failed(
            "window_identity_invalid",
            "CGWindowID and expected process id must be nonzero",
        ));
    }
    let enumerated = crate::selected::macos_foreign_windows::enumerate_top_level()
        .map_err(|error| failed("window_inspect_failed", format!("{error:?}")))?
        .into_iter()
        .find(|window| window.handle == handle)
        .ok_or_else(|| {
            failed(
                "window_stale",
                format!("CGWindowID {handle} is no longer visible"),
            )
        })?;
    if enumerated.process_id != expected_pid {
        return Err(failed(
            "window_stale",
            format!(
                "CGWindowID belongs to process {}, expected {expected_pid}",
                enumerated.process_id
            ),
        ));
    }
    if unsafe { AXIsProcessTrusted() } == 0 {
        return Err(failed(
            "window_inspect_access_denied",
            "Accessibility is not trusted for this process",
        ));
    }

    let element = ax_element_for_window(expected_pid, handle as u32)?;
    let role_raw = attribute_string(element.0, "AXRole")?;
    let subrole_raw = attribute_string(element.0, "AXSubrole")?;
    let role = classify_role(role_raw.as_deref(), subrole_raw.as_deref());
    let movable = attribute_settable(element.0, "AXPosition")?;
    let resizable = attribute_settable(element.0, "AXSize")?;
    let constraints = if matches!(resizable, Support::Yes | Support::No) {
        // AX exposes whether AXSize is settable but no trustworthy generic
        // numeric window min/max attributes. The application enforces its
        // limits and every mutation therefore requires AX bounds readback.
        SizeConstraints::ApplicationEnforced
    } else {
        SizeConstraints::Unknown
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

struct OwnedCf(CfTypeRef);

impl Drop for OwnedCf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn ax_element_for_window(process_id: u32, window_id: u32) -> Result<OwnedCf, WindowPlacementError> {
    let pid = i32::try_from(process_id)
        .map_err(|_| failed("window_identity_invalid", "process id exceeds pid_t range"))?;
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return Err(failed(
            "window_inspect_failed",
            "AXUIElementCreateApplication returned null",
        ));
    }
    let app = OwnedCf(app);
    let windows = copy_attribute(app.0, "AXWindows")?.ok_or_else(|| {
        failed(
            "window_inspect_failed",
            format!("AXWindows is unavailable for process {process_id}"),
        )
    })?;
    let count = unsafe { CFArrayGetCount(windows.0 as CfArrayRef) };
    for index in 0..count {
        let candidate = unsafe { CFArrayGetValueAtIndex(windows.0 as CfArrayRef, index) };
        if candidate.is_null() {
            continue;
        }
        let mut candidate_id = 0u32;
        if unsafe { _AXUIElementGetWindow(candidate as AxUiElementRef, &mut candidate_id) }
            == AX_SUCCESS
            && candidate_id == window_id
        {
            unsafe extern "C" {
                fn CFRetain(value: CfTypeRef) -> CfTypeRef;
            }
            let retained = unsafe { CFRetain(candidate) };
            return Ok(OwnedCf(retained));
        }
    }
    Err(failed(
        "window_stale",
        format!("no AX window for CGWindowID {window_id}"),
    ))
}

fn copy_attribute(
    element: AxUiElementRef,
    name: &str,
) -> Result<Option<OwnedCf>, WindowPlacementError> {
    let key = owned_cf_string(name)?;
    let mut value = std::ptr::null();
    let status =
        unsafe { AXUIElementCopyAttributeValue(element, key.0 as CfStringRef, &mut value) };
    match status {
        AX_SUCCESS if value.is_null() => Ok(None),
        AX_SUCCESS => Ok(Some(OwnedCf(value))),
        AX_ATTRIBUTE_UNSUPPORTED | AX_NO_VALUE => Ok(None),
        AX_API_DISABLED => Err(failed(
            "window_inspect_access_denied",
            format!("AX {name} is unavailable because Accessibility is disabled"),
        )),
        other => Err(failed(
            "window_inspect_failed",
            format!("AX {name} returned status {other}"),
        )),
    }
}

fn attribute_string(
    element: AxUiElementRef,
    name: &str,
) -> Result<Option<String>, WindowPlacementError> {
    let Some(value) = copy_attribute(element, name)? else {
        return Ok(None);
    };
    if unsafe { CFGetTypeID(value.0) } != unsafe { CFStringGetTypeID() } {
        return Err(failed(
            "window_metadata_invalid",
            format!("AX {name} is not a CFString"),
        ));
    }
    cf_string(value.0 as CfStringRef).map(Some)
}

fn attribute_settable(
    element: AxUiElementRef,
    name: &str,
) -> Result<Support, WindowPlacementError> {
    let key = owned_cf_string(name)?;
    let mut settable = 0u8;
    let status =
        unsafe { AXUIElementIsAttributeSettable(element, key.0 as CfStringRef, &mut settable) };
    match status {
        AX_SUCCESS => Ok(if settable != 0 {
            Support::Yes
        } else {
            Support::No
        }),
        AX_ATTRIBUTE_UNSUPPORTED | AX_NO_VALUE => Ok(Support::Unknown),
        AX_API_DISABLED => Err(failed(
            "window_inspect_access_denied",
            format!("AX {name} settable query is disabled"),
        )),
        other => Err(failed(
            "window_inspect_failed",
            format!("AX {name} settable query returned status {other}"),
        )),
    }
}

fn owned_cf_string(value: &str) -> Result<OwnedCf, WindowPlacementError> {
    let value = CString::new(value)
        .map_err(|_| failed("window_metadata_invalid", "AX attribute contains NUL"))?;
    let raw = unsafe {
        CFStringCreateWithCString(std::ptr::null(), value.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    };
    if raw.is_null() {
        Err(failed(
            "window_inspect_failed",
            "CFStringCreateWithCString returned null",
        ))
    } else {
        Ok(OwnedCf(raw))
    }
}

fn cf_string(value: CfStringRef) -> Result<String, WindowPlacementError> {
    let direct = unsafe { CFStringGetCStringPtr(value, K_CF_STRING_ENCODING_UTF8) };
    if !direct.is_null() {
        return Ok(unsafe { CStr::from_ptr(direct) }
            .to_string_lossy()
            .into_owned());
    }
    let mut buffer = [0i8; 256];
    if unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as CfIndex,
            K_CF_STRING_ENCODING_UTF8,
        )
    } {
        Ok(unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned())
    } else {
        Err(failed(
            "window_metadata_invalid",
            "AX role string exceeds the bounded UTF-8 buffer",
        ))
    }
}

fn classify_role(role: Option<&str>, subrole: Option<&str>) -> PlacementRole {
    match (role.unwrap_or_default(), subrole.unwrap_or_default()) {
        ("AXSheet", _) => PlacementRole::Sheet,
        ("AXWindow", "AXStandardWindow") => PlacementRole::Standard,
        ("AXWindow", "AXDialog") => PlacementRole::Dialog,
        ("AXWindow", "AXSystemDialog") => PlacementRole::SystemDialog,
        ("" | "AXUnknown", _) | ("AXWindow", _) => PlacementRole::Unknown,
        _ => PlacementRole::Other,
    }
}

fn failed(code: &'static str, message: impl ToString) -> WindowPlacementError {
    WindowPlacementError::failed(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_documented_ax_window_roles() {
        assert_eq!(
            classify_role(Some("AXWindow"), Some("AXStandardWindow")),
            PlacementRole::Standard
        );
        assert_eq!(
            classify_role(Some("AXWindow"), Some("AXDialog")),
            PlacementRole::Dialog
        );
        assert_eq!(classify_role(Some("AXSheet"), None), PlacementRole::Sheet);
        assert_eq!(
            classify_role(Some("AXWindow"), Some("AXSystemDialog")),
            PlacementRole::SystemDialog
        );
    }

    #[test]
    fn missing_or_future_ax_window_subrole_is_unknown_not_standard() {
        assert_eq!(
            classify_role(Some("AXWindow"), None),
            PlacementRole::Unknown
        );
        assert_eq!(
            classify_role(Some("AXWindow"), Some("AXFutureWindow")),
            PlacementRole::Unknown
        );
        assert_eq!(classify_role(None, None), PlacementRole::Unknown);
        assert_eq!(classify_role(Some("AXPopover"), None), PlacementRole::Other);
    }
}
