//! Shared inline tab editor state, validation, and snapshot policy.

use serde_json::json;

use crate::ui_bridge::{UI_TAB_NOTE_MAX_BYTES, UI_TAB_TITLE_MAX_BYTES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabEditorFocus {
    Name,
    Note,
}

impl TabEditorFocus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Note => "note",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TabEditorChanges {
    pub(crate) name: String,
    pub(crate) note: String,
}

#[derive(Clone, Debug)]
pub(crate) struct TabEditorDialog {
    open: bool,
    target: Option<String>,
    name_draft: String,
    note_draft: String,
    focus: TabEditorFocus,
}

impl TabEditorDialog {
    pub(crate) const fn new() -> Self {
        Self {
            open: false,
            target: None,
            name_draft: String::new(),
            note_draft: String::new(),
            focus: TabEditorFocus::Name,
        }
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub(crate) const fn focus(&self) -> TabEditorFocus {
        self.focus
    }

    pub(crate) fn name_draft(&self) -> &str {
        &self.name_draft
    }

    pub(crate) fn note_draft(&self) -> &str {
        &self.note_draft
    }

    pub(crate) fn active_draft_mut(&mut self) -> Option<&mut String> {
        if !self.open {
            return None;
        }
        Some(match self.focus {
            TabEditorFocus::Name => &mut self.name_draft,
            TabEditorFocus::Note => &mut self.note_draft,
        })
    }

    pub(crate) fn set_name_draft(&mut self, value: String) {
        if self.open {
            self.name_draft = value;
        }
    }

    pub(crate) fn set_note_draft(&mut self, value: String) {
        if self.open {
            self.note_draft = value;
        }
    }

    pub(crate) fn set_focus(&mut self, focus: TabEditorFocus) {
        if self.open {
            self.focus = focus;
        }
    }

    pub(crate) fn next_field(&mut self) {
        if self.open {
            self.focus = TabEditorFocus::Note;
        }
    }

    pub(crate) fn open(&mut self, target: String, name: String, note: String) {
        self.open = true;
        self.target = Some(target);
        self.name_draft = name;
        self.note_draft = note;
        self.focus = TabEditorFocus::Name;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.target = None;
        self.name_draft.clear();
        self.note_draft.clear();
        self.focus = TabEditorFocus::Name;
    }

    pub(crate) fn capture(&mut self, save: bool) -> Result<Option<TabEditorChanges>, String> {
        if !self.open {
            return Ok(None);
        }
        if !save {
            return Ok(None);
        }
        let name = self.name_draft.trim().to_owned();
        let note = self.note_draft.clone();
        if name.is_empty() {
            return Err("Tab title cannot be empty".to_owned());
        }
        if name.len() > UI_TAB_TITLE_MAX_BYTES {
            return Err(format!(
                "Tab title exceeds the {UI_TAB_TITLE_MAX_BYTES}-byte UI limit"
            ));
        }
        if note.len() > UI_TAB_NOTE_MAX_BYTES {
            return Err(format!(
                "Tab note exceeds the {UI_TAB_NOTE_MAX_BYTES}-byte UI limit"
            ));
        }
        Ok(Some(TabEditorChanges { name, note }))
    }

    pub(crate) fn snapshot_modal(&self) -> serde_json::Value {
        json!({
            "kind": "tab-editor",
            "target": self.target.as_deref().unwrap_or(""),
            "name_length": self.name_draft.chars().count(),
            "note_length": self.note_draft.chars().count(),
            "focus": self.focus.as_str(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tab_editor_is_closed() {
        let dialog = TabEditorDialog::new();
        assert!(!dialog.is_open());
        assert_eq!(dialog.target(), None);
        assert_eq!(dialog.focus(), TabEditorFocus::Name);
    }

    #[test]
    fn open_loads_target_and_drafts() {
        let mut dialog = TabEditorDialog::new();
        dialog.open("@9".to_owned(), "Name".to_owned(), "Note".to_owned());
        assert!(dialog.is_open());
        assert_eq!(dialog.target(), Some("@9"));
        assert_eq!(dialog.name_draft(), "Name");
        assert_eq!(dialog.note_draft(), "Note");
        assert_eq!(dialog.focus(), TabEditorFocus::Name);
    }

    #[test]
    fn active_draft_follows_focus() {
        let mut dialog = TabEditorDialog::new();
        dialog.open("@9".to_owned(), "Name".to_owned(), "Note".to_owned());
        assert_eq!(
            dialog.active_draft_mut().map(|draft| draft.as_str()),
            Some("Name")
        );
        dialog.next_field();
        assert_eq!(
            dialog.active_draft_mut().map(|draft| draft.as_str()),
            Some("Note")
        );
    }

    #[test]
    fn capture_cancel_leaves_dialog_open() {
        let mut dialog = TabEditorDialog::new();
        dialog.open("@9".to_owned(), "Name".to_owned(), "Note".to_owned());
        assert_eq!(dialog.capture(false).expect("cancel"), None);
        assert!(dialog.is_open());
    }

    #[test]
    fn capture_rejects_empty_title() {
        let mut dialog = TabEditorDialog::new();
        dialog.open("@9".to_owned(), "  ".to_owned(), String::new());
        let error = dialog.capture(true).expect_err("empty title");
        assert!(error.contains("cannot be empty"));
        assert!(dialog.is_open());
    }

    #[test]
    fn capture_returns_changes_and_keeps_dialog_open_for_persistence() {
        let mut dialog = TabEditorDialog::new();
        dialog.open("@9".to_owned(), "  Name  ".to_owned(), "Note".to_owned());
        let changes = dialog.capture(true).expect("valid").expect("changes");
        assert_eq!(changes.name, "Name");
        assert_eq!(changes.note, "Note");
        assert!(dialog.is_open());
    }

    #[test]
    fn capture_rejects_oversized_note() {
        let mut dialog = TabEditorDialog::new();
        dialog.open(
            "@9".to_owned(),
            "Name".to_owned(),
            "x".repeat(UI_TAB_NOTE_MAX_BYTES + 1),
        );
        let error = dialog.capture(true).expect_err("oversized note");
        assert!(error.contains("Tab note exceeds"));
    }

    #[test]
    fn snapshot_exposes_target_lengths_and_focus() {
        let mut dialog = TabEditorDialog::new();
        dialog.open("@9".to_owned(), "Name".to_owned(), "Note".to_owned());
        dialog.set_focus(TabEditorFocus::Note);
        let snapshot = dialog.snapshot_modal();
        assert_eq!(snapshot["kind"], "tab-editor");
        assert_eq!(snapshot["target"], "@9");
        assert_eq!(snapshot["name_length"], 4);
        assert_eq!(snapshot["note_length"], 4);
        assert_eq!(snapshot["focus"], "note");
    }
}
