//! Bounded multi-file sql check. Manifest/report shape, path-confinement,
//! and the check-many driver loop live in `agenterm_script_common::check_many`
//! (shared with rh/lua/qjs — see that module's doc for what's unified vs.
//! kept per-engine). This file keeps only what's genuinely sql-specific: the
//! manifest `kind` string, the report `kind` label, CLI flag parsing, and
//! the [`check`](crate::check::check) adapter closure (including its
//! `SqlError` → [`CheckFailure`] mapping). Modeled directly on
//! `agenterm-qjs`'s `check_many.rs`.
//!
//! sql's checker has no notion of a "project root"/import graph the way
//! qjs's `check_with_project_validation` does (sql has no `import`
//! statement), so the adapter closure below ignores the `root` argument the
//! shared driver still passes — kept for signature parity with the other
//! engines' closures, not because sql uses it.

use std::path::Path;

use agenterm_script_common::check_many::{self, CheckFailure};

pub use agenterm_script_common::check_many::{
    CheckManyFailure, CheckManyManifest, CheckManyOptions, CheckManyReport, DEFAULT_SOURCE_BYTES,
    DEFAULT_WALL_TIME_MS, FILES_MAX, MANIFEST_MAX_BYTES, ParsedCheckManyCli, PATH_MAX_BYTES,
    TOTAL_SOURCE_MAX_BYTES,
};

use crate::{check::check, error::SqlError};

const SQL_CHECK_MANIFEST_KIND: &str = "agenterm-sql-check-manifest";

pub fn read_manifest(path: &Path) -> Result<CheckManyManifest, SqlError> {
    check_many::read_manifest(path, &[SQL_CHECK_MANIFEST_KIND]).map_err(SqlError::Check)
}

pub fn run_check_many(manifest: CheckManyManifest, options: CheckManyOptions) -> CheckManyReport {
    check_many::run_check_many(
        manifest,
        options,
        "agenterm-sql-check-many",
        |source, path, _root| {
            let label = path.display().to_string();
            check(source, &label).map_err(sql_check_failure)
        },
    )
}

fn sql_check_failure(error: SqlError) -> CheckFailure {
    match error {
        SqlError::Parse(message) => CheckFailure::new("sql_parse", message, "script"),
        SqlError::Check(message) => CheckFailure::new("sql_check", message, "script"),
    }
}

/// Parse `check-many` argv — same flag surface as
/// `agenterm-rh`/`agenterm-lua`/`agenterm-qjs check-many`, including their
/// accepted-but-ignored compat flags, so the same wrapper scripts can call
/// any engine's `check-many` with identical argv.
pub fn parse_check_many_cli<I>(args: I) -> Result<ParsedCheckManyCli, SqlError>
where
    I: Iterator<Item = String>,
{
    agenterm_script_common::cli::parse_check_many_cli(args).map_err(SqlError::Check)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        CheckManyManifest, CheckManyOptions, SQL_CHECK_MANIFEST_KIND, parse_check_many_cli,
        read_manifest, run_check_many,
    };

    #[test]
    fn parses_fixture_manifest_and_flags() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let manifest_path = repo.join("fixtures/check-many.json");
        let parsed = parse_check_many_cli(
            [
                "--manifest",
                &manifest_path.display().to_string(),
                "--profile",
                "local",
                "--project-root",
                &repo.join("fixtures").display().to_string(),
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
        let manifest = read_manifest(&parsed.manifest_path).expect("manifest");
        let report = run_check_many(manifest, parsed.options);
        // ok.sql is valid; syntax-error.sql is not — report is expected NOT ok.
        assert!(!report.ok);
        assert_eq!(report.checked_files, 2);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].path, "syntax-error.sql");
        assert_eq!(report.failures[0].code, "sql_parse");
    }

    fn manifest_for(file: &str) -> CheckManyManifest {
        CheckManyManifest {
            schema_version: 1,
            kind: SQL_CHECK_MANIFEST_KIND.to_owned(),
            files: vec![file.to_owned()],
        }
    }

    #[test]
    fn check_many_enforces_manifest_kind() {
        let project = tempfile::tempdir().expect("project root");
        std::fs::write(project.path().join("ok.sql"), "SELECT 1;").unwrap();
        let manifest = CheckManyManifest {
            schema_version: 1,
            kind: "some-other-kind".to_owned(),
            files: vec!["ok.sql".to_owned()],
        };
        // read_manifest is what actually enforces `kind`; run_check_many
        // itself doesn't re-check it (matches qjs/rh/lua's split of
        // responsibility) — so exercise the enforcement at the read step,
        // using a hand-built manifest to bypass read_manifest's own kind
        // check would defeat the point of this test, so assert on
        // read_manifest directly instead of on this manifest value.
        let dir = tempfile::tempdir().expect("dir");
        let manifest_path = dir.path().join("manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_string(&serde_json::json!({
                "schema_version": 1,
                "kind": manifest.kind,
                "files": manifest.files,
            }))
            .unwrap(),
        )
        .unwrap();
        let error = super::read_manifest(&manifest_path).expect_err("wrong kind must be rejected");
        assert!(error.to_string().contains("schema"), "{error}");
    }

    #[test]
    fn check_many_all_green() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        std::fs::write(project.path().join("a.sql"), "SELECT 1;").unwrap();
        std::fs::write(project.path().join("b.sql"), "SELECT 2;").unwrap();
        let manifest = CheckManyManifest {
            schema_version: 1,
            kind: SQL_CHECK_MANIFEST_KIND.to_owned(),
            files: vec!["a.sql".to_owned(), "b.sql".to_owned()],
        };
        let report = run_check_many(
            manifest,
            CheckManyOptions {
                project_root: project_path,
                ..CheckManyOptions::default()
            },
        );
        assert!(report.ok, "{report:?}");
        assert_eq!(report.checked_files, 2);
        assert!(report.failures.is_empty());
    }

    #[test]
    fn check_many_surfaces_a_syntax_error() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        std::fs::write(project.path().join("bad.sql"), "SELEC 1 FORM;").unwrap();
        let report = run_check_many(
            manifest_for("bad.sql"),
            CheckManyOptions {
                project_root: project_path,
                ..CheckManyOptions::default()
            },
        );
        assert!(!report.ok);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].path, "bad.sql");
        assert_eq!(report.failures[0].code, "sql_parse");
    }
}
