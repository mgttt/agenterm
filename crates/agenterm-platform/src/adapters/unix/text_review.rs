//! Unix hosts have no native editable review yet.
//!
//! The type still exists so the contract, the product state machine and their
//! tests are written once for every platform. When a Linux or macOS adapter
//! lands it implements `open_review`/`try_poll` and nothing above this line
//! changes — in particular it must stay modeless, because the reason the
//! contract is polled rather than blocking is the host loop, which every
//! platform has.

use crate::text_review::{TextReviewError, TextReviewPoll};

pub(crate) struct NativeTextReview {
    never: std::convert::Infallible,
}

impl NativeTextReview {
    pub(crate) fn try_poll(&mut self) -> TextReviewPoll {
        match self.never {}
    }
}

pub(crate) fn open_review(
    _owner: Option<i64>,
    _title: &str,
    _prompt: &str,
    _initial: &str,
    _wake: Box<dyn FnOnce() + Send + 'static>,
) -> Result<NativeTextReview, TextReviewError> {
    Err(TextReviewError::unsupported(
        "this host does not expose an editable native review",
    ))
}
