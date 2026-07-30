use std::{collections::BTreeMap, env, path::PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    locale::LocaleId,
    theme::ThemeId,
    ui_geometry::{TABS_DEFAULT_WIDTH, clamp_configured_tabs_width},
};

pub(crate) const DEFAULT_TABS_WIDTH: u16 = TABS_DEFAULT_WIDTH as u16;
pub(crate) const MIN_TERMINAL_FONT_SIZE: u16 = 8;
pub(crate) const MAX_TERMINAL_FONT_SIZE: u16 = 36;
pub(crate) const DEFAULT_TERMINAL_FONT_SIZE: u16 = if cfg!(target_os = "macos") { 14 } else { 12 };

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TerminalAppearanceOverride {
    pub(crate) terminal_font_family: Option<String>,
    pub(crate) terminal_font_size: Option<u16>,
    pub(crate) color_theme: Option<ThemeId>,
}

impl TerminalAppearanceOverride {
    pub(crate) fn is_empty(&self) -> bool {
        self.terminal_font_family.is_none()
            && self.terminal_font_size.is_none()
            && self.color_theme.is_none()
    }

    fn normalize(&mut self) {
        self.terminal_font_family = self
            .terminal_font_family
            .take()
            .map(|family| family.trim().to_owned())
            .filter(|family| !family.is_empty() && family.len() <= 256);
        self.terminal_font_size = self
            .terminal_font_size
            .map(|size| size.clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EffectiveTerminalAppearance {
    pub(crate) terminal_font_family: String,
    pub(crate) terminal_font_size: u16,
    pub(crate) color_theme: ThemeId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    pub(crate) terminal_font_family: String,
    pub(crate) terminal_font_size: u16,
    pub(crate) color_theme: ThemeId,
    pub(crate) locale: LocaleId,
    pub(crate) terminal_overrides: BTreeMap<String, TerminalAppearanceOverride>,
    pub(crate) tabs_visible: bool,
    #[serde(deserialize_with = "deserialize_tabs_width")]
    pub(crate) tabs_width: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            terminal_font_family: "Consolas".to_owned(),
            terminal_font_size: DEFAULT_TERMINAL_FONT_SIZE,
            color_theme: ThemeId::Dark,
            locale: LocaleId::English,
            terminal_overrides: BTreeMap::new(),
            tabs_visible: true,
            tabs_width: DEFAULT_TABS_WIDTH,
        }
    }
}

impl AppConfig {
    pub(crate) fn normalize(&mut self) {
        self.terminal_font_family = self.terminal_font_family.trim().to_owned();
        if self.terminal_font_family.is_empty() || self.terminal_font_family.len() > 256 {
            self.terminal_font_family = Self::default().terminal_font_family;
        }
        self.terminal_font_size = self
            .terminal_font_size
            .clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE);
        self.tabs_width = clamp_tabs_width(i32::from(self.tabs_width));
        self.terminal_overrides.retain(|key, value| {
            value.normalize();
            !key.is_empty() && key.len() <= 320 && !value.is_empty()
        });
    }

    pub(crate) fn terminal_override_key(address: &str, tab_id: &str) -> String {
        format!("{address}|{tab_id}")
    }

    pub(crate) fn terminal_override(
        &self,
        address: &str,
        tab_id: &str,
    ) -> TerminalAppearanceOverride {
        self.terminal_override_entry(address, tab_id)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn terminal_override_entry(
        &self,
        address: &str,
        tab_id: &str,
    ) -> Option<&TerminalAppearanceOverride> {
        self.terminal_overrides
            .get(&Self::terminal_override_key(address, tab_id))
    }

    pub(crate) fn set_terminal_override(
        &mut self,
        address: &str,
        tab_id: &str,
        mut value: TerminalAppearanceOverride,
    ) {
        value.normalize();
        let key = Self::terminal_override_key(address, tab_id);
        if value.is_empty() {
            self.terminal_overrides.remove(&key);
        } else {
            self.terminal_overrides.insert(key, value);
        }
    }

    pub(crate) fn effective_terminal_appearance(
        &self,
        address: &str,
        tab_id: Option<&str>,
    ) -> EffectiveTerminalAppearance {
        let terminal_override = tab_id
            .map(|tab_id| self.terminal_override(address, tab_id))
            .unwrap_or_default();
        EffectiveTerminalAppearance {
            terminal_font_family: terminal_override
                .terminal_font_family
                .unwrap_or_else(|| self.terminal_font_family.clone()),
            terminal_font_size: terminal_override
                .terminal_font_size
                .unwrap_or(self.terminal_font_size),
            color_theme: terminal_override.color_theme.unwrap_or(self.color_theme),
        }
    }
}

pub(crate) fn clamp_tabs_width(width: i32) -> u16 {
    clamp_configured_tabs_width(width) as u16
}

fn deserialize_tabs_width<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let width = i64::deserialize(deserializer)?;
    let width = i32::try_from(width).unwrap_or(if width.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    });
    Ok(clamp_tabs_width(width))
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
    let mut normalized = config.clone();
    normalized.normalize();
    let json = serde_json::to_string_pretty(&normalized)?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_geometry::{TABS_MAX_WIDTH, TABS_MIN_WIDTH};

    #[test]
    fn missing_fields_receive_stable_defaults() {
        let config: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.terminal_font_family, "Consolas");
        assert_eq!(config.terminal_font_size, DEFAULT_TERMINAL_FONT_SIZE);
        assert_eq!(config.color_theme, ThemeId::Dark);
        assert_eq!(config.locale, LocaleId::English);
        assert!(config.terminal_overrides.is_empty());
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
        assert_eq!(config.locale, LocaleId::English);
        assert!(config.tabs_visible);
        assert_eq!(config.tabs_width, DEFAULT_TABS_WIDTH);
    }

    #[test]
    fn settings_round_trip_uses_stable_theme_id() {
        let config = AppConfig {
            terminal_font_family: "Consolas".to_owned(),
            terminal_font_size: 13,
            color_theme: ThemeId::Light,
            locale: LocaleId::TraditionalChinese,
            terminal_overrides: BTreeMap::new(),
            tabs_visible: false,
            tabs_width: 333,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""color_theme":"light""#));
        assert!(json.contains(r#""locale":"zh-Hant""#));
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
        assert_eq!(narrow.tabs_width, TABS_MIN_WIDTH as u16);
        assert_eq!(wide.tabs_width, TABS_MAX_WIDTH as u16);

        let mut config = AppConfig {
            tabs_width: 0,
            ..AppConfig::default()
        };
        config.normalize();
        assert_eq!(config.tabs_width, TABS_MIN_WIDTH as u16);
    }

    #[test]
    fn persisted_width_clamp_delegates_to_geometry_policy() {
        assert_eq!(clamp_tabs_width(250), 250);
        assert_eq!(clamp_tabs_width(0), TABS_MIN_WIDTH as u16);
        assert_eq!(clamp_tabs_width(10_000), TABS_MAX_WIDTH as u16);
    }

    #[test]
    fn terminal_overrides_merge_per_field_and_can_return_to_inheritance() {
        let mut config = AppConfig {
            terminal_font_family: "Consolas".to_owned(),
            terminal_font_size: 12,
            color_theme: ThemeId::Dark,
            ..AppConfig::default()
        };
        config.set_terminal_override(
            "127.0.0.1:48815",
            "@7",
            TerminalAppearanceOverride {
                terminal_font_family: None,
                terminal_font_size: Some(18),
                color_theme: Some(ThemeId::Light),
            },
        );

        let effective = config.effective_terminal_appearance("127.0.0.1:48815", Some("@7"));
        assert_eq!(effective.terminal_font_family, "Consolas");
        assert_eq!(effective.terminal_font_size, 18);
        assert_eq!(effective.color_theme, ThemeId::Light);
        assert_eq!(
            config
                .effective_terminal_appearance("127.0.0.1:48815", Some("@8"))
                .terminal_font_size,
            12
        );

        config.set_terminal_override(
            "127.0.0.1:48815",
            "@7",
            TerminalAppearanceOverride::default(),
        );
        assert!(
            config
                .terminal_override_entry("127.0.0.1:48815", "@7")
                .is_none()
        );
        assert!(config.terminal_override("127.0.0.1:48815", "@7").is_empty());
    }

    #[test]
    fn invalid_field_types_are_rejected() {
        assert!(serde_json::from_str::<AppConfig>(r#"{"color_theme":false}"#).is_err());
        assert!(serde_json::from_str::<AppConfig>(r#"{"tabs_width":"wide"}"#).is_err());
        assert!(serde_json::from_str::<AppConfig>(r#"{"tabs_width":250.5}"#).is_err());
        assert!(serde_json::from_str::<AppConfig>(r#"{"tabs_visible":"yes"}"#).is_err());
    }

    #[test]
    #[cfg(windows)]
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
