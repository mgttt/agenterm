//! CGWindowList + AX set-rect for foreign windows (macOS).
//!
//! Handles are `CGWindowID` values. Coordinates are top-origin (Quartz).

#![cfg(target_os = "macos")]

use std::ffi::{CStr, c_void};

use crate::CapabilityStatus;
use crate::contract::window_enumerate::{
    ScreenInfo, WindowBounds, WindowEnumerateError, WindowInfo,
};
use crate::contract::window_op::WindowOpError;

type CfTypeRef = *const c_void;
type CfArrayRef = *const c_void;
type CfDictionaryRef = *const c_void;
type CfStringRef = *const c_void;
type CfIndex = isize;
type AxUiElementRef = *const c_void;
type AxValueRef = *const c_void;
type CgWindowId = u32;

const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;

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

/// CoreGraphics `CGRect`, nested the way the framework declares it. A flat
/// four-double struct has the same layout and worked, but `process_window.rs`
/// declares `CGDisplayBounds` with the nested shape, and two extern
/// declarations of one symbol that differ even nominally are what
/// `clashing_extern_declarations` exists to catch: the day either shape
/// changes, the other silently reads the wrong offsets.
#[repr(C)]
#[derive(Clone, Copy)]
struct CgRect {
    origin: CgPoint,
    size: CgSize,
}

const AX_SUCCESS: i32 = 0;
const AX_API_DISABLED: i32 = -25211;
const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;

// One `#[link]` per framework is the documented way to attach several of them
// to a single extern block; clippy reads the repeated attribute name as a
// copy-paste slip. Same false positive as agenterm-cu's hotkeys.rs.
#[allow(clippy::duplicated_attributes)]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CfArrayRef;
    fn CGMainDisplayID() -> u32;
    fn CGGetActiveDisplayList(max: u32, list: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(id: u32) -> CgRect;

    fn CFRelease(cf: CfTypeRef);
    fn CFArrayGetCount(array: CfArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CfArrayRef, idx: isize) -> CfTypeRef;
    fn CFDictionaryGetValue(dict: CfDictionaryRef, key: CfTypeRef) -> CfTypeRef;
    fn CFStringCreateWithCString(alloc: CfTypeRef, c_str: *const i8, encoding: u32) -> CfStringRef;
    fn CFStringGetCStringPtr(s: CfStringRef, encoding: u32) -> *const i8;
    fn CFStringGetCString(s: CfStringRef, buf: *mut i8, size: CfIndex, encoding: u32) -> bool;
    fn CFNumberGetValue(number: CfTypeRef, the_type: CfIndex, value_ptr: *mut c_void) -> bool;
    fn CFDictionaryGetValueIfPresent(
        dict: CfDictionaryRef,
        key: CfTypeRef,
        value: *mut CfTypeRef,
    ) -> u8;
    fn CFGetTypeID(cf: CfTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;

    fn AXIsProcessTrusted() -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: CfTypeRef,
    ) -> i32;
    fn AXValueCreate(typ: u32, value_ptr: *const c_void) -> AxValueRef;
    fn AXValueGetValue(value: AxValueRef, typ: u32, value_ptr: *mut c_void) -> u8;
    fn _AXUIElementGetWindow(element: AxUiElementRef, out: *mut CgWindowId) -> i32;
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CF_NUMBER_SINT32: i32 = 3;
const K_CF_NUMBER_SINT64: i32 = 4;
const K_CF_NUMBER_DOUBLE: i32 = 13;
const K_CF_NUMBER_CGFLOAT: i32 = 16;

fn cfstr(name: &str) -> CfStringRef {
    let c = std::ffi::CString::new(name).expect("key");
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
        let mut buf = [0i8; 1024];
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
    if ok { Some(i64::from(out32)) } else { None }
}

fn cf_rect(dict: CfDictionaryRef) -> Option<WindowBounds> {
    unsafe {
        let key = cfstr("kCGWindowBounds");
        let mut value: CfTypeRef = std::ptr::null();
        let present = CFDictionaryGetValueIfPresent(dict, key as CfTypeRef, &mut value);
        CFRelease(key as CfTypeRef);
        if present == 0 || value.is_null() {
            return None;
        }
        let x = dict_f64(value as CfDictionaryRef, "X")?;
        let y = dict_f64(value as CfDictionaryRef, "Y")?;
        let w = dict_f64(value as CfDictionaryRef, "Width")?;
        let h = dict_f64(value as CfDictionaryRef, "Height")?;
        Some(WindowBounds {
            x: x.round() as i32,
            y: y.round() as i32,
            width: w.round().max(0.0) as u32,
            height: h.round().max(0.0) as u32,
        })
    }
}

fn dict_f64(dict: CfDictionaryRef, key: &str) -> Option<f64> {
    unsafe {
        let k = cfstr(key);
        let mut value: CfTypeRef = std::ptr::null();
        let present = CFDictionaryGetValueIfPresent(dict, k as CfTypeRef, &mut value);
        CFRelease(k as CfTypeRef);
        if present == 0 || value.is_null() {
            return None;
        }
        let mut out = 0f64;
        if CFNumberGetValue(
            value,
            K_CF_NUMBER_DOUBLE as CfIndex,
            &mut out as *mut f64 as *mut c_void,
        ) || CFNumberGetValue(
            value,
            K_CF_NUMBER_CGFLOAT as CfIndex,
            &mut out as *mut f64 as *mut c_void,
        ) {
            Some(out)
        } else {
            None
        }
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

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Available
}

pub(crate) fn enumerate_top_level() -> Result<Vec<WindowInfo>, WindowEnumerateError> {
    unsafe {
        let array = CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            0,
        );
        if array.is_null() {
            return Err(WindowEnumerateError::failed(
                "cg_window_list_failed",
                "CGWindowListCopyWindowInfo returned null",
            ));
        }
        let count = CFArrayGetCount(array);
        let mut out = Vec::new();
        for i in 0..count {
            let item = CFArrayGetValueAtIndex(array, i);
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
            let pid = cf_i64(dict_get(dict, "kCGWindowOwnerPID")).unwrap_or(0) as u32;
            let title = cf_string(dict_get(dict, "kCGWindowName"));
            let app_name = cf_string(dict_get(dict, "kCGWindowOwnerName"));
            let Some(bounds) = cf_rect(dict) else {
                continue;
            };
            if bounds.width == 0 || bounds.height == 0 {
                continue;
            }
            out.push(WindowInfo {
                handle: id as isize,
                title,
                process_id: pid,
                app_name,
                bounds,
                focused: false,
                minimized: false,
            });
        }
        CFRelease(array);
        mark_focused(&mut out);
        Ok(out)
    }
}

fn mark_focused(windows: &mut [WindowInfo]) {
    if let Some(id) = focused_window_id() {
        for window in windows.iter_mut() {
            window.focused = window.handle == id as isize;
        }
    } else if let Some(first) = windows.first_mut() {
        first.focused = true;
    }
}

fn focused_window_id() -> Option<u32> {
    unsafe {
        let system = ax_system_wide();
        if system.is_null() {
            return None;
        }
        let focused_app_key = cfstr("AXFocusedApplication");
        let mut app: CfTypeRef = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(system, focused_app_key, &mut app);
        CFRelease(focused_app_key as CfTypeRef);
        CFRelease(system as CfTypeRef);
        if status != AX_SUCCESS || app.is_null() {
            return None;
        }
        let focused_win_key = cfstr("AXFocusedWindow");
        let mut win: CfTypeRef = std::ptr::null();
        let status =
            AXUIElementCopyAttributeValue(app as AxUiElementRef, focused_win_key, &mut win);
        CFRelease(focused_win_key as CfTypeRef);
        CFRelease(app);
        if status != AX_SUCCESS || win.is_null() {
            return None;
        }
        let mut id = 0u32;
        let status = _AXUIElementGetWindow(win as AxUiElementRef, &mut id);
        CFRelease(win);
        if status == AX_SUCCESS && id != 0 {
            Some(id)
        } else {
            None
        }
    }
}

fn ax_system_wide() -> AxUiElementRef {
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> AxUiElementRef;
    }
    unsafe { AXUIElementCreateSystemWide() }
}

pub(crate) fn list_screens() -> Result<Vec<ScreenInfo>, WindowEnumerateError> {
    unsafe {
        let mut count = 0u32;
        if CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count) != 0 || count == 0 {
            return Err(WindowEnumerateError::failed(
                "display_list_failed",
                "CGGetActiveDisplayList failed",
            ));
        }
        let mut ids = vec![0u32; count as usize];
        if CGGetActiveDisplayList(count, ids.as_mut_ptr(), &mut count) != 0 {
            return Err(WindowEnumerateError::failed(
                "display_list_failed",
                "CGGetActiveDisplayList copy failed",
            ));
        }
        let main = CGMainDisplayID();
        let main_bounds = CGDisplayBounds(main);
        let primary_h = main_bounds.size.height;
        let mut out = Vec::new();
        for id in ids.into_iter().take(count as usize) {
            let b = CGDisplayBounds(id);
            // CGDisplayBounds is bottom-origin on the main display; convert to top-origin.
            let top_y = primary_h - (b.origin.y + b.size.height);
            let bounds = WindowBounds {
                x: b.origin.x.round() as i32,
                y: top_y.round() as i32,
                width: b.size.width.round().max(1.0) as u32,
                height: b.size.height.round().max(1.0) as u32,
            };
            out.push(ScreenInfo {
                frame: bounds,
                visible: bounds,
                primary: id == main,
            });
        }
        if out.is_empty() {
            return Err(WindowEnumerateError::failed(
                "no_displays",
                "no active displays",
            ));
        }
        Ok(out)
    }
}

pub(crate) fn window_rect(handle: isize) -> Result<WindowBounds, WindowOpError> {
    if let Ok(bounds) = ax_window_rect(handle) {
        return Ok(bounds);
    }
    let windows = enumerate_top_level().map_err(map_enum)?;
    windows
        .into_iter()
        .find(|w| w.handle == handle)
        .map(|w| w.bounds)
        .ok_or_else(|| WindowOpError::failed("window_not_found", format!("no window {handle}")))
}

fn ax_window_rect(handle: isize) -> Result<WindowBounds, WindowOpError> {
    let element = ax_element_for_handle(handle)?;
    unsafe {
        let pos_key = cfstr("AXPosition");
        let size_key = cfstr("AXSize");
        let mut pos_val: CfTypeRef = std::ptr::null();
        let mut size_val: CfTypeRef = std::ptr::null();
        let ps = AXUIElementCopyAttributeValue(element, pos_key, &mut pos_val);
        let ss = AXUIElementCopyAttributeValue(element, size_key, &mut size_val);
        CFRelease(pos_key as CfTypeRef);
        CFRelease(size_key as CfTypeRef);
        CFRelease(element as CfTypeRef);
        if ps != AX_SUCCESS || ss != AX_SUCCESS || pos_val.is_null() || size_val.is_null() {
            return Err(WindowOpError::failed(
                "ax_read_rect_failed",
                format!("AX get pos={ps} size={ss}"),
            ));
        }
        let mut pos = CgPoint { x: 0.0, y: 0.0 };
        let mut size = CgSize {
            width: 0.0,
            height: 0.0,
        };
        let pok = AXValueGetValue(
            pos_val as AxValueRef,
            AX_VALUE_CGPOINT,
            &mut pos as *mut CgPoint as *mut c_void,
        );
        let sok = AXValueGetValue(
            size_val as AxValueRef,
            AX_VALUE_CGSIZE,
            &mut size as *mut CgSize as *mut c_void,
        );
        CFRelease(pos_val);
        CFRelease(size_val);
        if pok == 0 || sok == 0 {
            return Err(WindowOpError::failed(
                "ax_value_decode_failed",
                "AXValueGetValue failed",
            ));
        }
        Ok(WindowBounds {
            x: pos.x.round() as i32,
            y: pos.y.round() as i32,
            width: size.width.round().max(0.0) as u32,
            height: size.height.round().max(0.0) as u32,
        })
    }
}

fn ax_element_for_handle(handle: isize) -> Result<AxUiElementRef, WindowOpError> {
    let pid = owner_pid(handle)?;
    unsafe {
        let app = AXUIElementCreateApplication(pid as i32);
        if app.is_null() {
            return Err(WindowOpError::failed(
                "ax_app_failed",
                "AXUIElementCreateApplication returned null",
            ));
        }
        let windows_key = cfstr("AXWindows");
        let mut windows: CfTypeRef = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(app, windows_key, &mut windows);
        CFRelease(windows_key as CfTypeRef);
        CFRelease(app as CfTypeRef);
        if status != AX_SUCCESS || windows.is_null() {
            return Err(WindowOpError::failed(
                "ax_windows_failed",
                format!("AXWindows status {status}"),
            ));
        }
        let count = CFArrayGetCount(windows as CfArrayRef);
        let target = handle as u32;
        for i in 0..count {
            let el = CFArrayGetValueAtIndex(windows as CfArrayRef, i);
            if el.is_null() {
                continue;
            }
            let mut id = 0u32;
            if _AXUIElementGetWindow(el as AxUiElementRef, &mut id) == AX_SUCCESS && id == target {
                // Retain the element independently of the array.
                // AXUIElement is CF-backed; the array owns it — copy by creating app window is enough if we don't release array first.
                let kept = el as AxUiElementRef;
                // Leak the array? We must CFRetain element then release array.
                unsafe extern "C" {
                    fn CFRetain(cf: CfTypeRef) -> CfTypeRef;
                }
                CFRetain(kept as CfTypeRef);
                CFRelease(windows);
                return Ok(kept);
            }
        }
        CFRelease(windows);
        Err(WindowOpError::failed(
            "ax_window_not_found",
            format!("no AX window for CGWindowID {target}"),
        ))
    }
}

pub(crate) fn move_window(
    handle: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), WindowOpError> {
    if unsafe { AXIsProcessTrusted() } == 0 {
        return Err(WindowOpError::failed(
            "ax_api_disabled",
            "Accessibility is not trusted for this process",
        ));
    }
    unsafe {
        let el = ax_element_for_handle(handle)?;
        let result = set_ax_rect(el, x, y, width, height);
        CFRelease(el as CfTypeRef);
        result
    }
}

fn set_ax_rect(
    element: AxUiElementRef,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), WindowOpError> {
    unsafe {
        let pos = CgPoint {
            x: f64::from(x),
            y: f64::from(y),
        };
        let size = CgSize {
            width: f64::from(width),
            height: f64::from(height),
        };
        let pos_val = AXValueCreate(AX_VALUE_CGPOINT, &pos as *const CgPoint as *const c_void);
        let size_val = AXValueCreate(AX_VALUE_CGSIZE, &size as *const CgSize as *const c_void);
        if pos_val.is_null() || size_val.is_null() {
            return Err(WindowOpError::failed(
                "ax_value_create_failed",
                "AXValueCreate returned null",
            ));
        }
        let size_key = cfstr("AXSize");
        let pos_key = cfstr("AXPosition");
        // Spectacle order: size, position, size.
        // Spectacle order: size, position, size. Position must succeed or
        // half/right placements look like no-ops.
        let s1 = AXUIElementSetAttributeValue(element, size_key, size_val as CfTypeRef);
        let p1 = AXUIElementSetAttributeValue(element, pos_key, pos_val as CfTypeRef);
        let s2 = AXUIElementSetAttributeValue(element, size_key, size_val as CfTypeRef);
        let p2 = AXUIElementSetAttributeValue(element, pos_key, pos_val as CfTypeRef);
        CFRelease(size_key as CfTypeRef);
        CFRelease(pos_key as CfTypeRef);
        CFRelease(pos_val as CfTypeRef);
        CFRelease(size_val as CfTypeRef);
        if s1 == AX_API_DISABLED
            || p1 == AX_API_DISABLED
            || s2 == AX_API_DISABLED
            || p2 == AX_API_DISABLED
        {
            return Err(WindowOpError::failed(
                "ax_api_disabled",
                "AX set-rect disabled",
            ));
        }
        if p1 != AX_SUCCESS && p2 != AX_SUCCESS {
            return Err(WindowOpError::failed(
                "ax_set_rect_failed",
                format!("AX set status size={s1}/{s2} pos={p1}/{p2}"),
            ));
        }
        Ok(())
    }
}

fn owner_pid(handle: isize) -> Result<u32, WindowOpError> {
    let windows = enumerate_top_level().map_err(map_enum)?;
    windows
        .into_iter()
        .find(|w| w.handle == handle)
        .map(|w| w.process_id)
        .ok_or_else(|| WindowOpError::failed("window_not_found", format!("no window {handle}")))
}

fn map_enum(err: WindowEnumerateError) -> WindowOpError {
    match err {
        WindowEnumerateError::Unsupported { reason } => WindowOpError::Unsupported { reason },
        WindowEnumerateError::Failed { code, message } => WindowOpError::Failed { code, message },
    }
}

// Unused helper kept so AXValueGetValue stays available if we later read AX rects.
#[allow(dead_code)]
fn ax_read_point(_value: AxValueRef) -> Option<CgPoint> {
    let mut point = CgPoint { x: 0.0, y: 0.0 };
    let ok = unsafe {
        AXValueGetValue(
            _value,
            AX_VALUE_CGPOINT,
            &mut point as *mut CgPoint as *mut c_void,
        )
    };
    (ok != 0).then_some(point)
}

pub(crate) fn show(
    _handle: isize,
    _state: crate::contract::window_op::WindowShowState,
) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window show-state is not wired on macOS yet".into(),
    })
}

pub(crate) fn set_topmost(_handle: isize, _topmost: bool) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window topmost is not wired on macOS yet".into(),
    })
}

pub(crate) fn close(_handle: isize) -> Result<(), WindowOpError> {
    Err(WindowOpError::Unsupported {
        reason: "window close is not wired on macOS yet".into(),
    })
}
