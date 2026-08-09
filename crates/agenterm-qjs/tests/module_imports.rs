//! Repo-committed end-to-end regression test for multi-file ES module
//! `import`/`export` support (`plan/design-qjs-module-imports.md`,
//! QJS-M5a..c). This exists because the design/implementation session
//! that landed QJS-M5a..c verified the CLI binary interactively (manual
//! terminal commands, not saved anywhere) and flagged that gap explicitly
//! in `plan-v0.1.16.md` rather than pretending "tested" covered it — this
//! file is that gap closed: real fixture files on disk under `fixtures/`
//! (not tempdir-generated strings, which the unit tests in
//! `module_resolver.rs`/`eval_module.rs`/`check.rs` already cover), run
//! through the crate's actual public API, same role
//! `agenterm-rh/tests/public_contract.rs` plays for rh.

use std::path::PathBuf;

use agenterm_qjs::{
    QjsHostFunctions, check, check_with_project_validation, eval_module_entry_with_host,
    wants_module_mode,
};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/module-import-project")
}

#[test]
fn fixture_entry_is_detected_as_wanting_module_mode() {
    let source = std::fs::read_to_string(project_root().join("entry.js")).unwrap();
    assert!(wants_module_mode(&source));
}

#[test]
fn plain_check_rejects_the_fixture_without_a_project_root() {
    // check() (no project root, no loader) must fail on real import
    // syntax the same way it does on synthetic strings in check.rs's own
    // tests — this fixture-backed test exists specifically so that claim
    // is checked against a file that ships with the repo, not only
    // against strings built inline in a unit test.
    let entry = project_root().join("entry.js");
    let source = std::fs::read_to_string(&entry).unwrap();
    let error = check(&source, &entry.to_string_lossy())
        .expect_err("no loader registered without a project root");
    assert!(
        error.to_string().contains("could not load module"),
        "{error}"
    );
}

#[test]
fn check_with_project_validation_passes_the_real_fixture_project() {
    let root = project_root();
    let entry = root.join("entry.js");
    let source = std::fs::read_to_string(&entry).unwrap();
    check_with_project_validation(&source, &entry, &root)
        .expect("the fixture project's import graph must check clean");
}

#[test]
fn eval_module_entry_runs_the_real_fixture_project_and_returns_the_imported_value() {
    let root = project_root();
    let entry = root.join("entry.js");
    let source = std::fs::read_to_string(&entry).unwrap();
    let host = QjsHostFunctions::default();
    let outcome = eval_module_entry_with_host(&source, &entry, &root, &host)
        .expect("the fixture project must evaluate end to end");
    assert_eq!(outcome.value, Some(serde_json::json!(42)));
    assert!(
        outcome.stdout.contains("imported value: 42"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn eval_module_entry_rejects_the_escape_attempt_fixture() {
    let root = project_root();
    let entry = root.join("escape-attempt.js");
    let source = std::fs::read_to_string(&entry).unwrap();
    let host = QjsHostFunctions::default();
    let error = eval_module_entry_with_host(&source, &entry, &root, &host)
        .expect_err("the escape-attempt fixture must always be rejected");
    assert!(
        error.to_string().contains("Error resolving module"),
        "{error}"
    );
}

#[test]
fn check_with_project_validation_also_rejects_the_escape_attempt_fixture() {
    // Same rejection, different entry point (check vs. eval) — proves the
    // confinement is enforced consistently across both call sites, not
    // just the one that happened to get tested first.
    let root = project_root();
    let entry = root.join("escape-attempt.js");
    let source = std::fs::read_to_string(&entry).unwrap();
    let error = check_with_project_validation(&source, &entry, &root)
        .expect_err("check must also reject the escape-attempt fixture");
    assert!(
        error.to_string().contains("Error resolving module"),
        "{error}"
    );
}
