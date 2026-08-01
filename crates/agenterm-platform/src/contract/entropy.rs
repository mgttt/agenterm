//! Product-neutral host cryptographic entropy contract.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum EntropyErrorKind {
    Unavailable,
    NativeFailure,
    NoProgress,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntropyError {
    pub(crate) kind: EntropyErrorKind,
    pub(crate) native_code: Option<i64>,
    pub(crate) message: String,
}

impl EntropyError {
    pub const fn kind(&self) -> EntropyErrorKind {
        self.kind
    }

    pub const fn native_code(&self) -> Option<i64> {
        self.native_code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for EntropyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.native_code {
            Some(code) => write!(formatter, "host entropy failed ({code}): {}", self.message),
            None => write!(formatter, "host entropy failed: {}", self.message),
        }
    }
}

impl std::error::Error for EntropyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_preserves_kind_code_and_message() {
        let error = EntropyError {
            kind: EntropyErrorKind::NativeFailure,
            native_code: Some(5),
            message: "native RNG failed".to_owned(),
        };
        assert_eq!(error.kind(), EntropyErrorKind::NativeFailure);
        assert_eq!(error.native_code(), Some(5));
        assert_eq!(error.message(), "native RNG failed");
        assert_eq!(
            error.to_string(),
            "host entropy failed (5): native RNG failed"
        );
    }
}
