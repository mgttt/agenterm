//! Bounded multi-file rh subset validation (parity with agenterm-rhai
//! check-many). Manifest/report shape, path-confinement, and the
//! check-many driver loop now live in `agenterm_script_common::check_many`
//! (shared with lua/qjs — see that module's doc for what's unified vs.
//! kept per-engine, and for the note that rh's own driver loop is what the
//! shared implementation was originally lifted from). This file keeps only
//! what's genuinely rh-specific: the two accepted manifest `kind` strings
//! (rh's own plus the legacy rhai kind, for thin-forward compat), CLI flag
//! parsing, and the `check_with_project_validation` adapter closure
//! (including its `RhError` → `CheckFailure` mapping).

use std::path::{Path, PathBuf};

use agenterm_script_common::check_many::{self, CheckFailure};

pub use agenterm_script_common::check_many::{
    CheckManyFailure, CheckManyManifest, CheckManyOptions, CheckManyReport, DEFAULT_SOURCE_BYTES,
    DEFAULT_WALL_TIME_MS, FILES_MAX, MANIFEST_MAX_BYTES, ParsedCheckManyCli, PATH_MAX_BYTES,
    TOTAL_SOURCE_MAX_BYTES,
};

use crate::{RhError, check_with_project_validation};

const RHAI_CHECK_MANIFEST_KIND: &str = "agenterm-rhai-check-manifest";
const RH_CHECK_MANIFEST_KIND: &str = "agenterm-rh-check-manifest";

pub fn read_manifest(path: &Path) -> Result<CheckManyManifest, RhError> {
    check_many::read_manifest(path, &[RH_CHECK_MANIFEST_KIND, RHAI_CHECK_MANIFEST_KIND])
        .map_err(RhError::Parse)
}

pub fn run_check_many(manifest: CheckManyManifest, options: CheckManyOptions) -> CheckManyReport {
    check_many::run_check_many(
        manifest,
        options,
        "agenterm-rhai-check-many",
        |source, _path, root| {
            check_with_project_validation(source, Some(root)).map_err(|error| check_failure(&error))
        },
    )
}

fn check_failure(error: &RhError) -> CheckFailure {
    match error {
        RhError::Parse(message) => CheckFailure::new("rh_subset", message.clone(), "script"),
        RhError::Subset { code, detail } => CheckFailure::new(*code, detail.clone(), "script"),
        RhError::Compile(message) => {
            let (code, detail) = message
                .split_once(": ")
                .filter(|(code, _)| !code.is_empty())
                .unwrap_or(("rh_check", message.as_str()));
            CheckFailure::new(code, detail.to_owned(), "script")
        }
        RhError::Transpile(message) => CheckFailure::new("rh_transpile", message.clone(), "script"),
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
        assert_eq!(report.checked_files, 77);
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
        assert_eq!(report.checked_files, 77);
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
            fixture_dir.join("unknown-api.rh"),
            "fn entry() { std::fs::not_shipped(`x`) }\n",
        )
        .expect("write temp script");
        let report = run_check_many(
            super::CheckManyManifest {
                schema_version: 1,
                kind: super::RH_CHECK_MANIFEST_KIND.to_owned(),
                files: vec!["unknown-api.rh".to_owned()],
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
