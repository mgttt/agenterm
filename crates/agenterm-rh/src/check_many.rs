//! Bounded multi-file rh subset validation (parity with agenterm-rhai check-many).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{check_with_project_validation, RhError};

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
    if manifest.kind != RH_CHECK_MANIFEST_KIND && manifest.kind != RHAI_CHECK_MANIFEST_KIND {
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
        if let Err(error) = check_with_project_validation(&source, Some(&root)) {
            failures.push(check_failure(label, &error));
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

fn check_failure(path: String, error: &RhError) -> CheckManyFailure {
    match error {
        RhError::Parse(message) => failure(path, "rh_subset", message.clone()),
        RhError::Subset { code, detail } => failure(path, code, detail.clone()),
        RhError::Compile(message) => {
            let (code, detail) = message
                .split_once(": ")
                .filter(|(code, _)| !code.is_empty())
                .unwrap_or(("rh_check", message.as_str()));
            failure(path, code, detail.to_owned())
        }
        RhError::Transpile(message) => failure(path, "rh_transpile", message.clone()),
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
            other => return Err(RhError::Parse(format!("unknown check-many option `{other}`"))),
        }
    }
    let manifest_path =
        manifest_path.ok_or_else(|| RhError::Parse("check-many requires --manifest FILE".into()))?;
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

    use super::{parse_check_many_cli, read_manifest, run_check_many, CheckManyOptions};

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
        assert_eq!(report.checked_files, 8);
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
        assert!(
            report.ok,
            "failures: {:?}",
            report.failures
        );
        assert_eq!(report.checked_files, 8);
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
}
