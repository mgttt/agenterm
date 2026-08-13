//! Non-Linux stub: publishing is a host no-op until AX/UIA publishers exist.

use crate::accessibility_publish::{
    AccessibilityPublishError, AccessibilityPublisher, PublishedActionHandler, PublishedTree,
};

pub(crate) struct PublisherInner;

impl PublisherInner {
    pub(crate) fn publish(&self, _tree: PublishedTree) {}

    pub(crate) fn set_handler(&self, _handler: PublishedActionHandler) {}

    pub(crate) fn set_window_handle(&self, _window_handle: Option<i64>) {}

    pub(crate) fn is_publishing(&self) -> bool {
        false
    }
}

pub(crate) fn start(
    _app_name: &str,
    _window_handle: Option<i64>,
) -> Result<AccessibilityPublisher, AccessibilityPublishError> {
    Ok(AccessibilityPublisher::from_inner(PublisherInner))
}
