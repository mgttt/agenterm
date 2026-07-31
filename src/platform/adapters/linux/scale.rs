//! Linux display-capability classification.

use crate::platform::{CapabilityStatus, DisplayBackendFacts};

pub(crate) fn capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    if facts.headless {
        CapabilityStatus::Unsupported {
            reason: "headless-display",
        }
    } else {
        CapabilityStatus::Available
    }
}
