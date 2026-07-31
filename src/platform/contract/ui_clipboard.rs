//! Typed clipboard outcomes for native frontend projections.

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiClipboardError {
    Unsupported {
        reason: &'static str,
    },
    #[allow(dead_code)] // Constructed by Unix adapters only.
    Failed {
        code: &'static str,
        message: String,
    },
}

impl UiClipboardError {
    #[allow(dead_code)] // Consumed by the Unix frontend compatibility projection.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Unsupported { reason } => format!("clipboard unsupported: {reason}"),
            Self::Failed { message, .. } => message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_and_failed_outcomes_remain_distinct() {
        let unsupported = UiClipboardError::Unsupported {
            reason: "headless-display",
        };
        let failed = UiClipboardError::Failed {
            code: "clipboard_timeout",
            message: "deadline elapsed".to_owned(),
        };
        assert_eq!(
            unsupported.message(),
            "clipboard unsupported: headless-display"
        );
        assert_eq!(failed.message(), "deadline elapsed");
    }
}
