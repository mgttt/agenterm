//! Shared settings modal state, validation, and ui-action policy.

use serde_json::json;

use crate::{
    settings::{
        EffectiveTerminalAppearance, MAX_TERMINAL_FONT_SIZE, MIN_TERMINAL_FONT_SIZE,
        TerminalAppearanceOverride,
    },
    theme::ThemeId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettingsScope {
    Defaults,
    CurrentTerminal,
}

impl SettingsScope {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Defaults => "defaults",
            Self::CurrentTerminal => "current-terminal",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppearanceField {
    FontFamily,
    FontSize,
    Theme,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SettingsChanges {
    pub(crate) default_appearance: EffectiveTerminalAppearance,
    pub(crate) override_draft: TerminalAppearanceOverride,
    pub(crate) target_tab_id: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct SettingsDialog {
    open: bool,
    scope: SettingsScope,
    default_draft: EffectiveTerminalAppearance,
    override_draft: TerminalAppearanceOverride,
    target_tab_id: Option<String>,
    theme_draft: ThemeId,
    font_family_draft: String,
    font_size_draft: String,
}

impl SettingsDialog {
    pub(crate) fn new(default_draft: EffectiveTerminalAppearance) -> Self {
        Self {
            open: false,
            scope: SettingsScope::Defaults,
            default_draft: default_draft.clone(),
            override_draft: TerminalAppearanceOverride::default(),
            target_tab_id: None,
            theme_draft: default_draft.color_theme,
            font_family_draft: default_draft.terminal_font_family.clone(),
            font_size_draft: default_draft.terminal_font_size.to_string(),
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn scope(&self) -> SettingsScope {
        self.scope
    }

    pub(crate) fn target_tab_id(&self) -> Option<&str> {
        self.target_tab_id.as_deref()
    }

    pub(crate) fn theme_draft(&self) -> ThemeId {
        self.theme_draft
    }

    pub(crate) fn font_family_draft(&self) -> &str {
        &self.font_family_draft
    }

    pub(crate) fn font_size_draft(&self) -> &str {
        &self.font_size_draft
    }

    pub(crate) fn override_draft(&self) -> &TerminalAppearanceOverride {
        &self.override_draft
    }

    pub(crate) fn open(
        &mut self,
        default_draft: EffectiveTerminalAppearance,
        target_tab_id: Option<String>,
        override_draft: TerminalAppearanceOverride,
    ) {
        self.open = true;
        self.scope = SettingsScope::Defaults;
        self.default_draft = default_draft;
        self.target_tab_id = target_tab_id;
        self.override_draft = override_draft;
        self.load_effective_drafts();
    }

    pub(crate) fn close_without_apply(&mut self) {
        self.open = false;
    }

    pub(crate) fn set_font_family_draft(&mut self, value: String) {
        if self.open {
            self.font_family_draft = value;
        }
    }

    pub(crate) fn set_font_size_draft(&mut self, value: String) {
        if self.open {
            self.font_size_draft = value;
        }
    }

    pub(crate) fn capture(&mut self) -> Result<(), String> {
        if !self.open {
            return Ok(());
        }
        let family = self.font_family_draft.trim().to_owned();
        let size = self
            .font_size_draft
            .trim()
            .parse::<u16>()
            .map_err(|_| "font size must be a number from 8 to 36".to_owned())?;
        if family.is_empty()
            || family.len() > 256
            || !(MIN_TERMINAL_FONT_SIZE..=MAX_TERMINAL_FONT_SIZE).contains(&size)
        {
            return Err(
                "font family is required (maximum 256 bytes) and size must be 8 to 36".to_owned(),
            );
        }
        match self.scope {
            SettingsScope::Defaults => {
                self.default_draft.terminal_font_family = family;
                self.default_draft.terminal_font_size = size;
                self.default_draft.color_theme = self.theme_draft;
            }
            SettingsScope::CurrentTerminal => {
                if self.override_draft.terminal_font_family.is_some() {
                    self.override_draft.terminal_font_family = Some(family);
                }
                if self.override_draft.terminal_font_size.is_some() {
                    self.override_draft.terminal_font_size = Some(size);
                }
                if self.override_draft.color_theme.is_some() {
                    self.override_draft.color_theme = Some(self.theme_draft);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn changes(&self) -> SettingsChanges {
        SettingsChanges {
            default_appearance: self.default_draft.clone(),
            override_draft: self.override_draft.clone(),
            target_tab_id: self.target_tab_id.clone(),
        }
    }

    pub(crate) fn effective_drafts(&self) -> (String, u16, ThemeId) {
        match self.scope {
            SettingsScope::Defaults => (
                self.default_draft.terminal_font_family.clone(),
                self.default_draft.terminal_font_size,
                self.default_draft.color_theme,
            ),
            SettingsScope::CurrentTerminal => (
                self.override_draft
                    .terminal_font_family
                    .clone()
                    .unwrap_or_else(|| self.default_draft.terminal_font_family.clone()),
                self.override_draft
                    .terminal_font_size
                    .unwrap_or(self.default_draft.terminal_font_size),
                self.override_draft
                    .color_theme
                    .unwrap_or(self.default_draft.color_theme),
            ),
        }
    }

    pub(crate) fn load_effective_drafts(&mut self) {
        let (family, size, theme) = self.effective_drafts();
        self.font_family_draft = family;
        self.font_size_draft = size.to_string();
        self.theme_draft = theme;
    }

    pub(crate) fn switch_scope(&mut self, scope: SettingsScope) -> Result<bool, String> {
        if !self.open
            || scope == self.scope
            || (scope == SettingsScope::CurrentTerminal && self.target_tab_id.is_none())
        {
            return Ok(false);
        }
        self.capture()?;
        self.scope = scope;
        self.load_effective_drafts();
        Ok(true)
    }

    pub(crate) fn preview_theme(&mut self, theme: ThemeId) -> bool {
        if !self.open
            || (self.scope == SettingsScope::CurrentTerminal
                && self.override_draft.color_theme.is_none())
        {
            return false;
        }
        self.theme_draft = theme;
        true
    }

    pub(crate) fn toggle_inheritance(&mut self, field: AppearanceField) -> Result<bool, String> {
        if !self.open || self.scope != SettingsScope::CurrentTerminal {
            return Ok(false);
        }
        self.capture()?;
        match field {
            AppearanceField::FontFamily => {
                self.override_draft.terminal_font_family = self
                    .override_draft
                    .terminal_font_family
                    .take()
                    .is_none()
                    .then(|| self.default_draft.terminal_font_family.clone());
            }
            AppearanceField::FontSize => {
                self.override_draft.terminal_font_size = self
                    .override_draft
                    .terminal_font_size
                    .take()
                    .is_none()
                    .then_some(self.default_draft.terminal_font_size);
            }
            AppearanceField::Theme => {
                self.override_draft.color_theme = self
                    .override_draft
                    .color_theme
                    .take()
                    .is_none()
                    .then_some(self.default_draft.color_theme);
            }
        }
        self.load_effective_drafts();
        Ok(true)
    }

    pub(crate) fn reset_overrides(&mut self) -> bool {
        if !self.open || self.scope != SettingsScope::CurrentTerminal {
            return false;
        }
        self.override_draft = TerminalAppearanceOverride::default();
        self.load_effective_drafts();
        true
    }

    pub(crate) fn snapshot_modal(&self) -> serde_json::Value {
        json!({
            "kind": "settings",
            "scope": self.scope.as_str(),
            "target_tab_id": self.target_tab_id,
            "theme_draft": self.theme_draft.as_str(),
            "font_family_configured": !self.font_family_draft.trim().is_empty(),
            "font_size_configured": !self.font_size_draft.trim().is_empty(),
            "default_action": "settings-apply",
            "actions": ["settings-apply", "settings-cancel"],
        })
    }
}

pub(crate) fn ui_action_open(
    dialog: &mut SettingsDialog,
    default_draft: EffectiveTerminalAppearance,
    target_tab_id: Option<String>,
    override_draft: TerminalAppearanceOverride,
) -> bool {
    if dialog.is_open() {
        return false;
    }
    dialog.open(default_draft, target_tab_id, override_draft);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appearance() -> EffectiveTerminalAppearance {
        EffectiveTerminalAppearance {
            terminal_font_family: "Consolas".to_owned(),
            terminal_font_size: 14,
            color_theme: ThemeId::Dark,
        }
    }

    #[test]
    fn open_resets_drafts_to_defaults() {
        let mut dialog = SettingsDialog::new(appearance());
        dialog.open(
            appearance(),
            None,
            TerminalAppearanceOverride {
                terminal_font_family: Some("Cascadia Mono".to_owned()),
                terminal_font_size: Some(18),
                color_theme: Some(ThemeId::Light),
            },
        );
        dialog.set_font_family_draft("Edited".to_owned());
        dialog.set_font_size_draft("20".to_owned());
        dialog.preview_theme(ThemeId::Light);
        dialog.open(appearance(), None, TerminalAppearanceOverride::default());
        assert!(dialog.is_open());
        assert_eq!(dialog.scope(), SettingsScope::Defaults);
        assert_eq!(dialog.font_family_draft(), "Consolas");
        assert_eq!(dialog.font_size_draft(), "14");
        assert_eq!(dialog.theme_draft(), ThemeId::Dark);
    }

    #[test]
    fn capture_rejects_invalid_size_and_keeps_dialog_open() {
        let mut dialog = SettingsDialog::new(appearance());
        dialog.open(appearance(), None, TerminalAppearanceOverride::default());
        dialog.set_font_size_draft("99".to_owned());
        let error = dialog.capture().expect_err("invalid size");
        assert!(error.contains("8 to 36"));
        assert!(dialog.is_open());
    }

    #[test]
    fn capture_updates_default_draft_and_returns_changes() {
        let mut dialog = SettingsDialog::new(appearance());
        dialog.open(appearance(), None, TerminalAppearanceOverride::default());
        dialog.set_font_family_draft("Cascadia Mono".to_owned());
        dialog.set_font_size_draft("18".to_owned());
        dialog.preview_theme(ThemeId::Light);
        dialog.capture().expect("valid draft");
        let changes = dialog.changes();
        assert_eq!(
            changes.default_appearance.terminal_font_family,
            "Cascadia Mono"
        );
        assert_eq!(changes.default_appearance.terminal_font_size, 18);
        assert_eq!(changes.default_appearance.color_theme, ThemeId::Light);
    }

    #[test]
    fn switch_scope_captures_edits_before_loading_effective_drafts() {
        let default = appearance();
        let mut dialog = SettingsDialog::new(default.clone());
        dialog.open(
            default.clone(),
            Some("@1".to_owned()),
            TerminalAppearanceOverride {
                terminal_font_family: Some("Consolas".to_owned()),
                terminal_font_size: None,
                color_theme: Some(ThemeId::Dark),
            },
        );
        dialog
            .switch_scope(SettingsScope::CurrentTerminal)
            .expect("switch");
        dialog.set_font_family_draft("Cascadia Mono".to_owned());
        dialog
            .switch_scope(SettingsScope::Defaults)
            .expect("switch back");
        assert_eq!(dialog.font_family_draft(), "Consolas");
        dialog
            .switch_scope(SettingsScope::CurrentTerminal)
            .expect("switch");
        assert_eq!(dialog.font_family_draft(), "Cascadia Mono");
    }

    #[test]
    fn toggle_inheritance_returns_to_defaults_and_back() {
        let default = appearance();
        let mut dialog = SettingsDialog::new(default.clone());
        dialog.open(
            default.clone(),
            Some("@1".to_owned()),
            TerminalAppearanceOverride::default(),
        );
        dialog
            .switch_scope(SettingsScope::CurrentTerminal)
            .expect("switch");
        dialog
            .toggle_inheritance(AppearanceField::FontSize)
            .expect("toggle");
        assert_eq!(dialog.font_size_draft(), "14");
        assert_eq!(dialog.changes().override_draft.terminal_font_size, Some(14));
        dialog
            .toggle_inheritance(AppearanceField::FontSize)
            .expect("toggle");
        assert_eq!(dialog.changes().override_draft.terminal_font_size, None);
        assert_eq!(dialog.font_size_draft(), "14");
    }

    #[test]
    fn reset_overrides_restores_inherited_effective_drafts() {
        let default = appearance();
        let mut dialog = SettingsDialog::new(default.clone());
        dialog.open(
            default.clone(),
            Some("@1".to_owned()),
            TerminalAppearanceOverride {
                terminal_font_family: Some("Cascadia Mono".to_owned()),
                terminal_font_size: Some(18),
                color_theme: Some(ThemeId::Light),
            },
        );
        dialog
            .switch_scope(SettingsScope::CurrentTerminal)
            .expect("switch");
        assert_eq!(dialog.font_family_draft(), "Cascadia Mono");
        assert!(dialog.reset_overrides());
        assert_eq!(dialog.font_family_draft(), "Consolas");
        assert_eq!(dialog.font_size_draft(), "14");
        assert_eq!(dialog.theme_draft(), ThemeId::Dark);
    }

    #[test]
    fn snapshot_redacts_no_settings_secrets_and_exposes_scope() {
        let mut dialog = SettingsDialog::new(appearance());
        dialog.open(
            appearance(),
            Some("@1".to_owned()),
            TerminalAppearanceOverride {
                terminal_font_family: Some("Cascadia Mono".to_owned()),
                terminal_font_size: Some(18),
                color_theme: Some(ThemeId::Light),
            },
        );
        dialog
            .switch_scope(SettingsScope::CurrentTerminal)
            .expect("switch");
        let snapshot = dialog.snapshot_modal();
        assert_eq!(snapshot["kind"], "settings");
        assert_eq!(snapshot["scope"], "current-terminal");
        assert_eq!(snapshot["target_tab_id"], "@1");
        assert_eq!(snapshot["theme_draft"], "light");
        assert!(snapshot["font_family_configured"].as_bool() == Some(true));
        assert!(snapshot["font_size_configured"].as_bool() == Some(true));
    }
}
