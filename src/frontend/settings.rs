//! Shared settings modal state, validation, and ui-action policy.

use serde_json::json;

use crate::{
    settings::{
        EffectiveTerminalAppearance, MAX_TERMINAL_FONT_SIZE, MIN_TERMINAL_FONT_SIZE,
        TerminalAppearanceOverride,
    },
    theme::AppearancePreset,
};

/// Horizontal gap between the two columns of the appearance preset picker.
pub(crate) const APPEARANCE_PRESET_GRID_GAP: i32 = 8;
/// Vertical stride from one preset row top to the next (includes button height).
pub(crate) const APPEARANCE_PRESET_GRID_ROW_STRIDE: i32 = 42;
/// Preset button height shared by Win and Unix settings chrome.
pub(crate) const APPEARANCE_PRESET_BUTTON_HEIGHT: i32 = 34;

/// One cell in the shared 2×2 appearance-preset picker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AppearancePresetGridCell {
    pub(crate) preset: AppearancePreset,
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl AppearancePresetGridCell {
    pub(crate) const fn right(self) -> i32 {
        self.left + self.width
    }

    pub(crate) const fn bottom(self) -> i32 {
        self.top + self.height
    }

    /// Unix paint/hit-test shape `(x, y, w, h)`. Negative origins clamp to zero.
    pub(crate) fn as_xywh_u32(self) -> (u32, u32, u32, u32) {
        (
            u32::try_from(self.left.max(0)).unwrap_or(0),
            u32::try_from(self.top.max(0)).unwrap_or(0),
            u32::try_from(self.width.max(0)).unwrap_or(0),
            u32::try_from(self.height.max(0)).unwrap_or(0),
        )
    }
}

/// Lay out built-in presets in `AppearancePreset::ALL` reading order:
/// classic-day | classic-night / fancy-day | fancy-night.
///
/// `content_left` / `content_width` are the inner picker band (modal inset already
/// applied). Hosts keep paint and native controls; only geometry+order is shared.
pub(crate) fn appearance_preset_grid(
    content_left: i32,
    grid_top: i32,
    content_width: i32,
) -> [AppearancePresetGridCell; 4] {
    let gap = APPEARANCE_PRESET_GRID_GAP;
    let button_width = ((content_width - gap) / 2).max(120);
    let height = APPEARANCE_PRESET_BUTTON_HEIGHT;
    let row_stride = APPEARANCE_PRESET_GRID_ROW_STRIDE;
    let mut cells = [AppearancePresetGridCell {
        preset: AppearancePreset::classic_day(),
        left: 0,
        top: 0,
        width: 0,
        height: 0,
    }; 4];
    for (index, preset) in AppearancePreset::ALL.into_iter().enumerate() {
        let col = i32::try_from(index % 2).unwrap_or(0);
        let row = i32::try_from(index / 2).unwrap_or(0);
        cells[index] = AppearancePresetGridCell {
            preset,
            left: content_left + col * (button_width + gap),
            top: grid_top + row * row_stride,
            width: button_width,
            height,
        };
    }
    cells
}

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
    preset_draft: AppearancePreset,
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
            preset_draft: default_draft.appearance_preset,
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

    pub(crate) fn preset_draft(&self) -> AppearancePreset {
        self.preset_draft
    }

    /// Compatibility alias for callers still named around theme drafts.
    pub(crate) fn theme_draft(&self) -> AppearancePreset {
        self.preset_draft
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
                self.default_draft.appearance_preset = self.preset_draft;
            }
            SettingsScope::CurrentTerminal => {
                if self.override_draft.terminal_font_family.is_some() {
                    self.override_draft.terminal_font_family = Some(family);
                }
                if self.override_draft.terminal_font_size.is_some() {
                    self.override_draft.terminal_font_size = Some(size);
                }
                if self.override_draft.appearance_preset.is_some() {
                    self.override_draft.appearance_preset = Some(self.preset_draft);
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

    pub(crate) fn effective_drafts(&self) -> (String, u16, AppearancePreset) {
        match self.scope {
            SettingsScope::Defaults => (
                self.default_draft.terminal_font_family.clone(),
                self.default_draft.terminal_font_size,
                self.default_draft.appearance_preset,
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
                    .appearance_preset
                    .unwrap_or(self.default_draft.appearance_preset),
            ),
        }
    }

    pub(crate) fn load_effective_drafts(&mut self) {
        let (family, size, preset) = self.effective_drafts();
        self.font_family_draft = family;
        self.font_size_draft = size.to_string();
        self.preset_draft = preset;
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

    pub(crate) fn preview_preset(&mut self, preset: AppearancePreset) -> bool {
        if !self.open
            || (self.scope == SettingsScope::CurrentTerminal
                && self.override_draft.appearance_preset.is_none())
        {
            return false;
        }
        self.preset_draft = preset;
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
                self.override_draft.appearance_preset = self
                    .override_draft
                    .appearance_preset
                    .take()
                    .is_none()
                    .then_some(self.default_draft.appearance_preset);
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
            "appearance_preset_draft": self.preset_draft.as_str(),
            "theme_draft": self.preset_draft.as_str(),
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
    use crate::theme::AppearancePreset;

    fn appearance() -> EffectiveTerminalAppearance {
        EffectiveTerminalAppearance {
            terminal_font_family: "Consolas".to_owned(),
            terminal_font_size: 14,
            appearance_preset: AppearancePreset::classic_night(),
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
                appearance_preset: Some(AppearancePreset::classic_day()),
            },
        );
        dialog.set_font_family_draft("Edited".to_owned());
        dialog.set_font_size_draft("20".to_owned());
        dialog.preview_preset(AppearancePreset::classic_day());
        dialog.open(appearance(), None, TerminalAppearanceOverride::default());
        assert!(dialog.is_open());
        assert_eq!(dialog.scope(), SettingsScope::Defaults);
        assert_eq!(dialog.font_family_draft(), "Consolas");
        assert_eq!(dialog.font_size_draft(), "14");
        assert_eq!(dialog.preset_draft(), AppearancePreset::classic_night());
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
        dialog.preview_preset(AppearancePreset::classic_day());
        dialog.capture().expect("valid draft");
        let changes = dialog.changes();
        assert_eq!(
            changes.default_appearance.terminal_font_family,
            "Cascadia Mono"
        );
        assert_eq!(changes.default_appearance.terminal_font_size, 18);
        assert_eq!(
            changes.default_appearance.appearance_preset,
            AppearancePreset::classic_day()
        );
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
                appearance_preset: Some(AppearancePreset::classic_night()),
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
                appearance_preset: Some(AppearancePreset::classic_day()),
            },
        );
        dialog
            .switch_scope(SettingsScope::CurrentTerminal)
            .expect("switch");
        assert_eq!(dialog.font_family_draft(), "Cascadia Mono");
        assert!(dialog.reset_overrides());
        assert_eq!(dialog.font_family_draft(), "Consolas");
        assert_eq!(dialog.font_size_draft(), "14");
        assert_eq!(dialog.preset_draft(), AppearancePreset::classic_night());
    }

    #[test]
    fn appearance_preset_grid_matches_all_order_and_2x2_geometry() {
        let cells = appearance_preset_grid(40, 200, 280);
        assert_eq!(cells.map(|cell| cell.preset), AppearancePreset::ALL);
        let button_width = ((280 - APPEARANCE_PRESET_GRID_GAP) / 2).max(120);
        assert_eq!(cells[0].left, 40);
        assert_eq!(cells[0].top, 200);
        assert_eq!(cells[0].width, button_width);
        assert_eq!(cells[0].height, APPEARANCE_PRESET_BUTTON_HEIGHT);
        assert_eq!(
            cells[1].left,
            40 + button_width + APPEARANCE_PRESET_GRID_GAP
        );
        assert_eq!(cells[1].top, 200);
        assert_eq!(cells[2].left, 40);
        assert_eq!(cells[2].top, 200 + APPEARANCE_PRESET_GRID_ROW_STRIDE);
        assert_eq!(cells[3].left, cells[1].left);
        assert_eq!(cells[3].top, cells[2].top);
        assert_eq!(cells[0].as_xywh_u32(), (40, 200, button_width as u32, 34));
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
                appearance_preset: Some(AppearancePreset::classic_day()),
            },
        );
        dialog
            .switch_scope(SettingsScope::CurrentTerminal)
            .expect("switch");
        let snapshot = dialog.snapshot_modal();
        assert_eq!(snapshot["kind"], "settings");
        assert_eq!(snapshot["scope"], "current-terminal");
        assert_eq!(snapshot["target_tab_id"], "@1");
        assert_eq!(snapshot["appearance_preset_draft"], "classic-day");
        assert_eq!(snapshot["theme_draft"], "classic-day");
        assert!(snapshot["font_family_configured"].as_bool() == Some(true));
        assert!(snapshot["font_size_configured"].as_bool() == Some(true));
    }
}
