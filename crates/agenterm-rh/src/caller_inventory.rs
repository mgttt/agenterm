//! Report-only inventory of operational `agenterm-rhai` references (M22 prep).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::RhError;

pub const NEEDLE: &str = "agenterm-rhai";
pub const SCAN_FILES_MAX: usize = 4096;
pub const SCAN_FILE_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct CallerHit {
    pub path: String,
    pub line: u32,
    pub category: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallerInventoryReport {
    pub schema_version: u32,
    pub kind: &'static str,
    pub needle: &'static str,
    pub scanned_files: usize,
    pub hit_count: usize,
    pub categories: BTreeMap<String, usize>,
    pub hits: Vec<CallerHit>,
}

#[derive(Clone, Debug)]
pub struct CallerInventoryOptions {
    pub project_root: PathBuf,
}

impl Default for CallerInventoryOptions {
    fn default() -> Self {
        Self {
            project_root: PathBuf::from("."),
        }
    }
}

pub fn scan_caller_inventory(
    options: CallerInventoryOptions,
) -> Result<CallerInventoryReport, RhError> {
    let paths = list_tracked_files(&options.project_root)?;
    let mut hits = Vec::new();
    let mut categories = BTreeMap::<String, usize>::new();

    let mut scanned_files = 0_usize;
    for relative in &paths {
        let path = options.project_root.join(relative);
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() as usize > SCAN_FILE_MAX_BYTES {
            continue;
        }
        scanned_files += 1;
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            if !line.contains(NEEDLE) {
                continue;
            }
            let category = categorize_path(relative);
            *categories.entry(category.clone()).or_default() += 1;
            hits.push(CallerHit {
                path: relative.clone(),
                line: index as u32 + 1,
                category,
                text: line.trim().to_owned(),
            });
        }
    }

    let hit_count = hits.len();
    Ok(CallerInventoryReport {
        schema_version: 1,
        kind: "agenterm-rh-caller-inventory",
        needle: NEEDLE,
        scanned_files,
        hit_count,
        categories,
        hits,
    })
}

fn list_tracked_files(root: &Path) -> Result<Vec<String>, RhError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|err| RhError::Parse(format!("git ls-files failed: {err}")))?;
    if !output.status.success() {
        return Err(RhError::Parse(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let mut paths = Vec::new();
    for entry in output.stdout.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let relative = String::from_utf8(entry.to_vec())
            .map_err(|err| RhError::Parse(format!("git path utf-8: {err}")))?;
        if should_scan_path(&relative) {
            paths.push(relative);
        }
        if paths.len() > SCAN_FILES_MAX {
            return Err(RhError::Parse(format!(
                "caller inventory exceeds {SCAN_FILES_MAX} tracked files"
            )));
        }
    }
    paths.sort();
    Ok(paths)
}

fn should_scan_path(relative: &str) -> bool {
    if relative.starts_with("target/") || relative.starts_with("dist/") {
        return false;
    }
    let Some(extension) = Path::new(relative).extension().and_then(|ext| ext.to_str()) else {
        return matches!(
            relative,
            "install.sh" | "AGENTS.md" | "README.md" | "PRD.md" | "Cargo.toml"
        );
    };
    matches!(
        extension,
        "rs" | "sh" | "cmd" | "bat" | "yml" | "yaml" | "json" | "rhai" | "md" | "toml"
    )
}

fn categorize_path(relative: &str) -> String {
    if relative.starts_with(".github/workflows/") {
        "ci".into()
    } else if relative.starts_with("scripts/bootstrap.") {
        "bootstrap".into()
    } else if relative == "install.sh" || relative.starts_with("install.") {
        "install".into()
    } else if relative == "agenterm.tasks.json" {
        "task-manifest".into()
    } else if relative.starts_with("scripts/rhai/") {
        "rhai-script".into()
    } else if relative.starts_with("src/") {
        "rust-src".into()
    } else if relative.starts_with("tests/") {
        "rust-test".into()
    } else if relative.starts_with("crates/") {
        "rust-crate".into()
    } else if relative.starts_with("scripts/artifacts.json")
        || relative.starts_with("scripts/qualification")
    {
        "release-artifacts".into()
    } else if relative.starts_with("plan/")
        || relative.starts_with("prd/")
        || relative.starts_with("docs/")
    {
        "documentation".into()
    } else {
        "other".into()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{scan_caller_inventory, CallerInventoryOptions, NEEDLE};

    #[test]
    fn caller_inventory_finds_operational_references() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let report = scan_caller_inventory(CallerInventoryOptions {
            project_root: repo,
        })
        .expect("inventory");
        assert_eq!(report.needle, NEEDLE);
        assert!(report.hit_count >= 40, "hits {}", report.hit_count);
        assert!(
            report.categories.get("bootstrap").copied().unwrap_or(0) >= 1,
            "bootstrap callers: {:?}",
            report.categories
        );
        assert!(
            report.categories.get("ci").copied().unwrap_or(0) >= 5,
            "ci callers: {:?}",
            report.categories
        );
        assert!(
            report
                .hits
                .iter()
                .any(|hit| hit.path == "scripts/bootstrap.sh"),
            "expected bootstrap.sh hit"
        );
    }
}
