use std::borrow::Cow;

use crate::{CapabilityStatus, contract::ime::ImeStatus};

/// Preedit state arrives through winit's IME events here rather than from a
/// pollable host API, so there is nothing to report synchronously.
pub(crate) fn status() -> Option<ImeStatus> {
    None
}

pub(crate) fn capability_status(display_available: bool) -> CapabilityStatus {
    if display_available {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("headless-display"),
        }
    }
}
