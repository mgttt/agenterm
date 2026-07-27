use std::{env, path::PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Deserializer, Serialize};

use crate::theme::ThemeId;

pub(crate) const MIN_TABS_WIDTH: u16 = 180;
pub(crate) const DEFAULT_TABS_WIDTH: u16 = 250;
pub(crate) const MAX_TABS_WIDTH: u16 = 480;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    pub(crate) terminal_font_family: String,
    pub(crate) terminal_font_size: u16,
    pub(crate) color_theme: ThemeId,
    pub(crate) tabs_visible: bool,
    #[serde(deserialize_with = "deserialize_tabs_width")]
    pub(crate) tabs_width: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            terminal_font_family: "Consolas".to_owned(),
            terminal_font_size: 12,
            color_theme: ThemeId::Dark,
            tabs_visible: true,
            tabs_width: DEFAULT_TABS_WIDTH,
        }
    }
}

impl AppConfig {
    pub(crate) fn normalize(&mut self) {
        self.tabs_width = clamp_tabs_width(i32::from(self.tabs_width));
    }
}

pub(crate) fn clamp_tabs_width(width: i32) -> u16 {
    width.clamp(i32::from(MIN_TABS_WIDTH), i32::from(MAX_TABS_WIDTH)) as u16
}

/// Resolves a persisted/configured width against a geometry-owned upper bound.
/// A very narrow window may necessarily return less than the normal minimum.
pub(crate) fn effective_tabs_width(configured: u16, available: u16) -> u16 {
    clamp_tabs_width(i32::from(configured)).min(available)
}

fn deserialize_tabs_width<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let width = i64::deserialize(deserializer)?;
    let width = width.clamp(i64::from(MIN_TABS_WIDTH), i64::from(MAX_TABS_WIDTH));
    Ok(width as u16)
}

pub(crate) fn config_path() -> PathBuf {
    config_path_from(
        env::var_os("AGENTERM_SETTINGS_PATH"),
        env::var_os("LOCALAPPDATA"),
    )
}

fn config_path_from(
    override_path: Option<std::ffi::OsString>,
    local_app_data: Option<std::ffi::OsString>,
) -> PathBuf {
    if let Some(path) = override_path
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    local_app_data
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("AgenTerm")
        .join("settings.json")
}

pub(crate) fn load_config() -> AppConfig {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub(crate) fn save_config(config: &AppConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_receive_stable_defaults() {
        let config: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.terminal_font_family, "Consolas");
        assert_eq!(config.terminal_font_size, 12);
        assert_eq!(config.color_theme, ThemeId::Dark);
        assert!(config.tabs_visible);
        assert_eq!(config.tabs_width, DEFAULT_TABS_WIDTH);
    }

    #[test]
    fn old_settings_json_migrates_without_losing_values() {
        let config: AppConfig = serde_json::from_str(
            r#"{"terminal_font_family":"Cascadia Mono","terminal_font_size":15}"#,
        )
        .unwrap();
        assert_eq!(config.terminal_font_family, "Cascadia Mono");
        assert_eq!(config.terminal_font_size, 15);
        assert_eq!(config.color_theme, ThemeId::Dark);
        assert!(config.tabs_visible);
        assert_eq!(config.tabs_width, DEFAULT_TABS_WIDTH);
    }

    #[test]
    fn settings_round_trip_uses_stable_theme_id() {
        let config = AppConfig {
            terminal_font_family: "Consolas".to_owned(),
            terminal_font_size: 13,
            color_theme: ThemeId::Light,
            tabs_visible: false,
            tabs_width: 333,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""color_theme":"light""#));
        assert_eq!(serde_json::from_str::<AppConfig>(&json).unwrap(), config);
    }

    #[test]
    fn unknown_theme_falls_back_without_discarding_other_settings() {
        let config: AppConfig = serde_json::from_str(
            r#"{"terminal_font_size":17,"color_theme":"from-the-future","tabs_width":300}"#,
        )
        .unwrap();
        assert_eq!(config.terminal_font_size, 17);
        assert_eq!(config.color_theme, ThemeId::Dark);
        assert_eq!(config.tabs_width, 300);
    }

    #[test]
    fn tabs_width_is_clamped_during_deserialization_and_normalization() {
        let narrow: AppConfig = serde_json::from_str(r#"{"tabs_width":-10}"#).unwrap();
        let wide: AppConfig = serde_json::from_str(r#"{"tabs_width":100000}"#).unwrap();
        assert_eq!(narrow.tabs_width, MIN_TABS_WIDTH);
        assert_eq!(wide.tabs_width, MAX_TABS_WIDTH);

        let mut config = AppConfig::default();
        config.tabs_width = 0;
        config.normalize();
        assert_eq!(config.tabs_width, MIN_TABS_WIDTH);
    }

    #[test]
    fn configured_and_effective_widths_are_separate() {
        assert_eq!(clamp_tabs_width(250), 250);
        assert_eq!(clamp_tabs_width(0), MIN_TABS_WIDTH);
        assert_eq!(clamp_tabs_width(10_000), MAX_TABS_WIDTH);
        assert_eq!(effective_tabs_width(300, 240), 240);
        assert_eq!(effective_tabs_width(300, 500), 300);
        assert_eq!(effective_tabs_width(0, 500), MIN_TABS_WIDTH);
    }

    #[test]
    fn invalid_field_types_are_rejected() {
        assert!(serde_json::from_str::<AppConfig>(r#"{"color_theme":false}"#).is_err());
        assert!(serde_json::from_str::<AppConfig>(r#"{"tabs_width":"wide"}"#).is_err());
        assert!(serde_json::from_str::<AppConfig>(r#"{"tabs_width":250.5}"#).is_err());
        assert!(serde_json::from_str::<AppConfig>(r#"{"tabs_visible":"yes"}"#).is_err());
    }

    #[test]
    fn explicit_settings_path_overrides_the_local_app_data_default() {
        assert_eq!(
            config_path_from(Some(r"D:\isolated\settings.json".into()), None),
            PathBuf::from(r"D:\isolated\settings.json")
        );
        assert_eq!(
            config_path_from(Some("".into()), Some(r"D:\profile".into())),
            PathBuf::from(r"D:\profile\AgenTerm\settings.json")
        );
    }
}
