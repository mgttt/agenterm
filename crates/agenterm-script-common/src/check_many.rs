//! Generic multi-file "check" driver: manifest → resolve/confine paths →
//! budget-check → read → per-engine checker → report.
//!
//! Before this crate existed, `agenterm-rh`, `agenterm-lua`, and
//! `agenterm-qjs` each carried their own ~300-600 line `check_many.rs` that
//! were, module-doc-comments aside, near-byte-identical: same manifest
//! shape, same report shape, same failure-code taxonomy, same
//! exit-code-by-`exit_class` mapping, same path-escape/duplicate/budget
//! guards (qjs's own doc literally said "structurally aligned with
//! `agenterm_rh::check_many`" and rh's driver loop is what qjs's was
//! copied from). This module makes that one implementation instead of
//! three kept in sync by hand.
//!
//! What stays per-engine, on purpose:
//! - the manifest `kind` string(s) a crate accepts (rh accepts both its own
//!   and the legacy rhai kind for thin-forward compat; lua/qjs accept
//!   exactly one each) — passed to [`read_manifest`] as `accepted_kinds`;
//! - the report's `kind` label — passed to [`run_check_many`] as
//!   `report_kind`;
//! - the actual per-file check — each engine's real checker
//!   (`agenterm_rh::check_with_project_validation`,
//!   `agenterm_qjs::check_with_project_validation`,
//!   `agenterm_lua::LuaEngine::check`) has a different signature and a
//!   different notion of "project root" support, so callers pass their own
//!   `FnMut(&str, &Path, &Path) -> Result<(), CheckFailure>` closure;
//! - CLI argv parsing (`parse_check_many_cli` stays in each crate) — small
//!   enough, and each engine's `?`-propagated error type differs, that
//!   unifying it would add more indirection than it removes.
//!
//! One real behavior fix folded in during extraction, not just a refactor:
//! `agenterm-lua`'s previous `check_many` resolved manifest paths with a
//! plain join-or-canonicalize and **no confinement check** — unlike rh and
//! qjs, which both reject a resolved path that escapes `project_root`.
//! That was a real gap (a manifest could point `../../../../etc/passwd`
//! style outside the intended project), not a stylistic difference between
//! implementations. Routing lua through this shared driver closes it for
//! free — no lua test asserted on the old, weaker behavior (verified
//! before migrating), so this isn't a silent breaking change.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const MANIFEST_MAX_BYTES: usize = 64 * 1024;
pub const FILES_MAX: usize = 256;
pub const PATH_MAX_BYTES: usize = 4 * 1024;
pub const TOTAL_SOURCE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_WALL_TIME_MS: u64 = 10_000;
pub const DEFAULT_SOURCE_BYTES: usize = 512 * 1024;

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

impl CheckManyReport {
    /// Shared exit-code-by-`exit_class` mapping — identical across all
    /// three engines before this extraction (verified by reading each,
    /// not assumed).
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

/// What a per-file checker closure reports on failure; [`run_check_many`]
/// turns this into a full [`CheckManyFailure`] (adding the manifest path
/// label and a generated `invocation_id`).
#[derive(Debug, Clone)]
pub struct CheckFailure {
    pub code: String,
    pub message: String,
    pub exit_class: &'static str,
}

impl CheckFailure {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        exit_class: &'static str,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            exit_class,
        }
    }
}

/// Read and validate a check-many manifest: size/shape bounds, then
/// `schema_version == 1` and `kind` in `accepted_kinds`. `accepted_kinds`
/// lets rh accept both its own and the legacy rhai manifest kind while
/// lua/qjs each accept exactly one — the size/shape checks are identical
/// either way.
pub fn read_manifest(path: &Path, accepted_kinds: &[&str]) -> Result<CheckManyManifest, String> {
    let metadata =
        std::fs::metadata(path).map_err(|err| format!("check_many_manifest_read: {err}"))?;
    if !metadata.is_file() || metadata.len() as usize > MANIFEST_MAX_BYTES {
        return Err(format!(
            "check_many_manifest_size: manifest must be a file of at most {MANIFEST_MAX_BYTES} bytes"
        ));
    }
    let bytes = std::fs::read(path).map_err(|err| format!("check_many_manifest_read: {err}"))?;
    let manifest: CheckManyManifest =
        serde_json::from_slice(&bytes).map_err(|err| format!("check_many_manifest_json: {err}"))?;
    if manifest.schema_version != 1 {
        return Err("check_many_manifest_schema".into());
    }
    if !accepted_kinds.contains(&manifest.kind.as_str()) {
        return Err("check_many_manifest_schema".into());
    }
    if manifest.files.is_empty() || manifest.files.len() > FILES_MAX {
        return Err(format!(
            "check_many_manifest_files: expected from 1 to {FILES_MAX} files"
        ));
    }
    Ok(manifest)
}

/// Drive a check-many run: resolve/confine every manifest path against
/// `options.project_root`, enforce the shared budgets (wall time, per-file
/// and aggregate source bytes) and duplicate-path rejection, then hand
/// each surviving file's `(source, resolved_path, canonical_root)` to
/// `checker`. `report_kind` becomes the report's `kind` field.
pub fn run_check_many<F>(
    manifest: CheckManyManifest,
    options: CheckManyOptions,
    report_kind: &'static str,
    mut checker: F,
) -> CheckManyReport
where
    F: FnMut(&str, &Path, &Path) -> Result<(), CheckFailure>,
{
    let started = Instant::now();
    let deadline = started + Duration::from_millis(options.wall_time_ms);
    let root = match std::fs::canonicalize(&options.project_root) {
        Ok(root) if root.is_dir() => root,
        Ok(_) => {
            return report(
                report_kind,
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
                report_kind,
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
            Err(SourceReadFailure::Host(message)) => {
                failures.push(failure(label, "host_source_read", message, ordinal, "host"));
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
        if let Err(check_failure) = checker(&source, &path, &root) {
            failures.push(failure(
                label,
                check_failure.code,
                check_failure.message,
                ordinal,
                check_failure.exit_class,
            ));
        }
    }

    report(
        report_kind,
        started,
        checked_files,
        total_source_bytes,
        failures,
    )
}

enum SourceReadFailure {
    Limit(String),
    Host(String),
}

fn read_source_file(path: &Path, max_bytes: usize) -> Result<String, SourceReadFailure> {
    let metadata =
        std::fs::metadata(path).map_err(|err| SourceReadFailure::Host(err.to_string()))?;
    if metadata.len() as usize > max_bytes {
        return Err(SourceReadFailure::Limit(format!(
            "source exceeds per-file limit of {max_bytes} bytes"
        )));
    }
    std::fs::read_to_string(path).map_err(|err| SourceReadFailure::Host(err.to_string()))
}

fn failure(
    path: String,
    code: impl Into<String>,
    message: String,
    ordinal: usize,
    exit_class: &'static str,
) -> CheckManyFailure {
    CheckManyFailure {
        path,
        code: code.into(),
        message,
        invocation_id: format!("check-many-{}-{ordinal}", std::process::id()),
        exit_class,
    }
}

fn report(
    kind: &'static str,
    started: Instant,
    checked_files: usize,
    total_source_bytes: usize,
    failures: Vec<CheckManyFailure>,
) -> CheckManyReport {
    CheckManyReport {
        schema_version: 1,
        kind,
        ok: failures.is_empty(),
        checked_files,
        total_source_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const KIND: &str = "agenterm-test-check-manifest";

    fn write_manifest(dir: &TempDir, files: &[&str]) -> PathBuf {
        let path = dir.path().join("manifest.json");
        let manifest = serde_json::json!({
            "schema_version": 1,
            "kind": KIND,
            "files": files,
        });
        std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        path
    }

    fn ok_unless_bad(source: &str, _path: &Path, _root: &Path) -> Result<(), CheckFailure> {
        if source.contains("BAD") {
            Err(CheckFailure::new("test_bad", "contains BAD", "script"))
        } else {
            Ok(())
        }
    }

    #[test]
    fn read_valid_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir, &["test.txt"]);
        let m = read_manifest(&path, &[KIND]).expect("read");
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.files.len(), 1);
    }

    #[test]
    fn read_manifest_rejects_wrong_kind() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("manifest.json");
        let json = serde_json::json!({"schema_version": 1, "kind": "wrong-kind", "files": []});
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
        let err = read_manifest(&path, &[KIND]).expect_err("wrong kind");
        assert!(err.contains("schema"), "{err}");
    }

    #[test]
    fn read_manifest_accepts_any_of_multiple_kinds() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir, &["a.txt"]);
        let m = read_manifest(&path, &["some-other-kind", KIND]).expect("read");
        assert_eq!(m.kind, KIND);
    }

    #[test]
    fn check_many_all_green() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "fine").unwrap();
        std::fs::write(dir.path().join("b.txt"), "also fine").unwrap();
        let manifest_path = write_manifest(&dir, &["a.txt", "b.txt"]);
        let manifest = read_manifest(&manifest_path, &[KIND]).expect("read");
        let options = CheckManyOptions {
            project_root: dir.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options, KIND, ok_unless_bad);
        assert!(report.ok);
        assert_eq!(report.checked_files, 2);
        assert!(report.failures.is_empty());
        assert_eq!(report.kind, KIND);
    }

    #[test]
    fn check_many_reports_checker_failures() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ok.txt"), "fine").unwrap();
        std::fs::write(dir.path().join("bad.txt"), "BAD").unwrap();
        let manifest_path = write_manifest(&dir, &["ok.txt", "bad.txt"]);
        let manifest = read_manifest(&manifest_path, &[KIND]).expect("read");
        let options = CheckManyOptions {
            project_root: dir.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options, KIND, ok_unless_bad);
        assert!(!report.ok);
        assert_eq!(report.checked_files, 2);
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].path.contains("bad.txt"));
        assert_eq!(report.failures[0].exit_class, "script");
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn check_many_rejects_absolute_paths_outside_project_root() {
        let project = TempDir::new().unwrap();
        let outside = tempfile::NamedTempFile::new().expect("outside file");
        std::fs::write(outside.path(), "fine").unwrap();
        let manifest = CheckManyManifest {
            schema_version: 1,
            kind: KIND.to_owned(),
            files: vec![outside.path().display().to_string()],
        };
        let options = CheckManyOptions {
            project_root: project.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options, KIND, ok_unless_bad);
        assert!(!report.ok);
        assert_eq!(report.failures[0].code, "check_many_path");
        assert_eq!(report.failures[0].exit_class, "configuration");
        assert_eq!(report.exit_code(), 2);
    }

    #[test]
    fn check_many_rejects_relative_paths_that_escape_the_root() {
        // The gap this extraction closed for lua: a `../` relative label
        // that resolves outside project_root must be rejected, not
        // silently followed. `project` and `secret_dir` are sibling
        // tempdirs, so a single `../<secret_dir_name>/secret.txt` from
        // inside `project` genuinely escapes it.
        let project = TempDir::new().unwrap();
        let secret_dir = TempDir::new().unwrap();
        std::fs::write(secret_dir.path().join("secret.txt"), "leak me").unwrap();
        let secret_dir_name = secret_dir
            .path()
            .file_name()
            .expect("tempdir has a name")
            .to_string_lossy()
            .into_owned();
        let relative = format!("../{secret_dir_name}/secret.txt");
        let manifest = CheckManyManifest {
            schema_version: 1,
            kind: KIND.to_owned(),
            files: vec![relative],
        };
        let options = CheckManyOptions {
            project_root: project.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options, KIND, ok_unless_bad);
        assert!(!report.ok, "{report:?}");
        assert_eq!(report.failures[0].code, "check_many_path");
    }

    #[test]
    fn check_many_rejects_duplicate_resolved_paths() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("dup.txt"), "fine").unwrap();
        let manifest = CheckManyManifest {
            schema_version: 1,
            kind: KIND.to_owned(),
            files: vec!["dup.txt".to_owned(), "dup.txt".to_owned()],
        };
        let options = CheckManyOptions {
            project_root: dir.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options, KIND, ok_unless_bad);
        assert!(!report.ok);
        assert_eq!(report.checked_files, 1);
        assert_eq!(report.failures[0].code, "check_many_duplicate");
    }

    #[test]
    fn check_many_enforces_zero_wall_time_without_sleeping() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("valid.txt"), "fine").unwrap();
        let manifest = CheckManyManifest {
            schema_version: 1,
            kind: KIND.to_owned(),
            files: vec!["valid.txt".to_owned()],
        };
        let options = CheckManyOptions {
            project_root: dir.path().to_path_buf(),
            wall_time_ms: 0,
            ..Default::default()
        };
        let report = run_check_many(manifest, options, KIND, ok_unless_bad);
        assert!(!report.ok);
        assert_eq!(report.checked_files, 0);
        assert_eq!(report.failures[0].code, "limit_wall_time");
        assert_eq!(report.exit_code(), 3);
    }

    #[test]
    fn check_many_respects_file_limit() {
        let dir = TempDir::new().unwrap();
        let files: Vec<String> = (0..(FILES_MAX + 1)).map(|i| format!("f{i}.txt")).collect();
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let manifest_path = write_manifest(&dir, &refs);
        let err = read_manifest(&manifest_path, &[KIND]).expect_err("too many files");
        assert!(err.contains("files"), "{err}");
    }
}
