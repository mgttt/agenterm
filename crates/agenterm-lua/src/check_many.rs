//! Bounded multi-file Lua syntax validation.
//! Aligned with agenterm-rh check_many pattern: manifest → parse → validate → report.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::LuaEngine;

pub const MANIFEST_MAX_BYTES: usize = 64 * 1024;
pub const FILES_MAX: usize = 256;
pub const PATH_MAX_BYTES: usize = 4 * 1024;
pub const TOTAL_SOURCE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_WALL_TIME_MS: u64 = 10_000;
pub const DEFAULT_SOURCE_BYTES: usize = 512 * 1024;

const LUA_CHECK_MANIFEST_KIND: &str = "agenterm-lua-check-manifest";

#[derive(Debug, Clone)]
pub struct ParsedCheckManyCli {
    pub manifest_path: PathBuf,
    pub options: CheckManyOptions,
    pub json: bool,
}

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
    pub invocation_id: String,
    pub exit_class: &'static str,
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

pub fn read_manifest(path: &Path) -> Result<CheckManyManifest, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("check_many_manifest_read: {err}"))?;
    if !metadata.is_file() || metadata.len() as usize > MANIFEST_MAX_BYTES {
        return Err(format!(
            "check_many_manifest_size: manifest must be a file of at most {MANIFEST_MAX_BYTES} bytes"
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|err| format!("check_many_manifest_read: {err}"))?;
    let manifest: CheckManyManifest = serde_json::from_slice(&bytes)
        .map_err(|err| format!("check_many_manifest_json: {err}"))?;
    if manifest.schema_version != 1 {
        return Err("check_many_manifest_schema: expected schema_version=1".into());
    }
    if manifest.kind != LUA_CHECK_MANIFEST_KIND {
        return Err(format!(
            "check_many_manifest_kind: expected {LUA_CHECK_MANIFEST_KIND}, got {}",
            manifest.kind
        ));
    }
    if manifest.files.is_empty() || manifest.files.len() > FILES_MAX {
        return Err(format!(
            "check_many_manifest_files: expected from 1 to {FILES_MAX} files"
        ));
    }
    Ok(manifest)
}

pub fn run_check_many(
    manifest: CheckManyManifest,
    options: CheckManyOptions,
) -> CheckManyReport {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(options.wall_time_ms);
    let root = match std::fs::canonicalize(&options.project_root) {
        Ok(root) if root.is_dir() => root,
        Ok(_) => {
            return report(
                started,
                0,
                0,
                vec![failure(
                    options.project_root.display().to_string(),
                    "check_many_project_root",
                    "project root is not a directory".to_owned(),
                    0,
                    "configuration",
                )],
            );
        }
        Err(error) => {
            return report(
                started,
                0,
                0,
                vec![failure(
                    options.project_root.display().to_string(),
                    "check_many_project_root",
                    error.to_string(),
                    0,
                    "configuration",
                )],
            );
        }
    };

    let engine = match LuaEngine::new() {
        Ok(e) => e,
        Err(err) => {
            return report(
                started,
                0,
                0,
                vec![failure(
                    "".to_owned(),
                    "lua_engine_init",
                    err.to_string(),
                    0,
                    "host",
                )],
            );
        }
    };

    let mut seen = HashSet::new();
    let mut failures = Vec::new();
    let mut total_source = 0usize;
    let mut checked = 0usize;
    let mut sequence = 0u64;

    for raw in &manifest.files {
        if Instant::now() >= deadline {
            failures.push(failure(
                raw.clone(),
                "limit_wall_time",
                "check-many wall-time limit exceeded".to_owned(),
                sequence,
                "limit",
            ));
            // sequence incremented; break out of loop
            break;
        }

        let path = normalize_path(raw, &root);
        if !seen.insert(path.clone()) {
            continue;
        }

        sequence += 1;

        match check_file(path.as_ref(), &engine, &mut total_source, options.source_bytes)
        {
            Ok(()) => {
                checked += 1;
                continue;
            }
            Err(failure_msg) => {
                failures.push(failure(
                    path,
                    failure_msg.code,
                    failure_msg.message,
                    sequence,
                    failure_msg.exit_class,
                ));
                checked += 1;
            }
        };
    }

    report(started, checked, total_source, failures)
}

struct FileCheckFailure {
    code: String,
    message: String,
    exit_class: &'static str,
}

fn check_file(
    path: &Path,
    engine: &LuaEngine,
    total_source: &mut usize,
    source_limit: usize,
) -> Result<(), FileCheckFailure> {
    let bytes = std::fs::read(path).map_err(|err| FileCheckFailure {
        code: "limit_source_read".into(),
        message: format!("failed to read {path}: {err}", path = path.display()),
        exit_class: "limit",
    })?;

    *total_source += bytes.len();
    if *total_source > TOTAL_SOURCE_MAX_BYTES {
        return Err(FileCheckFailure {
            code: "limit_source_bytes".into(),
            message: "check-many total source exceeds byte budget".into(),
            exit_class: "limit",
        });
    }

    let source = String::from_utf8_lossy(&bytes);
    if source.len() > source_limit {
        return Err(FileCheckFailure {
            code: "limit_source_bytes".into(),
            message: format!(
                "{}: source exceeds per-file byte budget",
                path.display()
            ),
            exit_class: "limit",
        });
    }

    engine.check(&source).map_err(|e| FileCheckFailure {
        code: "lua_check".into(),
        message: e.to_string(),
        exit_class: "script",
    })
}

fn normalize_path(raw: &str, root: &Path) -> String {
    let path = Path::new(raw);
    if path.is_absolute() {
        // Resolve to canonical form relative to root if possible
        if let Ok(canonical) = std::fs::canonicalize(path) {
            return canonical.to_string_lossy().into_owned();
        }
        path.to_string_lossy().into_owned()
    } else {
        root.join(path).to_string_lossy().into_owned()
    }
}

fn failure(
    path: String,
    code: impl Into<String>,
    message: String,
    sequence: u64,
    exit_class: &'static str,
) -> CheckManyFailure {
    CheckManyFailure {
        path,
        code: code.into(),
        message,
        invocation_id: format!("lua-check-many-{sequence}"),
        exit_class,
    }
}

fn report(
    started: Instant,
    checked_files: usize,
    total_source_bytes: usize,
    failures: Vec<CheckManyFailure>,
) -> CheckManyReport {
    let elapsed = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let ok = failures.is_empty();
    CheckManyReport {
        schema_version: 1,
        kind: LUA_CHECK_MANIFEST_KIND,
        ok,
        checked_files,
        total_source_bytes,
        duration_ms: elapsed,
        failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_manifest(dir: &TempDir, files: &[&str]) -> PathBuf {
        let path = dir.path().join("manifest.json");
        let manifest = serde_json::json!({
            "schema_version": 1,
            "kind": LUA_CHECK_MANIFEST_KIND,
            "files": files,
        });
        std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        path
    }

    #[test]
    fn read_valid_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir, &["test.lua"]);
        let m = read_manifest(&path).expect("read");
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.files.len(), 1);
    }

    #[test]
    fn read_manifest_rejects_wrong_kind() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("manifest.json");
        let json = serde_json::json!({
            "schema_version": 1,
            "kind": "wrong-kind",
            "files": [],
        });
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
        let err = read_manifest(&path).expect_err("wrong kind");
        assert!(err.contains("kind"), "{err}");
    }

    #[test]
    fn check_many_all_green() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.lua"), "return 42").unwrap();
        std::fs::write(dir.path().join("b.lua"), "return 0").unwrap();
        let manifest_path = write_manifest(&dir, &["a.lua", "b.lua"]);
        let manifest = read_manifest(&manifest_path).expect("read");
        let options = CheckManyOptions {
            project_root: dir.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options);
        assert!(report.ok);
        assert_eq!(report.checked_files, 2);
        assert!(report.failures.is_empty());
    }

    #[test]
    fn check_many_with_syntax_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ok.lua"), "return 1").unwrap();
        std::fs::write(dir.path().join("bad.lua"), "return !!").unwrap();
        let manifest_path = write_manifest(&dir, &["ok.lua", "bad.lua"]);
        let manifest = read_manifest(&manifest_path).expect("read");
        let options = CheckManyOptions {
            project_root: dir.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options);
        assert!(!report.ok);
        assert_eq!(report.checked_files, 2);
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].path.contains("bad.lua"));
        assert_eq!(report.failures[0].exit_class, "script");
    }

    #[test]
    fn check_many_respects_file_limit() {
        let dir = TempDir::new().unwrap();
        let mut files = Vec::new();
        for i in 0..260 {
            let name = format!("f{i}.lua");
            std::fs::write(dir.path().join(&name), "return 0").unwrap();
            files.push(name);
        }
        let manifest_path = write_manifest(&dir, &(0..260).map(|i| format!("f{i}.lua")).collect::<Vec<_>>()
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>());
        let err = read_manifest(&manifest_path).expect_err("too many files");
        assert!(err.contains("files"), "{err}");
    }

    #[test]
    fn check_many_report_serialization() {
        let dir = TempDir::new().unwrap();
        let manifest_path = write_manifest(&dir, &["nonexistent.lua"]);
        let manifest = read_manifest(&manifest_path).expect("read");
        let options = CheckManyOptions {
            project_root: dir.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options);
        let json = serde_json::to_string_pretty(&report).expect("serialize");
        assert!(json.contains("checked_files"));
        assert!(json.contains("failures"));
    }
}
