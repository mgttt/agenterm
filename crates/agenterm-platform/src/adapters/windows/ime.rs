use std::borrow::Cow;

use crate::CapabilityStatus;

pub(crate) fn capability_status(_display_available: bool) -> CapabilityStatus {
    CapabilityStatus::Unsupported {
        reason: Cow::Borrowed("ime-preedit-not-yet-adapted"),
    }
}
