//! Platform-neutral resident desktop action host contract.

use std::{borrow::Cow, fmt};

pub const MAX_DESKTOP_ACTIONS: usize = 64;
pub const MAX_DESKTOP_LABEL_BYTES: usize = 256;
pub const MAX_DESKTOP_SHORTCUT_BYTES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopActionSpec {
    pub action_id: u32,
    pub label: String,
    pub shortcut: Option<String>,
}

impl DesktopActionSpec {
    pub fn new(action_id: u32, label: impl Into<String>) -> Self {
        Self {
            action_id,
            label: label.into(),
            shortcut: None,
        }
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DesktopHostError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl DesktopHostError {
    pub fn unsupported(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::Unsupported {
            reason: reason.into(),
        }
    }

    pub fn failed(code: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Self {
        Self::Failed {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for DesktopHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "desktop host unsupported: {reason}"),
            Self::Failed { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for DesktopHostError {}
