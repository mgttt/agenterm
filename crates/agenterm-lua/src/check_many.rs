//! Bounded multi-file Lua syntax validation. Manifest/report shape,
//! path-confinement, and the check-many driver loop now live in
//! `agenterm_script_common::check_many` (shared with rh/qjs — see that
//! module's doc for what's unified vs. kept per-engine). This file keeps
//! only what's genuinely lua-specific: the manifest `kind` string, CLI
//! flag parsing, and the `LuaEngine::check` adapter closure.
//!
//! Behavior note: the previous hand-written version of this file resolved
//! manifest paths with a plain join-or-canonicalize and **no confinement
//! check** against `project_root` — unlike rh/qjs, which both reject a
//! resolved path that escapes the project root. Going through the shared
//! driver closes that gap (see `agenterm_script_common::check_many`'s
//! module doc); no existing lua test asserted on the old, weaker behavior.

use std::path::Path;

use agenterm_script_common::check_many::{self, CheckFailure};

pub use agenterm_script_common::check_many::{
    CheckManyFailure, CheckManyManifest, CheckManyOptions, CheckManyReport, DEFAULT_SOURCE_BYTES,
    DEFAULT_WALL_TIME_MS, FILES_MAX, MANIFEST_MAX_BYTES, ParsedCheckManyCli, PATH_MAX_BYTES,
    TOTAL_SOURCE_MAX_BYTES,
};

use crate::LuaEngine;

const LUA_CHECK_MANIFEST_KIND: &str = "agenterm-lua-check-manifest";

pub fn read_manifest(path: &Path) -> Result<CheckManyManifest, String> {
    check_many::read_manifest(path, &[LUA_CHECK_MANIFEST_KIND])
}

pub fn run_check_many(manifest: CheckManyManifest, options: CheckManyOptions) -> CheckManyReport {
    let engine = match LuaEngine::new() {
        Ok(engine) => engine,
        Err(err) => {
            // Mirror the previous behavior: an engine-init failure is
            // reported as a single `host`-class failure covering the run,
            // before any file is looked at.
            return CheckManyReport {
                schema_version: 1,
                kind: LUA_CHECK_MANIFEST_KIND,
                ok: false,
                checked_files: 0,
                total_source_bytes: 0,
                duration_ms: 0,
                failures: vec![CheckManyFailure {
                    path: String::new(),
                    code: "lua_engine_init".into(),
                    message: err.to_string(),
                    invocation_id: "check-many-0".into(),
                    exit_class: "host",
                }],
            };
        }
    };
    check_many::run_check_many(
        manifest,
        options,
        LUA_CHECK_MANIFEST_KIND,
        |source, _path, _root| {
            engine
                .check(source)
                .map_err(|err| CheckFailure::new("lua_check", err.to_string(), "script"))
        },
    )
}

/// Parse `check-many` argv — same flag surface as `agenterm-rh check-many`
/// (rejects unknown flags, accepts-but-ignores a few rhai-compat ones).
pub fn parse_check_many_cli<I>(args: I) -> Result<ParsedCheckManyCli, String>
where
    I: Iterator<Item = String>,
{
    agenterm_script_common::cli::parse_check_many_cli(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_manifest(dir: &TempDir, files: &[&str]) -> std::path::PathBuf {
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
        assert!(err.contains("schema"), "{err}");
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
        let files: Vec<String> = (0..(FILES_MAX + 1)).map(|i| format!("f{i}.lua")).collect();
        for name in &files {
            std::fs::write(dir.path().join(name), "return 0").unwrap();
        }
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let manifest_path = write_manifest(&dir, &refs);
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

    #[test]
    fn check_many_rejects_a_relative_path_that_escapes_the_project_root() {
        // Regression test for the confinement gap this migration closed.
        let project = TempDir::new().unwrap();
        let secret_dir = TempDir::new().unwrap();
        std::fs::write(secret_dir.path().join("secret.lua"), "return 1").unwrap();
        let secret_dir_name = secret_dir
            .path()
            .file_name()
            .expect("tempdir has a name")
            .to_string_lossy()
            .into_owned();
        let manifest = CheckManyManifest {
            schema_version: 1,
            kind: LUA_CHECK_MANIFEST_KIND.to_owned(),
            files: vec![format!("../{secret_dir_name}/secret.lua")],
        };
        let options = CheckManyOptions {
            project_root: project.path().to_path_buf(),
            ..Default::default()
        };
        let report = run_check_many(manifest, options);
        assert!(!report.ok, "{report:?}");
        assert_eq!(report.failures[0].code, "check_many_path");
    }

    #[test]
    fn parses_cli_flags() {
        let dir = TempDir::new().unwrap();
        let manifest_path = write_manifest(&dir, &["a.lua"]);
        let parsed = parse_check_many_cli(
            [
                "--manifest",
                &manifest_path.display().to_string(),
                "--project-root",
                &dir.path().display().to_string(),
                "--json",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect("parse");
        assert!(parsed.json);
        assert_eq!(parsed.options.project_root, dir.path());
    }
}
