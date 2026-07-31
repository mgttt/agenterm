//! Typed Script Runtime clipboard failures, independent from GUI paste policy.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScriptClipboardError {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
    pub(crate) cause: Option<&'static str>,
}

impl ScriptClipboardError {
    pub(crate) const fn new(
        code: &'static str,
        message: &'static str,
        cause: Option<&'static str>,
    ) -> Self {
        Self {
            code,
            message,
            cause,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_error_keeps_public_receipt_fields() {
        assert_eq!(
            ScriptClipboardError::new("clipboard_unsupported", "unavailable", Some("unsupported")),
            ScriptClipboardError {
                code: "clipboard_unsupported",
                message: "unavailable",
                cause: Some("unsupported"),
            }
        );
    }
}
