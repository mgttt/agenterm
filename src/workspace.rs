use std::{env, path::PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SavedWorkspace {
    pub(crate) version: u32,
    pub(crate) session_name: String,
    pub(crate) active_id: Option<u64>,
    pub(crate) collapsed_ids: Vec<u64>,
    pub(crate) tabs: Vec<SavedTab>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SavedTab {
    pub(crate) id: u64,
    pub(crate) index: u32,
    pub(crate) parent_id: Option<u64>,
    pub(crate) title: String,
    pub(crate) note: String,
    pub(crate) composer: String,
    pub(crate) command_line: Vec<String>,
}

pub(crate) fn workspace_path() -> PathBuf {
    env::var_os("AGENTERM_WORKSPACE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_workspace_path)
}

fn default_workspace_path() -> PathBuf {
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("AgenTerm")
            .join("workspace.json")
    }
    #[cfg(unix)]
    {
        use crate::ipc_endpoint::{LogicalInstance, ServerScopeId};

        let instance = env::var("AGENTERM_INSTANCE")
            .ok()
            .and_then(|value| value.parse::<LogicalInstance>().ok())
            .unwrap_or_default();
        ServerScopeId::current(&instance)
            .map(|scope| crate::ipc_endpoint::default_workspace_path(&scope))
            .unwrap_or_else(|_| {
                crate::ipc_endpoint::unix_data_root_from(
                    env::var_os("XDG_DATA_HOME"),
                    env::var_os("HOME"),
                    env::temp_dir(),
                )
                .join("workspaces")
                .join("main.json")
            })
    }
}

pub(crate) fn load_workspace() -> Option<SavedWorkspace> {
    let workspace: SavedWorkspace =
        serde_json::from_str(&std::fs::read_to_string(workspace_path()).ok()?).ok()?;
    (workspace.version == 1 && !workspace.tabs.is_empty()).then_some(workspace)
}

pub(crate) fn save_workspace(workspace: &SavedWorkspace) -> Result<()> {
    let path = workspace_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(workspace)?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_or_empty_workspaces_are_not_restored() {
        let old: SavedWorkspace =
            serde_json::from_str(r#"{"version":0,"tabs":[{"id":1}]}"#).unwrap();
        assert_eq!(old.version, 0);
        let empty: SavedWorkspace = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(empty.tabs.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn unix_default_workspace_is_absolute_and_not_cwd_relative() {
        assert!(default_workspace_path().is_absolute());
        assert!(
            default_workspace_path()
                .components()
                .any(|component| component.as_os_str() == "workspaces")
        );
    }
}
