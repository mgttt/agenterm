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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublishedAction {
    Click,
    Focus,
    SetText(String),
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
}
