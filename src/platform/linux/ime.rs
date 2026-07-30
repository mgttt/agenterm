//! Linux IME capability bridge for platform migration slice-2 (contract rev 1).
//!
//! Preedit / commit event shapes stay Linux-private. Shared contract already
//! provides [`CapabilityKind::Ime`], [`CapabilityStatus`], and text-commit
//! classification via [`super::input::classify_ime_commit`]. Do not invent
//! shared `ImeEvent` types here — request a contract revision if needed.

#![cfg(target_os = "linux")]

use crate::platform::{CapabilityStatus, DisplayBackendFacts, KeyClassification};

use super::display_facts_from_env;
use super::input::classify_ime_commit;

/// Linux-local IME event (adapter detail; not a shared contract type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinuxImeEvent {
    Enabled,
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
    Disabled,
}

/// Action the Linux GUI host should take after classifying an IME event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinuxImeAction {
    None,
    UpdatePreedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    ClearPreedit,
    /// Already classified as a text commit (empty commits are dropped).
    CommitText(String),
}

/// IME capability status for the current display backend.
///
/// Headless must not report Available. With a display, winit/X11/Wayland IME
/// is treated as Available; individual commits still go through classification.
pub(crate) fn ime_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    if facts.headless {
        CapabilityStatus::Unsupported {
            reason: "headless-display",
        }
    } else {
        CapabilityStatus::Available
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
/// [`classify_ime_commit`] so empty or non-text results never reach product
/// handlers as fake Available input.
pub(crate) fn classify_ime_event(event: LinuxImeEvent, anchor_available: bool) -> LinuxImeAction {
    match event {
        LinuxImeEvent::Enabled => LinuxImeAction::None,
        LinuxImeEvent::Preedit { text, cursor } => {
            if anchor_available {
                LinuxImeAction::UpdatePreedit { text, cursor }
            } else {
                LinuxImeAction::ClearPreedit
            }
        }
        LinuxImeEvent::Commit(text) => match classify_ime_commit(&text) {
            KeyClassification::TextCommit(commit) => LinuxImeAction::CommitText(commit),
            _ => LinuxImeAction::ClearPreedit,
        },
        LinuxImeEvent::Disabled => LinuxImeAction::ClearPreedit,
    }
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

    #[test]
    fn classify_ime_commit_matches_shared_helper() {
        use crate::platform::{ModifierState, classify_key_press};
        assert_eq!(
            classify_ime_commit("a"),
            classify_key_press(ModifierState::empty(), None, None, Some("a"))
        );
    }
}
