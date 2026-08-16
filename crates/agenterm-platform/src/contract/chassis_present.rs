//! Narrow native presentation contract for the Chassis-L1 loader.

use std::{borrow::Cow, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChassisPresentOptions {
    pub title: String,
    pub loaded_rows: u16,
}

impl ChassisPresentOptions {
    #[must_use]
    pub fn new(title: impl Into<String>, loaded_rows: u16) -> Self {
        Self {
            title: title.into(),
            loaded_rows,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChassisPresentError {
    pub code: Cow<'static, str>,
    pub message: String,
}

impl ChassisPresentError {
    pub(crate) fn failed(code: &'static str, error: impl fmt::Display) -> Self {
        Self {
            code: Cow::Borrowed(code),
            message: error.to_string(),
        }
    }
}

impl fmt::Display for ChassisPresentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ChassisPresentError {}
