//! Platform-neutral terminal input encoding: keyboard, paste, and mouse.
//!
//! Every terminal host in this repo receives the same normalized events
//! ([`NormalizedKeyEvent`], pointer/wheel events) and must turn them into the
//! byte sequences the running application negotiated over VT. Doing that
//! per-host is how the GUI and `agenterm-con` drifted apart: the GUI grew
//! modifier encoding, bracketed paste, and mouse reporting but never honored
//! DECCKM, while `agenterm-con` honored neither. This module is the single
//! mechanism both call, so a fix lands once.
//!
//! Everything here is pure: no OS calls, no I/O. That is deliberate — input
//! encoding is exactly the kind of fiddly table work that deserves unit tests
//! rather than a running terminal.

use crate::contract::input::{KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent};

/// Terminal input modes negotiated by the running application.
///
/// These come from the VT parser's screen state (`vt100::Screen`), not from
/// user configuration — the application turns them on and off as it runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalKeyMode {
    /// DECCKM (`CSI ?1h`). Cursor keys emit SS3 (`ESC O A`) instead of CSI
    /// (`ESC [ A`). Full-screen readline/editor apps rely on this to tell a
    /// cursor key apart from a literal escape sequence in the input stream.
    pub application_cursor: bool,
}

/// xterm's modifier parameter for CSI sequences: 1 + shift(1) + alt(2) + ctrl(4).
///
/// Returns `None` when no modifier is held, so callers emit the short form
/// (`CSI A`) rather than a redundant `CSI 1;1A`.
pub fn xterm_modifier_code(modifiers: ModifierState) -> Option<u8> {
    let code = 1
        + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.control);
    (code != 1).then_some(code)
}

/// How a named key's byte sequence interacts with the Alt/Meta ESC prefix.
enum NamedEncoding {
    /// The sequence already carries the modifier parameter. Adding an ESC
    /// prefix on top would double-encode Alt and confuse the application.
    Encoded(Vec<u8>),
    /// A bare sequence with no modifier slot; Alt still applies as ESC prefix
    /// (how readline/emacs Meta bindings are conventionally delivered).
    Plain(Vec<u8>),
}

/// Encodes a named key, honoring both the xterm modifier parameter and DECCKM.
///
/// Returns `None` for keys with no fixed sequence, so the caller can fall back
/// to the event's text.
fn named_key_encoding(
    named: NamedKey,
    modifiers: ModifierState,
    mode: TerminalKeyMode,
) -> Option<NamedEncoding> {
    // Shift+Tab is CBT, not a modified Tab — check before the generic path.
    if named == NamedKey::Tab {
        return Some(if modifiers.shift {
            NamedEncoding::Encoded(b"\x1b[Z".to_vec())
        } else {
            NamedEncoding::Plain(b"\t".to_vec())
        });
    }

    // Keys whose sequence never varies with cursor mode or modifiers.
    let plain: Option<&[u8]> = match named {
        NamedKey::Enter => Some(b"\r"),
        NamedKey::Backspace => Some(b"\x7f"),
        NamedKey::Escape => Some(b"\x1b"),
        NamedKey::Space => Some(b" "),
        _ => None,
    };
    if let Some(bytes) = plain {
        return Some(NamedEncoding::Plain(bytes.to_vec()));
    }

    // Cursor-ish keys: SS3 under DECCKM when unmodified, CSI with the
    // modifier parameter otherwise. xterm always uses the CSI form once a
    // modifier is present, even in application-cursor mode.
    let cursor_final: Option<u8> = match named {
        NamedKey::ArrowUp => Some(b'A'),
        NamedKey::ArrowDown => Some(b'B'),
        NamedKey::ArrowRight => Some(b'C'),
        NamedKey::ArrowLeft => Some(b'D'),
        NamedKey::Home => Some(b'H'),
        NamedKey::End => Some(b'F'),
        _ => None,
    };
    if let Some(final_byte) = cursor_final {
        let bytes = match xterm_modifier_code(modifiers) {
            Some(code) => format!("\x1b[1;{code}{}", final_byte as char).into_bytes(),
            None if mode.application_cursor => vec![0x1b, b'O', final_byte],
            None => vec![0x1b, b'[', final_byte],
        };
        return Some(NamedEncoding::Encoded(bytes));
    }

    // F1..F4 use SS3 unmodified and CSI 1;<mod><P..S> when modified.
    let ss3_function: Option<u8> = match named {
        NamedKey::F1 => Some(b'P'),
        NamedKey::F2 => Some(b'Q'),
        NamedKey::F3 => Some(b'R'),
        NamedKey::F4 => Some(b'S'),
        _ => None,
    };
    if let Some(final_byte) = ss3_function {
        let bytes = match xterm_modifier_code(modifiers) {
            Some(code) => format!("\x1b[1;{code}{}", final_byte as char).into_bytes(),
            None => vec![0x1b, b'O', final_byte],
        };
        return Some(NamedEncoding::Encoded(bytes));
    }

    // Tilde-terminated keys: `CSI n ~` or `CSI n ; <mod> ~`.
    let tilde: Option<u8> = match named {
        NamedKey::Insert => Some(2),
        NamedKey::Delete => Some(3),
        NamedKey::PageUp => Some(5),
        NamedKey::PageDown => Some(6),
        NamedKey::F5 => Some(15),
        NamedKey::F6 => Some(17),
        NamedKey::F7 => Some(18),
        NamedKey::F8 => Some(19),
        NamedKey::F9 => Some(20),
        NamedKey::F10 => Some(21),
        NamedKey::F11 => Some(23),
        NamedKey::F12 => Some(24),
        _ => None,
    };
    if let Some(number) = tilde {
        let bytes = match xterm_modifier_code(modifiers) {
            Some(code) => format!("\x1b[{number};{code}~").into_bytes(),
            None => format!("\x1b[{number}~").into_bytes(),
        };
        return Some(NamedEncoding::Encoded(bytes));
    }

    None
}

/// Converts a normalized key event into the bytes to write to the PTY.
///
/// Returns `None` for key releases and for events that carry no input (pure
/// modifier presses, unmapped named keys with no text).
pub fn key_event_to_bytes(event: &NormalizedKeyEvent, mode: TerminalKeyMode) -> Option<Vec<u8>> {
    if event.state == KeyPressState::Released {
        return None;
    }

    if let LogicalKey::Named(named) = &event.logical {
        match named_key_encoding(*named, event.modifiers, mode) {
            // Modifier already folded into the sequence; must not ESC-prefix.
            Some(NamedEncoding::Encoded(bytes)) => return Some(bytes),
            Some(NamedEncoding::Plain(bytes)) => return Some(alt_prefixed(bytes, event.modifiers)),
            None => {}
        }
    }

    // Ctrl+letter → C0 control code (Ctrl+A = 0x01 .. Ctrl+Z = 0x1a).
    if event.modifiers.control {
        if let LogicalKey::Character(text) = &event.logical {
            let upper = text
                .chars()
                .next()
                .map(|character| character.to_ascii_uppercase())
                .filter(char::is_ascii_alphabetic);
            if let Some(upper) = upper {
                return Some(alt_prefixed(vec![(upper as u8) - b'@'], event.modifiers));
            }
        }
        // Ctrl with a non-letter produces no C0 code; fall through rather than
        // emitting the bare character, which would type a stray glyph.
        return None;
    }

    // Plain printable text. `text` is the layout-resolved commit (respects
    // dead keys and shift); `logical` is the fallback when the backend did not
    // supply one.
    let text = event
        .text
        .as_deref()
        .filter(|value| !value.is_empty())
        .or(match &event.logical {
            LogicalKey::Character(value) if !value.is_empty() => Some(value.as_str()),
            _ => None,
        })?;

    Some(alt_prefixed(text.as_bytes().to_vec(), event.modifiers))
}

/// Prepends ESC when Alt/Meta is held, the conventional Meta encoding.
fn alt_prefixed(bytes: Vec<u8>, modifiers: ModifierState) -> Vec<u8> {
    if !modifiers.alt {
        return bytes;
    }
    let mut prefixed = Vec::with_capacity(bytes.len() + 1);
    prefixed.push(0x1b);
    prefixed.extend_from_slice(&bytes);
    prefixed
}

// ---------------------------------------------------------------------------
// Paste
// ---------------------------------------------------------------------------

/// Upper bound on a single paste, matching the GUI's limit.
pub const TERMINAL_PASTE_LIMIT_BYTES: usize = 256 * 1024;

/// Normalizes clipboard text for PTY input: CRLF/LF → CR, keep tabs, and drop
/// every other control character.
///
/// Dropping controls is a security property, not tidiness: it removes ESC from
/// pasted text, so a payload containing `ESC [ 201 ~` cannot close the
/// bracketed-paste guard early and get the remainder executed as keystrokes.
pub fn normalize_terminal_paste(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push('\r');
            }
            '\n' => normalized.push('\r'),
            '\t' => normalized.push('\t'),
            value if !value.is_control() => normalized.push(value),
            _ => {}
        }
    }
    normalized
}

/// Frames normalized paste text for the target terminal mode.
///
/// With bracketed paste (DECSET 2004) the application can tell pasted text
/// apart from typing — editors use it to suppress auto-indent, and shells to
/// avoid executing every pasted line.
pub fn terminal_paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(text.as_bytes());
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

// ---------------------------------------------------------------------------
// Mouse
// ---------------------------------------------------------------------------

/// The xterm mouse reporting mode the application negotiated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ApplicationMouseMode {
    /// No reporting; the host owns the mouse for local selection.
    #[default]
    None,
    /// X10 (`?9h`): press only.
    Press,
    /// VT200 (`?1000h`): press and release.
    PressRelease,
    /// `?1002h`: press, release, and motion while a button is held.
    ButtonMotion,
    /// `?1003h`: all motion, button or not.
    AnyMotion,
}

/// The wire encoding for mouse reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseReportEncoding {
    /// Classic single-byte encoding; cannot express coordinates past 223.
    #[default]
    Default,
    /// UTF-8 extension (`?1005h`).
    Utf8,
    /// SGR (`?1006h`): unbounded coordinates, distinguishes release.
    Sgr,
}

/// Whether an event belongs to the application or to local selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseDelivery {
    LocalSelection,
    Application,
}

/// Wheel button codes, which xterm reports as buttons 64/65.
pub const MOUSE_WHEEL_UP: u8 = 64;
pub const MOUSE_WHEEL_DOWN: u8 = 65;

/// Encodes one xterm mouse report.
///
/// Coordinates are zero-based grid cells. The classic encodings cannot express
/// release-button identity or cells beyond their byte range, so those degrade
/// exactly as xterm does: release folds to button 3, out-of-range is dropped.
pub fn mouse_report_bytes(
    encoding: MouseReportEncoding,
    code: u8,
    column: u16,
    row: u16,
    pressed: bool,
) -> Option<Vec<u8>> {
    match encoding {
        MouseReportEncoding::Sgr => {
            let suffix = if pressed { 'M' } else { 'm' };
            Some(
                format!(
                    "\x1b[<{code};{};{}{suffix}",
                    u32::from(column) + 1,
                    u32::from(row) + 1
                )
                .into_bytes(),
            )
        }
        MouseReportEncoding::Default => {
            let code = if pressed { code } else { (code & !0b11) | 3 };
            let column = u8::try_from(column.checked_add(33)?).ok()?;
            let row = u8::try_from(row.checked_add(33)?).ok()?;
            Some(vec![0x1b, b'[', b'M', 32 + code, column, row])
        }
        MouseReportEncoding::Utf8 => {
            let code = if pressed { code } else { (code & !0b11) | 3 };
            let mut bytes = vec![0x1b, b'[', b'M'];
            let mut push = |scalar: u32| {
                let character = char::from_u32(scalar)?;
                let mut buffer = [0u8; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                Some(())
            };
            push(u32::from(code) + 32)?;
            push(u32::from(column) + 33)?;
            push(u32::from(row) + 33)?;
            Some(bytes)
        }
    }
}

/// Decides whether an event goes to the application or to local selection.
///
/// Shift always forces local selection (the xterm convention that lets a user
/// select text inside a mouse-grabbing TUI). Scrollback suppresses reports so
/// reported cells match what the application actually drew — except mid-drag,
/// where the press was already delivered and the release must follow it.
pub fn mouse_delivery(
    mode: ApplicationMouseMode,
    shift_override: bool,
    scrolled_back: bool,
    motion: bool,
    dragging: bool,
    pressed: bool,
) -> MouseDelivery {
    if shift_override || mode == ApplicationMouseMode::None {
        return MouseDelivery::LocalSelection;
    }
    if scrolled_back && !dragging {
        return MouseDelivery::LocalSelection;
    }
    let reportable = match mode {
        ApplicationMouseMode::None => false,
        ApplicationMouseMode::Press => pressed && !motion,
        ApplicationMouseMode::PressRelease | ApplicationMouseMode::ButtonMotion => {
            !motion || dragging
        }
        ApplicationMouseMode::AnyMotion => true,
    };
    if reportable {
        MouseDelivery::Application
    } else {
        MouseDelivery::LocalSelection
    }
}

/// Folds motion and modifier bits into a mouse button code.
pub fn mouse_code_with_modifiers(button: u8, motion: bool, modifiers: ModifierState) -> u8 {
    let mut code = button;
    if motion {
        code |= 32;
    }
    if modifiers.alt {
        code |= 8;
    }
    if modifiers.control {
        code |= 16;
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::input::PhysicalKeyCode;

    fn mods(control: bool, shift: bool, alt: bool) -> ModifierState {
        ModifierState {
            control,
            shift,
            alt,
            meta: false,
        }
    }

    fn named(key: NamedKey, modifiers: ModifierState) -> NormalizedKeyEvent {
        NormalizedKeyEvent {
            logical: LogicalKey::Named(key),
            physical: PhysicalKeyCode::Other,
            text: None,
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers,
        }
    }

    fn character(text: &str, modifiers: ModifierState) -> NormalizedKeyEvent {
        NormalizedKeyEvent {
            logical: LogicalKey::Character(text.to_owned()),
            physical: PhysicalKeyCode::Other,
            text: Some(text.to_owned()),
            state: KeyPressState::Pressed,
            repeat: false,
            modifiers,
        }
    }

    const NORMAL: TerminalKeyMode = TerminalKeyMode {
        application_cursor: false,
    };
    const APP: TerminalKeyMode = TerminalKeyMode {
        application_cursor: true,
    };

    #[test]
    fn cursor_keys_switch_between_csi_and_ss3_on_deccm() {
        let up = named(NamedKey::ArrowUp, mods(false, false, false));
        assert_eq!(key_event_to_bytes(&up, NORMAL), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_event_to_bytes(&up, APP), Some(b"\x1bOA".to_vec()));

        let home = named(NamedKey::Home, mods(false, false, false));
        assert_eq!(key_event_to_bytes(&home, NORMAL), Some(b"\x1b[H".to_vec()));
        assert_eq!(key_event_to_bytes(&home, APP), Some(b"\x1bOH".to_vec()));
    }

    #[test]
    fn modified_cursor_keys_use_csi_form_even_in_application_mode() {
        // xterm keeps the CSI form once a modifier is present, so an app in
        // DECCKM still receives the parameterized sequence.
        let ctrl_right = named(NamedKey::ArrowRight, mods(true, false, false));
        assert_eq!(
            key_event_to_bytes(&ctrl_right, NORMAL),
            Some(b"\x1b[1;5C".to_vec())
        );
        assert_eq!(
            key_event_to_bytes(&ctrl_right, APP),
            Some(b"\x1b[1;5C".to_vec())
        );
    }

    #[test]
    fn xterm_modifier_codes_match_the_published_table() {
        assert_eq!(xterm_modifier_code(mods(false, false, false)), None);
        assert_eq!(xterm_modifier_code(mods(false, true, false)), Some(2));
        assert_eq!(xterm_modifier_code(mods(false, false, true)), Some(3));
        assert_eq!(xterm_modifier_code(mods(true, false, false)), Some(5));
        assert_eq!(xterm_modifier_code(mods(true, true, true)), Some(8));
    }

    #[test]
    fn tilde_and_function_keys_carry_modifier_parameters() {
        assert_eq!(
            key_event_to_bytes(&named(NamedKey::Delete, mods(false, true, false)), NORMAL),
            Some(b"\x1b[3;2~".to_vec())
        );
        assert_eq!(
            key_event_to_bytes(&named(NamedKey::F12, mods(true, true, false)), NORMAL),
            Some(b"\x1b[24;6~".to_vec())
        );
        // F1 is SS3 unmodified but CSI once modified.
        assert_eq!(
            key_event_to_bytes(&named(NamedKey::F1, mods(false, false, false)), NORMAL),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            key_event_to_bytes(&named(NamedKey::F1, mods(true, false, false)), NORMAL),
            Some(b"\x1b[1;5P".to_vec())
        );
    }

    #[test]
    fn shift_tab_is_cbt_not_a_modified_tab() {
        assert_eq!(
            key_event_to_bytes(&named(NamedKey::Tab, mods(false, true, false)), NORMAL),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            key_event_to_bytes(&named(NamedKey::Tab, mods(false, false, false)), NORMAL),
            Some(b"\t".to_vec())
        );
    }

    #[test]
    fn alt_prefixes_esc_without_double_encoding_modified_named_keys() {
        // Character keys get the ESC prefix.
        assert_eq!(
            key_event_to_bytes(&character("b", mods(false, false, true)), NORMAL),
            Some(vec![0x1b, b'b'])
        );
        // Alt+Enter has no modifier slot, so ESC prefix applies.
        assert_eq!(
            key_event_to_bytes(&named(NamedKey::Enter, mods(false, false, true)), NORMAL),
            Some(vec![0x1b, b'\r'])
        );
        // Alt+Up folds into the parameter (code 3) and must NOT also be
        // ESC-prefixed, which would read as two separate events.
        assert_eq!(
            key_event_to_bytes(&named(NamedKey::ArrowUp, mods(false, false, true)), NORMAL),
            Some(b"\x1b[1;3A".to_vec())
        );
    }

    #[test]
    fn ctrl_letters_map_to_c0_and_ctrl_alt_adds_esc() {
        assert_eq!(
            key_event_to_bytes(&character("c", mods(true, false, false)), NORMAL),
            Some(vec![0x03])
        );
        assert_eq!(
            key_event_to_bytes(&character("c", mods(true, false, true)), NORMAL),
            Some(vec![0x1b, 0x03])
        );
    }

    #[test]
    fn ctrl_with_non_letter_emits_nothing_rather_than_a_stray_glyph() {
        assert_eq!(
            key_event_to_bytes(&character("5", mods(true, false, false)), NORMAL),
            None
        );
    }

    #[test]
    fn releases_and_empty_events_produce_no_bytes() {
        let mut event = named(NamedKey::Enter, mods(false, false, false));
        event.state = KeyPressState::Released;
        assert_eq!(key_event_to_bytes(&event, NORMAL), None);
    }

    #[test]
    fn paste_normalization_strips_escape_so_the_guard_cannot_be_closed_early() {
        // The dangerous payload: text that tries to end bracketed paste and
        // have the rest run as typed input.
        let hostile = "safe\x1b[201~rm -rf /\r";
        let normalized = normalize_terminal_paste(hostile);
        assert!(!normalized.contains('\x1b'));
        let framed = terminal_paste_bytes(&normalized, true);
        let text = String::from_utf8(framed).expect("utf8");
        assert_eq!(text.matches("\x1b[201~").count(), 1);
        assert!(text.starts_with("\x1b[200~"));
        assert!(text.ends_with("\x1b[201~"));
    }

    #[test]
    fn paste_normalizes_newlines_and_respects_mode() {
        assert_eq!(normalize_terminal_paste("a\r\nb\nc\t\u{7}d"), "a\rb\rc\td");
        assert_eq!(terminal_paste_bytes("a\rb", false), b"a\rb".to_vec());
        assert_eq!(
            terminal_paste_bytes("x", true),
            b"\x1b[200~x\x1b[201~".to_vec()
        );
    }

    #[test]
    fn sgr_reports_distinguish_press_from_release_and_are_one_based() {
        assert_eq!(
            mouse_report_bytes(MouseReportEncoding::Sgr, 0, 0, 0, true),
            Some(b"\x1b[<0;1;1M".to_vec())
        );
        assert_eq!(
            mouse_report_bytes(MouseReportEncoding::Sgr, 0, 9, 4, false),
            Some(b"\x1b[<0;10;5m".to_vec())
        );
    }

    #[test]
    fn default_encoding_folds_release_to_button_three_and_drops_far_cells() {
        assert_eq!(
            mouse_report_bytes(MouseReportEncoding::Default, 0, 0, 0, true),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
        assert_eq!(
            mouse_report_bytes(MouseReportEncoding::Default, 0, 0, 0, false),
            Some(vec![0x1b, b'[', b'M', 35, 33, 33])
        );
        // Past the single-byte range the classic encoding cannot represent the
        // cell, so the report is dropped rather than sent wrong.
        assert_eq!(
            mouse_report_bytes(MouseReportEncoding::Default, 0, 300, 0, true),
            None
        );
    }

    #[test]
    fn shift_forces_local_selection_even_when_an_app_grabs_the_mouse() {
        assert_eq!(
            mouse_delivery(ApplicationMouseMode::AnyMotion, true, false, false, false, true),
            MouseDelivery::LocalSelection
        );
        assert_eq!(
            mouse_delivery(ApplicationMouseMode::AnyMotion, false, false, false, false, true),
            MouseDelivery::Application
        );
    }

    #[test]
    fn scrollback_suppresses_reports_but_not_an_in_flight_drag() {
        assert_eq!(
            mouse_delivery(
                ApplicationMouseMode::ButtonMotion,
                false,
                true,
                false,
                false,
                true
            ),
            MouseDelivery::LocalSelection
        );
        assert_eq!(
            mouse_delivery(
                ApplicationMouseMode::ButtonMotion,
                false,
                true,
                true,
                true,
                false
            ),
            MouseDelivery::Application
        );
    }

    #[test]
    fn press_only_mode_ignores_release_and_motion() {
        assert_eq!(
            mouse_delivery(ApplicationMouseMode::Press, false, false, false, false, false),
            MouseDelivery::LocalSelection
        );
        assert_eq!(
            mouse_delivery(ApplicationMouseMode::Press, false, false, true, true, true),
            MouseDelivery::LocalSelection
        );
        assert_eq!(
            mouse_delivery(ApplicationMouseMode::Press, false, false, false, false, true),
            MouseDelivery::Application
        );
    }

    #[test]
    fn modifier_and_motion_bits_fold_into_the_button_code() {
        assert_eq!(mouse_code_with_modifiers(0, false, mods(false, false, false)), 0);
        assert_eq!(mouse_code_with_modifiers(0, true, mods(false, false, false)), 32);
        assert_eq!(mouse_code_with_modifiers(0, false, mods(false, false, true)), 8);
        assert_eq!(mouse_code_with_modifiers(0, false, mods(true, false, false)), 16);
        assert_eq!(mouse_code_with_modifiers(MOUSE_WHEEL_UP, false, mods(false, false, false)), 64);
    }
}
