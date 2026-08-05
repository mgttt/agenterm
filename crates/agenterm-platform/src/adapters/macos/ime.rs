use std::borrow::Cow;
use std::ffi::c_void;

use crate::{
    CapabilityStatus,
    contract::ime::{ImeComposition, ImeStatus},
};

// Text Input Sources bindings.
//
// macOS reports the active input method through HIToolbox's Text Input
// Sources API rather than through a per-window input context, so unlike the
// Windows adapter there is no focus handle to pass in: the query answers for
// whichever source the user has selected system-wide. Declared here by hand
// because the crate only needs three symbols out of Carbon and pulling a
// binding crate in for them would widen the dependency surface for nothing.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(source: *mut c_void, key: *const c_void) -> *mut c_void;
    static kTISPropertyInputSourceType: *const c_void;
    static kTISPropertyLocalizedName: *const c_void;
}

// Signatures deliberately match the crate's other CoreFoundation
// declarations (see `process_window.rs`) so the two agree at link time.
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
    fn CFStringGetCString(
        string: *const c_void,
        buffer: *mut i8,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
}

/// `kCFStringEncodingUTF8`.
const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

/// Input source type reported for a keyboard *input mode* — the sub-selection
/// of an input method, e.g. 微信输入法's pinyin mode. Plain layouts such as
/// ABC report `TISTypeKeyboardLayout` instead, which is how this adapter
/// tells "an IME is active" from "a plain keyboard is active".
const KEYBOARD_INPUT_MODE: &str = "TISTypeKeyboardInputMode";

/// Read a `CFStringRef` into an owned `String`.
///
/// The buffer is generously sized rather than queried: input-source names are
/// short display strings, and a truncated read here would only ever mislabel
/// a status bar.
fn cf_string(value: *const c_void) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut buffer = [0i8; 512];
    let ok = unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            CF_STRING_ENCODING_UTF8,
        )
    };
    if !ok {
        return None;
    }
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8(bytes).ok()
}

/// Input method the user has selected, as far as macOS is willing to say.
///
/// `open` and `native_mode` are both derived from *which source is selected*
/// rather than read separately, because macOS has no equivalent of IMM's
/// open/closed toggle: selecting a keyboard input mode (as opposed to a plain
/// keyboard layout) is itself the state of "composition is being intercepted".
/// Reporting `open: false` while 微信输入法 is selected would not be a modest
/// omission — it would assert that keystrokes pass straight through, which is
/// false, and would make the status bar read `latin` while the user types
/// Chinese.
///
/// `full_shape` genuinely is unobservable: full-width punctuation mode lives
/// inside the input method's own process with no public API to read it, so it
/// stays at its default per the contract's instruction to leave unreportable
/// fields empty rather than guess. The cost is that a macOS status bar never
/// shows the `· full-width` suffix.
pub(crate) fn status() -> Option<ImeStatus> {
    let source = unsafe { TISCopyCurrentKeyboardInputSource() };
    if source.is_null() {
        return None;
    }

    let source_type = unsafe { TISGetInputSourceProperty(source, kTISPropertyInputSourceType) };
    let is_input_mode = cf_string(source_type)
        .map(|kind| kind == KEYBOARD_INPUT_MODE)
        .unwrap_or(false);

    let name = if is_input_mode {
        let localized = unsafe { TISGetInputSourceProperty(source, kTISPropertyLocalizedName) };
        cf_string(localized).unwrap_or_default()
    } else {
        String::new()
    };

    unsafe { CFRelease(source) };

    Some(ImeStatus {
        name,
        available: is_input_mode,
        open: is_input_mode,
        native_mode: is_input_mode,
        // Not observable from outside the input method's process.
        full_shape: false,
    })
}

/// Preedit state arrives through winit's IME events here rather than from a
/// pollable host API, so there is nothing to report synchronously.
pub(crate) fn composition() -> Option<ImeComposition> {
    None
}

/// winit positions the composition/candidate UI on our behalf; no-op.
pub(crate) fn set_anchor_position(_x: i32, _y: i32) {}

pub(crate) fn capability_status(display_available: bool) -> CapabilityStatus {
    if display_available {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("headless-display"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the real Text Input Sources path. The assertion is
    /// deliberately weak about *which* source is selected — CI runners and
    /// developer machines differ — but it does pin the invariants the status
    /// bar depends on.
    #[test]
    fn current_input_source_reports_a_consistent_status() {
        let Some(status) = status() else {
            // Headless or no selected source: absence is a valid answer.
            return;
        };
        // `available` is derived from the source type, so a named source must
        // be an input mode and an unnamed one must not claim to be an IME.
        if !status.name.is_empty() {
            assert!(status.available, "a named input source must be available");
        }
        // On macOS all three track input-mode selection: there is no separate
        // open/closed toggle to read.
        assert_eq!(status.available, status.native_mode);
        assert_eq!(status.available, status.open);
        // Full-width mode is genuinely unobservable, so it stays defaulted
        // rather than guessed.
        assert!(!status.full_shape);
        // A selected input mode must produce a label that names it rather
        // than falling back to "off".
        if status.available {
            assert!(status.label().starts_with("IME: "));
            assert!(!status.label().contains("latin"));
        }
    }

    #[test]
    fn cf_string_rejects_null() {
        assert_eq!(cf_string(std::ptr::null()), None);
    }
}
