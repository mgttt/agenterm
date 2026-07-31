//! macOS Cocoa IME preedit and committed-text classification.
//! Adapter-private native mechanism selected only by platform::selected.

#![cfg(target_os = "macos")]

use crate::platform::KeyClassification;

use super::input::classify_ime_commit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MacosImeEvent {
    Enabled,
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MacosImeAction {
    None,
    UpdatePreedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    ClearPreedit,
    CommitText(String),
}

pub(crate) fn classify_ime_event(event: MacosImeEvent, anchor_available: bool) -> MacosImeAction {
    match event {
        MacosImeEvent::Enabled => MacosImeAction::None,
        MacosImeEvent::Preedit { text, cursor } if anchor_available => {
            MacosImeAction::UpdatePreedit { text, cursor }
        }
        MacosImeEvent::Preedit { .. } | MacosImeEvent::Disabled => MacosImeAction::ClearPreedit,
        MacosImeEvent::Commit(text) => match classify_ime_commit(&text) {
            KeyClassification::TextCommit(text) => MacosImeAction::CommitText(text),
            _ => MacosImeAction::ClearPreedit,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preedit_requires_an_editable_anchor() {
        let event = MacosImeEvent::Preedit {
            text: "ni".to_owned(),
            cursor: Some((0, 2)),
        };
        assert_eq!(
            classify_ime_event(event.clone(), true),
            MacosImeAction::UpdatePreedit {
                text: "ni".to_owned(),
                cursor: Some((0, 2)),
            }
        );
        assert_eq!(
            classify_ime_event(event, false),
            MacosImeAction::ClearPreedit
        );
    }

    #[test]
    fn cjk_commit_uses_the_input_adapter() {
        assert_eq!(
            classify_ime_event(MacosImeEvent::Commit("你好".to_owned()), true),
            MacosImeAction::CommitText("你好".to_owned())
        );
        assert_eq!(
            classify_ime_event(MacosImeEvent::Commit(String::new()), true),
            MacosImeAction::ClearPreedit
        );
    }

    #[test]
    fn disabled_clears_preedit() {
        assert_eq!(
            classify_ime_event(MacosImeEvent::Disabled, true),
            MacosImeAction::ClearPreedit
        );
    }
}
