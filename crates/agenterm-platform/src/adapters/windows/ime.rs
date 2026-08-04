use std::borrow::Cow;

use windows_sys::Win32::{
    Foundation::{POINT, RECT},
    Globalization::{GetLocaleInfoW, LOCALE_SLOCALIZEDLANGUAGENAME},
    Graphics::Gdi::ClientToScreen,
    UI::{
        Input::{
            Ime::{
                CANDIDATEFORM, CFS_POINT, COMPOSITIONFORM, IME_CMODE_FULLSHAPE, IME_CMODE_NATIVE,
                ImmGetContext, ImmGetConversionStatus, ImmGetDescriptionW, ImmGetOpenStatus,
                ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow,
            },
            KeyboardAndMouse::{GetFocus, GetKeyboardLayout},
        },
        WindowsAndMessaging::GetForegroundWindow,
    },
};

use crate::{CapabilityStatus, contract::ime::ImeStatus};

pub(crate) fn capability_status(_display_available: bool) -> CapabilityStatus {
    CapabilityStatus::Unsupported {
        reason: Cow::Borrowed("ime-preedit-not-yet-adapted"),
    }
}

/// Point the IME composition and candidate windows at a client-area caret.
///
/// The terminal grid is not a native editable control, so the OS has no caret
/// to anchor the candidate window to; without this call the candidate bar
/// appears at a default position. IMM32 positioning is honored by both legacy
/// IMM32 IMEs and modern TSF text services (Microsoft Pinyin, MS-IME, ...).
/// Coordinates are client-area pixels; IMM32 wants screen coordinates, so the
/// point is converted with `ClientToScreen` before being reported.
pub(crate) fn set_anchor_position(x: i32, y: i32) {
    let focus = unsafe { GetFocus() };
    if focus.is_null() {
        return;
    }
    let mut point = POINT { x, y };
    if unsafe { ClientToScreen(focus, &mut point) } == 0 {
        return;
    }
    let context = unsafe { ImmGetContext(focus) };
    if context.is_null() {
        return;
    }
    let empty_rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let composition = COMPOSITIONFORM {
        dwStyle: CFS_POINT,
        ptCurrentPos: point,
        rcArea: empty_rect,
    };
    let candidate = CANDIDATEFORM {
        dwIndex: 0,
        dwStyle: CFS_POINT,
        ptCurrentPos: point,
        rcArea: empty_rect,
    };
    unsafe {
        ImmSetCompositionWindow(context, &composition);
        ImmSetCandidateWindow(context, &candidate);
        ImmReleaseContext(focus, context);
    }
}

/// Describe the input method the caller's focused surface is typing through.
///
/// Deliberately resolves the window itself instead of accepting an HWND, so
/// native handles stay inside this module. `GetFocus` is thread-local: it
/// answers for whichever surface of *our* window has keyboard focus, and only
/// while this thread owns the foreground window — a background AgenTerm must
/// not report the IME state of whatever the user is actually typing into.
pub(crate) fn status() -> Option<ImeStatus> {
    let focus = unsafe { GetFocus() };
    if focus.is_null() || unsafe { GetForegroundWindow() }.is_null() {
        return None;
    }

    let context = unsafe { ImmGetContext(focus) };
    if context.is_null() {
        // No input context: a plain keyboard layout, not an IME.
        return Some(ImeStatus::default());
    }

    let open = unsafe { ImmGetOpenStatus(context) } != 0;
    let mut conversion = 0u32;
    let mut sentence = 0u32;
    let converted = unsafe { ImmGetConversionStatus(context, &mut conversion, &mut sentence) } != 0;
    unsafe {
        ImmReleaseContext(focus, context);
    }

    Some(ImeStatus {
        name: active_layout_description(),
        available: true,
        open,
        native_mode: converted && conversion & IME_CMODE_NATIVE != 0,
        full_shape: converted && conversion & IME_CMODE_FULLSHAPE != 0,
    })
}

/// Human-readable name of the active input method.
///
/// `ImmGetDescriptionW` only answers for legacy IMM32 IMEs; the text services
/// most people actually run (Microsoft Pinyin, MS-IME) are TSF based and
/// report an empty description. Fall back to the layout's language name, so
/// the label says "中文" rather than nothing on a normal Windows install.
fn active_layout_description() -> String {
    let layout = unsafe { GetKeyboardLayout(0) };
    let length = unsafe { ImmGetDescriptionW(layout, std::ptr::null_mut(), 0) } as usize;
    if length > 0 {
        let mut buffer = vec![0u16; length + 1];
        let written = unsafe {
            ImmGetDescriptionW(
                layout,
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(u32::MAX),
            )
        } as usize;
        if written > 0 {
            return String::from_utf16_lossy(&buffer[..written.min(buffer.len())]);
        }
    }
    // The low word of an HKL is the layout's language identifier, which is
    // also a valid LCID for the neutral locale lookup below.
    layout_language_name((layout as usize & 0xffff) as u32)
}

fn layout_language_name(language_id: u32) -> String {
    let length = unsafe {
        GetLocaleInfoW(
            language_id,
            LOCALE_SLOCALIZEDLANGUAGENAME,
            std::ptr::null_mut(),
            0,
        )
    };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize];
    let written = unsafe {
        GetLocaleInfoW(
            language_id,
            LOCALE_SLOCALIZEDLANGUAGENAME,
            buffer.as_mut_ptr(),
            length,
        )
    };
    if written <= 0 {
        return String::new();
    }
    // GetLocaleInfoW counts the terminating NUL in its returned length.
    let text = &buffer[..(written as usize).min(buffer.len())];
    String::from_utf16_lossy(text.strip_suffix(&[0]).unwrap_or(text))
}
