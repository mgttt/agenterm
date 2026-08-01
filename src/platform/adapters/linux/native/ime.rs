//! Linux IME capability bridge for platform migration slice-2 (contract rev 1).
//! Adapter-private native mechanism selected only by platform::selected.
//!
//! The reusable crate owns preedit/commit classification. This compatibility
//! projection maps only AgenTerm's legacy capability status shape.

#![cfg(target_os = "linux")]

use crate::platform::{CapabilityStatus, DisplayBackendFacts};

use super::display_facts_from_env;
pub(crate) use agenterm_platform::ime::{ImeAction as LinuxImeAction, ImeEvent as LinuxImeEvent};

/// IME capability status for the current display backend.
///
/// Headless must not report Available. With a display, winit/X11/Wayland IME
/// is treated as Available; individual commits still go through classification.
pub(crate) fn ime_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    match agenterm_platform::ime::capability_status(!facts.headless) {
        agenterm_platform::CapabilityStatus::Available => CapabilityStatus::Available,
        agenterm_platform::CapabilityStatus::Unsupported { reason } => {
            CapabilityStatus::Unsupported {
                reason: if reason == "headless-display" {
                    "headless-display"
                } else {
                    "ime-unsupported"
                },
            }
        }
        agenterm_platform::CapabilityStatus::Failed { message, .. } => CapabilityStatus::Failed {
            code: "ime-failed",
            message,
        },
    }
}

/// Probe IME capability from the process environment.
pub(crate) fn ime_capability_status_from_env() -> CapabilityStatus {
    ime_capability_status(display_facts_from_env())
}

/// Classify a Linux IME event for the unix_app host.
///
/// `anchor_available` is true when the focused surface can accept composition
/// (composer / text field / terminal). Commit text is filtered through
/// the public crate state machine so empty results never reach product handlers
/// as fake Available input.
pub(crate) fn classify_ime_event(event: LinuxImeEvent, anchor_available: bool) -> LinuxImeAction {
    agenterm_platform::ime::classify_event(event, anchor_available)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::DisplayBackendFacts;

    #[test]
    fn headless_ime_is_unsupported_not_available() {
        let status = ime_capability_status(DisplayBackendFacts {
            x11: false,
            wayland: false,
            headless: true,
        });
        assert!(matches!(
            status,
            CapabilityStatus::Unsupported {
                reason: "headless-display"
            }
        ));
    }

    #[test]
    fn display_backend_marks_ime_available() {
        let status = ime_capability_status(DisplayBackendFacts {
            x11: true,
            wayland: false,
            headless: false,
        });
        assert_eq!(status, CapabilityStatus::Available);
    }

    #[test]
    fn preedit_requires_anchor() {
        let event = LinuxImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        };
        assert_eq!(
            classify_ime_event(event.clone(), true),
            LinuxImeAction::UpdatePreedit {
                text: "ni".to_string(),
                cursor: Some((0, 2)),
            }
        );
        assert_eq!(
            classify_ime_event(event, false),
            LinuxImeAction::ClearPreedit
        );
    }

    #[test]
    fn commit_goes_through_text_classification() {
        assert_eq!(
            classify_ime_event(LinuxImeEvent::Commit("你好".to_string()), true),
            LinuxImeAction::CommitText("你好".to_string())
        );
        assert_eq!(
            classify_ime_event(LinuxImeEvent::Commit(String::new()), true),
            LinuxImeAction::ClearPreedit
        );
    }

    #[test]
    fn disabled_clears_preedit() {
        assert_eq!(
            classify_ime_event(LinuxImeEvent::Disabled, true),
            LinuxImeAction::ClearPreedit
        );
    }
}
