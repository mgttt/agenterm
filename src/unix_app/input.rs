use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};

/// Result of handling a key in composer focus.
pub(super) enum ComposerKeyAction {
    /// Text changed; redraw only.
    Edited,
    /// Submit the draft to the active tab.
    Submit,
    /// Return focus to the terminal without submitting.
    Escape,
    /// Ignore this key.
    Ignored,
}

/// Maps a winit key event to composer edits when the composer strip has focus.
pub(super) fn composer_key_action(
    event: &KeyEvent,
    modifiers: ModifiersState,
    buffer: &mut String,
) -> ComposerKeyAction {
    if event.state != ElementState::Pressed || event.repeat {
        return ComposerKeyAction::Ignored;
    }

    let control = modifiers.control_key();
    let shift = modifiers.shift_key();

    match &event.logical_key {
        Key::Named(NamedKey::Enter) if control => ComposerKeyAction::Submit,
        Key::Named(NamedKey::Escape) => ComposerKeyAction::Escape,
        Key::Named(NamedKey::Enter) => {
            buffer.push('\n');
            ComposerKeyAction::Edited
        }
        Key::Named(NamedKey::Backspace) => {
            if buffer.pop().is_some() {
                ComposerKeyAction::Edited
            } else {
                ComposerKeyAction::Ignored
            }
        }
        Key::Character(text) if !control && !shift => {
            let mut changed = false;
            for ch in text.chars() {
                if ch == '\r' {
                    buffer.push('\n');
                    changed = true;
                } else if !ch.is_control() {
                    buffer.push(ch);
                    changed = true;
                }
            }
            if changed {
                ComposerKeyAction::Edited
            } else {
                ComposerKeyAction::Ignored
            }
        }
        _ => ComposerKeyAction::Ignored,
    }
}

/// Maps a winit key event to bytes suitable for PTY input.
///
/// Returns `None` for keys that should not be forwarded (modifiers-only, arrows, etc.).
pub(super) fn key_event_to_bytes(event: &KeyEvent) -> Option<Vec<u8>> {
    if event.state != ElementState::Pressed {
        return None;
    }
    if event.repeat {
        return None;
    }

    match &event.logical_key {
        Key::Named(NamedKey::Enter) => Some(vec![b'\r']),
        Key::Named(NamedKey::Backspace) => Some(vec![0x7F]),
        Key::Named(NamedKey::Tab) => Some(vec![b'\t']),
        Key::Character(text) => {
            let mut bytes = Vec::new();
            for ch in text.chars() {
                if ch.is_ascii() && !ch.is_ascii_control() {
                    bytes.push(ch as u8);
                }
            }
            if bytes.is_empty() {
                None
            } else {
                Some(bytes)
            }
        }
        Key::Unidentified(_) => match event.physical_key {
            PhysicalKey::Code(code) => physical_code_to_byte(code),
            PhysicalKey::Unidentified(_) => None,
        },
        Key::Named(_) | Key::Dead(_) => None,
    }
}

fn physical_code_to_byte(code: winit::keyboard::KeyCode) -> Option<Vec<u8>> {
    use winit::keyboard::KeyCode;
    match code {
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7F]),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::Space => Some(vec![b' ']),
        KeyCode::KeyA => Some(vec![b'a']),
        KeyCode::KeyB => Some(vec![b'b']),
        KeyCode::KeyC => Some(vec![b'c']),
        KeyCode::KeyD => Some(vec![b'd']),
        KeyCode::KeyE => Some(vec![b'e']),
        KeyCode::KeyF => Some(vec![b'f']),
        KeyCode::KeyG => Some(vec![b'g']),
        KeyCode::KeyH => Some(vec![b'h']),
        KeyCode::KeyI => Some(vec![b'i']),
        KeyCode::KeyJ => Some(vec![b'j']),
        KeyCode::KeyK => Some(vec![b'k']),
        KeyCode::KeyL => Some(vec![b'l']),
        KeyCode::KeyM => Some(vec![b'm']),
        KeyCode::KeyN => Some(vec![b'n']),
        KeyCode::KeyO => Some(vec![b'o']),
        KeyCode::KeyP => Some(vec![b'p']),
        KeyCode::KeyQ => Some(vec![b'q']),
        KeyCode::KeyR => Some(vec![b'r']),
        KeyCode::KeyS => Some(vec![b's']),
        KeyCode::KeyT => Some(vec![b't']),
        KeyCode::KeyU => Some(vec![b'u']),
        KeyCode::KeyV => Some(vec![b'v']),
        KeyCode::KeyW => Some(vec![b'w']),
        KeyCode::KeyX => Some(vec![b'x']),
        KeyCode::KeyY => Some(vec![b'y']),
        KeyCode::KeyZ => Some(vec![b'z']),
        KeyCode::Digit0 => Some(vec![b'0']),
        KeyCode::Digit1 => Some(vec![b'1']),
        KeyCode::Digit2 => Some(vec![b'2']),
        KeyCode::Digit3 => Some(vec![b'3']),
        KeyCode::Digit4 => Some(vec![b'4']),
        KeyCode::Digit5 => Some(vec![b'5']),
        KeyCode::Digit6 => Some(vec![b'6']),
        KeyCode::Digit7 => Some(vec![b'7']),
        KeyCode::Digit8 => Some(vec![b'8']),
        KeyCode::Digit9 => Some(vec![b'9']),
        _ => None,
    }
}
