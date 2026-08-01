//! Unix frontend native input translation.

use agenterm_platform::input::{
    KeyPressState, LogicalKey as Key, ModifierState, NamedKey, NormalizedKeyEvent as KeyEvent,
    PhysicalKeyCode,
};

use crate::commands::{tmux_key_bytes, tmux_key_bytes_with_modifiers};

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
    /// Select all composer text.
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

pub(super) fn primary_shortcut(modifiers: ModifierState) -> bool {
    agenterm_platform::input::is_primary_shortcut(modifiers)
}

/// Maps a normalized key event to composer edits when the composer strip has focus.
pub(super) fn composer_key_action(
    event: &KeyEvent,
    buffer: &mut String,
    select_all: &mut bool,
) -> ComposerKeyAction {
    if event.state != KeyPressState::Pressed || event.repeat {
        return ComposerKeyAction::Ignored;
    }

    platform_composer_key_action(event, buffer, select_all)
}

fn composer_logical_key_action(
    logical_key: &Key,
    modifiers: ModifierState,
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
    SelectAll,
    Copy,
    Cut,
    Paste,
    Ignored,
}

/// Maps a normalized key event to inline tab-editor field edits.
pub(super) fn text_field_key_action(
    event: &KeyEvent,
    buffer: &mut String,
    multiline: bool,
    select_all: &mut bool,
) -> TextFieldKeyAction {
    if event.state != KeyPressState::Pressed || event.repeat {
        return TextFieldKeyAction::Ignored;
    }

    platform_text_field_key_action(event, buffer, multiline, select_all)
}

fn text_field_logical_key_action(
    logical_key: &Key,
    modifiers: ModifierState,
    buffer: &mut String,
    multiline: bool,
    select_all: &mut bool,
) -> TextFieldKeyAction {
    let shortcut = primary_shortcut(modifiers);

    match logical_key {
        Key::Named(NamedKey::Enter) if shortcut => TextFieldKeyAction::Submit,
        Key::Named(NamedKey::Enter) if multiline => {
            prepare_composer_edit(buffer, select_all);
            buffer.push('\n');
            TextFieldKeyAction::Edited
        }
        Key::Named(NamedKey::Enter) => TextFieldKeyAction::NextField,
        Key::Named(NamedKey::Escape) => TextFieldKeyAction::Escape,
        Key::Character(text) if shortcut => {
            if text.eq_ignore_ascii_case("a") {
                *select_all = !buffer.is_empty();
                TextFieldKeyAction::SelectAll
            } else if text.eq_ignore_ascii_case("c") {
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
            if prepare_composer_edit(buffer, select_all) {
                return TextFieldKeyAction::Edited;
            }
            if buffer.pop().is_some() {
                TextFieldKeyAction::Edited
            } else {
                TextFieldKeyAction::Ignored
            }
        }
        Key::Named(NamedKey::Space) if !shortcut => {
            prepare_composer_edit(buffer, select_all);
            buffer.push(' ');
            TextFieldKeyAction::Edited
        }
        Key::Character(text) if !shortcut => {
            let replaced = prepare_composer_edit(buffer, select_all);
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
            if changed || replaced {
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
    modifiers: ModifierState,
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
        } else if modifiers.meta {
            TerminalShortcutAction::Suppress
        } else {
            TerminalShortcutAction::Forward
        }
    } else if text.eq_ignore_ascii_case("v") {
        TerminalShortcutAction::Paste
    } else if modifiers.meta {
        TerminalShortcutAction::Suppress
    } else {
        TerminalShortcutAction::Forward
    }
}

/// Maps a normalized key event to bytes suitable for PTY input.
///
/// Returns `None` for keys that should not be forwarded (modifiers-only, dead keys, etc.).
pub(super) fn key_event_to_bytes(event: &KeyEvent) -> Option<Vec<u8>> {
    if event.state != KeyPressState::Pressed {
        return None;
    }
    platform_key_event_to_bytes(event)
}

fn logical_key_to_bytes(key: &Key, modifiers: ModifierState) -> Option<Vec<u8>> {
    match key {
        Key::Named(named) => {
            named_key_name(*named).and_then(|name| tmux_key_bytes_with_modifiers(name, modifiers))
        }
        Key::Character(text) if modifiers.control => {
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
        Key::Unidentified => None,
        _ => None,
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
        NamedKey::Insert => Some("Insert"),
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

fn physical_code_to_byte(code: PhysicalKeyCode) -> Option<Vec<u8>> {
    match code {
        PhysicalKeyCode::Enter => Some(vec![b'\r']),
        PhysicalKeyCode::Backspace => Some(vec![0x7F]),
        PhysicalKeyCode::Tab => Some(vec![b'\t']),
        PhysicalKeyCode::Space => Some(vec![b' ']),
        PhysicalKeyCode::Letter(letter) if letter.is_ascii_alphabetic() => {
            Some(vec![letter.to_ascii_lowercase() as u8])
        }
        PhysicalKeyCode::Digit(digit) if digit <= 9 => Some(vec![b'0' + digit]),
        _ => None,
    }
}

fn logical_key_parts(key: &Key) -> (Option<&str>, Option<&'static str>) {
    match key {
        Key::Character(text) => (Some(text.as_str()), None),
        Key::Named(named) => (None, named_key_name(*named)),
        Key::Unidentified => (None, None),
        _ => (None, None),
    }
}

fn platform_classify_key_press(event: &KeyEvent) -> crate::platform::KeyClassification {
    let (logical_character, named_key) = logical_key_parts(&event.logical);
    agenterm_platform::input::classify_key_press(
        event.modifiers,
        logical_character,
        named_key,
        event.text.as_deref(),
    )
}

fn classified_product_shortcut(modifiers: crate::platform::ModifierState) -> bool {
    agenterm_platform::input::is_primary_shortcut(modifiers)
}

fn platform_composer_shortcut(key: &str) -> ComposerKeyAction {
    if key.eq_ignore_ascii_case("Enter") {
        ComposerKeyAction::Submit
    } else if key.eq_ignore_ascii_case("a") {
        ComposerKeyAction::SelectAll
    } else if key.eq_ignore_ascii_case("c") {
        ComposerKeyAction::Copy
    } else if key.eq_ignore_ascii_case("x") {
        ComposerKeyAction::Cut
    } else if key.eq_ignore_ascii_case("v") {
        ComposerKeyAction::Paste
    } else {
        ComposerKeyAction::Ignored
    }
}

fn push_committed_text(buffer: &mut String, select_all: &mut bool, text: &str) -> bool {
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
    changed || replaced
}

fn platform_composer_key_action(
    event: &KeyEvent,
    buffer: &mut String,
    select_all: &mut bool,
) -> ComposerKeyAction {
    use crate::platform::KeyClassification;

    match platform_classify_key_press(event) {
        KeyClassification::Shortcut {
            key,
            modifiers: classified,
        } if classified_product_shortcut(classified) => platform_composer_shortcut(&key),
        KeyClassification::Shortcut { .. } => ComposerKeyAction::Ignored,
        KeyClassification::TextCommit(text) => {
            if push_committed_text(buffer, select_all, &text) {
                ComposerKeyAction::Edited
            } else {
                ComposerKeyAction::Ignored
            }
        }
        KeyClassification::ControlKey { name, .. } => match name.as_str() {
            "Enter" => {
                prepare_composer_edit(buffer, select_all);
                buffer.push('\n');
                ComposerKeyAction::Edited
            }
            "Escape" => ComposerKeyAction::Escape,
            "Backspace" => {
                if prepare_composer_edit(buffer, select_all) {
                    return ComposerKeyAction::Edited;
                }
                if buffer.pop().is_some() {
                    ComposerKeyAction::Edited
                } else {
                    ComposerKeyAction::Ignored
                }
            }
            "Space" => {
                prepare_composer_edit(buffer, select_all);
                buffer.push(' ');
                ComposerKeyAction::Edited
            }
            _ => ComposerKeyAction::Ignored,
        },
        KeyClassification::Ignored => ComposerKeyAction::Ignored,
        _ => ComposerKeyAction::Ignored,
    }
}

fn platform_text_field_shortcut(
    key: &str,
    buffer: &str,
    select_all: &mut bool,
) -> TextFieldKeyAction {
    if key.eq_ignore_ascii_case("Enter") {
        TextFieldKeyAction::Submit
    } else if key.eq_ignore_ascii_case("a") {
        *select_all = !buffer.is_empty();
        TextFieldKeyAction::SelectAll
    } else if key.eq_ignore_ascii_case("c") {
        TextFieldKeyAction::Copy
    } else if key.eq_ignore_ascii_case("x") {
        TextFieldKeyAction::Cut
    } else if key.eq_ignore_ascii_case("v") {
        TextFieldKeyAction::Paste
    } else {
        TextFieldKeyAction::Ignored
    }
}

fn platform_text_field_key_action(
    event: &KeyEvent,
    buffer: &mut String,
    multiline: bool,
    select_all: &mut bool,
) -> TextFieldKeyAction {
    use crate::platform::KeyClassification;

    match platform_classify_key_press(event) {
        KeyClassification::Shortcut {
            key,
            modifiers: classified,
        } if classified_product_shortcut(classified) => {
            platform_text_field_shortcut(&key, buffer, select_all)
        }
        KeyClassification::Shortcut { .. } => TextFieldKeyAction::Ignored,
        KeyClassification::TextCommit(text) => {
            let replaced = prepare_composer_edit(buffer, select_all);
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
            if changed || replaced {
                TextFieldKeyAction::Edited
            } else {
                TextFieldKeyAction::Ignored
            }
        }
        KeyClassification::ControlKey { name, .. } => match name.as_str() {
            "Enter" if multiline => {
                prepare_composer_edit(buffer, select_all);
                buffer.push('\n');
                TextFieldKeyAction::Edited
            }
            "Enter" => TextFieldKeyAction::NextField,
            "Escape" => TextFieldKeyAction::Escape,
            "Backspace" => {
                if prepare_composer_edit(buffer, select_all) {
                    return TextFieldKeyAction::Edited;
                }
                if buffer.pop().is_some() {
                    TextFieldKeyAction::Edited
                } else {
                    TextFieldKeyAction::Ignored
                }
            }
            "Space" => {
                prepare_composer_edit(buffer, select_all);
                buffer.push(' ');
                TextFieldKeyAction::Edited
            }
            _ => TextFieldKeyAction::Ignored,
        },
        KeyClassification::Ignored => TextFieldKeyAction::Ignored,
        _ => TextFieldKeyAction::Ignored,
    }
}

fn platform_key_event_to_bytes(event: &KeyEvent) -> Option<Vec<u8>> {
    use crate::platform::KeyClassification;

    if event.modifiers.control
        && let Some(bytes) = terminal_control_key_bytes(&event.logical, event.physical)
    {
        return Some(bytes);
    }

    match platform_classify_key_press(event) {
        KeyClassification::Shortcut {
            key,
            modifiers: classified,
        } => {
            if !classified.control {
                return None;
            }
            let mut characters = key.chars();
            let character = characters.next()?;
            if characters.next().is_some() {
                return None;
            }
            tmux_key_bytes(&format!("C-{character}"))
        }
        KeyClassification::TextCommit(text) => {
            let bytes = text
                .chars()
                .filter(|character| !character.is_control())
                .collect::<String>()
                .into_bytes();
            (!bytes.is_empty()).then_some(bytes)
        }
        KeyClassification::ControlKey { name, modifiers } => {
            tmux_key_bytes_with_modifiers(&name, modifiers)
        }
        KeyClassification::Ignored if matches!(event.logical, Key::Unidentified) => {
            physical_code_to_byte(event.physical)
        }
        KeyClassification::Ignored => None,
        _ => None,
    }
}

fn terminal_control_key_bytes(logical: &Key, physical: PhysicalKeyCode) -> Option<Vec<u8>> {
    let logical_character = match logical {
        Key::Character(text) => {
            let mut characters = text.chars();
            let character = characters.next()?;
            (characters.next().is_none() && !character.is_control()).then_some(character)
        }
        _ => None,
    };
    let character = logical_character.or_else(|| match physical {
        PhysicalKeyCode::Letter(letter) if letter.is_ascii_alphabetic() => Some(letter),
        PhysicalKeyCode::Digit(digit) if digit <= 9 => Some(char::from(b'0' + digit)),
        _ => None,
    })?;
    tmux_key_bytes(&format!("C-{character}"))
}

#[cfg(test)]
mod tests {
    use super::{
        ComposerKeyAction, TerminalShortcutAction, composer_logical_key_action, key_event_to_bytes,
        logical_key_to_bytes, normalize_ime_commit, physical_code_to_byte, prepare_composer_edit,
        primary_shortcut, terminal_shortcut_action, text_field_logical_key_action,
    };
    use agenterm_platform::input::{
        KeyPressState, LogicalKey as Key, ModifierState, NamedKey, NormalizedKeyEvent,
        PhysicalKeyCode,
    };

    const fn modifiers(control: bool, shift: bool, alt: bool, meta: bool) -> ModifierState {
        ModifierState {
            control,
            shift,
            alt,
            meta,
        }
    }

    #[test]
    fn terminal_text_is_forwarded_as_utf8() {
        assert_eq!(
            logical_key_to_bytes(&Key::Character("终端".into()), ModifierState::empty()),
            Some("终端".as_bytes().to_vec())
        );
        assert_eq!(
            logical_key_to_bytes(&Key::Character("a\u{0007}b".into()), ModifierState::empty()),
            Some(b"ab".to_vec())
        );
    }

    #[test]
    fn space_is_forwarded_for_named_and_physical_key_forms() {
        assert_eq!(
            logical_key_to_bytes(&Key::Named(NamedKey::Space), ModifierState::empty()),
            Some(vec![b' '])
        );
        assert_eq!(
            physical_code_to_byte(PhysicalKeyCode::Space),
            Some(vec![b' '])
        );
    }

    #[test]
    fn composer_accepts_shifted_characters() {
        let mut buffer = String::new();
        let mut select_all = false;

        assert!(matches!(
            composer_logical_key_action(
                &Key::Character("A".into()),
                modifiers(false, true, false, false),
                &mut buffer,
                &mut select_all,
            ),
            ComposerKeyAction::Edited
        ));
        assert!(matches!(
            composer_logical_key_action(
                &Key::Character("!".into()),
                modifiers(false, true, false, false),
                &mut buffer,
                &mut select_all,
            ),
            ComposerKeyAction::Edited
        ));
        assert_eq!(buffer, "A!");
    }

    #[test]
    fn text_field_select_all_replaces_the_next_edit_once() {
        let mut buffer = String::from("existing");
        let mut select_all = false;
        let primary = match agenterm_platform::platform_kind() {
            agenterm_platform::PlatformKind::Macos => modifiers(false, false, false, true),
            _ => modifiers(true, false, false, false),
        };

        assert!(matches!(
            text_field_logical_key_action(
                &Key::Character("a".into()),
                primary,
                &mut buffer,
                false,
                &mut select_all,
            ),
            super::TextFieldKeyAction::SelectAll
        ));
        assert!(select_all);
        assert!(matches!(
            text_field_logical_key_action(
                &Key::Character("N".into()),
                modifiers(false, true, false, false),
                &mut buffer,
                false,
                &mut select_all,
            ),
            super::TextFieldKeyAction::Edited
        ));
        assert_eq!(buffer, "N");
        assert!(!select_all);

        assert!(matches!(
            text_field_logical_key_action(
                &Key::Named(NamedKey::Space),
                ModifierState::empty(),
                &mut buffer,
                false,
                &mut select_all,
            ),
            super::TextFieldKeyAction::Edited
        ));
        assert_eq!(buffer, "N ");
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
            logical_key_to_bytes(
                &Key::Character("c".into()),
                modifiers(true, false, false, false),
            ),
            Some(vec![3])
        );
        assert_eq!(
            logical_key_to_bytes(
                &Key::Character("Z".into()),
                modifiers(true, false, false, false),
            ),
            Some(vec![26])
        );
        assert_eq!(
            logical_key_to_bytes(
                &Key::Character("终端".into()),
                modifiers(true, false, false, false),
            ),
            None
        );
    }

    #[test]
    fn macos_control_text_does_not_swallow_terminal_keys() {
        let cases = [
            (
                Key::Named(NamedKey::Enter),
                PhysicalKeyCode::Enter,
                Some("\r"),
                ModifierState::empty(),
                b"\r".as_slice(),
            ),
            (
                Key::Named(NamedKey::Backspace),
                PhysicalKeyCode::Backspace,
                Some("\u{7f}"),
                ModifierState::empty(),
                b"\x7f".as_slice(),
            ),
            (
                Key::Named(NamedKey::Escape),
                PhysicalKeyCode::Other,
                Some("\u{1b}"),
                ModifierState::empty(),
                b"\x1b".as_slice(),
            ),
            (
                Key::Character("h".into()),
                PhysicalKeyCode::Letter('H'),
                Some("\u{8}"),
                modifiers(true, false, false, false),
                b"\x08".as_slice(),
            ),
        ];

        for (logical, physical, text, modifiers, expected) in cases {
            let event = NormalizedKeyEvent {
                logical,
                physical,
                text: text.map(str::to_owned),
                state: KeyPressState::Pressed,
                repeat: false,
                modifiers,
            };
            assert_eq!(key_event_to_bytes(&event).as_deref(), Some(expected));
        }
    }

    #[test]
    fn primary_shortcut_uses_platform_policy() {
        let control = primary_shortcut(modifiers(true, false, false, false));
        let meta = primary_shortcut(modifiers(false, false, false, true));
        match agenterm_platform::platform_kind() {
            agenterm_platform::PlatformKind::Windows => assert_eq!((control, meta), (true, false)),
            agenterm_platform::PlatformKind::Linux => assert_eq!((control, meta), (true, true)),
            agenterm_platform::PlatformKind::Macos => assert_eq!((control, meta), (false, true)),
            _ => panic!("unsupported test platform"),
        }
        assert!(!primary_shortcut(modifiers(false, false, true, false)));
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
            terminal_shortcut_action(&c, modifiers(true, false, false, false), false),
            TerminalShortcutAction::Forward
        );
        let (primary, empty_copy_action) = match agenterm_platform::platform_kind() {
            agenterm_platform::PlatformKind::Windows => (
                modifiers(true, false, false, false),
                TerminalShortcutAction::Forward,
            ),
            agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => (
                modifiers(false, false, false, true),
                TerminalShortcutAction::Suppress,
            ),
            _ => panic!("unsupported test platform"),
        };
        assert_eq!(
            terminal_shortcut_action(&c, primary, true),
            TerminalShortcutAction::Copy
        );
        assert_eq!(
            terminal_shortcut_action(&c, primary, false),
            empty_copy_action
        );
        assert_eq!(
            terminal_shortcut_action(&v, primary, false),
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
            (NamedKey::Insert, b"\x1b[2~".as_slice()),
            (NamedKey::Delete, b"\x1b[3~".as_slice()),
            (NamedKey::PageUp, b"\x1b[5~".as_slice()),
            (NamedKey::PageDown, b"\x1b[6~".as_slice()),
            (NamedKey::F12, b"\x1b[24~".as_slice()),
        ];
        for (key, expected) in cases {
            assert_eq!(
                logical_key_to_bytes(&Key::Named(key), ModifierState::empty()).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn terminal_modified_navigation_preserves_xterm_modifier_bytes() {
        let cases = [
            (
                NamedKey::Tab,
                modifiers(false, true, false, false),
                b"\x1b[Z".as_slice(),
            ),
            (
                NamedKey::ArrowLeft,
                modifiers(true, false, false, false),
                b"\x1b[1;5D".as_slice(),
            ),
            (
                NamedKey::Insert,
                modifiers(false, true, false, false),
                b"\x1b[2;2~".as_slice(),
            ),
            (
                NamedKey::F12,
                modifiers(true, true, false, false),
                b"\x1b[24;6~".as_slice(),
            ),
        ];
        for (key, modifiers, expected) in cases {
            assert_eq!(
                logical_key_to_bytes(&Key::Named(key), modifiers).as_deref(),
                Some(expected)
            );
        }
    }
}
