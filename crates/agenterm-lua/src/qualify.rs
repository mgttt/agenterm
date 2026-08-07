//! Lua pack qualification: build + load + entry execution → receipt.

use std::path::Path;

use crate::pack::{LuaPack, build_pack_dir};

/// Lua qualification receipt (JSON-serializable).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LuaQualificationReceipt {
    pub schema: String,
    pub version: String,
    pub source_hash: String,
    pub bytecode_hash: String,
    pub entry_value: i64,
}

impl LuaQualificationReceipt {
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("receipt_serialize: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("receipt_write: {e}"))
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("receipt_read: {e}"))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| format!("receipt_parse: {e}"))
    }
}

/// Build a pack, load it, and execute the entry to produce a qualification receipt.
pub fn qualify_pack_dir(
    source: &str,
    dir: &Path,
    host: &crate::LuaHostFunctions,
) -> Result<LuaQualificationReceipt, String> {
    build_pack_dir(source, dir)?;
    let pack = LuaPack::load(dir)?;
    let result = pack.eval(host).map_err(|e| e.to_string())?;
    Ok(LuaQualificationReceipt {
        schema: "agenterm.lua-qualification/v1".to_string(),
        version: "0.1.0".to_string(),
        source_hash: pack.manifest.source_hash.clone(),
        bytecode_hash: pack.manifest.bytecode_hash.clone(),
        entry_value: result.value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn qualify_produces_valid_receipt() {
        let dir = TempDir::new().expect("tempdir");
        let host = crate::LuaHostFunctions::default();
        let receipt = qualify_pack_dir("return 42", dir.path(), &host).expect("qualify");
        assert_eq!(receipt.schema, "agenterm.lua-qualification/v1");
        assert_eq!(receipt.entry_value, 42);
        assert_eq!(receipt.source_hash.len(), 64);
        assert_eq!(receipt.bytecode_hash.len(), 64);
    }

    #[test]
    fn receipt_write_read_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let host = crate::LuaHostFunctions::default();
        let receipt = qualify_pack_dir("return 7", dir.path(), &host).expect("qualify");

        let receipt_path = dir.path().join("receipt.json");
        receipt.write(&receipt_path).expect("write");
        let loaded = LuaQualificationReceipt::read(&receipt_path).expect("read");
        assert_eq!(loaded.entry_value, 7);
        assert_eq!(loaded.source_hash, receipt.source_hash);
    }
}
