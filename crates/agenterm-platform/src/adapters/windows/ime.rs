use std::borrow::Cow;
use std::io::Write as _;

use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Globalization::{GetLocaleInfoW, LOCALE_SLOCALIZEDLANGUAGENAME},
    Graphics::Gdi::ClientToScreen,
    UI::{
        HiDpi::GetDpiForWindow,
        Input::{
            Ime::{
                CANDIDATEFORM, CFS_POINT, COMPOSITIONFORM, GCS_COMPSTR, GCS_CURSORPOS,
                IME_CMODE_FULLSHAPE, IME_CMODE_NATIVE, ImmGetCompositionStringW, ImmGetContext,
                ImmGetConversionStatus, ImmGetDescriptionW, ImmGetOpenStatus, ImmReleaseContext,
                ImmSetCandidateWindow, ImmSetCompositionWindow,
            },
            KeyboardAndMouse::{GetFocus, GetKeyboardLayout},
        },
        WindowsAndMessaging::{
            GetClientRect, GetForegroundWindow, GetWindowRect, WM_IME_COMPOSITION,
            WM_IME_ENDCOMPOSITION, WM_IME_STARTCOMPOSITION,
        },
    },
};

use crate::{
    CapabilityStatus,
    contract::ime::{ImeComposition, ImeStatus},
};

use std::sync::Mutex;

/// Latest composition read while WM_IME_* messages were processed. The GUI
/// polls this from the paint path so the terminal can render the in-progress
/// pinyin inline and anchor the candidate window next to it.
static COMPOSITION: Mutex<Option<ImeComposition>> = Mutex::new(None);

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
    let converted = unsafe { ClientToScreen(focus, &mut point) };
    trace_anchor(focus, x, y, &point, converted);
    if converted == 0 {
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

/// Refresh the cached composition state from the window message being
/// processed. The composition text is only readable while the input context
/// is live on this window, so the adapter caches it here and lets callers
/// poll it later from the paint path.
pub(crate) fn refresh_from_message(hwnd: HWND, message: u32) {
    let next = match message {
        WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION => read_composition(hwnd),
        WM_IME_ENDCOMPOSITION => None,
        _ => return,
    };
    if let Ok(mut slot) = COMPOSITION.lock() {
        *slot = next;
    }
}

/// The composition currently being typed on this window, if any.
pub(crate) fn composition() -> Option<ImeComposition> {
    COMPOSITION.lock().ok().and_then(|slot| slot.clone())
}

fn read_composition(hwnd: HWND) -> Option<ImeComposition> {
    let context = unsafe { ImmGetContext(hwnd) };
    if context.is_null() {
        return None;
    }
    let text = composition_text(context);
    let cursor = composition_cursor(context);
    unsafe {
        ImmReleaseContext(hwnd, context);
    }
    text.map(|text| {
        let char_count = text.chars().count();
        ImeComposition {
            text,
            cursor: cursor.unwrap_or(char_count),
        }
    })
}

fn composition_text(context: *mut core::ffi::c_void) -> Option<String> {
    let needed = unsafe { ImmGetCompositionStringW(context, GCS_COMPSTR, std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return None;
    }
    let mut buffer = vec![0u16; needed as usize / 2 + 1];
    let copied = unsafe {
        ImmGetCompositionStringW(
            context,
            GCS_COMPSTR,
            buffer.as_mut_ptr() as *mut _,
            (buffer.len() * 2) as u32,
        )
    };
    if copied <= 0 {
        return None;
    }
    let units = (copied as usize / 2).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..units]);
    (!text.is_empty()).then_some(text)
}

fn composition_cursor(context: *mut core::ffi::c_void) -> Option<usize> {
    let units =
        unsafe { ImmGetCompositionStringW(context, GCS_CURSORPOS, std::ptr::null_mut(), 0) };
    // GCS_CURSORPOS returns 0 when the caret sits at the start of the
    // composition, which is a valid position; only a negative result means the
    // call produced no data.
    (units >= 0).then_some(units.max(0) as usize)
}

/// Diagnostic trace behind `PLATFORM_IME_DEBUG=1` for candidate-window
/// position debugging. Writes one line per anchor update to
/// `%TEMP%\platform-ime-debug.log`.
///
/// The gate is deliberately product-neutral: this crate must stay
/// independently consumable, so it reads no product-branded environment.
fn trace_anchor(focus: HWND, x: i32, y: i32, screen: &POINT, converted: i32) {
    if std::env::var_os("PLATFORM_IME_DEBUG").is_none() {
        return;
    }
    let mut window_rect: RECT = unsafe { std::mem::zeroed() };
    let mut client_rect: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        GetWindowRect(focus, &mut window_rect);
        GetClientRect(focus, &mut client_rect);
    }
    let dpi = unsafe { GetDpiForWindow(focus) };
    let line = format!(
        "hwnd={focus:p} client=({x},{y}) screen=({},{}) win=({},{},{},{}) cli=({},{},{},{}) dpi={dpi} cs={converted}\n",
        screen.x,
        screen.y,
        window_rect.left,
        window_rect.top,
        window_rect.right,
        window_rect.bottom,
        client_rect.left,
        client_rect.top,
        client_rect.right,
        client_rect.bottom,
    );
    let path = std::env::temp_dir().join("platform-ime-debug.log");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
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
