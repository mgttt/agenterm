//! Bounded multi-file qjs check. Manifest/report shape, path-confinement,
//! and the check-many driver loop now live in
//! `agenterm_script_common::check_many` (shared with rh/lua — see that
//! module's doc for what's unified vs. kept per-engine, and its own doc for
//! why: qjs's own driver loop is literally where the shared logic was
//! lifted from). This file keeps only what's genuinely qjs-specific: the
//! manifest `kind` string, the report `kind` label, CLI flag parsing, and
//! the [`check_with_project_validation`](crate::check::check_with_project_validation)
//! adapter closure (including its `QjsError` → [`CheckFailure`] mapping).

use std::path::Path;

use agenterm_script_common::check_many::{self, CheckFailure};

pub use agenterm_script_common::check_many::{
    CheckManyFailure, CheckManyManifest, CheckManyOptions, CheckManyReport, DEFAULT_SOURCE_BYTES,
    DEFAULT_WALL_TIME_MS, FILES_MAX, MANIFEST_MAX_BYTES, ParsedCheckManyCli, PATH_MAX_BYTES,
    TOTAL_SOURCE_MAX_BYTES,
};

use crate::{check::check_with_project_validation, error::QjsError};

const QJS_CHECK_MANIFEST_KIND: &str = "agenterm-qjs-check-manifest";

pub fn read_manifest(path: &Path) -> Result<CheckManyManifest, QjsError> {
    // Unreadable manifest, malformed JSON, wrong `kind` — all
    // usage/configuration problems, not anything about a script's content.
    // See `error.rs`'s doc for the exit-code rationale.
    check_many::read_manifest(path, &[QJS_CHECK_MANIFEST_KIND]).map_err(QjsError::Usage)
}

pub fn run_check_many(manifest: CheckManyManifest, options: CheckManyOptions) -> CheckManyReport {
    check_many::run_check_many(
        manifest,
        options,
        "agenterm-qjs-check-many",
        |source, path, root| {
            // `path`/`root` are already canonicalized and confinement-checked
            // by the shared driver's own manifest-path validation — reuse
            // them rather than re-deriving, so a script using import/export
            // gets its import graph checked against the same project root
            // the manifest itself is scoped to. Non-module scripts (the
            // common case today) behave identically to a plain `check(&source,
            // &label)` call — see `check_with_project_validation`'s doc.
            check_with_project_validation(source, path, root).map_err(qjs_check_failure)
        },
    )
}

fn qjs_check_failure(error: QjsError) -> CheckFailure {
    match error {
        QjsError::Parse(message) => CheckFailure::new("qjs_parse", message, "script"),
        QjsError::Check(message) => CheckFailure::new("qjs_check", message, "script"),
        // Not actually reachable today: the driver canonicalizes/confines
        // `path`/`root` itself before calling `check_with_project_validation`
        // (see this file's module doc), so the `Usage`-classified
        // canonicalize/confinement errors in `check.rs` can't fire from
        // here. Matched anyway for exhaustiveness, and classified under the
        // same `"configuration"` `exit_class` the shared driver already
        // uses for its own usage-level failures (see
        // `agenterm_script_common::check_many::CheckManyReport::exit_code`).
        QjsError::Usage(message) => CheckFailure::new("qjs_usage", message, "configuration"),
    }
}

/// Parse `check-many` argv — same flag surface as `agenterm-rh check-many`
/// (`crates/agenterm-rh/src/check_many.rs::parse_check_many_cli`), including
/// its accepted-but-ignored compat flags, so the same wrapper scripts can
/// call either engine's `check-many` with identical argv.
pub fn parse_check_many_cli<I>(args: I) -> Result<ParsedCheckManyCli, QjsError>
where
    I: Iterator<Item = String>,
{
    // Bad/missing `--manifest`, `--timeout-ms`, etc. — argv/usage, not
    // script content.
    agenterm_script_common::cli::parse_check_many_cli(args).map_err(QjsError::Usage)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        CheckManyManifest, CheckManyOptions, QJS_CHECK_MANIFEST_KIND, TOTAL_SOURCE_MAX_BYTES,
        parse_check_many_cli, read_manifest, run_check_many,
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
        // ok.js is valid; syntax-error.js is not — report is expected NOT ok.
        assert!(!report.ok);
        assert_eq!(report.checked_files, 2);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].path, "syntax-error.js");
        assert_eq!(report.failures[0].code, "qjs_parse");
    }

    fn manifest_for(file: &str) -> CheckManyManifest {
        CheckManyManifest {
            schema_version: 1,
            kind: QJS_CHECK_MANIFEST_KIND.to_owned(),
            files: vec![file.to_owned()],
        }
    }

    #[test]
    fn check_many_validates_a_multi_file_import_graph_through_the_manifest() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        std::fs::write(project.path().join("leaf.js"), "export const value = 42;").unwrap();
        std::fs::write(
            project.path().join("entry.js"),
            "import { value } from './leaf.js';\nfunction entry() { return value; }",
        )
        .unwrap();

        let report = run_check_many(
            manifest_for("entry.js"),
            CheckManyOptions {
                project_root: project_path,
                ..CheckManyOptions::default()
            },
        );

        assert!(report.ok, "{report:?}");
        assert_eq!(report.checked_files, 1);
        assert!(report.failures.is_empty());
    }

    #[test]
    fn check_many_surfaces_a_syntax_error_inside_an_imported_file() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        std::fs::write(project.path().join("leaf.js"), "export const value = ((( ;").unwrap();
        std::fs::write(
            project.path().join("entry.js"),
            "import { value } from './leaf.js';\nfunction entry() { return value; }",
        )
        .unwrap();

        let report = run_check_many(
            manifest_for("entry.js"),
            CheckManyOptions {
                project_root: project_path,
                ..CheckManyOptions::default()
            },
        );

        assert!(!report.ok);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].path, "entry.js");
        assert_eq!(report.failures[0].code, "qjs_parse");
    }

    #[test]
    fn check_many_classifies_per_file_source_budget_as_limit() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        std::fs::write(project.path().join("oversized.js"), "export default 42;\n")
            .expect("write oversized script");

        let report = run_check_many(
            manifest_for("oversized.js"),
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
        let source_path = project.path().join("aggregate.js");
        let source = std::fs::File::create(&source_path).expect("create aggregate script");
        source
            .set_len((TOTAL_SOURCE_MAX_BYTES + 1) as u64)
            .expect("size aggregate script");
        drop(source);

        let report = run_check_many(
            manifest_for("aggregate.js"),
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
        std::fs::write(project.path().join("valid.js"), "export default 42;\n")
            .expect("write valid script");

        let report = run_check_many(
            manifest_for("valid.js"),
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
    fn check_many_rejects_absolute_paths_outside_project_root() {
        let project = tempfile::tempdir().expect("project root");
        let outside = tempfile::NamedTempFile::new().expect("outside script");
        std::fs::write(outside.path(), "export default 42;\n").expect("write outside script");
        let report = run_check_many(
            CheckManyManifest {
                schema_version: 1,
                kind: QJS_CHECK_MANIFEST_KIND.to_owned(),
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

    #[test]
    fn check_many_rejects_duplicate_resolved_paths() {
        let project = tempfile::tempdir().expect("project root");
        let project_path = project.path().to_path_buf();
        std::fs::write(project.path().join("dup.js"), "export default 42;\n")
            .expect("write script");
        let report = run_check_many(
            CheckManyManifest {
                schema_version: 1,
                kind: QJS_CHECK_MANIFEST_KIND.to_owned(),
                files: vec!["dup.js".to_owned(), "dup.js".to_owned()],
            },
            CheckManyOptions {
                project_root: project_path,
                ..CheckManyOptions::default()
            },
        );
        assert!(!report.ok);
        assert_eq!(report.checked_files, 1);
        assert_eq!(report.failures[0].code, "check_many_duplicate");
    }
}
