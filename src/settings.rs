use std::{env, path::PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    pub(crate) terminal_font_family: String,
    pub(crate) terminal_font_size: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            terminal_font_family: "Consolas".to_owned(),
            terminal_font_size: 12,
        }
    }
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
