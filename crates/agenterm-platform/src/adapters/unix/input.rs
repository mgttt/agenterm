//! Winit key-event normalization shared by Linux and macOS adapters.

use crate::{
    contract::input::{
        KeyPressState, LogicalKey, ModifierState, NamedKey, NormalizedKeyEvent, PhysicalKeyCode,
    },
    input::{NativeKeyEventExt, NativeModifierStateExt},
};

impl NativeModifierStateExt for winit::keyboard::ModifiersState {
    fn to_platform_modifiers(&self) -> ModifierState {
        crate::input::modifiers(
            self.control_key(),
            self.shift_key(),
            self.alt_key(),
            self.super_key(),
        )
    }
}
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{Key, KeyCode, NamedKey as WinitNamedKey, PhysicalKey},
};

impl NativeKeyEventExt for KeyEvent {
    fn to_normalized_key_event(&self, modifiers: ModifierState) -> NormalizedKeyEvent {
        NormalizedKeyEvent {
            logical: match &self.logical_key {
                Key::Character(text) => LogicalKey::Character(text.to_string()),
                Key::Named(key) => named_key(*key)
                    .map(LogicalKey::Named)
                    .unwrap_or(LogicalKey::Unidentified),
                _ => LogicalKey::Unidentified,
            },
            physical: match self.physical_key {
                PhysicalKey::Code(code) => physical_key(code),
                _ => PhysicalKeyCode::Other,
            },
            text: self.text.as_ref().map(ToString::to_string),
            state: match self.state {
                ElementState::Pressed => KeyPressState::Pressed,
                ElementState::Released => KeyPressState::Released,
            },
            repeat: self.repeat,
            modifiers,
        }
    }
}

fn named_key(key: WinitNamedKey) -> Option<NamedKey> {
    Some(match key {
        WinitNamedKey::ArrowDown => NamedKey::ArrowDown,
        WinitNamedKey::ArrowLeft => NamedKey::ArrowLeft,
        WinitNamedKey::ArrowRight => NamedKey::ArrowRight,
        WinitNamedKey::ArrowUp => NamedKey::ArrowUp,
        WinitNamedKey::Backspace => NamedKey::Backspace,
        WinitNamedKey::Delete => NamedKey::Delete,
        WinitNamedKey::End => NamedKey::End,
        WinitNamedKey::Enter => NamedKey::Enter,
        WinitNamedKey::Escape => NamedKey::Escape,
        WinitNamedKey::F1 => NamedKey::F1,
        WinitNamedKey::F2 => NamedKey::F2,
        WinitNamedKey::F3 => NamedKey::F3,
        WinitNamedKey::F4 => NamedKey::F4,
        WinitNamedKey::F5 => NamedKey::F5,
        WinitNamedKey::F6 => NamedKey::F6,
        WinitNamedKey::F7 => NamedKey::F7,
        WinitNamedKey::F8 => NamedKey::F8,
        WinitNamedKey::F9 => NamedKey::F9,
        WinitNamedKey::F10 => NamedKey::F10,
        WinitNamedKey::F11 => NamedKey::F11,
        WinitNamedKey::F12 => NamedKey::F12,
        WinitNamedKey::Home => NamedKey::Home,
        WinitNamedKey::Insert => NamedKey::Insert,
        WinitNamedKey::PageDown => NamedKey::PageDown,
        WinitNamedKey::PageUp => NamedKey::PageUp,
        WinitNamedKey::Space => NamedKey::Space,
        WinitNamedKey::Tab => NamedKey::Tab,
        _ => return None,
    })
}

fn physical_key(key: KeyCode) -> PhysicalKeyCode {
    match key {
        KeyCode::KeyA => PhysicalKeyCode::Letter('A'),
        KeyCode::KeyB => PhysicalKeyCode::Letter('B'),
        KeyCode::KeyC => PhysicalKeyCode::Letter('C'),
        KeyCode::KeyD => PhysicalKeyCode::Letter('D'),
        KeyCode::KeyE => PhysicalKeyCode::Letter('E'),
        KeyCode::KeyF => PhysicalKeyCode::Letter('F'),
        KeyCode::KeyG => PhysicalKeyCode::Letter('G'),
        KeyCode::KeyH => PhysicalKeyCode::Letter('H'),
        KeyCode::KeyI => PhysicalKeyCode::Letter('I'),
        KeyCode::KeyJ => PhysicalKeyCode::Letter('J'),
        KeyCode::KeyK => PhysicalKeyCode::Letter('K'),
        KeyCode::KeyL => PhysicalKeyCode::Letter('L'),
        KeyCode::KeyM => PhysicalKeyCode::Letter('M'),
        KeyCode::KeyN => PhysicalKeyCode::Letter('N'),
        KeyCode::KeyO => PhysicalKeyCode::Letter('O'),
        KeyCode::KeyP => PhysicalKeyCode::Letter('P'),
        KeyCode::KeyQ => PhysicalKeyCode::Letter('Q'),
        KeyCode::KeyR => PhysicalKeyCode::Letter('R'),
        KeyCode::KeyS => PhysicalKeyCode::Letter('S'),
        KeyCode::KeyT => PhysicalKeyCode::Letter('T'),
        KeyCode::KeyU => PhysicalKeyCode::Letter('U'),
        KeyCode::KeyV => PhysicalKeyCode::Letter('V'),
        KeyCode::KeyW => PhysicalKeyCode::Letter('W'),
        KeyCode::KeyX => PhysicalKeyCode::Letter('X'),
        KeyCode::KeyY => PhysicalKeyCode::Letter('Y'),
        KeyCode::KeyZ => PhysicalKeyCode::Letter('Z'),
        KeyCode::Digit0 => PhysicalKeyCode::Digit(0),
        KeyCode::Digit1 => PhysicalKeyCode::Digit(1),
        KeyCode::Digit2 => PhysicalKeyCode::Digit(2),
        KeyCode::Digit3 => PhysicalKeyCode::Digit(3),
        KeyCode::Digit4 => PhysicalKeyCode::Digit(4),
        KeyCode::Digit5 => PhysicalKeyCode::Digit(5),
        KeyCode::Digit6 => PhysicalKeyCode::Digit(6),
        KeyCode::Digit7 => PhysicalKeyCode::Digit(7),
        KeyCode::Digit8 => PhysicalKeyCode::Digit(8),
        KeyCode::Digit9 => PhysicalKeyCode::Digit(9),
        KeyCode::Backspace => PhysicalKeyCode::Backspace,
        KeyCode::Enter => PhysicalKeyCode::Enter,
        KeyCode::Space => PhysicalKeyCode::Space,
        KeyCode::Tab => PhysicalKeyCode::Tab,
        _ => PhysicalKeyCode::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_tab_components_normalize_without_native_types() {
        assert_eq!(named_key(WinitNamedKey::Tab), Some(NamedKey::Tab));
        assert_eq!(physical_key(KeyCode::Tab), PhysicalKeyCode::Tab);
    }

    #[test]
    fn physical_letters_and_digits_keep_stable_identity() {
        assert_eq!(physical_key(KeyCode::KeyC), PhysicalKeyCode::Letter('C'));
        assert_eq!(physical_key(KeyCode::Digit7), PhysicalKeyCode::Digit(7));
    }
}
