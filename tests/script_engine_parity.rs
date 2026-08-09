//! Cross-engine parity tests for the shared `check-many` driver
//! (`agenterm_script_common::check_many`, re-exported unchanged through
//! each of `agenterm-rh`, `agenterm-lua`, `agenterm-qjs`, `agenterm-sql`).
//!
//! `CheckManyManifest` / `CheckManyOptions` / `CheckManyReport` are the same
//! type across all four engines now (each crate does a bare
//! `pub use agenterm_script_common::check_many::{...}`), so this file
//! standardizes on `agenterm_rh::check_many`'s re-export for the shared
//! types and only reaches into each engine's own `check_many` module for
//! the per-engine `read_manifest` / `run_check_many` entry points.
//!
//! `agenterm-sql` (see `crates/agenterm-sql/src/lib.rs`'s module doc) is a
//! fourth backend whose `execute` is still a fail-closed placeholder — but
//! its `check`/`check_many` are real, thin adapters over this exact shared
//! driver (same as rh/lua/qjs), so it belongs in *this* file's parity net
//! today even though it can't join `tests/script_engine_exec_parity.rs`'s
//! execute-level scenarios yet.
//!
//! Each scenario below runs the *same structural situation* through all
//! four engines (own tempdir, language-appropriate files, a manifest of
//! the engine's own `kind`) and asserts the reports agree on the
//! engine-neutral structural fields that come straight out of the shared
//! driver: `ok`, `checked_files`, failure count, failure `code` (for codes
//! that are literally emitted by the shared driver, not per-engine
//! checkers), failure `exit_class`, and `report.exit_code()`. Per-engine
//! syntax-failure codes (`rh_subset`/`rh_check` vs `lua_check` vs
//! `qjs_parse` vs `sql_parse`) are deliberately *not* compared
//! string-for-string — only `exit_class == "script"` and `exit_code() == 1`
//! are asserted there, per the task brief.

use std::fs;
use std::path::Path;

use agenterm_rh::check_many::{CheckManyManifest, CheckManyOptions, CheckManyReport};

/// One engine's fixed facts needed to drive it through an identical
/// structural scenario: its manifest `kind`, its source file extension, a
/// trivially-valid source, a source that fails to parse/compile, and a
/// `read_manifest` + `run_check_many` entry point unified into a single
/// `Result<CheckManyReport, String>`-returning function (the four crates'
/// `read_manifest` return different error types — `RhError`, `String`,
/// `QjsError`, `SqlError` — so this is where that gets flattened for
/// uniform assertions).
/// A `read_manifest` entry point normalized to a single `String` error type
/// (the four crates return `RhError`, `String`, `QjsError`, `SqlError`
/// respectively).
type ManifestReader = fn(&Path) -> Result<CheckManyManifest, String>;

struct EngineSpec {
    name: &'static str,
    kind: &'static str,
    ext: &'static str,
    valid_source: &'static str,
    broken_source: &'static str,
    read_and_run: fn(&Path, CheckManyOptions) -> Result<CheckManyReport, String>,
}

fn rh_read_and_run(path: &Path, options: CheckManyOptions) -> Result<CheckManyReport, String> {
    let manifest = agenterm_rh::check_many::read_manifest(path).map_err(|err| err.to_string())?;
    Ok(agenterm_rh::check_many::run_check_many(manifest, options))
}

fn lua_read_and_run(path: &Path, options: CheckManyOptions) -> Result<CheckManyReport, String> {
    let manifest = agenterm_lua::check_many::read_manifest(path)?;
    Ok(agenterm_lua::check_many::run_check_many(manifest, options))
}

fn qjs_read_and_run(path: &Path, options: CheckManyOptions) -> Result<CheckManyReport, String> {
    let manifest = agenterm_qjs::check_many::read_manifest(path).map_err(|err| err.to_string())?;
    Ok(agenterm_qjs::check_many::run_check_many(manifest, options))
}

fn sql_read_and_run(path: &Path, options: CheckManyOptions) -> Result<CheckManyReport, String> {
    let manifest = agenterm_sql::check_many::read_manifest(path).map_err(|err| err.to_string())?;
    Ok(agenterm_sql::check_many::run_check_many(manifest, options))
}

const RH: EngineSpec = EngineSpec {
    name: "rh",
    kind: "agenterm-rh-check-manifest",
    ext: "rh",
    valid_source: "40 + 2",
    broken_source: "fn {{{",
    read_and_run: rh_read_and_run,
};

const RH_LEGACY_RHAI_KIND: &str = "agenterm-rhai-check-manifest";

const LUA: EngineSpec = EngineSpec {
    name: "lua",
    kind: "agenterm-lua-check-manifest",
    ext: "lua",
    valid_source: "return 42",
    broken_source: "return !!",
    read_and_run: lua_read_and_run,
};

const QJS: EngineSpec = EngineSpec {
    name: "qjs",
    kind: "agenterm-qjs-check-manifest",
    ext: "js",
    valid_source: "function entry() { return 42; }",
    broken_source: "this is not valid js (((",
    read_and_run: qjs_read_and_run,
};

// Fixtures match `agenterm_sql::check`'s own tests verbatim
// (crates/agenterm-sql/src/check.rs's `accepts_a_valid_select` /
// `rejects_syntax_errors`) and `src/script_engine.rs`'s
// `SQL_VALID_SOURCE`/`SQL_BROKEN_SOURCE` test consts — not invented here.
const SQL: EngineSpec = EngineSpec {
    name: "sql",
    kind: "agenterm-sql-check-manifest",
    ext: "sql",
    valid_source: "SELECT 1;",
    broken_source: "SELEC 1 FORM;",
    read_and_run: sql_read_and_run,
};

fn engines() -> [EngineSpec; 4] {
    [RH, LUA, QJS, SQL]
}

/// Write a check-many manifest JSON with the given `kind` and file labels
/// into `dir`, returning its path.
fn write_manifest(dir: &Path, kind: &str, files: &[&str]) -> std::path::PathBuf {
    let path = dir.join("manifest.json");
    let manifest = serde_json::json!({
        "schema_version": 1,
        "kind": kind,
        "files": files,
    });
    fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    path
}

fn options_for(dir: &Path) -> CheckManyOptions {
    CheckManyOptions {
        project_root: dir.to_path_buf(),
        ..CheckManyOptions::default()
    }
}

fn options_for_with(dir: &Path, wall_time_ms: u64, source_bytes: usize) -> CheckManyOptions {
    CheckManyOptions {
        project_root: dir.to_path_buf(),
        wall_time_ms,
        source_bytes,
    }
}

#[test]
fn all_green() {
    for engine in engines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_a = format!("a.{}", engine.ext);
        let file_b = format!("b.{}", engine.ext);
        fs::write(dir.path().join(&file_a), engine.valid_source).unwrap();
        fs::write(dir.path().join(&file_b), engine.valid_source).unwrap();
        let manifest_path = write_manifest(dir.path(), engine.kind, &[&file_a, &file_b]);

        let report = (engine.read_and_run)(&manifest_path, options_for(dir.path()))
            .unwrap_or_else(|err| panic!("{}: expected Ok report, got {err}", engine.name));

        assert!(report.ok, "{}: expected ok report: {report:?}", engine.name);
        assert_eq!(report.checked_files, 2, "{}", engine.name);
        assert!(report.failures.is_empty(), "{}", engine.name);
        assert_eq!(report.exit_code(), 0, "{}", engine.name);
    }
}

#[test]
fn syntax_error_in_one_file() {
    for engine in engines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_ok = format!("ok.{}", engine.ext);
        let file_bad = format!("bad.{}", engine.ext);
        fs::write(dir.path().join(&file_ok), engine.valid_source).unwrap();
        fs::write(dir.path().join(&file_bad), engine.broken_source).unwrap();
        let manifest_path = write_manifest(dir.path(), engine.kind, &[&file_ok, &file_bad]);

        let report = (engine.read_and_run)(&manifest_path, options_for(dir.path()))
            .unwrap_or_else(|err| panic!("{}: expected Ok report, got {err}", engine.name));

        assert!(!report.ok, "{}: expected failure report", engine.name);
        assert_eq!(report.checked_files, 2, "{}", engine.name);
        assert_eq!(report.failures.len(), 1, "{}: {:?}", engine.name, report.failures);
        // Engine-specific syntax-failure codes (rh_subset/rh_check vs
        // lua_check vs qjs_parse) intentionally NOT compared here — only
        // the engine-neutral exit_class/exit_code contract is shared.
        assert_eq!(report.failures[0].exit_class, "script", "{}", engine.name);
        assert_eq!(report.exit_code(), 1, "{}", engine.name);
    }
}

#[test]
fn manifest_path_escapes_root() {
    for engine in engines() {
        let project = tempfile::tempdir().expect("project tempdir");
        let sibling = tempfile::tempdir().expect("sibling tempdir");
        let sibling_name = sibling
            .path()
            .file_name()
            .expect("tempdir has a name")
            .to_string_lossy()
            .into_owned();
        let sibling_file = format!("secret.{}", engine.ext);
        fs::write(sibling.path().join(&sibling_file), engine.valid_source).unwrap();

        let escaping_label = format!("../{sibling_name}/{sibling_file}");
        let manifest_path = write_manifest(project.path(), engine.kind, &[&escaping_label]);

        let report = (engine.read_and_run)(&manifest_path, options_for(project.path()))
            .unwrap_or_else(|err| panic!("{}: expected Ok report, got {err}", engine.name));

        assert!(!report.ok, "{}", engine.name);
        assert_eq!(report.failures.len(), 1, "{}", engine.name);
        assert_eq!(report.failures[0].code, "check_many_path", "{}", engine.name);
        assert_eq!(report.failures[0].exit_class, "configuration", "{}", engine.name);
        assert_eq!(report.exit_code(), 2, "{}", engine.name);
    }
}

#[test]
fn absolute_path_rejected() {
    for engine in engines() {
        let project = tempfile::tempdir().expect("project tempdir");
        let outside = tempfile::NamedTempFile::new().expect("outside file");
        fs::write(outside.path(), engine.valid_source).unwrap();
        let absolute_label = outside.path().display().to_string();

        let manifest_path = write_manifest(project.path(), engine.kind, &[&absolute_label]);

        let report = (engine.read_and_run)(&manifest_path, options_for(project.path()))
            .unwrap_or_else(|err| panic!("{}: expected Ok report, got {err}", engine.name));

        assert!(!report.ok, "{}", engine.name);
        assert_eq!(report.failures.len(), 1, "{}", engine.name);
        assert_eq!(report.failures[0].code, "check_many_path", "{}", engine.name);
        assert_eq!(report.failures[0].exit_class, "configuration", "{}", engine.name);
        assert_eq!(report.exit_code(), 2, "{}", engine.name);
    }
}

#[test]
fn duplicate_resolved_path() {
    for engine in engines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = format!("dup.{}", engine.ext);
        fs::write(dir.path().join(&file), engine.valid_source).unwrap();
        let manifest_path = write_manifest(dir.path(), engine.kind, &[&file, &file]);

        let report = (engine.read_and_run)(&manifest_path, options_for(dir.path()))
            .unwrap_or_else(|err| panic!("{}: expected Ok report, got {err}", engine.name));

        assert!(!report.ok, "{}", engine.name);
        assert_eq!(report.checked_files, 1, "{}", engine.name);
        assert_eq!(report.failures.len(), 1, "{}", engine.name);
        assert_eq!(report.failures[0].code, "check_many_duplicate", "{}", engine.name);
        assert_eq!(report.failures[0].exit_class, "configuration", "{}", engine.name);
        assert_eq!(report.exit_code(), 2, "{}", engine.name);
    }
}

#[test]
fn zero_wall_time() {
    for engine in engines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = format!("valid.{}", engine.ext);
        fs::write(dir.path().join(&file), engine.valid_source).unwrap();
        let manifest_path = write_manifest(dir.path(), engine.kind, &[&file]);

        let options = options_for_with(dir.path(), 0, CheckManyOptions::default().source_bytes);
        let report = (engine.read_and_run)(&manifest_path, options)
            .unwrap_or_else(|err| panic!("{}: expected Ok report, got {err}", engine.name));

        assert!(!report.ok, "{}", engine.name);
        assert_eq!(report.checked_files, 0, "{}", engine.name);
        assert_eq!(report.failures[0].code, "limit_wall_time", "{}", engine.name);
        assert_eq!(report.failures[0].exit_class, "limit", "{}", engine.name);
        assert_eq!(report.exit_code(), 3, "{}", engine.name);
    }
}

#[test]
fn per_file_source_budget() {
    for engine in engines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = format!("oversized.{}", engine.ext);
        assert!(
            engine.valid_source.len() > 1,
            "{}: fixture must exceed the 1-byte budget",
            engine.name
        );
        fs::write(dir.path().join(&file), engine.valid_source).unwrap();
        let manifest_path = write_manifest(dir.path(), engine.kind, &[&file]);

        let options = options_for_with(dir.path(), CheckManyOptions::default().wall_time_ms, 1);
        let report = (engine.read_and_run)(&manifest_path, options)
            .unwrap_or_else(|err| panic!("{}: expected Ok report, got {err}", engine.name));

        assert!(!report.ok, "{}", engine.name);
        assert_eq!(report.checked_files, 0, "{}", engine.name);
        assert_eq!(report.failures[0].code, "limit_source_bytes", "{}", engine.name);
        assert_eq!(report.failures[0].exit_class, "limit", "{}", engine.name);
        assert_eq!(report.exit_code(), 3, "{}", engine.name);
    }
}

#[test]
fn wrong_manifest_kind_rejected_by_each_reader() {
    // Each engine's read_manifest must reject the *other* engines' kind
    // strings. 4 readers x 3 "other" kinds each = the 4x4 cross-kind
    // rejection matrix (minus the 4 diagonal "own kind" cells, covered by
    // the per-reader sanity check below instead).
    let readers: [(&str, ManifestReader); 4] = [
        ("rh", |p| agenterm_rh::check_many::read_manifest(p).map_err(|e| e.to_string())),
        ("lua", agenterm_lua::check_many::read_manifest),
        ("qjs", |p| agenterm_qjs::check_many::read_manifest(p).map_err(|e| e.to_string())),
        ("sql", |p| agenterm_sql::check_many::read_manifest(p).map_err(|e| e.to_string())),
    ];

    for (name, reader) in readers {
        let own_engine = engines().into_iter().find(|e| e.name == name).unwrap();
        for other_engine in engines() {
            if other_engine.name == name {
                continue;
            }
            let dir = tempfile::tempdir().expect("tempdir");
            let manifest_path = write_manifest(dir.path(), other_engine.kind, &["x"]);
            let result = reader(&manifest_path);
            assert!(
                result.is_err(),
                "{name}: expected rejection of {} manifest kind `{}`",
                other_engine.name,
                other_engine.kind
            );
        }
        // Sanity: the same reader must still accept its own kind.
        let dir = tempfile::tempdir().expect("tempdir");
        let file = format!("a.{}", own_engine.ext);
        fs::write(dir.path().join(&file), own_engine.valid_source).unwrap();
        let manifest_path = write_manifest(dir.path(), own_engine.kind, &[&file]);
        assert!(reader(&manifest_path).is_ok(), "{name}: expected own kind to be accepted");
    }

    // Deliberate compat lock-in: rh additionally accepts the legacy rhai
    // manifest kind (thin-forward compat, documented in
    // crates/agenterm-rh/src/check_many.rs).
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.rh"), RH.valid_source).unwrap();
    let manifest_path = write_manifest(dir.path(), RH_LEGACY_RHAI_KIND, &["a.rh"]);
    let manifest =
        agenterm_rh::check_many::read_manifest(&manifest_path).expect("rh accepts legacy rhai kind");
    assert_eq!(manifest.kind, RH_LEGACY_RHAI_KIND);
}
