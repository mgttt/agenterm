use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};

use crate::commands::tmux_key_bytes;

/// Result of handling a key in composer focus.
pub(super) enum ComposerKeyAction {
    /// Text changed; redraw only.
    Edited,
    /// Submit the draft to the active tab.
    Submit,
    /// Return focus to the terminal without submitting.
    Escape,
    /// Copy the composer buffer to the clipboard.
    Copy,
    /// Cut the composer buffer to the clipboard.
    Cut,
    /// Paste clipboard text into the composer buffer.
    Paste,
    /// Select all composer text (no visible selection chrome on Unix yet).
    SelectAll,
    /// Ignore this key.
    Ignored,
}

/// Filters platform input-method commits before they reach an editable surface.
pub(super) fn normalize_ime_commit(text: &str, multiline: bool) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|ch| {
            (!ch.is_control() && *ch != '\u{7f}') || (multiline && matches!(ch, '\n' | '\r'))
        })
        .map(|ch| if ch == '\r' { '\n' } else { ch })
        .collect()
}

pub(super) fn primary_shortcut(modifiers: ModifiersState) -> bool {
    modifiers.control_key() || modifiers.super_key()
}

/// Maps a winit key event to composer edits when the composer strip has focus.
pub(super) fn composer_key_action(
    event: &KeyEvent,
    modifiers: ModifiersState,
    buffer: &mut String,
    select_all: &mut bool,
) -> ComposerKeyAction {
    if event.state != ElementState::Pressed || event.repeat {
        return ComposerKeyAction::Ignored;
    }

    composer_logical_key_action(&event.logical_key, modifiers, buffer, select_all)
}

fn composer_logical_key_action(
    logical_key: &Key,
    modifiers: ModifiersState,
    buffer: &mut String,
    select_all: &mut bool,
) -> ComposerKeyAction {
    let shortcut = primary_shortcut(modifiers);

    match logical_key {
        Key::Named(NamedKey::Enter) if shortcut => ComposerKeyAction::Submit,
        Key::Named(NamedKey::Escape) => ComposerKeyAction::Escape,
        Key::Character(text) if shortcut => {
            if text.eq_ignore_ascii_case("a") {
                ComposerKeyAction::SelectAll
            } else if text.eq_ignore_ascii_case("c") {
                ComposerKeyAction::Copy
            } else if text.eq_ignore_ascii_case("x") {
                ComposerKeyAction::Cut
            } else if text.eq_ignore_ascii_case("v") {
                ComposerKeyAction::Paste
            } else {
                ComposerKeyAction::Ignored
            }
        }
        Key::Named(NamedKey::Enter) => {
            prepare_composer_edit(buffer, select_all);
            buffer.push('\n');
            ComposerKeyAction::Edited
        }
        Key::Named(NamedKey::Backspace) => {
            if prepare_composer_edit(buffer, select_all) {
                return ComposerKeyAction::Edited;
            }
            if buffer.pop().is_some() {
                ComposerKeyAction::Edited
            } else {
                ComposerKeyAction::Ignored
            }
        }
        Key::Named(NamedKey::Space) if !shortcut => {
            prepare_composer_edit(buffer, select_all);
            buffer.push(' ');
            ComposerKeyAction::Edited
        }
        Key::Character(text) if !shortcut => {
            let replaced = prepare_composer_edit(buffer, select_all);
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
            if changed || replaced {
                ComposerKeyAction::Edited
            } else {
                ComposerKeyAction::Ignored
            }
        }
        _ => ComposerKeyAction::Ignored,
    }
}

pub(super) fn prepare_composer_edit(buffer: &mut String, select_all: &mut bool) -> bool {
    if !std::mem::take(select_all) {
        return false;
    }
    let changed = !buffer.is_empty();
    buffer.clear();
    changed
}

/// Result of handling a key in a single-line sidebar tab editor field.
pub(super) enum TextFieldKeyAction {
    Edited,
    NextField,
    Submit,
    Escape,
    Copy,
    Cut,
    Paste,
    Ignored,
}

/// Maps a winit key event to inline tab-editor field edits.
pub(super) fn text_field_key_action(
    event: &KeyEvent,
    modifiers: ModifiersState,
    buffer: &mut String,
    multiline: bool,
) -> TextFieldKeyAction {
    if event.state != ElementState::Pressed || event.repeat {
        return TextFieldKeyAction::Ignored;
    }

    let shortcut = primary_shortcut(modifiers);

    match &event.logical_key {
        Key::Named(NamedKey::Enter) if shortcut => TextFieldKeyAction::Submit,
        Key::Named(NamedKey::Enter) if multiline => {
            buffer.push('\n');
            TextFieldKeyAction::Edited
        }
        Key::Named(NamedKey::Enter) => TextFieldKeyAction::NextField,
        Key::Named(NamedKey::Escape) => TextFieldKeyAction::Escape,
        Key::Character(text) if shortcut => {
            if text.eq_ignore_ascii_case("c") {
                TextFieldKeyAction::Copy
            } else if text.eq_ignore_ascii_case("x") {
                TextFieldKeyAction::Cut
            } else if text.eq_ignore_ascii_case("v") {
                TextFieldKeyAction::Paste
            } else {
                TextFieldKeyAction::Ignored
            }
        }
        Key::Named(NamedKey::Backspace) => {
            if buffer.pop().is_some() {
                TextFieldKeyAction::Edited
            } else {
                TextFieldKeyAction::Ignored
            }
        }
        Key::Named(NamedKey::Space) if !shortcut => {
            buffer.push(' ');
            TextFieldKeyAction::Edited
        }
        Key::Character(text) if !shortcut => {
            let mut changed = false;
            for ch in text.chars() {
                if ch == '\r' {
                    if multiline {
                        buffer.push('\n');
                        changed = true;
                    }
                } else if !ch.is_control() {
                    buffer.push(ch);
                    changed = true;
                }
            }
            if changed {
                TextFieldKeyAction::Edited
            } else {
                TextFieldKeyAction::Ignored
            }
        }
        _ => TextFieldKeyAction::Ignored,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TerminalShortcutAction {
    Copy,
    Paste,
    Suppress,
    Forward,
}

pub(super) fn terminal_shortcut_action(
    key: &Key,
    modifiers: ModifiersState,
    has_selection: bool,
) -> TerminalShortcutAction {
    let Key::Character(text) = key else {
        return TerminalShortcutAction::Forward;
    };
    if !primary_shortcut(modifiers) {
        return TerminalShortcutAction::Forward;
    }
    if text.eq_ignore_ascii_case("c") {
        if has_selection {
            TerminalShortcutAction::Copy
        } else if modifiers.super_key() {
            TerminalShortcutAction::Suppress
        } else {
            TerminalShortcutAction::Forward
        }
    } else if text.eq_ignore_ascii_case("v") {
        TerminalShortcutAction::Paste
    } else if modifiers.super_key() {
        TerminalShortcutAction::Suppress
    } else {
        TerminalShortcutAction::Forward
    }
}

/// Maps a winit key event to bytes suitable for PTY input.
///
/// Returns `None` for keys that should not be forwarded (modifiers-only, dead keys, etc.).
pub(super) fn key_event_to_bytes(event: &KeyEvent, modifiers: ModifiersState) -> Option<Vec<u8>> {
    if event.state != ElementState::Pressed {
        return None;
    }

    logical_key_to_bytes(&event.logical_key, modifiers).or_else(|| match &event.logical_key {
        Key::Unidentified(_) => match event.physical_key {
            PhysicalKey::Code(code) => physical_code_to_byte(code),
            PhysicalKey::Unidentified(_) => None,
        },
        _ => None,
    })
}

fn logical_key_to_bytes(key: &Key, modifiers: ModifiersState) -> Option<Vec<u8>> {
    match key {
        Key::Named(named) => named_key_name(*named).and_then(tmux_key_bytes),
        Key::Character(text) if modifiers.control_key() => {
            let mut characters = text.chars();
            let character = characters.next()?;
            if characters.next().is_some() {
                return None;
            }
            tmux_key_bytes(&format!("C-{character}"))
        }
        Key::Character(text) => {
            let bytes = text
                .chars()
                .filter(|character| !character.is_control())
                .collect::<String>()
                .into_bytes();
            (!bytes.is_empty()).then_some(bytes)
        }
        Key::Unidentified(_) | Key::Dead(_) => None,
    }
}

fn named_key_name(key: NamedKey) -> Option<&'static str> {
    match key {
        NamedKey::Enter => Some("Enter"),
        NamedKey::Escape => Some("Escape"),
        NamedKey::Backspace => Some("Backspace"),
        NamedKey::Space => Some("Space"),
        NamedKey::Tab => Some("Tab"),
        NamedKey::ArrowUp => Some("Up"),
        NamedKey::ArrowDown => Some("Down"),
        NamedKey::ArrowRight => Some("Right"),
        NamedKey::ArrowLeft => Some("Left"),
        NamedKey::Home => Some("Home"),
        NamedKey::End => Some("End"),
        NamedKey::Delete => Some("Delete"),
        NamedKey::PageUp => Some("PageUp"),
        NamedKey::PageDown => Some("PageDown"),
        NamedKey::F1 => Some("F1"),
        NamedKey::F2 => Some("F2"),
        NamedKey::F3 => Some("F3"),
        NamedKey::F4 => Some("F4"),
        NamedKey::F5 => Some("F5"),
        NamedKey::F6 => Some("F6"),
        NamedKey::F7 => Some("F7"),
        NamedKey::F8 => Some("F8"),
        NamedKey::F9 => Some("F9"),
        NamedKey::F10 => Some("F10"),
        NamedKey::F11 => Some("F11"),
        NamedKey::F12 => Some("F12"),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::{
        ComposerKeyAction, TerminalShortcutAction, composer_logical_key_action,
        logical_key_to_bytes, normalize_ime_commit, physical_code_to_byte, prepare_composer_edit,
        primary_shortcut, terminal_shortcut_action,
    };
    use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey};

    #[test]
    fn terminal_text_is_forwarded_as_utf8() {
        assert_eq!(
            logical_key_to_bytes(&Key::Character("终端".into()), ModifiersState::empty()),
            Some("终端".as_bytes().to_vec())
        );
        assert_eq!(
            logical_key_to_bytes(
                &Key::Character("a\u{0007}b".into()),
                ModifiersState::empty()
            ),
            Some(b"ab".to_vec())
        );
    }

    #[test]
    fn space_is_forwarded_for_named_and_physical_key_forms() {
        assert_eq!(
            logical_key_to_bytes(&Key::Named(NamedKey::Space), ModifiersState::empty()),
            Some(vec![b' '])
        );
        assert_eq!(physical_code_to_byte(KeyCode::Space), Some(vec![b' ']));
    }

    #[test]
    fn composer_accepts_shifted_characters() {
        let mut buffer = String::new();
        let mut select_all = false;

        assert!(matches!(
            composer_logical_key_action(
                &Key::Character("A".into()),
                ModifiersState::SHIFT,
                &mut buffer,
                &mut select_all,
            ),
            ComposerKeyAction::Edited
        ));
        assert!(matches!(
            composer_logical_key_action(
                &Key::Character("!".into()),
                ModifiersState::SHIFT,
                &mut buffer,
                &mut select_all,
            ),
            ComposerKeyAction::Edited
        ));
        assert_eq!(buffer, "A!");
    }

    #[test]
    fn ime_commit_preserves_unicode_and_filters_controls() {
        assert_eq!(
            normalize_ime_commit("中文🙂\0\u{7f}\n下一行", false),
            "中文🙂下一行"
        );
        assert_eq!(normalize_ime_commit("中文\r\n下一行", true), "中文\n下一行");
    }

    #[test]
    fn terminal_control_letters_use_control_bytes() {
        assert_eq!(
            logical_key_to_bytes(&Key::Character("c".into()), ModifiersState::CONTROL),
            Some(vec![3])
        );
        assert_eq!(
            logical_key_to_bytes(&Key::Character("Z".into()), ModifiersState::CONTROL),
            Some(vec![26])
        );
        assert_eq!(
            logical_key_to_bytes(&Key::Character("终端".into()), ModifiersState::CONTROL),
            None
        );
    }

    #[test]
    fn primary_shortcut_accepts_control_and_command() {
        assert!(primary_shortcut(ModifiersState::CONTROL));
        assert!(primary_shortcut(ModifiersState::SUPER));
        assert!(!primary_shortcut(ModifiersState::ALT));
    }

    #[test]
    fn selected_composer_text_is_replaced_once() {
        let mut buffer = String::from("existing draft");
        let mut select_all = true;
        assert!(prepare_composer_edit(&mut buffer, &mut select_all));
        assert!(buffer.is_empty());
        assert!(!select_all);

        buffer.push_str("replacement");
        assert!(!prepare_composer_edit(&mut buffer, &mut select_all));
        assert_eq!(buffer, "replacement");
    }

    #[test]
    fn terminal_clipboard_shortcuts_preserve_interrupt_semantics() {
        let c = Key::Character("c".into());
        let v = Key::Character("v".into());
        assert_eq!(
            terminal_shortcut_action(&c, ModifiersState::CONTROL, false),
            TerminalShortcutAction::Forward
        );
        assert_eq!(
            terminal_shortcut_action(&c, ModifiersState::CONTROL, true),
            TerminalShortcutAction::Copy
        );
        assert_eq!(
            terminal_shortcut_action(&c, ModifiersState::SUPER, false),
            TerminalShortcutAction::Suppress
        );
        assert_eq!(
            terminal_shortcut_action(&v, ModifiersState::SUPER, false),
            TerminalShortcutAction::Paste
        );
    }

    #[test]
    fn terminal_navigation_uses_shared_key_protocol() {
        let cases = [
            (NamedKey::ArrowUp, b"\x1b[A".as_slice()),
            (NamedKey::ArrowDown, b"\x1b[B".as_slice()),
            (NamedKey::ArrowRight, b"\x1b[C".as_slice()),
            (NamedKey::ArrowLeft, b"\x1b[D".as_slice()),
            (NamedKey::Home, b"\x1b[H".as_slice()),
            (NamedKey::End, b"\x1b[F".as_slice()),
            (NamedKey::Delete, b"\x1b[3~".as_slice()),
            (NamedKey::PageUp, b"\x1b[5~".as_slice()),
            (NamedKey::PageDown, b"\x1b[6~".as_slice()),
            (NamedKey::F12, b"\x1b[24~".as_slice()),
        ];
        for (key, expected) in cases {
            assert_eq!(
                logical_key_to_bytes(&Key::Named(key), ModifiersState::empty()).as_deref(),
                Some(expected)
            );
        }
    }
}
