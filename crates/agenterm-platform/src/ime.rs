//! Selected IME capability and platform-neutral composition state machine.

pub use crate::contract::ime::{ImeAction, ImeEvent, ImeStatus};
use crate::{CapabilityStatus, input::KeyClassification, selected};

pub fn capability_status(display_available: bool) -> CapabilityStatus {
    selected::ime::capability_status(display_available)
}

/// Input method currently backing this thread's focused surface, when the
/// host can report one. `None` means "unknown", which callers should render
/// as absence rather than as a disabled IME.
#[must_use]
pub fn status() -> Option<ImeStatus> {
    selected::ime::status()
}

pub fn classify_event(event: ImeEvent, anchor_available: bool) -> ImeAction {
    match event {
        ImeEvent::Enabled => ImeAction::None,
        ImeEvent::Preedit { text, cursor } if anchor_available => {
            ImeAction::UpdatePreedit { text, cursor }
        }
        ImeEvent::Preedit { .. } | ImeEvent::Disabled => ImeAction::ClearPreedit,
        ImeEvent::Commit(text) => match crate::input::classify_ime_commit(&text) {
            KeyClassification::TextCommit(text) => ImeAction::CommitText(text),
            _ => ImeAction::ClearPreedit,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preedit_requires_an_editable_anchor() {
        let event = ImeEvent::Preedit {
            text: "ni".to_owned(),
            cursor: Some((0, 2)),
        };
        assert_eq!(
            classify_event(event.clone(), true),
            ImeAction::UpdatePreedit {
                text: "ni".to_owned(),
                cursor: Some((0, 2)),
            }
        );
        assert_eq!(classify_event(event, false), ImeAction::ClearPreedit);
    }

    #[test]
    fn empty_commits_are_not_text() {
        assert_eq!(
            classify_event(ImeEvent::Commit(String::new()), true),
            ImeAction::ClearPreedit
        );
        assert_eq!(
            classify_event(ImeEvent::Commit("你好".to_owned()), true),
            ImeAction::CommitText("你好".to_owned())
        );
    }
}
