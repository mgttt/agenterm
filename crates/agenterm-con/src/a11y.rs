//! Accessibility snapshot for the standalone console chrome.
//!
//! The host is a custom-raster (winit/softbuffer) toolkit, so GTK atk-bridge
//! never sees it. This module names the painted chrome as a small widget tree
//! and lets the Linux AT-SPI publisher register those children.

use std::collections::VecDeque;
use std::sync::Mutex;

use agenterm_platform::accessibility_publish::{
    AccessibilityBounds, NODE_APPLICATION, NODE_COMMAND, NODE_FRAME, NODE_SEND, NODE_SESSION,
    NODE_TABS, PublishedAction, PublishedNode, PublishedRole, PublishedTree,
};

use crate::ui::{Layout, Rect};

pub const COMMAND_NAME: &str = "Command";
pub const SEND_NAME: &str = "SEND";
pub const TABS_NAME: &str = "Tabs";
pub const SESSION_NAME: &str = "Session";
const ACTION_QUEUE_CAPACITY: usize = 64;
const ACTION_QUEUE_MAX_BYTES: usize = 256 * 1024;
pub const ACTION_DRAIN_BUDGET: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub node: u32,
    pub action: PublishedAction,
}

impl Request {
    fn payload_bytes(&self) -> usize {
        match &self.action {
            PublishedAction::SetText(text) => text.len(),
            PublishedAction::Key(key) => key.event_string.len(),
            PublishedAction::Click | PublishedAction::Focus => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActionInboxStats {
    pub pending: usize,
    pub pending_bytes: usize,
    pub dropped: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActionPush {
    pub accepted: bool,
    pub should_wake: bool,
}

#[derive(Default)]
struct ActionQueue {
    requests: VecDeque<Request>,
    pending_bytes: usize,
    dropped: u64,
}

#[derive(Default)]
pub struct ActionInbox {
    queue: Mutex<ActionQueue>,
}

impl ActionInbox {
    pub fn push(&self, request: Request) -> ActionPush {
        let payload_bytes = request.payload_bytes();
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        let next_bytes = queue.pending_bytes.checked_add(payload_bytes);
        if queue.requests.len() >= ACTION_QUEUE_CAPACITY
            || payload_bytes > crate::composer::PASTE_LIMIT_BYTES
            || next_bytes.is_none_or(|bytes| bytes > ACTION_QUEUE_MAX_BYTES)
        {
            queue.dropped = queue.dropped.saturating_add(1);
            return ActionPush::default();
        }
        let should_wake = queue.requests.is_empty();
        queue.pending_bytes = next_bytes.unwrap_or(queue.pending_bytes);
        queue.requests.push_back(request);
        ActionPush {
            accepted: true,
            should_wake,
        }
    }

    pub fn pop_batch(&self, limit: usize) -> (Vec<Request>, bool) {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        let count = limit.min(queue.requests.len());
        let batch: Vec<_> = queue.requests.drain(..count).collect();
        let drained_bytes = batch
            .iter()
            .map(Request::payload_bytes)
            .fold(0usize, usize::saturating_add);
        queue.pending_bytes = queue.pending_bytes.saturating_sub(drained_bytes);
        (batch, !queue.requests.is_empty())
    }

    pub fn stats(&self) -> ActionInboxStats {
        let queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        ActionInboxStats {
            pending: queue.requests.len(),
            pending_bytes: queue.pending_bytes,
            dropped: queue.dropped,
        }
    }
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

    #[test]
    fn action_inbox_is_fifo_bounded_and_budgeted() {
        let inbox = ActionInbox::default();
        for node in 0..ACTION_QUEUE_CAPACITY {
            let outcome = inbox.push(Request {
                node: node as u32,
                action: PublishedAction::Focus,
            });
            assert!(outcome.accepted);
            assert_eq!(outcome.should_wake, node == 0);
        }
        assert_eq!(
            inbox.push(Request {
                node: u32::MAX,
                action: PublishedAction::Click,
            }),
            ActionPush::default()
        );
        assert_eq!(
            inbox.stats(),
            ActionInboxStats {
                pending: ACTION_QUEUE_CAPACITY,
                pending_bytes: 0,
                dropped: 1,
            }
        );

        let (first, backlog) = inbox.pop_batch(ACTION_DRAIN_BUDGET);
        assert!(backlog);
        assert_eq!(first.len(), ACTION_DRAIN_BUDGET);
        assert_eq!(first.first().map(|request| request.node), Some(0));
        assert_eq!(
            first.last().map(|request| request.node),
            Some(ACTION_DRAIN_BUDGET as u32 - 1)
        );
        let (second, backlog) = inbox.pop_batch(usize::MAX);
        assert!(!backlog);
        assert_eq!(second.len(), ACTION_QUEUE_CAPACITY - ACTION_DRAIN_BUDGET);
        assert_eq!(inbox.stats().pending, 0);
    }

    #[test]
    fn action_inbox_bounds_payload_bytes_and_returns_drained_capacity() {
        let inbox = ActionInbox::default();
        let chunk = "x".repeat(crate::composer::PASTE_LIMIT_BYTES);
        for _ in 0..ACTION_QUEUE_MAX_BYTES / chunk.len() {
            assert!(
                inbox
                    .push(Request {
                        node: NODE_COMMAND,
                        action: PublishedAction::SetText(chunk.clone()),
                    })
                    .accepted
            );
        }
        assert!(
            !inbox
                .push(Request {
                    node: NODE_COMMAND,
                    action: PublishedAction::SetText("overflow".into()),
                })
                .accepted
        );
        assert_eq!(inbox.stats().pending_bytes, ACTION_QUEUE_MAX_BYTES);

        let (drained, _) = inbox.pop_batch(1);
        assert_eq!(drained.len(), 1);
        assert_eq!(
            inbox.stats().pending_bytes,
            ACTION_QUEUE_MAX_BYTES - crate::composer::PASTE_LIMIT_BYTES
        );
        assert!(
            !inbox
                .push(Request {
                    node: NODE_COMMAND,
                    action: PublishedAction::SetText(
                        "y".repeat(crate::composer::PASTE_LIMIT_BYTES + 1),
                    ),
                })
                .accepted
        );
    }
}
