//! Shared CWD editor dialog state and snapshot policy.
//!
//! The CWD editor is a product modal on both hosts. Adapters still own the
//! native edit control, focus, and platform command dispatch, but the open
//! target, modal snapshot, and prepare action names come from this module.

use serde_json::json;

use super::composer::ComposerWriteMode;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CwdEditorDialog {
    open: bool,
    target: Option<String>,
}

impl CwdEditorDialog {
    pub(crate) const fn new() -> Self {
        Self {
            open: false,
            target: None,
        }
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub(crate) fn open(&mut self, target: String) {
        self.open = true;
        self.target = Some(target);
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.target = None;
    }

    pub(crate) fn close_and_take_target(&mut self) -> Option<String> {
        self.open = false;
        self.target.take()
    }

    pub(crate) fn prepare_action(mode: ComposerWriteMode) -> &'static str {
        match mode {
            ComposerWriteMode::EmptyOnly => "cwd-prepare",
            ComposerWriteMode::Append => "cwd-prepare-append",
            ComposerWriteMode::Replace => "cwd-prepare-replace",
        }
    }

    pub(crate) fn submit_mode(
        modifiers: agenterm_platform::input::ModifierState,
    ) -> Option<ComposerWriteMode> {
        let primary = if crate::platform::is_primary_shortcut_via_meta() {
            modifiers.meta
        } else {
            modifiers.control
        };
        if !primary {
            return None;
        }
        Some(if modifiers.shift {
            ComposerWriteMode::Append
        } else if modifiers.alt {
            ComposerWriteMode::Replace
        } else {
            ComposerWriteMode::EmptyOnly
        })
    }

    pub(crate) fn snapshot_modal(&self) -> serde_json::Value {
        json!({
            "kind": "cwd-editor",
            "target": self.target.as_deref().unwrap_or(""),
            "window_id": self.target.as_deref().unwrap_or(""),
            "default_action": Self::prepare_action(ComposerWriteMode::EmptyOnly),
            "actions": [
                Self::prepare_action(ComposerWriteMode::EmptyOnly),
                Self::prepare_action(ComposerWriteMode::Append),
                Self::prepare_action(ComposerWriteMode::Replace),
                "cwd-send-now",
                "cancel",
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cwd_editor_is_closed() {
        let dialog = CwdEditorDialog::new();
        assert!(!dialog.is_open());
        assert_eq!(dialog.target(), None);
    }

    #[test]
    fn open_exposes_target() {
        let mut dialog = CwdEditorDialog::new();
        dialog.open("@7".to_owned());
        assert!(dialog.is_open());
        assert_eq!(dialog.target(), Some("@7"));
    }

    #[test]
    fn close_clears_state() {
        let mut dialog = CwdEditorDialog::new();
        dialog.open("@7".to_owned());
        assert_eq!(dialog.close_and_take_target().as_deref(), Some("@7"));
        assert!(!dialog.is_open());
        assert_eq!(dialog.target(), None);
    }

    #[test]
    fn snapshot_exposes_shared_modal_contract() {
        let mut dialog = CwdEditorDialog::new();
        dialog.open("@7".to_owned());
        let snapshot = dialog.snapshot_modal();
        assert_eq!(snapshot["kind"], "cwd-editor");
        assert_eq!(snapshot["target"], "@7");
        assert_eq!(snapshot["window_id"], "@7");
        assert_eq!(snapshot["default_action"], "cwd-prepare");
        assert_eq!(snapshot["actions"][3], "cwd-send-now");
    }

    #[test]
    fn submit_mode_uses_platform_primary_shortcut_and_modes() {
        let via_meta = crate::platform::is_primary_shortcut_via_meta();
        let primary = agenterm_platform::input::ModifierState {
            control: !via_meta,
            shift: false,
            alt: false,
            meta: via_meta,
        };
        assert_eq!(
            CwdEditorDialog::submit_mode(primary),
            Some(ComposerWriteMode::EmptyOnly)
        );
        assert_eq!(
            CwdEditorDialog::submit_mode(agenterm_platform::input::ModifierState {
                control: false,
                shift: false,
                alt: false,
                meta: false,
            }),
            None
        );
        assert_eq!(
            CwdEditorDialog::submit_mode(agenterm_platform::input::ModifierState {
                control: !via_meta,
                shift: true,
                alt: false,
                meta: via_meta,
            }),
            Some(ComposerWriteMode::Append)
        );
        assert_eq!(
            CwdEditorDialog::submit_mode(agenterm_platform::input::ModifierState {
                control: !via_meta,
                shift: false,
                alt: true,
                meta: via_meta,
            }),
            Some(ComposerWriteMode::Replace)
        );
    }

    #[test]
    fn prepare_actions_follow_composer_modes() {
        assert_eq!(
            CwdEditorDialog::prepare_action(ComposerWriteMode::EmptyOnly),
            "cwd-prepare"
        );
        assert_eq!(
            CwdEditorDialog::prepare_action(ComposerWriteMode::Append),
            "cwd-prepare-append"
        );
        assert_eq!(
            CwdEditorDialog::prepare_action(ComposerWriteMode::Replace),
            "cwd-prepare-replace"
        );
    }
}
