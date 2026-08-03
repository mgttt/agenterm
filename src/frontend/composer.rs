//! Shared composer editing policy used by embedded and remote frontends.
//!
//! The CWD editor exposes three write modes on both hosts. Keeping them here
//! avoids each platform adapter re-encoding mode names and server command
//! selection.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComposerWriteMode {
    EmptyOnly,
    Append,
    Replace,
}

impl ComposerWriteMode {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("empty-only") {
            "empty" | "empty-only" => Ok(Self::EmptyOnly),
            "append" => Ok(Self::Append),
            "replace" => Ok(Self::Replace),
            other => Err(format!("unknown composer write mode: {other}")),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyOnly => "empty-only",
            Self::Append => "append",
            Self::Replace => "replace",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_write_mode_parses_canonical_and_legacy_names() {
        assert_eq!(
            ComposerWriteMode::parse(None),
            Ok(ComposerWriteMode::EmptyOnly)
        );
        assert_eq!(
            ComposerWriteMode::parse(Some("empty")),
            Ok(ComposerWriteMode::EmptyOnly)
        );
        assert_eq!(
            ComposerWriteMode::parse(Some("append")),
            Ok(ComposerWriteMode::Append)
        );
        assert_eq!(
            ComposerWriteMode::parse(Some("replace")),
            Ok(ComposerWriteMode::Replace)
        );
        assert!(ComposerWriteMode::parse(Some("bogus")).is_err());
    }

    #[test]
    fn composer_write_mode_round_trips_public_names() {
        for mode in [
            ComposerWriteMode::EmptyOnly,
            ComposerWriteMode::Append,
            ComposerWriteMode::Replace,
        ] {
            assert_eq!(ComposerWriteMode::parse(Some(mode.as_str())), Ok(mode));
        }
    }
}
