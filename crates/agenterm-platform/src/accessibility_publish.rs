//! Facade for publishing a product widget tree to the host accessibility stack.

pub use crate::contract::accessibility_publish::{
    KeyEffect, NODE_APPLICATION, NODE_COMMAND, NODE_FRAME, NODE_OFFSCREEN_FIELD, NODE_SEND,
    NODE_SESSION, NODE_TABS, PublishedAction, PublishedKey, PublishedNode, PublishedRole,
    PublishedTree, published_key_effect,
};

pub use crate::contract::accessibility_tree::AccessibilityBounds;

/// Starts (or no-ops on hosts without a publisher) an accessibility server
/// for this process. Linux registers an AT-SPI2 application; other hosts
/// return a handle that accepts snapshots and ignores them.
pub fn start(
    app_name: &str,
    window_handle: Option<i64>,
) -> Result<AccessibilityPublisher, AccessibilityPublishError> {
    crate::selected::accessibility_publish::start(app_name, window_handle)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessibilityPublishError {
    Failed { code: &'static str, message: String },
}

impl AccessibilityPublishError {
    // Only a real publisher can fail to register, so this constructor is dead
    // in builds that select the no-op backend.
    #[allow(dead_code)]
    pub(crate) fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code,
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for AccessibilityPublishError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for AccessibilityPublishError {}

/// Process-local publisher. Updates replace the visible child tree.
pub struct AccessibilityPublisher {
    inner: crate::selected::accessibility_publish::PublisherInner,
}

impl AccessibilityPublisher {
    pub(crate) fn from_inner(
        inner: crate::selected::accessibility_publish::PublisherInner,
    ) -> Self {
        Self { inner }
    }

    pub fn publish(&self, tree: PublishedTree) {
        self.inner.publish(tree);
    }

    pub fn set_handler(&self, handler: PublishedActionHandler) {
        self.inner.set_handler(handler);
    }

    pub fn set_window_handle(&self, window_handle: Option<i64>) {
        self.inner.set_window_handle(window_handle);
    }

    /// Whether callers should keep this handle and keep publishing snapshots.
    /// True for a reconnectable host publisher even while the bus is down.
    /// False only for a no-op backend that discards every snapshot.
    pub fn retains_snapshots(&self) -> bool {
        self.inner.retains_snapshots()
    }

    /// Whether a live host connection is serving the current snapshot.
    /// False while a reconnectable publisher is waiting for a bus, and on
    /// hosts whose backend discards snapshots.
    pub fn is_publishing(&self) -> bool {
        self.inner.is_publishing()
    }
}

pub type PublishedActionHandler =
    std::sync::Arc<dyn Fn(u32, PublishedAction) -> bool + Send + Sync>;
