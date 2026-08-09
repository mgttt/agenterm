//! qjs pack: build (source → real bytecode fingerprint + manifest + stored
//! entry source) into a directory, and load from a directory.
//!
//! **Execution re-parses from stored source, not from the bytecode file.**
//! This mirrors `agenterm_lua::pack`'s documented shortcut (`LuaPack::eval`:
//! "Bytecode can't be executed via `lua.load(bytes)` in mlua directly, so we
//! re-evaluate the source from the entry file"), for a different but
//! related reason: rquickjs 0.12's bytecode load path (`Module::load`) only
//! covers ES *modules*, and this engine's `entry()` convention
//! (`eval.rs`/`eval_entry_with_host`: a plain global script with a
//! top-level `function entry()` read off `globalThis`) is not a module —
//! modules don't populate `globalThis` from top-level declarations, so a
//! loaded-module `entry` would need to be an explicit named `export` plus
//! promise/job-queue draining to observe module evaluation completing,
//! which is a real (tracked) feature, not a two-line fix. Until that lands,
//! `bytecode_hash` in the manifest is a genuine reproducibility fingerprint
//! (see `compile.rs`) that a pack's *build* step can be verified against,
//! while *execution* takes the same global-script path `check`/`eval` use
//! everywhere else — so a pack behaves identically to running the same
//! source through `eval_entry`, not a second execution semantics.

use std::path::{Path, PathBuf};

use crate::compile::{compile_qjs, hash_source};
use crate::error::QjsError;
use crate::eval::{EvalOutcome, eval_entry_with_host};
use crate::host::QjsHostFunctions;
use crate::manifest::QjsPackManifest;

const DEFAULT_BYTECODE_FILE: &str = "pack.qjsc";
const DEFAULT_MANIFEST_FILE: &str = "manifest.json";
const DEFAULT_ENTRY_FILE: &str = "entry.js";

/// A loaded qjs pack: directory root + manifest + entry source.
#[derive(Debug)]
pub struct QjsPack {
    pub root: PathBuf,
    pub manifest: QjsPackManifest,
    pub source: String,
}

impl QjsPack {
    /// Load a pack from a directory containing `manifest.json`,
    /// `pack.qjsc`, and `entry.js`. Verifies both hashes recorded in the
    /// manifest against what's actually on disk.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let manifest_path = dir.join(DEFAULT_MANIFEST_FILE);
        let manifest = QjsPackManifest::read(&manifest_path)?;
        manifest.verify_bytecode(dir)?;
        manifest.verify_source(dir)?;
        let entry_path = dir.join(&manifest.entry_file);
        let source =
            std::fs::read_to_string(&entry_path).map_err(|e| format!("pack_load_entry: {e}"))?;
        Ok(Self {
            root: dir.to_path_buf(),
            manifest,
            source,
        })
    }

    /// Execute the pack's entry source (see module doc for why this
    /// re-parses rather than loading the bytecode file).
    pub fn eval(&self, host: &QjsHostFunctions) -> Result<EvalOutcome, QjsError> {
        let label = self
            .root
            .join(&self.manifest.entry_file)
            .display()
            .to_string();
        eval_entry_with_host(&self.source, &label, host)
    }
}

/// Build a qjs pack directory: compile source → bytecode fingerprint →
/// manifest → entry file.
pub fn build_pack_dir(source: &str, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("pack_create_dir: {e}"))?;

    let entry_path = dir.join(DEFAULT_ENTRY_FILE);
    let label = entry_path.display().to_string();
    let (bytecode, bytecode_hash) =
        compile_qjs(source, &label).map_err(|e| format!("pack_compile: {e}"))?;
    let source_hash = hash_source(source);

    let bytecode_path = dir.join(DEFAULT_BYTECODE_FILE);
    std::fs::write(&bytecode_path, &bytecode).map_err(|e| format!("pack_write_bytecode: {e}"))?;

    std::fs::write(&entry_path, source).map_err(|e| format!("pack_write_entry: {e}"))?;

    let manifest_path = dir.join(DEFAULT_MANIFEST_FILE);
    QjsPackManifest::write(
        &manifest_path,
        &source_hash,
        &bytecode_hash,
        DEFAULT_BYTECODE_FILE,
        DEFAULT_ENTRY_FILE,
    )?;

    Ok(bytecode_path)
}

/// Check if a directory is a valid qjs pack (has manifest.json + bytecode
/// file + entry file).
pub fn is_pack_dir(dir: &Path) -> bool {
    dir.join(DEFAULT_MANIFEST_FILE).exists()
        && dir.join(DEFAULT_BYTECODE_FILE).exists()
        && dir.join(DEFAULT_ENTRY_FILE).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn build_and_load_pack() {
        let dir = TempDir::new().expect("tempdir");
        build_pack_dir("function entry() { return 42; }", dir.path()).expect("build");
        assert!(is_pack_dir(dir.path()));

        let pack = QjsPack::load(dir.path()).expect("load");
        assert_eq!(pack.manifest.schema, "agenterm.qjs-pack-manifest/v1");

        let host = QjsHostFunctions::default();
        let result = pack.eval(&host).expect("eval");
        assert_eq!(result.value, Some(json!(42)));
    }

    #[test]
    fn build_pack_with_print() {
        let dir = TempDir::new().expect("tempdir");
        build_pack_dir("function entry() { print('hello'); return 7; }", dir.path())
            .expect("build");
        let pack = QjsPack::load(dir.path()).expect("load");
        let host = QjsHostFunctions::default();
        let result = pack.eval(&host).expect("eval");
        assert_eq!(result.value, Some(json!(7)));
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn pack_load_rejects_bad_manifest() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("manifest.json"), "not json").expect("write");
        let err = QjsPack::load(dir.path()).expect_err("bad manifest");
        assert!(err.contains("manifest"), "{err}");
    }

    #[test]
    fn pack_load_rejects_missing_bytecode() {
        let dir = TempDir::new().expect("tempdir");
        build_pack_dir("function entry() { return 1; }", dir.path()).expect("build");
        std::fs::remove_file(dir.path().join("pack.qjsc")).expect("remove");
        let err = QjsPack::load(dir.path()).expect_err("missing bytecode");
        assert!(err.contains("manifest_verify_bytecode_read"), "{err}");
    }

    #[test]
    fn pack_load_rejects_tampered_entry_source() {
        let dir = TempDir::new().expect("tempdir");
        build_pack_dir("function entry() { return 1; }", dir.path()).expect("build");
        std::fs::write(
            dir.path().join("entry.js"),
            "function entry() { return 999; }",
        )
        .expect("tamper");
        let err = QjsPack::load(dir.path()).expect_err("tampered source");
        assert!(err.contains("manifest_source_hash_mismatch"), "{err}");
    }

    #[test]
    fn build_pack_rejects_syntax_errors() {
        let dir = TempDir::new().expect("tempdir");
        let err = build_pack_dir("this is not valid js (((", dir.path()).expect_err("syntax error");
        assert!(err.contains("pack_compile"), "{err}");
    }
}
