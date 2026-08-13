//! Product-neutral widget snapshot that a host can publish to the native
//! accessibility stack (Linux AT-SPI2). Callers own names, roles, and
//! client-relative bounds; the adapter owns bus registration.

use crate::contract::accessibility_tree::AccessibilityBounds;

/// Stable ids for a small published chrome tree. The adapter maps these onto
/// AT-SPI object paths; product code must keep them stable across updates.
pub const NODE_APPLICATION: u32 = 0;
pub const NODE_FRAME: u32 = 1;
pub const NODE_TABS: u32 = 2;
pub const NODE_SESSION: u32 = 3;
pub const NODE_COMMAND: u32 = 4;
pub const NODE_SEND: u32 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishedRole {
    Application,
    Frame,
    Panel,
    Terminal,
    Text,
    Button,
    Label,
}

impl PublishedRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Frame => "frame",
            Self::Panel => "panel",
            Self::Terminal => "terminal",
            Self::Text => "text",
            Self::Button => "button",
            Self::Label => "label",
        }
    }

    pub const fn atspi_role(self) -> u32 {
        match self {
            Self::Application => 75,
            Self::Frame => 23,
            Self::Panel => 39,
            Self::Terminal => 60,
            Self::Text => 61,
            Self::Button => 43,
            Self::Label => 29,
        }
    }
}

/// AT-SPI `DeviceEvent.modifiers` bits (`AtspiModifierType`).
pub const ATSPI_MOD_SHIFT: i32 = 1 << 0;
pub const ATSPI_MOD_CONTROL: i32 = 1 << 2;
pub const ATSPI_MOD_ALT: i32 = 1 << 3;
pub const ATSPI_MOD_META: i32 = 1 << 4;

/// X11 keysyms used as AT-SPI `DeviceEvent.id` for named keys.
pub const XK_BACKSPACE: i32 = 0xff08;
pub const XK_TAB: i32 = 0xff09;
pub const XK_RETURN: i32 = 0xff0d;
pub const XK_ESCAPE: i32 = 0xff1b;
pub const XK_SPACE: i32 = 0x0020;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedKey {
    pub keysym: i32,
    pub event_string: String,
    pub is_text: bool,
    pub modifiers: i32,
    pub pressed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyEffect {
    Ignore,
    Insert(String),
    Backspace,
    Submit,
    Cancel,
    SelectAll,
}

/// Map one AT-SPI Device/key event onto a published buffer. Release events
/// and modifier-only chords (except Ctrl+A select-all) are ignored.
pub fn published_key_effect(key: &PublishedKey) -> KeyEffect {
    if !key.pressed {
        return KeyEffect::Ignore;
    }
    let control = key.modifiers & ATSPI_MOD_CONTROL != 0;
    let alt = key.modifiers & ATSPI_MOD_ALT != 0;
    let meta = key.modifiers & ATSPI_MOD_META != 0;
    if control && !alt && !meta && key.event_string.eq_ignore_ascii_case("a") {
        return KeyEffect::SelectAll;
    }
    if control || alt || meta {
        return KeyEffect::Ignore;
    }
    let named = key.event_string.as_str();
    if key.keysym == XK_BACKSPACE || named.eq_ignore_ascii_case("BackSpace") {
        return KeyEffect::Backspace;
    }
    if key.keysym == XK_RETURN
        || named.eq_ignore_ascii_case("Return")
        || named.eq_ignore_ascii_case("Enter")
    {
        return KeyEffect::Submit;
    }
    if key.keysym == XK_ESCAPE || named.eq_ignore_ascii_case("Escape") {
        return KeyEffect::Cancel;
    }
    if key.keysym == XK_SPACE || named.eq_ignore_ascii_case("space") {
        return KeyEffect::Insert(" ".into());
    }
    if key.is_text && !key.event_string.is_empty() {
        return KeyEffect::Insert(key.event_string.clone());
    }
    KeyEffect::Ignore
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublishedAction {
    Click,
    Focus,
    SetText(String),
    Key(PublishedKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedNode {
    pub id: u32,
    pub parent: Option<u32>,
    pub role: PublishedRole,
    pub name: String,
    pub text: String,
    pub bounds: AccessibilityBounds,
    pub focusable: bool,
    pub focused: bool,
    pub editable: bool,
    pub clickable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedTree {
    pub app_name: String,
    pub nodes: Vec<PublishedNode>,
}

impl PublishedTree {
    pub fn node(&self, id: u32) -> Option<&PublishedNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn children_of(&self, id: u32) -> Vec<u32> {
        self.nodes
            .iter()
            .filter(|node| node.parent == Some(id))
            .map(|node| node.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_names_are_not_the_x11_frame_fallback() {
        assert_eq!(PublishedRole::Text.as_str(), "text");
        assert_eq!(PublishedRole::Button.as_str(), "button");
        assert_eq!(PublishedRole::Terminal.as_str(), "terminal");
        assert_ne!(PublishedRole::Text.as_str(), "frame");
        assert_ne!(PublishedRole::Button.as_str(), "application");
    }

    #[test]
    fn children_follow_declared_parent_links() {
        let tree = PublishedTree {
            app_name: "agenterm-con".into(),
            nodes: vec![
                PublishedNode {
                    id: NODE_APPLICATION,
                    parent: None,
                    role: PublishedRole::Application,
                    name: "agenterm-con".into(),
                    text: String::new(),
                    bounds: AccessibilityBounds {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 10,
                    },
                    focusable: false,
                    focused: false,
                    editable: false,
                    clickable: false,
                },
                PublishedNode {
                    id: NODE_FRAME,
                    parent: Some(NODE_APPLICATION),
                    role: PublishedRole::Frame,
                    name: "title".into(),
                    text: String::new(),
                    bounds: AccessibilityBounds {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 10,
                    },
                    focusable: true,
                    focused: false,
                    editable: false,
                    clickable: true,
                },
                PublishedNode {
                    id: NODE_COMMAND,
                    parent: Some(NODE_FRAME),
                    role: PublishedRole::Text,
                    name: "Command".into(),
                    text: "probe".into(),
                    bounds: AccessibilityBounds {
                        x: 1,
                        y: 1,
                        width: 4,
                        height: 2,
                    },
                    focusable: true,
                    focused: true,
                    editable: true,
                    clickable: true,
                },
            ],
        };
        assert_eq!(tree.children_of(NODE_APPLICATION), vec![NODE_FRAME]);
        assert_eq!(tree.children_of(NODE_FRAME), vec![NODE_COMMAND]);
        assert_eq!(
            tree.node(NODE_COMMAND).map(|node| node.name.as_str()),
            Some("Command")
        );
        assert_eq!(
            tree.node(NODE_COMMAND).map(|node| node.text.as_str()),
            Some("probe")
        );
    }

    #[test]
    fn device_key_effect_inserts_text_and_maps_named_keys() {
        let letter = PublishedKey {
            keysym: i32::from(b'k'),
            event_string: "k".into(),
            is_text: true,
            modifiers: 0,
            pressed: true,
        };
        assert_eq!(published_key_effect(&letter), KeyEffect::Insert("k".into()));
        assert_eq!(
            published_key_effect(&PublishedKey {
                keysym: XK_BACKSPACE,
                event_string: "BackSpace".into(),
                is_text: false,
                modifiers: 0,
                pressed: true,
            }),
            KeyEffect::Backspace
        );
        assert_eq!(
            published_key_effect(&PublishedKey {
                keysym: XK_RETURN,
                event_string: "Return".into(),
                is_text: false,
                modifiers: 0,
                pressed: true,
            }),
            KeyEffect::Submit
        );
        assert_eq!(
            published_key_effect(&PublishedKey {
                keysym: i32::from(b'a'),
                event_string: "a".into(),
                is_text: true,
                modifiers: ATSPI_MOD_CONTROL,
                pressed: true,
            }),
            KeyEffect::SelectAll
        );
        assert_eq!(
            published_key_effect(&PublishedKey {
                keysym: i32::from(b'k'),
                event_string: "k".into(),
                is_text: true,
                modifiers: 0,
                pressed: false,
            }),
            KeyEffect::Ignore
        );
    }
}
