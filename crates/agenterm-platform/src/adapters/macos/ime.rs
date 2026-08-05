use std::borrow::Cow;

use crate::{
    CapabilityStatus,
    contract::ime::{ImeComposition, ImeStatus},
};

/// Preedit state arrives through winit's IME events here rather than from a
/// pollable host API, so there is nothing to report synchronously.
pub(crate) fn status() -> Option<ImeStatus> {
    None
}

/// Preedit state arrives through winit's IME events here; nothing to poll.
pub(crate) fn composition() -> Option<ImeComposition> {
    None
}

/// winit positions the composition/candidate UI on our behalf; no-op.
pub(crate) fn set_anchor_position(_x: i32, _y: i32) {}

pub(crate) fn capability_status(display_available: bool) -> CapabilityStatus {
    if display_available {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("headless-display"),
        }
    }
}
