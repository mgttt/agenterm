//! Scan production `.rhai` scripts for rh subset compatibility (report-only).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{check, RhError};

pub const SCAN_FILES_MAX: usize = 256;
pub const SCAN_TOTAL_SOURCE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const SCAN_FILE_MAX_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct CorpusScanEntry {
    pub path: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CorpusScanReport {
    pub schema_version: u32,
    pub kind: &'static str,
    pub scanned: usize,
    pub passed: usize,
    pub failed: usize,
    pub total_source_bytes: usize,
    pub entries: Vec<CorpusScanEntry>,
}

#[derive(Clone, Debug)]
pub struct CorpusScanOptions {
    pub project_root: PathBuf,
    pub relative_dir: String,
    pub tasks_manifest: Option<PathBuf>,
}

impl Default for CorpusScanOptions {
    fn default() -> Self {
        Self {
            project_root: PathBuf::from("."),
            relative_dir: "scripts/rhai".to_owned(),
            tasks_manifest: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TaskManifest {
    tasks: Vec<TaskEntry>,
}

#[derive(Debug, Deserialize)]
struct TaskEntry {
    entry: String,
}

pub fn extract_task_entries(manifest_path: &Path) -> Result<Vec<String>, RhError> {
    let metadata = std::fs::metadata(manifest_path).map_err(|err| RhError::Parse(err.to_string()))?;
    if metadata.len() as usize > 512 * 1024 {
        return Err(RhError::Parse("task manifest exceeds 512 KiB".into()));
    }
    let bytes = std::fs::read(manifest_path).map_err(|err| RhError::Parse(err.to_string()))?;
    let manifest: TaskManifest =
        serde_json::from_slice(&bytes).map_err(|err| RhError::Parse(err.to_string()))?;
    if manifest.tasks.is_empty() || manifest.tasks.len() > SCAN_FILES_MAX {
        return Err(RhError::Parse(format!(
            "task manifest must contain 1..={SCAN_FILES_MAX} tasks"
        )));
    }
    let mut entries = HashSet::new();
    for task in manifest.tasks {
        if !task.entry.ends_with(".rhai") {
            return Err(RhError::Parse(format!(
                "task entry must be a .rhai path: {}",
                task.entry
            )));
        }
        entries.insert(task.entry.replace('\\', "/"));
    }
    let mut sorted = entries.into_iter().collect::<Vec<_>>();
    sorted.sort();
    Ok(sorted)
}

pub fn scan_task_manifest(options: CorpusScanOptions) -> Result<CorpusScanReport, RhError> {
    let manifest_path = options
        .tasks_manifest
        .clone()
        .unwrap_or_else(|| options.project_root.join("agenterm.tasks.json"));
    let entries = extract_task_entries(&manifest_path)?;
    let mut report = scan_relative_files(&options.project_root, &entries)?;
    report.kind = "agenterm-rh-corpus-scan-tasks";
    Ok(report)
}

pub fn scan_rhai_directory(options: CorpusScanOptions) -> Result<CorpusScanReport, RhError> {
    if options.tasks_manifest.is_some() {
        return scan_task_manifest(options);
    }
    let dir = options.project_root.join(&options.relative_dir);
    if !dir.is_dir() {
        return Err(RhError::Parse(format!(
            "corpus dir not found: {}",
            dir.display()
        )));
    }
    let mut relative_paths = Vec::new();
    collect_rhai_files(&dir, &options.project_root, &mut relative_paths)?;
    relative_paths.sort();
    if relative_paths.len() > SCAN_FILES_MAX {
        return Err(RhError::Parse(format!(
            "corpus exceeds {SCAN_FILES_MAX} files"
        )));
    }
    scan_relative_files(&options.project_root, &relative_paths)
}

fn collect_rhai_files(
    dir: &Path,
    root: &Path,
    out: &mut Vec<String>,
) -> Result<(), RhError> {
    for entry in std::fs::read_dir(dir).map_err(|err| RhError::Parse(err.to_string()))? {
        let entry = entry.map_err(|err| RhError::Parse(err.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rhai_files(&path, root, out)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rhai") {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|err| RhError::Parse(err.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        out.push(relative);
        if out.len() > SCAN_FILES_MAX {
            break;
        }
    }
    Ok(())
}

pub fn scan_relative_files(root: &Path, paths: &[String]) -> Result<CorpusScanReport, RhError> {
    let mut entries = Vec::new();
    let mut total_source_bytes = 0_usize;
    let mut passed = 0_usize;

    for relative in paths {
        let path = root.join(relative);
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(err) => {
                entries.push(CorpusScanEntry {
                    path: relative.clone(),
                    ok: false,
                    error: Some(err.to_string()),
                });
                continue;
            }
        };
        if source.len() > SCAN_FILE_MAX_BYTES {
            entries.push(CorpusScanEntry {
                path: relative.clone(),
                ok: false,
                error: Some(format!(
                    "source exceeds per-file limit of {SCAN_FILE_MAX_BYTES} bytes"
                )),
            });
            continue;
        }
        total_source_bytes = total_source_bytes.saturating_add(source.len());
        if total_source_bytes > SCAN_TOTAL_SOURCE_MAX_BYTES {
            return Err(RhError::Parse(format!(
                "corpus aggregate exceeds {SCAN_TOTAL_SOURCE_MAX_BYTES} bytes"
            )));
        }
        match check(&source) {
            Ok(()) => {
                passed += 1;
                entries.push(CorpusScanEntry {
                    path: relative.clone(),
                    ok: true,
                    error: None,
                });
            }
            Err(error) => {
                entries.push(CorpusScanEntry {
                    path: relative.clone(),
                    ok: false,
                    error: Some(error.to_string()),
                });
            }
        }
    }

    let scanned = entries.len();
    Ok(CorpusScanReport {
        schema_version: 1,
        kind: "agenterm-rh-corpus-scan",
        scanned,
        passed,
        failed: scanned.saturating_sub(passed),
        total_source_bytes,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{extract_task_entries, scan_rhai_directory, scan_task_manifest, CorpusScanOptions};

    #[test]
    fn scripts_rhai_scan_produces_report() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let report = scan_rhai_directory(CorpusScanOptions {
            project_root: repo.clone(),
            relative_dir: "scripts/rhai".to_owned(),
            ..CorpusScanOptions::default()
        })
        .expect("scan");
        assert!(report.scanned >= 50, "scanned {}", report.scanned);
        assert_eq!(report.passed, report.scanned, "compat check should pass scripts/rhai");
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn task_manifest_scan_covers_named_tasks() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let entries = extract_task_entries(&repo.join("agenterm.tasks.json")).expect("entries");
        assert!(entries.len() >= 50, "entries {}", entries.len());
        let report = scan_task_manifest(CorpusScanOptions {
            project_root: repo,
            tasks_manifest: None,
            ..CorpusScanOptions::default()
        })
        .expect("scan tasks");
        assert_eq!(report.kind, "agenterm-rh-corpus-scan-tasks");
        assert_eq!(report.scanned, entries.len());
        assert_eq!(report.passed, report.scanned);
        assert_eq!(report.failed, 0);
    }
}
