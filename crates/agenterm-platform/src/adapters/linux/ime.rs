use std::borrow::Cow;

use crate::CapabilityStatus;

pub(crate) fn capability_status(display_available: bool) -> CapabilityStatus {
    if display_available {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("headless-display"),
        }
    }
}
