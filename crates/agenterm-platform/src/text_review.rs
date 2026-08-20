//! Editable confirmation for text that a human is about to send elsewhere.
//!
//! The review is **modeless by contract**. A host that owns an event loop must
//! keep pumping it while a review is open, and observes completion by polling.
//! This is deliberate and is the difference between this facade and an ordinary
//! native modal: a blocking modal implies a nested message pump inside the
//! caller's own callback, which stops the host repainting, stops it answering
//! its control endpoint, and lets producer notifications accumulate in whatever
//! bounded queue the host defers reentrant work into.
//!
//! Adapters therefore never pump. They create, report, and dismiss; the caller's
//! existing loop supplies every message the review needs.

use std::{borrow::Cow, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextReviewError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl TextReviewError {
    pub fn unsupported(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }

    pub fn failed(code: impl Into<Cow<'static, str>>, message: impl fmt::Display) -> Self {
        Self::Failed {
            code: code.into(),
            message: message.to_string(),
        }
    }
}

impl fmt::Display for TextReviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "text review unsupported: {reason}"),
            Self::Failed { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for TextReviewError {}

/// The outcome of one non-blocking observation of an open review.
#[derive(Debug)]
pub enum TextReviewPoll {
    /// The human has not finished. The caller must keep pumping its loop.
    Pending,
    /// Terminal. `Some` is the exact edited text and still requires
    /// product-level validation before it crosses a process boundary; `None`
    /// means the review was cancelled or dismissed and nothing may be sent.
    Ready(Option<String>),
}

/// An open review. Dropping it dismisses the review and restores the owner,
/// so a caller that loses interest — its tab closed, its window went away —
/// does not have to reach for a separate cancel path.
pub struct TextReview {
    native: crate::selected::text_review::NativeTextReview,
}

impl TextReview {
    /// Observes the review without blocking. `Ready` is terminal: the review is
    /// dismissed and every later poll reports `Ready(None)`.
    pub fn try_poll(&mut self) -> TextReviewPoll {
        self.native.try_poll()
    }
}

impl fmt::Debug for TextReview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TextReview")
    }
}

/// Opens an owner-attached modeless editor and returns immediately.
///
/// `wake` is invoked at most once, on the thread that owns the review, when it
/// reaches a terminal state. It exists so a host that only polls on its own
/// wake path does not have to poll every frame; a host that polls
/// unconditionally may pass a no-op.
///
/// The owner is disabled for input for as long as the review is open, which is
/// what makes this *application*-modal in behavior. It is never *loop*-modal:
/// the caller's loop keeps running and must keep running, or the review will
/// never receive the input that completes it.
pub fn open_review(
    owner: Option<i64>,
    title: &str,
    prompt: &str,
    initial: &str,
    wake: impl FnOnce() + Send + 'static,
) -> Result<TextReview, TextReviewError> {
    crate::selected::text_review::open_review(owner, title, prompt, initial, Box::new(wake))
        .map(|native| TextReview { native })
}
