//! Terminal input encoding facade.
//!
//! Pure, platform-neutral: keyboard/paste/mouse byte encoding shared by every
//! terminal host in the workspace. See [`crate::contract::terminal_input`].

pub use crate::contract::terminal_input::{
    key_event_to_bytes, mouse_code_with_modifiers, mouse_delivery, mouse_report_bytes,
    normalize_terminal_paste, terminal_paste_bytes, xterm_modifier_code, ApplicationMouseMode,
    MouseDelivery, MouseReportEncoding, TerminalKeyMode, MOUSE_WHEEL_DOWN, MOUSE_WHEEL_UP,
    TERMINAL_PASTE_LIMIT_BYTES,
};
