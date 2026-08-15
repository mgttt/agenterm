//! Editable confirmation for text that a human is about to send elsewhere.

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
    #[cfg(not(windows))]
    pub(crate) fn unsupported(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }

    #[cfg(windows)]
    pub(crate) fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code: Cow::Borrowed(code),
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

/// Shows an owner-modal multiline editor. `None` means that the user cancelled;
/// a confirmed result is the exact edited text and still requires product-level
/// validation before it crosses a process boundary.
pub fn review_text(
    owner: Option<i64>,
    title: &str,
    prompt: &str,
    initial: &str,
) -> Result<Option<String>, TextReviewError> {
    crate::selected::text_review::review_text(owner, title, prompt, initial)
}
