//! macOS Cocoa IME preedit and committed-text classification.
//! Adapter-private native mechanism selected only by platform::selected.

#![cfg(target_os = "macos")]

pub(crate) use agenterm_platform::ime::{ImeAction as MacosImeAction, ImeEvent as MacosImeEvent};

pub(crate) fn classify_ime_event(event: MacosImeEvent, anchor_available: bool) -> MacosImeAction {
    agenterm_platform::ime::classify_event(event, anchor_available)
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
