//! Bounded multi-file rh subset validation (parity with agenterm-rhai check-many).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{check, RhError};

pub const MANIFEST_MAX_BYTES: usize = 64 * 1024;
pub const FILES_MAX: usize = 256;
pub const PATH_MAX_BYTES: usize = 4 * 1024;
pub const TOTAL_SOURCE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_WALL_TIME_MS: u64 = 10_000;
pub const DEFAULT_SOURCE_BYTES: usize = 512 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckManyManifest {
    pub schema_version: u32,
    pub kind: String,
    pub files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckManyFailure {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct CheckManyReport {
    pub schema_version: u32,
    pub kind: &'static str,
    pub ok: bool,
    pub checked_files: usize,
    pub total_source_bytes: usize,
    pub duration_ms: u64,
    pub failures: Vec<CheckManyFailure>,
}

#[derive(Clone, Debug)]
pub struct CheckManyOptions {
    pub project_root: PathBuf,
    pub wall_time_ms: u64,
    pub source_bytes: usize,
}

impl Default for CheckManyOptions {
    fn default() -> Self {
        Self {
            project_root: PathBuf::from("."),
            wall_time_ms: DEFAULT_WALL_TIME_MS,
            source_bytes: DEFAULT_SOURCE_BYTES,
        }
    }
}

pub fn read_manifest(path: &Path) -> Result<CheckManyManifest, RhError> {
    let metadata = std::fs::metadata(path).map_err(|err| RhError::Parse(err.to_string()))?;
    if metadata.len() as usize > MANIFEST_MAX_BYTES {
        return Err(RhError::Parse(format!(
            "manifest exceeds {MANIFEST_MAX_BYTES} bytes"
        )));
    }
    let bytes = std::fs::read(path).map_err(|err| RhError::Parse(err.to_string()))?;
    let manifest: CheckManyManifest =
        serde_json::from_slice(&bytes).map_err(|err| RhError::Parse(err.to_string()))?;
    if manifest.schema_version != 1 {
        return Err(RhError::Parse("unsupported manifest schema_version".into()));
    }
    if manifest.kind != "agenterm-rh-check-manifest" {
        return Err(RhError::Parse(format!(
            "unexpected manifest kind `{}`",
            manifest.kind
        )));
    }
    if manifest.files.is_empty() || manifest.files.len() > FILES_MAX {
        return Err(RhError::Parse(format!(
            "manifest must list 1..={FILES_MAX} files"
        )));
    }
    Ok(manifest)
}

pub fn run_check_many(
    manifest: CheckManyManifest,
    options: CheckManyOptions,
) -> CheckManyReport {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(options.wall_time_ms);
    let root = options.project_root;
    let mut failures = Vec::new();
    let mut checked_files = 0;
    let mut total_source_bytes = 0_usize;
    let mut seen = HashSet::new();

    for (ordinal, label) in manifest.files.into_iter().enumerate() {
        if Instant::now() >= deadline {
            failures.push(failure(
                label,
                "limit_wall_time",
                "check-many reached its aggregate wall-time budget".to_owned(),
            ));
            continue;
        }
        if label.is_empty() || label.len() > PATH_MAX_BYTES {
            failures.push(failure(
                label,
                "check_many_path",
                "path label is empty or exceeds byte limit".to_owned(),
            ));
            continue;
        }
        if !seen.insert(label.clone()) {
            failures.push(failure(
                label,
                "check_many_duplicate",
                "duplicate manifest path".to_owned(),
            ));
            continue;
        }
        let path = root.join(&label);
        let source = match read_source_file(&path, options.source_bytes) {
            Ok(source) => source,
            Err(error) => {
                failures.push(failure(label, "check_many_read", error.to_string()));
                continue;
            }
        };
        let next_total = total_source_bytes.saturating_add(source.len());
        if next_total > TOTAL_SOURCE_MAX_BYTES {
            failures.push(failure(
                label,
                "check_many_total_source_bytes",
                format!("aggregate source exceeds {TOTAL_SOURCE_MAX_BYTES} bytes"),
            ));
            continue;
        }
        total_source_bytes = next_total;
        checked_files += 1;
        if let Err(error) = check(&source) {
            failures.push(failure(
                label,
                "rh_subset",
                error.to_string(),
            ));
            let _ = ordinal;
        }
    }

    CheckManyReport {
        schema_version: 1,
        kind: "agenterm-rh-check-many",
        ok: failures.is_empty(),
        checked_files,
        total_source_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        failures,
    }
}

fn read_source_file(path: &Path, max_bytes: usize) -> Result<String, RhError> {
    let metadata = std::fs::metadata(path).map_err(|err| RhError::Parse(err.to_string()))?;
    if metadata.len() as usize > max_bytes {
        return Err(RhError::Parse(format!(
            "source exceeds per-file limit of {max_bytes} bytes"
        )));
    }
    std::fs::read_to_string(path).map_err(|err| RhError::Parse(err.to_string()))
}

fn failure(path: String, code: &str, message: String) -> CheckManyFailure {
    CheckManyFailure {
        path,
        code: code.to_owned(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{read_manifest, run_check_many, CheckManyOptions};

    #[test]
    fn fixture_manifest_checks_all_rh_files() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest_path = repo.join("fixtures/rh/check-many.json");
        let manifest = read_manifest(&manifest_path).expect("manifest");
        let report = run_check_many(
            manifest,
            CheckManyOptions {
                project_root: repo,
                ..CheckManyOptions::default()
            },
        );
        assert!(
            report.ok,
            "failures: {:?}",
            report.failures
        );
        assert_eq!(report.checked_files, 7);
    }
}
