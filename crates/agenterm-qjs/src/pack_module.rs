//! Self-contained multi-file "module packs" — the second half of QJS-M5d
//! in `plan/design-qjs-module-imports.md`. A module pack copies its
//! entire *static* import graph (entry + every transitively-imported
//! file, confined to `project_root` by the same `ProjectModuleResolver`
//! checks everything else in this engine uses) into the pack directory,
//! so `load` never needs to know the original `project_root` again — the
//! pack directory *is* its own project root at load time.
//!
//! This is a verified design choice, not a shortcut: empirically
//! confirmed before writing this file that rquickjs's `Module::write()`
//! does **not** embed an imported module's bytecode/content into the
//! entry module's serialized bytecode — changing an imported file's
//! content left the entry's own serialized bytecode byte-identical (same
//! length, same bytes). So a single-blob "bytecode captures the whole
//! graph" pack format isn't something rquickjs 0.12's public API can
//! produce; copying source files is the correct shape for a
//! self-contained multi-file pack, not a fallback taken because the
//! better option was too much work.
//!
//! Distinct manifest schema (`agenterm.qjs-module-pack-manifest/v1`), not
//! a variant bolted onto `pack.rs`'s single-file `QjsPackManifest` — the
//! two have genuinely different shapes (a file list vs. a single
//! bytecode file) and conflating them under one struct with
//! sometimes-populated fields is exactly the kind of "one type, two
//! silently-different meanings" bug worth avoiding on purpose.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agenterm_script_common::hex::sha256_hex;
use agenterm_script_common::pack_support::write_json_receipt;
use rquickjs::loader::{ImportAttributes, Loader, ScriptLoader};
use rquickjs::{CatchResultExt, Context, Module, Runtime};

use crate::error::QjsError;
use crate::eval::EvalOutcome;
use crate::eval_module::eval_module_entry_with_host;
use crate::host::QjsHostFunctions;
use crate::module_resolver::ProjectModuleResolver;

pub const MODULE_PACK_SCHEMA: &str = "agenterm.qjs-module-pack-manifest/v1";

/// Wraps `ScriptLoader`, recording the resolved path of every module it
/// successfully loads — used to discover a script's full static import
/// graph by piggybacking on the same real `Module::declare` linking pass
/// `check_with_project_validation`/`eval_module_entry_with_host` already
/// use, instead of writing a second, independently-drifting graph walker
/// (e.g. a textual `import` scanner like `module_sniff.rs`'s, which is
/// deliberately only a cheap routing heuristic, not a resolver).
struct RecordingLoader {
    inner: ScriptLoader,
    touched: Arc<Mutex<Vec<PathBuf>>>,
}

impl Loader for RecordingLoader {
    fn load<'js>(
        &mut self,
        ctx: &rquickjs::Ctx<'js>,
        path: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        let module = self.inner.load(ctx, path, attributes)?;
        if let Ok(mut touched) = self.touched.lock() {
            touched.push(PathBuf::from(path));
        }
        Ok(module)
    }
}

/// Discover a module-mode script's full static import graph: the entry
/// file itself plus every file transitively `import`ed from it, each a
/// canonical absolute path confined to `project_root` — an escaping
/// import fails this call the same way it fails `check`/`eval`, not
/// silently omitted from the list.
pub fn discover_import_graph(
    source: &str,
    entry_path: &Path,
    project_root: &Path,
) -> Result<Vec<PathBuf>, QjsError> {
    let canonical_root = std::fs::canonicalize(project_root).map_err(|err| {
        QjsError::Check(format!("qjs_module_root: {}: {err}", project_root.display()))
    })?;
    let canonical_entry = std::fs::canonicalize(entry_path).map_err(|err| {
        QjsError::Check(format!("qjs_module_entry: {}: {err}", entry_path.display()))
    })?;
    if !canonical_entry.starts_with(&canonical_root) {
        return Err(QjsError::Check(format!(
            "qjs_module_entry_outside_root: {} is not inside {}",
            canonical_entry.display(),
            canonical_root.display()
        )));
    }

    let touched: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
    let runtime = Runtime::new().map_err(|err| QjsError::Check(err.to_string()))?;
    runtime.set_loader(
        ProjectModuleResolver::new(canonical_root.clone()),
        RecordingLoader {
            inner: ScriptLoader::default().with_extension("mjs"),
            touched: Arc::clone(&touched),
        },
    );
    let context = Context::full(&runtime).map_err(|err| QjsError::Check(err.to_string()))?;
    let label = canonical_entry.to_string_lossy().into_owned();
    context.with(|ctx| -> Result<(), QjsError> {
        Module::declare(ctx.clone(), label.as_str(), source)
            .catch(&ctx)
            .map_err(|err| QjsError::Parse(err.to_string()))?;
        Ok(())
    })?;

    let mut files = vec![canonical_entry];
    let mut recorded = touched
        .lock()
        .map_err(|err| QjsError::Check(err.to_string()))?
        .clone();
    files.append(&mut recorded);
    files.sort();
    files.dedup();
    Ok(files)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct QjsModulePackFile {
    /// Path relative to the pack directory (== relative to the original
    /// `project_root` at build time, since the copy preserves structure).
    pub path: String,
    pub content_hash: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct QjsModulePackManifest {
    pub schema: String,
    pub version: String,
    /// Entry file's path, relative to the pack directory.
    pub entry_file: String,
    /// Every file in the graph, including the entry itself, relative
    /// paths + content hashes — the pack's own load-time integrity check
    /// (`QjsModulePack::load` verifies every one) doubles as a
    /// reproducibility fingerprint, no separate "bytecode_hash" field
    /// (see this module's doc for why one isn't meaningful here).
    pub files: Vec<QjsModulePackFile>,
    /// SHA256 over the sorted `"{path}\0{content_hash}\n"` lines of
    /// `files` — one fingerprint for the whole graph's shape+content.
    pub graph_hash: String,
}

impl QjsModulePackManifest {
    // NOT adopted onto `agenterm_script_common::pack_support`'s
    // `write_json_receipt`/`read_json_receipt`: those bake in fixed
    // `receipt_serialize`/`receipt_write`/`receipt_read`/`receipt_parse`
    // error prefixes, but this manifest's observable text has always been
    // `manifest_serialize`/`manifest_write`/`manifest_read`/`manifest_parse`
    // (matching `pack.rs`'s `QjsPackManifest`, a distinct type with its own
    // hand-rolled write/read predating `pack_support`). Adopting the shared
    // helpers here would change that text with no parameter in either
    // helper to preserve it, so this stays a local, structurally-identical
    // duplicate rather than a signature change to `pack_support` for one
    // caller's wording.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let json =
            serde_json::to_string_pretty(self).map_err(|err| format!("manifest_serialize: {err}"))?;
        std::fs::write(path, json).map_err(|err| format!("manifest_write: {err}"))
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|err| format!("manifest_read: {err}"))?;
        serde_json::from_slice(&bytes).map_err(|err| format!("manifest_parse: {err}"))
    }

    /// Verify every file's `content_hash` against what's actually on disk
    /// in `dir` — load-bearing (unlike `pack.rs`'s `verify_bytecode`,
    /// every file here is what execution actually reads).
    ///
    /// NOT adopted onto `pack_support::verify_file_hash`: that helper's
    /// mismatch text is fixed as `manifest_{kind}_hash_mismatch: expected
    /// {expected}, got {actual}` (no file path — `pack.rs`'s single-file
    /// manifest only ever verifies one bytecode/source file per call, so it
    /// never needed one), whereas this loop verifies N files per manifest
    /// and its `module_pack_content_hash_mismatch: {path}: expected
    /// {expected}, got {actual}` text — asserted by
    /// `load_rejects_a_tampered_file` — needs the path to say which of the
    /// N files failed. `verify_file_hash`'s read-error half *would* line up
    /// (its `{read_err_prefix}: {err}` matches `module_pack_verify_read:
    /// {path}: {err}` if `read_err_prefix` is built as
    /// `format!("module_pack_verify_read: {}", file.path)`), but splitting
    /// read and compare across two different helpers/paths per file is not
    /// simpler than the single local loop, so both stay local together
    /// rather than changing `verify_file_hash`'s signature to also carry a
    /// path into its mismatch text.
    fn verify_files(&self, dir: &Path) -> Result<(), String> {
        for file in &self.files {
            let path = dir.join(&file.path);
            let bytes = std::fs::read(&path)
                .map_err(|err| format!("module_pack_verify_read: {}: {err}", file.path))?;
            let actual = sha256_hex(&bytes);
            if actual != file.content_hash {
                return Err(format!(
                    "module_pack_content_hash_mismatch: {}: expected {}, got {actual}",
                    file.path, file.content_hash
                ));
            }
        }
        Ok(())
    }
}

/// Hash the whole graph's shape+content. Not a `pack_support` duplicate —
/// nothing there hashes a *list* of (path, hash) pairs into one fingerprint,
/// this is unique to the multi-file module-pack manifest.
fn graph_hash(files: &[QjsModulePackFile]) -> String {
    let mut sorted = files.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut buffer = String::new();
    for file in &sorted {
        buffer.push_str(&file.path);
        buffer.push('\0');
        buffer.push_str(&file.content_hash);
        buffer.push('\n');
    }
    sha256_hex(buffer.as_bytes())
}

/// A loaded module pack: directory root + manifest + the entry's source
/// (re-read at load time, same "execution re-parses" choice `pack.rs`
/// makes for single-file packs and for the same underlying reason — see
/// `eval_module.rs`'s doc on why real bytecode loading isn't wired up
/// yet).
#[derive(Debug)]
pub struct QjsModulePack {
    pub root: PathBuf,
    pub manifest: QjsModulePackManifest,
    entry_source: String,
}

impl QjsModulePack {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let manifest_path = dir.join("manifest.json");
        let manifest = QjsModulePackManifest::read(&manifest_path)?;
        manifest.verify_files(dir)?;
        let entry_path = dir.join(&manifest.entry_file);
        let entry_source = std::fs::read_to_string(&entry_path)
            .map_err(|err| format!("module_pack_load_entry: {err}"))?;
        Ok(Self {
            root: dir.to_path_buf(),
            manifest,
            entry_source,
        })
    }

    /// Execute the pack's entry — `project_root` for import resolution is
    /// the pack directory itself, since every file the graph could ever
    /// reference was copied in at build time under matching relative
    /// paths. A pack moved to a different machine/path still works; it
    /// never needed the original `project_root` to exist.
    pub fn eval(&self, host: &QjsHostFunctions) -> Result<EvalOutcome, QjsError> {
        let entry_path = self.root.join(&self.manifest.entry_file);
        eval_module_entry_with_host(&self.entry_source, &entry_path, &self.root, host)
    }
}

/// Build a module pack directory: discover the import graph, copy every
/// file into `dir` preserving its path relative to `project_root`, write
/// the manifest.
pub fn build_module_pack_dir(
    source: &str,
    entry_path: &Path,
    project_root: &Path,
    dir: &Path,
) -> Result<PathBuf, String> {
    let graph = discover_import_graph(source, entry_path, project_root)
        .map_err(|err| format!("module_pack_discover: {err}"))?;
    let canonical_root = std::fs::canonicalize(project_root)
        .map_err(|err| format!("module_pack_root: {err}"))?;
    let canonical_entry =
        std::fs::canonicalize(entry_path).map_err(|err| format!("module_pack_entry: {err}"))?;

    std::fs::create_dir_all(dir).map_err(|err| format!("module_pack_create_dir: {err}"))?;

    let mut files = Vec::with_capacity(graph.len());
    for file in &graph {
        let relative = file
            .strip_prefix(&canonical_root)
            .map_err(|err| format!("module_pack_relative: {err}"))?;
        let dest = dir.join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("module_pack_mkdir: {err}"))?;
        }
        let bytes = std::fs::read(file).map_err(|err| format!("module_pack_read: {err}"))?;
        std::fs::write(&dest, &bytes).map_err(|err| format!("module_pack_write: {err}"))?;
        files.push(QjsModulePackFile {
            path: relative.to_string_lossy().replace('\\', "/"),
            content_hash: sha256_hex(&bytes),
        });
    }

    let entry_relative = canonical_entry
        .strip_prefix(&canonical_root)
        .map_err(|err| format!("module_pack_relative: {err}"))?
        .to_string_lossy()
        .replace('\\', "/");

    let manifest = QjsModulePackManifest {
        schema: MODULE_PACK_SCHEMA.to_owned(),
        version: "0.1.0".to_owned(),
        entry_file: entry_relative,
        graph_hash: graph_hash(&files),
        files,
    };
    let manifest_path = dir.join("manifest.json");
    manifest.write(&manifest_path)?;
    Ok(manifest_path)
}

/// Build + load + entry() → receipt, the module-pack analog of
/// `qualify::qualify_pack_dir`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct QjsModuleQualificationReceipt {
    pub schema: String,
    pub version: String,
    pub graph_hash: String,
    pub file_count: usize,
    pub entry_value: Option<serde_json::Value>,
    pub stdout: String,
}

impl QjsModuleQualificationReceipt {
    // Adopted onto `pack_support::write_json_receipt`: this type's
    // `receipt_serialize`/`receipt_write` text was already byte-for-byte
    // identical to the shared helper's (both predate/postdate `pack.rs`'s
    // `QjsQualificationReceipt`, which already made this same call), so no
    // local duplicate is needed here.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        write_json_receipt(path, self)
    }
}

pub fn qualify_module_pack_dir(
    source: &str,
    entry_path: &Path,
    project_root: &Path,
    dir: &Path,
    host: &QjsHostFunctions,
) -> Result<QjsModuleQualificationReceipt, String> {
    build_module_pack_dir(source, entry_path, project_root, dir)?;
    let pack = QjsModulePack::load(dir)?;
    let result = pack.eval(host).map_err(|err| err.to_string())?;
    Ok(QjsModuleQualificationReceipt {
        schema: "agenterm.qjs-module-qualification/v1".to_owned(),
        version: "0.1.0".to_owned(),
        graph_hash: pack.manifest.graph_hash.clone(),
        file_count: pack.manifest.files.len(),
        entry_value: result.value,
        stdout: result.stdout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn write_project(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("lib/value.js"), "export const value = 42;").unwrap();
        let entry = dir.join("entry.js");
        std::fs::write(
            &entry,
            "import { value } from './lib/value.js';\nexport function entry() { print('graph value: ' + value); return value; }",
        )
        .unwrap();
        entry
    }

    #[test]
    fn discovers_the_entry_plus_every_imported_file() {
        let project = TempDir::new().unwrap();
        let entry = write_project(project.path());
        let source = std::fs::read_to_string(&entry).unwrap();
        let graph = discover_import_graph(&source, &entry, project.path()).expect("discover");
        assert_eq!(graph.len(), 2);
        assert!(graph.contains(&std::fs::canonicalize(&entry).unwrap()));
        assert!(graph.contains(&std::fs::canonicalize(project.path().join("lib/value.js")).unwrap()));
    }

    #[test]
    fn discover_rejects_an_import_that_escapes_the_root() {
        let outer = TempDir::new().unwrap();
        std::fs::write(outer.path().join("secret.js"), "export const value = 1;").unwrap();
        let inner = outer.path().join("project");
        std::fs::create_dir_all(&inner).unwrap();
        let entry = inner.join("entry.js");
        std::fs::write(
            &entry,
            "import { value } from '../secret.js';\nexport function entry() { return value; }",
        )
        .unwrap();
        let source = std::fs::read_to_string(&entry).unwrap();
        let error =
            discover_import_graph(&source, &entry, &inner).expect_err("must reject escape");
        assert!(error.to_string().contains("Error resolving module"), "{error}");
    }

    #[test]
    fn build_and_load_a_module_pack() {
        let project = TempDir::new().unwrap();
        let entry = write_project(project.path());
        let source = std::fs::read_to_string(&entry).unwrap();

        let pack_dir = TempDir::new().unwrap();
        build_module_pack_dir(&source, &entry, project.path(), pack_dir.path()).expect("build");

        let pack = QjsModulePack::load(pack_dir.path()).expect("load");
        assert_eq!(pack.manifest.files.len(), 2);
        let host = QjsHostFunctions::default();
        let outcome = pack.eval(&host).expect("eval");
        assert_eq!(outcome.value, Some(json!(42)));
        assert!(outcome.stdout.contains("graph value: 42"));
    }

    #[test]
    fn a_built_pack_is_self_contained_after_the_original_project_is_gone() {
        // The whole point of copying the graph in: prove the pack doesn't
        // secretly still depend on `project_root` existing.
        let project = TempDir::new().unwrap();
        let entry = write_project(project.path());
        let source = std::fs::read_to_string(&entry).unwrap();

        let pack_dir = TempDir::new().unwrap();
        build_module_pack_dir(&source, &entry, project.path(), pack_dir.path()).expect("build");

        drop(project); // delete the original project directory entirely

        let pack = QjsModulePack::load(pack_dir.path()).expect("load after original is gone");
        let host = QjsHostFunctions::default();
        let outcome = pack.eval(&host).expect("eval after original is gone");
        assert_eq!(outcome.value, Some(json!(42)));
    }

    #[test]
    fn load_rejects_a_tampered_file() {
        let project = TempDir::new().unwrap();
        let entry = write_project(project.path());
        let source = std::fs::read_to_string(&entry).unwrap();

        let pack_dir = TempDir::new().unwrap();
        build_module_pack_dir(&source, &entry, project.path(), pack_dir.path()).expect("build");

        std::fs::write(
            pack_dir.path().join("lib/value.js"),
            "export const value = 999999;",
        )
        .unwrap();

        let error = QjsModulePack::load(pack_dir.path()).expect_err("tampered file must be rejected");
        assert!(error.contains("module_pack_content_hash_mismatch"), "{error}");
    }

    #[test]
    fn qualify_produces_a_receipt() {
        let project = TempDir::new().unwrap();
        let entry = write_project(project.path());
        let source = std::fs::read_to_string(&entry).unwrap();

        let pack_dir = TempDir::new().unwrap();
        let host = QjsHostFunctions::default();
        let receipt =
            qualify_module_pack_dir(&source, &entry, project.path(), pack_dir.path(), &host)
                .expect("qualify");
        assert_eq!(receipt.file_count, 2);
        assert_eq!(receipt.entry_value, Some(json!(42)));
        assert_eq!(receipt.graph_hash.len(), 64);
    }
}
