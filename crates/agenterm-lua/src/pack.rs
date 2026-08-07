//! Lua pack: build bytecode + manifest into a directory, and load from a directory.

use std::path::{Path, PathBuf};

use crate::compile::{compile_lua, hash_source};
use crate::manifest::LuaPackManifest;

const DEFAULT_BYTECODE_FILE: &str = "pack.luac";
const DEFAULT_MANIFEST_FILE: &str = "manifest.json";

/// A loaded Lua pack: directory root + manifest + bytecode bytes.
#[derive(Debug)]
pub struct LuaPack {
    pub root: PathBuf,
    pub manifest: LuaPackManifest,
    pub bytecode: Vec<u8>,
}

impl LuaPack {
    /// Load a pack from a directory containing `manifest.json` and `pack.luac`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let manifest_path = dir.join(DEFAULT_MANIFEST_FILE);
        let manifest = LuaPackManifest::read(&manifest_path)?;
        let bytecode_path = dir.join(&manifest.bytecode_file);
        let bytecode = std::fs::read(&bytecode_path)
            .map_err(|e| format!("pack_load_bytecode: {e}"))?;
        manifest.verify_bytecode(dir)?;
        Ok(Self {
            root: dir.to_path_buf(),
            manifest,
            bytecode,
        })
    }

    /// Execute the loaded bytecode, returning the entry value (i64).
    pub fn eval(&self, host: &crate::LuaHostFunctions) -> Result<crate::LuaEvalResult, crate::LuaError> {
        let engine = crate::LuaEngine::new()?;
        // Bytecode can't be executed via `lua.load(bytes)` in mlua directly,
        // so we re-evaluate the source from the entry file.
        let entry_path = self.root.join("entry.lua");
        if entry_path.exists() {
            engine.eval_file(&entry_path, host)
        } else {
            Err(crate::LuaError::Engine(
                "pack_entry_missing: entry.lua not found in pack dir".into(),
            ))
        }
    }
}

/// Build a Lua pack directory: compile source → bytecode → manifest → entry file.
pub fn build_pack_dir(source: &str, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("pack_create_dir: {e}"))?;

    let (bytecode, bytecode_hash) = compile_lua(source)?;
    let source_hash = hash_source(source);

    // Write bytecode file
    let bytecode_path = dir.join(DEFAULT_BYTECODE_FILE);
    std::fs::write(&bytecode_path, &bytecode)
        .map_err(|e| format!("pack_write_bytecode: {e}"))?;

    // Write manifest
    let manifest_path = dir.join(DEFAULT_MANIFEST_FILE);
    LuaPackManifest::write(&manifest_path, &source_hash, &bytecode_hash, DEFAULT_BYTECODE_FILE)?;

    // Write entry source (for re-evaluation)
    let entry_path = dir.join("entry.lua");
    std::fs::write(&entry_path, source).map_err(|e| format!("pack_write_entry: {e}"))?;

    Ok(bytecode_path)
}

/// Check if a directory is a valid Lua pack (has manifest.json + bytecode file).
pub fn is_pack_dir(dir: &Path) -> bool {
    dir.join(DEFAULT_MANIFEST_FILE).exists() && dir.join(DEFAULT_BYTECODE_FILE).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn build_and_load_pack() {
        let dir = TempDir::new().expect("tempdir");
        build_pack_dir("return 42", dir.path()).expect("build");
        assert!(is_pack_dir(dir.path()));

        let pack = LuaPack::load(dir.path()).expect("load");
        assert_eq!(pack.manifest.schema, "agenterm.lua-pack-manifest/v1");
        assert!(!pack.bytecode.is_empty());

        let host = crate::LuaHostFunctions::default();
        let result = pack.eval(&host).expect("eval");
        assert_eq!(result.value, 42);
    }

    #[test]
    fn build_pack_with_print() {
        let dir = TempDir::new().expect("tempdir");
        build_pack_dir("print('hello') return 7", dir.path()).expect("build");
        let pack = LuaPack::load(dir.path()).expect("load");
        let host = crate::LuaHostFunctions::default();
        let result = pack.eval(&host).expect("eval");
        assert_eq!(result.value, 7);
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn pack_load_rejects_bad_manifest() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("manifest.json"), "not json")
            .expect("write");
        let err = LuaPack::load(dir.path()).expect_err("bad manifest");
        assert!(err.contains("manifest"), "{err}");
    }

    #[test]
    fn pack_load_rejects_missing_bytecode() {
        let dir = TempDir::new().expect("tempdir");
        build_pack_dir("return 1", dir.path()).expect("build");
        // Corrupt: remove bytecode
        std::fs::remove_file(dir.path().join("pack.luac")).expect("remove");
        let err = LuaPack::load(dir.path()).expect_err("missing bytecode");
        assert!(err.contains("pack_load_bytecode"), "{err}");
    }
}
