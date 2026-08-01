//! Platform-neutral window activation requests and typed failures.

use std::{borrow::Cow, fmt, num::NonZeroIsize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ActivationRequest {
    ShowWithoutActivation,
    ShowNewAndRequestActivation,
    RestoreAndActivate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivationPolicy {
    pub request_activation: bool,
    pub initial_window_focused: bool,
}

impl ActivationPolicy {
    pub const fn from_no_activate(no_activate: bool) -> Self {
        Self {
            request_activation: !no_activate,
            initial_window_focused: !no_activate,
        }
    }
}

/// Opaque native top-level-window identity. Construction is unsafe because the
/// caller must guarantee the handle remains valid for the duration of a call.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NativeWindowHandle(NonZeroIsize);

impl NativeWindowHandle {
    /// # Safety
    ///
    /// `raw` must identify a live top-level window owned by the current process.
    pub const unsafe fn from_raw(raw: isize) -> Option<Self> {
        match NonZeroIsize::new(raw) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    #[allow(dead_code)] // Read by the selected Windows adapter.
    pub(crate) const fn raw(self) -> isize {
        self.0.get()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ActivationError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl fmt::Display for ActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "activation unsupported: {reason}"),
            Self::Failed { message, .. } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ActivationError {}
