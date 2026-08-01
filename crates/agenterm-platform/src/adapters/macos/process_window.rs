#![allow(non_snake_case, non_upper_case_globals)]

use std::{ffi::c_void, ptr};

use crate::contract::process_window::*;

type CfIndex = isize;
type CfTypeRef = *const c_void;
type CfArrayRef = *const c_void;
type CfDictionaryRef = *const c_void;
type CfStringRef = *const c_void;
type CgEventRef = *const c_void;
type CgEventSourceRef = *const c_void;
type CgDirectDisplayId = u32;

const WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1 << 0;
const WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
const NULL_WINDOW_ID: u32 = 0;
const CF_NUMBER_SINT32_TYPE: CfIndex = 3;
const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const CG_EVENT_SOURCE_HID_SYSTEM_STATE: i32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CgSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CgRect {
    origin: CgPoint,
    size: CgSize,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: CfArrayRef) -> CfIndex;
    fn CFArrayGetValueAtIndex(array: CfArrayRef, index: CfIndex) -> *const c_void;
    fn CFDictionaryGetValue(dictionary: CfDictionaryRef, key: *const c_void) -> *const c_void;
    fn CFGetTypeID(value: CfTypeRef) -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: CfTypeRef, kind: CfIndex, value: *mut c_void) -> bool;
    fn CFRelease(value: CfTypeRef);
    fn CFStringGetCString(
        string: CfStringRef,
        buffer: *mut i8,
        buffer_size: CfIndex,
        encoding: u32,
    ) -> bool;
    fn CFStringGetLength(string: CfStringRef) -> CfIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CfIndex, encoding: u32) -> CfIndex;
    fn CFStringGetTypeID() -> usize;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGWindowBounds: CfStringRef;
    static kCGWindowLayer: CfStringRef;
    static kCGWindowName: CfStringRef;
    static kCGWindowNumber: CfStringRef;
    static kCGWindowOwnerName: CfStringRef;
    static kCGWindowOwnerPID: CfStringRef;

    fn CGRectMakeWithDictionaryRepresentation(
        dictionary: CfDictionaryRef,
        rect: *mut CgRect,
    ) -> u32;
    fn CGDisplayBounds(display: CgDirectDisplayId) -> CgRect;
    fn CGDisplayPixelsWide(display: CgDirectDisplayId) -> usize;
    fn CGEventCreateKeyboardEvent(
        source: CgEventSourceRef,
        virtual_key: u16,
        key_down: bool,
    ) -> CgEventRef;
    fn CGEventPostToPid(process_id: i32, event: CgEventRef);
    fn CGEventSourceCreate(state_id: i32) -> CgEventSourceRef;
    fn CGGetActiveDisplayList(
        maximum_displays: u32,
        displays: *mut CgDirectDisplayId,
        display_count: *mut u32,
    ) -> i32;
    fn CGPreflightPostEventAccess() -> bool;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CfArrayRef;
}

struct OwnedCf(CfTypeRef);

impl OwnedCf {
    fn new(value: CfTypeRef) -> Option<Self> {
        if value.is_null() {
            None
        } else {
            Some(Self(value))
        }
    }
}

impl Drop for OwnedCf {
    fn drop(&mut self) {
        // SAFETY: every OwnedCf is created from a create/copy-rule Core Foundation value.
        unsafe { CFRelease(self.0) };
    }
}

#[derive(Clone, Debug)]
struct NativeWindow {
    id: u32,
    title: String,
    bounds: CgRect,
}

#[derive(Clone, Debug)]
struct WindowSearch {
    candidates: Vec<NativeWindow>,
    foreground_window_id: u32,
}

#[derive(Clone, Copy, Debug)]
enum CandidateResolution<'a> {
    Missing,
    Unique(&'a NativeWindow),
    Ambiguous,
}

const fn error(
    code: &'static str,
    message: &'static str,
    cause: &'static str,
) -> ProcessWindowError {
    ProcessWindowError::new(code, message, Some(cause))
}

const fn unsupported(message: &'static str) -> ProcessWindowError {
    error("process_window_unsupported", message, "unsupported")
}

fn dictionary_value(dictionary: CfDictionaryRef, key: CfStringRef) -> CfTypeRef {
    if dictionary.is_null() || key.is_null() {
        return ptr::null();
    }
    // SAFETY: the window server returned a CFDictionary and the key is a public CFString constant.
    unsafe { CFDictionaryGetValue(dictionary, key.cast()) }
}

fn dictionary_i32(dictionary: CfDictionaryRef, key: CfStringRef) -> Option<i32> {
    let value = dictionary_value(dictionary, key);
    if value.is_null() {
        return None;
    }
    // SAFETY: type IDs may be queried for every non-null Core Foundation object.
    if unsafe { CFGetTypeID(value) } != unsafe { CFNumberGetTypeID() } {
        return None;
    }
    let mut result = 0_i32;
    // SAFETY: the destination matches kCFNumberSInt32Type and remains valid for the call.
    unsafe {
        CFNumberGetValue(
            value,
            CF_NUMBER_SINT32_TYPE,
            (&mut result as *mut i32).cast(),
        )
    }
    .then_some(result)
}

fn dictionary_rect(dictionary: CfDictionaryRef, key: CfStringRef) -> Option<CgRect> {
    let value = dictionary_value(dictionary, key);
    if value.is_null() {
        return None;
    }
    let mut rect = CgRect::default();
    // SAFETY: CGRectMakeWithDictionaryRepresentation validates the dictionary shape.
    (unsafe { CGRectMakeWithDictionaryRepresentation(value, &mut rect) } != 0)
        .then_some(rect)
        .filter(valid_bounds)
}

fn dictionary_string(dictionary: CfDictionaryRef, key: CfStringRef) -> Option<String> {
    let value = dictionary_value(dictionary, key);
    if value.is_null() {
        return None;
    }
    // SAFETY: type IDs may be queried for every non-null Core Foundation object.
    if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    // SAFETY: value has been checked to be a CFString.
    let length = unsafe { CFStringGetLength(value) };
    // SAFETY: the encoding and non-negative CFString length are valid inputs.
    let maximum = unsafe { CFStringGetMaximumSizeForEncoding(length, CF_STRING_ENCODING_UTF8) };
    let capacity = usize::try_from(maximum).ok()?.checked_add(1)?;
    let mut bytes = vec![0_i8; capacity];
    // SAFETY: bytes is writable for capacity bytes and value is a CFString.
    if !unsafe {
        CFStringGetCString(
            value,
            bytes.as_mut_ptr(),
            CfIndex::try_from(capacity).ok()?,
            CF_STRING_ENCODING_UTF8,
        )
    } {
        return None;
    }
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let bytes = bytes[..length].iter().map(|byte| *byte as u8).collect();
    String::from_utf8(bytes).ok()
}

fn valid_bounds(rect: &CgRect) -> bool {
    rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.size.width > 0.0
        && rect.size.height > 0.0
}

fn resolve_candidates(candidates: &[NativeWindow]) -> CandidateResolution<'_> {
    match candidates {
        [] => CandidateResolution::Missing,
        [window] => CandidateResolution::Unique(window),
        _ => CandidateResolution::Ambiguous,
    }
}

fn required_candidate(candidates: &[NativeWindow]) -> Result<NativeWindow, ProcessWindowError> {
    match resolve_candidates(candidates) {
        CandidateResolution::Missing => Err(error(
            "process_window_not_found",
            "child has no visible top-level window",
            "not_found",
        )),
        CandidateResolution::Unique(window) => Ok(window.clone()),
        CandidateResolution::Ambiguous => Err(error(
            "process_window_ambiguous",
            "child has multiple visible top-level windows",
            "ambiguous",
        )),
    }
}

fn search_windows(process_id: u32) -> Result<WindowSearch, ProcessWindowError> {
    // SAFETY: this is a read-only WindowServer query with public option flags.
    let list = unsafe {
        CGWindowListCopyWindowInfo(
            WINDOW_LIST_ON_SCREEN_ONLY | WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            NULL_WINDOW_ID,
        )
    };
    let list = OwnedCf::new(list).ok_or_else(|| {
        error(
            "process_window_not_found",
            "native window list could not be read",
            "platform_error",
        )
    })?;
    // SAFETY: list owns a valid CFArray for the duration of this function.
    let count = unsafe { CFArrayGetCount(list.0) };
    let mut candidates = Vec::new();
    let mut foreground_window_id = 0;
    for index in 0..count {
        // SAFETY: index is inside the array count returned by Core Foundation.
        let dictionary = unsafe { CFArrayGetValueAtIndex(list.0, index) };
        let owner = dictionary_i32(dictionary, unsafe { kCGWindowOwnerPID });
        let layer = dictionary_i32(dictionary, unsafe { kCGWindowLayer });
        let id = dictionary_i32(dictionary, unsafe { kCGWindowNumber });
        let bounds = dictionary_rect(dictionary, unsafe { kCGWindowBounds });
        let (Some(owner), Some(0), Some(id), Some(bounds)) = (owner, layer, id, bounds) else {
            continue;
        };
        let Ok(id) = u32::try_from(id) else {
            continue;
        };
        if foreground_window_id == 0 {
            foreground_window_id = id;
        }
        if owner == i32::try_from(process_id).unwrap_or(-1) {
            let title = dictionary_string(dictionary, unsafe { kCGWindowName })
                .or_else(|| dictionary_string(dictionary, unsafe { kCGWindowOwnerName }))
                .unwrap_or_default();
            candidates.push(NativeWindow { id, title, bounds });
        }
    }
    Ok(WindowSearch {
        candidates,
        foreground_window_id,
    })
}

fn required_window(process_id: u32) -> Result<NativeWindow, ProcessWindowError> {
    let search = search_windows(process_id)?;
    required_candidate(&search.candidates)
}

fn require_input_access() -> Result<(), ProcessWindowError> {
    // SAFETY: this is the non-interactive TCC preflight API; it never requests authorization.
    if unsafe { CGPreflightPostEventAccess() } {
        Ok(())
    } else {
        Err(error(
            "process_window_input_unsupported",
            "macOS has not granted native event-posting access",
            "permission_denied",
        ))
    }
}

fn key_code(key: ProcessWindowKey) -> u16 {
    match key {
        ProcessWindowKey::Backspace => 0x33,
        ProcessWindowKey::Delete => 0x75,
        ProcessWindowKey::Down => 0x7d,
        ProcessWindowKey::End => 0x77,
        ProcessWindowKey::Enter => 0x24,
        ProcessWindowKey::Escape => 0x35,
        ProcessWindowKey::F2 => 0x78,
        ProcessWindowKey::Home => 0x73,
        ProcessWindowKey::Left => 0x7b,
        ProcessWindowKey::Right => 0x7c,
        ProcessWindowKey::Tab => 0x30,
        ProcessWindowKey::Up => 0x7e,
    }
}

fn display_scale(bounds: CgRect) -> f64 {
    const MAXIMUM_DISPLAYS: usize = 32;
    let mut displays = [0_u32; MAXIMUM_DISPLAYS];
    let mut count = 0_u32;
    // SAFETY: displays and count are writable for the sizes supplied.
    if unsafe { CGGetActiveDisplayList(MAXIMUM_DISPLAYS as u32, displays.as_mut_ptr(), &mut count) }
        != 0
    {
        return 1.0;
    }
    let center = CgPoint {
        x: bounds.origin.x + bounds.size.width / 2.0,
        y: bounds.origin.y + bounds.size.height / 2.0,
    };
    for display in displays
        .into_iter()
        .take((count as usize).min(MAXIMUM_DISPLAYS))
    {
        // SAFETY: display came from CGGetActiveDisplayList.
        let display_bounds = unsafe { CGDisplayBounds(display) };
        let contains = center.x >= display_bounds.origin.x
            && center.y >= display_bounds.origin.y
            && center.x < display_bounds.origin.x + display_bounds.size.width
            && center.y < display_bounds.origin.y + display_bounds.size.height;
        if contains && display_bounds.size.width > 0.0 {
            // SAFETY: display came from CGGetActiveDisplayList.
            let scale = unsafe { CGDisplayPixelsWide(display) } as f64 / display_bounds.size.width;
            if scale.is_finite() && scale >= 1.0 {
                return scale;
            }
        }
    }
    1.0
}

fn event_source() -> Result<OwnedCf, ProcessWindowError> {
    // SAFETY: the state ID is a public CGEventSourceStateID constant.
    OwnedCf::new(unsafe { CGEventSourceCreate(CG_EVENT_SOURCE_HID_SYSTEM_STATE) }).ok_or_else(
        || {
            error(
                "process_window_input",
                "native event source could not be created",
                "platform_error",
            )
        },
    )
}

pub(crate) fn facts(process_id: u32) -> ProcessWindowFacts {
    match search_windows(process_id) {
        Ok(search) => match resolve_candidates(&search.candidates) {
            CandidateResolution::Unique(window) => ProcessWindowFacts {
                supported: true,
                present: true,
                window_id: i64::from(window.id),
                title: window.title.clone(),
                foreground_window_id: i64::from(search.foreground_window_id),
                is_foreground: window.id == search.foreground_window_id,
            },
            CandidateResolution::Missing | CandidateResolution::Ambiguous => ProcessWindowFacts {
                supported: true,
                present: false,
                window_id: 0,
                title: String::new(),
                foreground_window_id: i64::from(search.foreground_window_id),
                is_foreground: false,
            },
        },
        Err(_) => ProcessWindowFacts {
            supported: false,
            present: false,
            window_id: 0,
            title: String::new(),
            foreground_window_id: 0,
            is_foreground: false,
        },
    }
}

pub(crate) fn key(process_id: u32, key: ProcessWindowKey) -> Result<(), ProcessWindowError> {
    required_window(process_id)?;
    require_input_access()?;
    let process_id = i32::try_from(process_id).map_err(|_| {
        error(
            "process_window_not_found",
            "child process identifier is outside the native range",
            "not_found",
        )
    })?;
    let source = event_source()?;
    // Construct both halves before posting so creation failure cannot leave a key held down.
    // SAFETY: source is a valid CGEventSource and key_code returns native virtual keycodes.
    let down = OwnedCf::new(unsafe { CGEventCreateKeyboardEvent(source.0, key_code(key), true) })
        .ok_or_else(|| {
        error(
            "process_window_input",
            "native key-down event could not be created",
            "platform_error",
        )
    })?;
    // SAFETY: source is a valid CGEventSource and key_code returns native virtual keycodes.
    let up = OwnedCf::new(unsafe { CGEventCreateKeyboardEvent(source.0, key_code(key), false) })
        .ok_or_else(|| {
            error(
                "process_window_input",
                "native key-up event could not be created",
                "platform_error",
            )
        })?;
    // SAFETY: events are valid and the PID was range-checked and owns the selected window.
    unsafe {
        CGEventPostToPid(process_id, down.0);
        CGEventPostToPid(process_id, up.0);
    }
    Ok(())
}

pub(crate) fn pointer(
    process_id: u32,
    action: ProcessWindowPointerAction,
    x: i32,
    y: i32,
) -> Result<(), ProcessWindowError> {
    let _ = (process_id, action, x, y);
    Err(error(
        "process_window_input_unsupported",
        "macOS does not provide reliable process-targeted background pointer delivery",
        "unsupported",
    ))
}

pub(crate) fn pointer_coordinate_scale(process_id: u32) -> Result<f64, ProcessWindowError> {
    let window = required_window(process_id)?;
    Ok(display_scale(window.bounds))
}

pub(crate) fn message(_: u32, _: ProcessWindowMessage) -> Result<isize, ProcessWindowError> {
    Err(unsupported(
        "native child-window messages are not available on macOS",
    ))
}

pub(crate) fn rect(process_id: u32, client: bool) -> Result<ProcessWindowRect, ProcessWindowError> {
    if client {
        return Err(unsupported(
            "exact native client geometry is not available from the macOS WindowServer",
        ));
    }
    let bounds = required_window(process_id)?.bounds;
    Ok(ProcessWindowRect {
        left: bounds.origin.x.round() as i64,
        top: bounds.origin.y.round() as i64,
        right: (bounds.origin.x + bounds.size.width).round() as i64,
        bottom: (bounds.origin.y + bounds.size.height).round() as i64,
    })
}

pub(crate) fn resize(_: u32, _: i32, _: i32) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child-window resize is not implemented on macOS",
    ))
}

pub(crate) fn control_exists(_: u32, _: i32) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child controls are not available on macOS",
    ))
}

pub(crate) fn control_visible(_: u32, _: i32) -> Result<bool, ProcessWindowError> {
    Err(unsupported(
        "native child controls are not available on macOS",
    ))
}

pub(crate) fn control_text(_: u32, _: i32) -> Result<String, ProcessWindowError> {
    Err(unsupported(
        "native child controls are not available on macOS",
    ))
}

pub(crate) fn control_set_text(_: u32, _: i32, _: &str) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child controls are not available on macOS",
    ))
}

pub(crate) fn control_click(_: u32, _: i32) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "native child controls are not available on macOS",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: u32) -> NativeWindow {
        NativeWindow {
            id,
            title: format!("window-{id}"),
            bounds: CgRect {
                origin: CgPoint::default(),
                size: CgSize {
                    width: 640.0,
                    height: 480.0,
                },
            },
        }
    }

    #[test]
    fn native_key_codes_cover_the_public_contract() {
        assert_eq!(key_code(ProcessWindowKey::Home), 0x73);
        assert_eq!(key_code(ProcessWindowKey::Down), 0x7d);
        assert_eq!(key_code(ProcessWindowKey::Enter), 0x24);
        assert_eq!(key_code(ProcessWindowKey::Tab), 0x30);
        assert_eq!(key_code(ProcessWindowKey::F2), 0x78);
    }

    #[test]
    fn exact_client_rect_fails_typed_instead_of_relabeling_outer_bounds() {
        assert_eq!(
            rect(0, true),
            Err(unsupported(
                "exact native client geometry is not available from the macOS WindowServer"
            ))
        );
    }

    #[test]
    fn process_targeted_pointer_delivery_fails_typed() {
        assert_eq!(
            pointer(7, ProcessWindowPointerAction::Click, 120, 401),
            Err(error(
                "process_window_input_unsupported",
                "macOS does not provide reliable process-targeted background pointer delivery",
                "unsupported"
            ))
        );
    }

    #[test]
    fn required_candidate_rejects_missing_and_ambiguous_windows() {
        assert_eq!(
            required_candidate(&[]).unwrap_err(),
            error(
                "process_window_not_found",
                "child has no visible top-level window",
                "not_found",
            )
        );
        assert_eq!(required_candidate(&[candidate(7)]).unwrap().id, 7);
        assert_eq!(
            required_candidate(&[candidate(7), candidate(8)]).unwrap_err(),
            error(
                "process_window_ambiguous",
                "child has multiple visible top-level windows",
                "ambiguous",
            )
        );
    }
}
