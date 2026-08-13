//! Platform-neutral input injection contract.

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PointerPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InputInjectError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl InputInjectError {
    /// Built by the Linux and Windows adapters; the macOS one reports its
    /// failures another way, so this looks unused when linting that target.
    #[allow(dead_code)]
    pub(crate) fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code: code.into(),
            message: message.to_string(),
        }
    }
}
