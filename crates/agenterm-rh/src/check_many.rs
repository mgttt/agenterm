//! Bounded multi-file rh subset validation (parity with agenterm-rhai check-many).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{RhError, check_with_project_validation};

pub const MANIFEST_MAX_BYTES: usize = 64 * 1024;
pub const FILES_MAX: usize = 256;
pub const PATH_MAX_BYTES: usize = 4 * 1024;
pub const TOTAL_SOURCE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_WALL_TIME_MS: u64 = 10_000;
pub const DEFAULT_SOURCE_BYTES: usize = 512 * 1024;

const RHAI_CHECK_MANIFEST_KIND: &str = "agenterm-rhai-check-manifest";
const RH_CHECK_MANIFEST_KIND: &str = "agenterm-rh-check-manifest";

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

pub fn read_manifest(path: &Path) -> Result<CheckManyManifest, RhError> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| RhError::Parse(format!("check_many_manifest_read: {err}")))?;
    if !metadata.is_file() || metadata.len() as usize > MANIFEST_MAX_BYTES {
        return Err(RhError::Parse(format!(
            "check_many_manifest_size: manifest must be a file of at most {MANIFEST_MAX_BYTES} bytes"
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|err| RhError::Parse(format!("check_many_manifest_read: {err}")))?;
    let manifest: CheckManyManifest = serde_json::from_slice(&bytes)
        .map_err(|err| RhError::Parse(format!("check_many_manifest_json: {err}")))?;
    if manifest.schema_version != 1 {
        return Err(RhError::Parse("check_many_manifest_schema".into()));
    }
    if manifest.kind != RH_CHECK_MANIFEST_KIND && manifest.kind != RHAI_CHECK_MANIFEST_KIND {
        return Err(RhError::Parse("check_many_manifest_schema".into()));
    }
    if manifest.files.is_empty() || manifest.files.len() > FILES_MAX {
        return Err(RhError::Parse(format!(
            "check_many_manifest_files: expected from 1 to {FILES_MAX} files"
        )));
    }
    Ok(manifest)
}

pub fn run_check_many(manifest: CheckManyManifest, options: CheckManyOptions) -> CheckManyReport {
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
                ordinal,
                "limit",
            ));
            continue;
        }
        if label.is_empty() || label.len() > PATH_MAX_BYTES || Path::new(&label).is_absolute() {
            failures.push(failure(
                label,
                "check_many_path",
                "path label is empty or exceeds byte limit".to_owned(),
                ordinal,
                "configuration",
            ));
            continue;
        }
        let requested = root.join(&label);
        let path = match std::fs::canonicalize(&requested) {
            Ok(path) if path.is_file() && path.starts_with(&root) => path,
            Ok(path) if !path.starts_with(&root) => {
                failures.push(failure(
                    label,
                    "check_many_path",
                    "manifest path escapes the project root".to_owned(),
                    ordinal,
                    "configuration",
                ));
                continue;
            }
            Ok(_) => {
                failures.push(failure(
                    label,
                    "host_source_read",
                    "script source is not a file".to_owned(),
                    ordinal,
                    "host",
                ));
                continue;
            }
            Err(error) => {
                failures.push(failure(
                    label,
                    "host_source_resolve",
                    error.to_string(),
                    ordinal,
                    "host",
                ));
                continue;
            }
        };
        if !seen.insert(path.clone()) {
            failures.push(failure(
                label,
                "check_many_duplicate",
                "manifest resolves the same file more than once".to_owned(),
                ordinal,
                "configuration",
            ));
            continue;
        }
        let source = match read_source_file(&path, options.source_bytes) {
            Ok(source) => source,
            Err(SourceReadFailure::Limit(message)) => {
                failures.push(failure(
                    label,
                    "limit_source_bytes",
                    message,
                    ordinal,
                    "limit",
                ));
                continue;
            }
            Err(SourceReadFailure::Host(error)) => {
                failures.push(failure(
                    label,
                    "host_source_read",
                    error.to_string(),
                    ordinal,
                    "host",
                ));
                continue;
            }
        };
        let next_total = total_source_bytes.saturating_add(source.len());
        if next_total > TOTAL_SOURCE_MAX_BYTES {
            failures.push(failure(
                label,
                "check_many_total_source_bytes",
                format!("aggregate source exceeds {TOTAL_SOURCE_MAX_BYTES} bytes"),
                ordinal,
                "limit",
            ));
            continue;
        }
        total_source_bytes = next_total;
        checked_files += 1;
        if let Err(error) = check_with_project_validation(&source, Some(&root)) {
            failures.push(check_failure(label, &error, ordinal));
        }
    }

    report(started, checked_files, total_source_bytes, failures)
}

fn report(
    started: Instant,
    checked_files: usize,
    total_source_bytes: usize,
    failures: Vec<CheckManyFailure>,
) -> CheckManyReport {
    CheckManyReport {
        schema_version: 1,
        kind: "agenterm-rhai-check-many",
        ok: failures.is_empty(),
        checked_files,
        total_source_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        failures,
    }
}

enum SourceReadFailure {
    Limit(String),
    Host(RhError),
}

fn read_source_file(path: &Path, max_bytes: usize) -> Result<String, SourceReadFailure> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| SourceReadFailure::Host(RhError::Parse(err.to_string())))?;
    if metadata.len() as usize > max_bytes {
        return Err(SourceReadFailure::Limit(format!(
            "source exceeds per-file limit of {max_bytes} bytes"
        )));
    }
    std::fs::read_to_string(path)
        .map_err(|err| SourceReadFailure::Host(RhError::Parse(err.to_string())))
}

fn failure(
    path: String,
    code: &str,
    message: String,
    ordinal: usize,
    exit_class: &'static str,
) -> CheckManyFailure {
    CheckManyFailure {
        path,
        code: code.to_owned(),
        message,
        invocation_id: format!("check-many-{}-{ordinal}", std::process::id()),
        exit_class,
    }
}

fn check_failure(path: String, error: &RhError, ordinal: usize) -> CheckManyFailure {
    match error {
        RhError::Parse(message) => failure(path, "rh_subset", message.clone(), ordinal, "script"),
        RhError::Subset { code, detail } => failure(path, code, detail.clone(), ordinal, "script"),
        RhError::Compile(message) => {
            let (code, detail) = message
                .split_once(": ")
                .filter(|(code, _)| !code.is_empty())
                .unwrap_or(("rh_check", message.as_str()));
            failure(path, code, detail.to_owned(), ordinal, "script")
        }
        RhError::Transpile(message) => {
            failure(path, "rh_transpile", message.clone(), ordinal, "script")
        }
    }
}

impl CheckManyReport {
    pub fn exit_code(&self) -> u8 {
        self.failures
            .first()
            .map_or(0, |failure| match failure.exit_class {
                "configuration" => 2,
                "limit" => 3,
                "child" => 4,
                "cancelled" => 5,
                "fleet" => 6,
                _ => 1,
            })
    }
}

/// Parse `check-many` argv with rhai-compatible options accepted for thin-forward migration.
pub fn parse_check_many_cli<I>(mut args: I) -> Result<ParsedCheckManyCli, RhError>
where
    I: Iterator<Item = String>,
{
    let mut manifest_path = None::<PathBuf>;
    let mut project_root = PathBuf::from(".");
    let mut wall_time_ms = DEFAULT_WALL_TIME_MS;
    let mut source_bytes = DEFAULT_SOURCE_BYTES;
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => {
                manifest_path = Some(PathBuf::from(next_value(&mut args, "--manifest")?));
            }
            "--project-root" => {
                project_root = PathBuf::from(next_value(&mut args, "--project-root")?);
            }
            "--timeout-ms" => {
                wall_time_ms = next_value(&mut args, "--timeout-ms")?
                    .parse()
                    .map_err(|err| RhError::Parse(format!("timeout-ms: {err}")))?;
            }
            "--max-output-bytes" => {
                let value = next_value(&mut args, "--max-output-bytes")?
                    .parse::<usize>()
                    .map_err(|err| RhError::Parse(format!("max-output-bytes: {err}")))?;
                source_bytes = source_bytes.min(value);
            }
            "--profile" => {
                let profile = next_value(&mut args, "--profile")?;
                if !matches!(profile.as_str(), "local" | "pure" | "observe") {
                    return Err(RhError::Parse(format!("unknown script profile: {profile}")));
                }
            }
            "--max-operations" | "--max-collection-items" | "--max-string-bytes" => {
                let _ = next_value(&mut args, arg.as_str())?;
            }
            "--json" => json = true,
            other => {
                return Err(RhError::Parse(format!(
                    "unknown check-many option `{other}`"
                )));
            }
        }
    }
    let manifest_path = manifest_path
        .ok_or_else(|| RhError::Parse("check-many requires --manifest FILE".into()))?;
    Ok(ParsedCheckManyCli {
        manifest_path,
        options: CheckManyOptions {
            project_root,
            wall_time_ms,
            source_bytes,
        },
        json,
    })
}

fn next_value<I>(args: &mut I, option: &str) -> Result<String, RhError>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| RhError::Parse(format!("missing value after {option}")))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        CheckManyManifest, CheckManyOptions, RH_CHECK_MANIFEST_KIND, TOTAL_SOURCE_MAX_BYTES,
        parse_check_many_cli, read_manifest, run_check_many,
    };

    #[test]
    fn accepts_rhai_manifest_kind_and_compat_flags() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest_path = repo.join("fixtures/rh/check-many.json");
        let parsed = parse_check_many_cli(
            [
                "--manifest",
                &manifest_path.display().to_string(),
                "--profile",
                "local",
                "--project-root",
                &repo.display().to_string(),
                "--timeout-ms",
                "10000",
                "--max-operations",
                "1000000",
                "--json",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect("parse");
        let mut manifest = read_manifest(&parsed.manifest_path).expect("manifest");
        manifest.kind = super::RHAI_CHECK_MANIFEST_KIND.to_owned();
        let report = run_check_many(manifest, parsed.options);
        assert!(report.ok, "failures: {:?}", report.failures);
        assert_eq!(report.checked_files, 20);
    }

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
        assert!(report.ok, "failures: {:?}", report.failures);
        assert_eq!(report.checked_files, 20);
    }

    fn manifest_for(file: &str) -> CheckManyManifest {
        CheckManyManifest {
            schema_version: 1,
            kind: RH_CHECK_MANIFEST_KIND.to_owned(),
            files: vec![file.to_owned()],
        }
    }

    #[test]
    fn check_many_classifies_per_file_source_budget_as_limit() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        std::fs::write(project.path().join("oversized.rh"), "40 + 2\n")
            .expect("write oversized script");

        let report = run_check_many(
            manifest_for("oversized.rh"),
            CheckManyOptions {
                project_root: project_path.clone(),
                source_bytes: 1,
                ..CheckManyOptions::default()
            },
        );

        assert!(!report.ok);
        assert_eq!(report.checked_files, 0);
        assert_eq!(report.total_source_bytes, 0);
        assert_eq!(report.failures[0].code, "limit_source_bytes");
        assert_eq!(report.failures[0].exit_class, "limit");
        assert_eq!(report.exit_code(), 3);
        drop(project);
        assert!(!project_path.exists(), "temporary project was not removed");
    }

    #[test]
    fn check_many_enforces_aggregate_source_budget_with_limit_exit() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        let source_path = project.path().join("aggregate.rh");
        let source = std::fs::File::create(&source_path).expect("create aggregate script");
        source
            .set_len((TOTAL_SOURCE_MAX_BYTES + 1) as u64)
            .expect("size aggregate script");
        drop(source);

        let report = run_check_many(
            manifest_for("aggregate.rh"),
            CheckManyOptions {
                project_root: project_path.clone(),
                source_bytes: TOTAL_SOURCE_MAX_BYTES + 1,
                ..CheckManyOptions::default()
            },
        );

        assert!(!report.ok);
        assert_eq!(report.checked_files, 0);
        assert_eq!(report.total_source_bytes, 0);
        assert_eq!(report.failures[0].code, "check_many_total_source_bytes");
        assert_eq!(report.failures[0].exit_class, "limit");
        assert_eq!(report.exit_code(), 3);
        drop(project);
        assert!(!project_path.exists(), "temporary project was not removed");
    }

    #[test]
    fn check_many_enforces_zero_wall_time_without_sleeping() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        std::fs::write(project.path().join("valid.rh"), "40 + 2\n").expect("write valid script");

        let report = run_check_many(
            manifest_for("valid.rh"),
            CheckManyOptions {
                project_root: project_path.clone(),
                wall_time_ms: 0,
                ..CheckManyOptions::default()
            },
        );

        assert!(!report.ok);
        assert_eq!(report.checked_files, 0);
        assert_eq!(report.total_source_bytes, 0);
        assert_eq!(report.failures[0].code, "limit_wall_time");
        assert_eq!(report.failures[0].exit_class, "limit");
        assert_eq!(report.exit_code(), 3);
        drop(project);
        assert!(!project_path.exists(), "temporary project was not removed");
    }

    #[test]
    fn check_many_rejects_unknown_api() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixture_dir = repo.join("fixtures/rh/check-many-tmp");
        std::fs::create_dir_all(&fixture_dir).expect("create temp dir");
        std::fs::write(
            fixture_dir.join("unknown-api.rhai"),
            "fn entry() { std::fs::not_shipped(`x`) }\n",
        )
        .expect("write temp script");
        let report = run_check_many(
            super::CheckManyManifest {
                schema_version: 1,
                kind: super::RH_CHECK_MANIFEST_KIND.to_owned(),
                files: vec!["unknown-api.rhai".to_owned()],
            },
            CheckManyOptions {
                project_root: fixture_dir.clone(),
                ..CheckManyOptions::default()
            },
        );
        let _ = std::fs::remove_dir_all(fixture_dir);
        assert!(!report.ok, "expected failure: {:?}", report.failures);
        assert_eq!(report.failures[0].code, "script_api_unknown");
    }

    #[test]
    fn check_many_rejects_absolute_paths_outside_project_root() {
        let project = tempfile::tempdir().expect("project root");
        let outside = tempfile::NamedTempFile::new().expect("outside script");
        std::fs::write(outside.path(), "40 + 2\n").expect("write outside script");
        let report = run_check_many(
            super::CheckManyManifest {
                schema_version: 1,
                kind: super::RH_CHECK_MANIFEST_KIND.to_owned(),
                files: vec![outside.path().display().to_string()],
            },
            CheckManyOptions {
                project_root: project.path().to_path_buf(),
                ..CheckManyOptions::default()
            },
        );
        assert!(!report.ok);
        assert_eq!(report.failures[0].code, "check_many_path");
        assert_eq!(report.failures[0].exit_class, "configuration");
    }
}
