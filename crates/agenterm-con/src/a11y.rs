//! Accessibility snapshot for the standalone console chrome.
//!
//! The host is a custom-raster (winit/softbuffer) toolkit, so GTK atk-bridge
//! never sees it. This module names the painted chrome as a small widget tree
//! and lets the Linux AT-SPI publisher register those children.

use agenterm_platform::accessibility_publish::{
    AccessibilityBounds, NODE_APPLICATION, NODE_COMMAND, NODE_FRAME, NODE_SEND, NODE_SESSION,
    NODE_TABS, PublishedAction, PublishedNode, PublishedRole, PublishedTree,
};

use crate::ui::{Layout, Rect};

pub const COMMAND_NAME: &str = "Command";
pub const SEND_NAME: &str = "SEND";
pub const TABS_NAME: &str = "Tabs";
pub const SESSION_NAME: &str = "Session";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub node: u32,
    pub action: PublishedAction,
}

pub fn tree(
    app_name: &str,
    frame_title: &str,
    layout: Layout,
    frame_width: u32,
    frame_height: u32,
    composer_focused: bool,
    command_text: &str,
) -> PublishedTree {
    let frame = Rect {
        x: 0,
        y: 0,
        width: frame_width,
        height: frame_height,
    };
    PublishedTree {
        app_name: app_name.to_owned(),
        nodes: vec![
            published(
                NODE_APPLICATION,
                None,
                PublishedRole::Application,
                app_name,
                "",
                frame,
                Flags::CONTAINER,
            ),
            published(
                NODE_FRAME,
                Some(NODE_APPLICATION),
                PublishedRole::Frame,
                frame_title,
                "",
                frame,
                Flags::FOCUSABLE.with_click(),
            ),
            published(
                NODE_TABS,
                Some(NODE_FRAME),
                PublishedRole::Panel,
                TABS_NAME,
                "",
                layout.sidebar,
                Flags::FOCUSABLE,
            ),
            published(
                NODE_SESSION,
                Some(NODE_FRAME),
                PublishedRole::Terminal,
                SESSION_NAME,
                "",
                session_rect(layout, frame_width, frame_height),
                Flags {
                    focusable: true,
                    focused: !composer_focused,
                    editable: false,
                    clickable: false,
                },
            ),
            published(
                NODE_COMMAND,
                Some(NODE_FRAME),
                PublishedRole::Text,
                COMMAND_NAME,
                command_text,
                layout.composer_input,
                Flags {
                    focusable: true,
                    focused: composer_focused,
                    editable: true,
                    clickable: true,
                },
            ),
            published(
                NODE_SEND,
                Some(NODE_FRAME),
                PublishedRole::Button,
                SEND_NAME,
                "",
                layout.composer_send,
                Flags::FOCUSABLE.with_click(),
            ),
        ],
    }
}

#[derive(Clone, Copy)]
struct Flags {
    focusable: bool,
    focused: bool,
    editable: bool,
    clickable: bool,
}

impl Flags {
    const CONTAINER: Self = Self {
        focusable: false,
        focused: false,
        editable: false,
        clickable: false,
    };
    const FOCUSABLE: Self = Self {
        focusable: true,
        focused: false,
        editable: false,
        clickable: false,
    };

    const fn with_click(self) -> Self {
        Self {
            clickable: true,
            ..self
        }
    }
}

fn session_rect(layout: Layout, frame_width: u32, frame_height: u32) -> Rect {
    let left = layout.sidebar.width;
    let bottom = layout.composer.y;
    Rect {
        x: left,
        y: 0,
        width: frame_width.saturating_sub(left),
        height: bottom.min(frame_height),
    }
}

fn published(
    id: u32,
    parent: Option<u32>,
    role: PublishedRole,
    name: &str,
    text: &str,
    rect: Rect,
    flags: Flags,
) -> PublishedNode {
    PublishedNode {
        id,
        parent,
        role,
        name: name.to_owned(),
        text: text.to_owned(),
        bounds: AccessibilityBounds {
            x: i32::try_from(rect.x).unwrap_or(0),
            y: i32::try_from(rect.y).unwrap_or(0),
            width: i32::try_from(rect.width).unwrap_or(0),
            height: i32::try_from(rect.height).unwrap_or(0),
        },
        focusable: flags.focusable,
        focused: flags.focused,
        editable: flags.editable,
        clickable: flags.clickable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_has_inner_named_controls() {
        let layout = Layout::new(960, 600, 1.0);
        let tree = tree(
            "agenterm-con",
            "Inspect [@1]",
            layout,
            960,
            600,
            true,
            "probe-text",
        );
        assert!(tree.nodes.len() >= 5);
        let command = tree.node(NODE_COMMAND).expect("command input is published");
        assert_eq!(command.name, COMMAND_NAME);
        assert_eq!(command.role, PublishedRole::Text);
        assert_eq!(command.text, "probe-text");
        assert!(command.focused);
        assert!(command.editable);
        let send = tree.node(NODE_SEND).expect("send button is published");
        assert_eq!(send.name, SEND_NAME);
        assert_eq!(send.role, PublishedRole::Button);
        assert_ne!(send.role.as_str(), "frame");
        assert_ne!(send.role.as_str(), "application");
        assert_eq!(tree.children_of(NODE_FRAME).len(), 4);
    }
}
