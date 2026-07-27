use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

const INSTANCE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct InstanceRecord {
    pub schema_version: u32,
    pub pid: u32,
    pub address: String,
    pub version: String,
    pub session: String,
    pub workspace_path: String,
    pub started_at_unix_ms: u128,
}

#[derive(Debug)]
pub(crate) struct DiscoveredInstance {
    pub record: InstanceRecord,
    path: PathBuf,
}

pub(crate) struct InstanceRegistration {
    path: PathBuf,
    pid: u32,
    address: String,
}

impl Drop for InstanceRegistration {
    fn drop(&mut self) {
        let matches_registration = fs::read(&self.path)
            .ok()
            .and_then(|content| serde_json::from_slice::<InstanceRecord>(&content).ok())
            .is_some_and(|record| record.pid == self.pid && record.address == self.address);
        if matches_registration {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn register_instance(
    address: &str,
    workspace_path: &Path,
    session: &str,
) -> Result<InstanceRegistration> {
    register_instance_in(
        &instances_dir(),
        InstanceRecord {
            schema_version: INSTANCE_SCHEMA_VERSION,
            pid: std::process::id(),
            address: address.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            session: session.to_owned(),
            workspace_path: workspace_path.display().to_string(),
            started_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        },
    )
}

pub(crate) fn discover_instances() -> Result<Vec<DiscoveredInstance>> {
    discover_instances_in(&instances_dir())
}

pub(crate) fn prune_instance(instance: &DiscoveredInstance) -> Result<()> {
    fs::remove_file(&instance.path)
        .with_context(|| format!("failed to prune {}", instance.path.display()))
}

fn instances_dir() -> PathBuf {
    if let Some(path) = env::var_os("AGENTERM_INSTANCE_DIR").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("AgenTerm")
        .join("instances")
}

fn register_instance_in(directory: &Path, record: InstanceRecord) -> Result<InstanceRegistration> {
    fs::create_dir_all(directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    let path = directory.join(format!("{}.json", record.pid));
    let temporary = directory.join(format!(
        ".{}.{}.tmp",
        record.pid,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::write(&temporary, serde_json::to_vec_pretty(&record)?)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to replace {}", path.display()))?;
    }
    fs::rename(&temporary, &path)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    Ok(InstanceRegistration {
        path,
        pid: record.pid,
        address: record.address,
    })
}

fn discover_instances_in(directory: &Path) -> Result<Vec<DiscoveredInstance>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut instances = Vec::new();
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<InstanceRecord>(&content) else {
            continue;
        };
        if record.schema_version == INSTANCE_SCHEMA_VERSION {
            instances.push(DiscoveredInstance {
                record,
                path: entry.path(),
            });
        }
    }
    instances.sort_by(|left, right| left.record.address.cmp(&right.record.address));
    Ok(instances)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_is_discoverable_and_removed_on_drop() {
        let directory = env::temp_dir().join(format!(
            "agenterm-instance-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let record = InstanceRecord {
            schema_version: INSTANCE_SCHEMA_VERSION,
            pid: std::process::id(),
            address: "127.0.0.1:49999".to_owned(),
            version: "test".to_owned(),
            session: "fleet".to_owned(),
            workspace_path: "workspace.json".to_owned(),
            started_at_unix_ms: 1,
        };
        let registration = register_instance_in(&directory, record).unwrap();
        let discovered = discover_instances_in(&directory).unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].record.address, "127.0.0.1:49999");
        drop(registration);
        assert!(discover_instances_in(&directory).unwrap().is_empty());
        fs::remove_dir(&directory).unwrap();
    }
}
