//! External contract tests for the public `agenterm_rh` API.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use agenterm_rh::{
    CheckManyOptions, RH_CODEGEN_REVISION, RhError, check, check_with_project_validation,
    read_manifest, run_check_many, transpile_cdylib,
};

const FIXTURE_NAMES: [&str; 79] = [
    "append-sync-probe.rh",
    "atomic-write-probe.rh",
    "break-continue.rh",
    "bytes-append-probe.rh",
    "bytes-from-array-probe.rh",
    "bytes-from-text-probe.rh",
    "bytes-to-text-probe.rh",
    "child-lifecycle-probe.rh",
    "child-stdout-probe.rh",
    "command-arg-probe.rh",
    "command-args-json-probe.rh",
    "command-stdin-text-probe.rh",
    "crypto-sha256-file-probe.rh",
    "direntry-file-name-probe.rh",
    "direntry-is-file-probe.rh",
    "direntry-metadata-probe.rh",
    "duration-task-sleep-probe.rh",
    "entry.rh",
    "env-current-dir-probe.rh",
    "env-has-get-probe.rh",
    "env-parse-int-probe.rh",
    "fail-dynamic.rh",
    "fleet.rh",
    "for-dyn-range.rh",
    "for-range.rh",
    "for-span-overflow.rh",
    "fs-mutation-probe.rh",
    "hash-fnv1a64-probe.rh",
    "import-bundle-probe.rh",
    "int-string-concat-probe.rh",
    "json-array-index-assign-probe.rh",
    "json-array-index-map-return-probe.rh",
    "json-array-literal-probe.rh",
    "json-array-push-probe.rh",
    "json-array-walk.rh",
    "json-keys-probe.rh",
    "json-marker-run-probe.rh",
    "json-null-eq-probe.rh",
    "json-scalar-concat-probe.rh",
    "json-param-index-assign-probe.rh",
    "json-parse-file-probe.rh",
    "json-parse-schema.rh",
    "json-path-index-probe.rh",
    "json-stringify-probe.rh",
    "json-stringify-pretty-probe.rh",
    "json-type-string.rh",
    "map-set-membership.rh",
    "output-fn-arg-probe.rh",
    "path-file-name-probe.rh",
    "path-is-absolute-probe.rh",
    "path-join-probe.rh",
    "path-metadata-probe.rh",
    "path-metadata-sugar.rh",
    "path-parent-probe.rh",
    "process-id-probe.rh",
    "process-kill-probe.rh",
    "process-output-probe.rh",
    "process-stdout-file.rh",
    "remove-dir-all-probe.rh",
    "set-map-loop-assign-probe.rh",
    "set-map-value-assign-probe.rh",
    "std-fs-exists-probe.rh",
    "std-fs-read-to-string-probe.rh",
    "std-fs-write-probe.rh",
    "stdlib.rh",
    "string-contains-probe.rh",
    "string-fn-bundle.rh",
    "string-index-of-probe.rh",
    "string-list-index-probe.rh",
    "string-list-set-probe.rh",
    "string-split-probe.rh",
    "string-to-lower-probe.rh",
    "string-validate.rh",
    "system-time-rfc3339-probe.rh",
    "system-time-unix-millis-probe.rh",
    "try-catch.rh",
    "try-ok.rh",
    "while-count.rh",
    "while.rh",
];

const CONTROL_FLOW_FIXTURES: [&str; 7] = [
    "break-continue.rh",
    "for-dyn-range.rh",
    "for-range.rh",
    "try-catch.rh",
    "try-ok.rh",
    "while-count.rh",
    "while.rh",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn fixture_dir() -> PathBuf {
    repo_root().join("fixtures/rh")
}

fn fixture_names_on_disk() -> BTreeSet<String> {
    std::fs::read_dir(fixture_dir())
        .expect("read fixtures/rh")
        .map(|entry| entry.expect("fixture directory entry").path())
        .filter(|path| path.extension() == Some(OsStr::new("rh")))
        .map(|path| {
            path.file_name()
                .expect("fixture file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn assert_compile_code(error: RhError, expected_code: &str) {
    match error {
        RhError::Compile(message) => assert!(
            message.starts_with(&format!("{expected_code}:")),
            "expected compile code {expected_code}, got {message}"
        ),
        other => panic!("expected typed compile error {expected_code}, got {other:?}"),
    }
}

#[test]
fn public_codegen_revision_is_eighty_nine() {
    assert_eq!(RH_CODEGEN_REVISION, 90);
}

#[test]
fn public_check_covers_the_complete_fixture_directory() {
    let expected = FIXTURE_NAMES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let actual = fixture_names_on_disk();
    assert_eq!(
        actual, expected,
        "update the public fixture gate whenever fixtures/rh changes"
    );

    for name in actual {
        let source = std::fs::read_to_string(fixture_dir().join(&name))
            .unwrap_or_else(|error| panic!("read {name}: {error}"));
        check(&source).unwrap_or_else(|error| panic!("public check failed for {name}: {error}"));
    }
}

#[test]
fn public_cdylib_transpile_is_deterministic_without_whole_script_compat_fallback() {
    for name in CONTROL_FLOW_FIXTURES {
        let source = std::fs::read_to_string(fixture_dir().join(name))
            .unwrap_or_else(|error| panic!("read {name}: {error}"));
        let first = transpile_cdylib(&source)
            .unwrap_or_else(|error| panic!("first public transpile failed for {name}: {error}"));
        let second = transpile_cdylib(&source)
            .unwrap_or_else(|error| panic!("second public transpile failed for {name}: {error}"));

        assert_eq!(
            first, second,
            "transpile output changed for identical {name}"
        );
        assert!(
            !first.contains("compat delegating"),
            "{name} unexpectedly fell back to compat-delegating output"
        );
    }
}

#[test]
fn public_project_validation_returns_stable_typed_codes() {
    let unknown_api =
        check_with_project_validation("fn entry() { std::fs::not_shipped(`x`) }", None)
            .expect_err("unknown API must fail");
    assert_compile_code(unknown_api, "script_api_unknown");

    let root_escape = check_with_project_validation(
        "import \"../outside\" as outside; fn entry() { 1 }",
        Some(&repo_root()),
    )
    .expect_err("project import root escape must fail");
    assert_compile_code(root_escape, "script_module_root_escape");
}

#[test]
fn public_check_many_accepts_the_canonical_fixture_manifest() {
    let repo = repo_root();
    let manifest =
        read_manifest(&repo.join("fixtures/rh/check-many.json")).expect("canonical manifest");
    let expected_files = manifest.files.len();
    let report = run_check_many(
        manifest,
        CheckManyOptions {
            project_root: repo,
            ..CheckManyOptions::default()
        },
    );

    assert!(report.ok, "check-many failures: {:?}", report.failures);
    assert_eq!(report.checked_files, expected_files);
    assert!(report.failures.is_empty());
}
