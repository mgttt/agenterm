//! qjs pack qualification: build + load + entry execution → receipt.
//! Same shape/purpose as `agenterm_lua::qualify` — proves a pack was built
//! and its entry actually runs, not just that it parses.

use std::path::Path;

use serde_json::Value as JsonValue;

use crate::host::QjsHostFunctions;
use crate::pack::{QjsPack, build_pack_dir};

/// qjs qualification receipt (JSON-serializable).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct QjsQualificationReceipt {
    pub schema: String,
    pub version: String,
    pub source_hash: String,
    pub bytecode_hash: String,
    pub entry_value: Option<JsonValue>,
    pub stdout: String,
}

impl QjsQualificationReceipt {
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| format!("receipt_serialize: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("receipt_write: {e}"))
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("receipt_read: {e}"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("receipt_parse: {e}"))
    }
}

/// Build a pack, load it, and execute the entry to produce a qualification
/// receipt.
pub fn qualify_pack_dir(
    source: &str,
    dir: &Path,
    host: &QjsHostFunctions,
) -> Result<QjsQualificationReceipt, String> {
    build_pack_dir(source, dir)?;
    let pack = QjsPack::load(dir)?;
    let result = pack.eval(host).map_err(|e| e.to_string())?;
    Ok(QjsQualificationReceipt {
        schema: "agenterm.qjs-qualification/v1".to_string(),
        version: "0.1.0".to_string(),
        source_hash: pack.manifest.source_hash.clone(),
        bytecode_hash: pack.manifest.bytecode_hash.clone(),
        entry_value: result.value,
        stdout: result.stdout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn qualify_produces_valid_receipt() {
        let dir = TempDir::new().expect("tempdir");
        let host = QjsHostFunctions::default();
        let receipt = qualify_pack_dir("function entry() { return 42; }", dir.path(), &host)
            .expect("qualify");
        assert_eq!(receipt.schema, "agenterm.qjs-qualification/v1");
        assert_eq!(receipt.entry_value, Some(json!(42)));
        assert_eq!(receipt.source_hash.len(), 64);
        assert_eq!(receipt.bytecode_hash.len(), 64);
    }

    #[test]
    fn receipt_write_read_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let host = QjsHostFunctions::default();
        let receipt =
            qualify_pack_dir("function entry() { return 7; }", dir.path(), &host).expect("qualify");

        let receipt_path = dir.path().join("receipt.json");
        receipt.write(&receipt_path).expect("write");
        let loaded = QjsQualificationReceipt::read(&receipt_path).expect("read");
        assert_eq!(loaded.entry_value, Some(json!(7)));
        assert_eq!(loaded.source_hash, receipt.source_hash);
    }

    #[test]
    fn qualify_fails_closed_without_entry() {
        let dir = TempDir::new().expect("tempdir");
        let host = QjsHostFunctions::default();
        let err = qualify_pack_dir("40 + 2;", dir.path(), &host).expect_err("no entry()");
        assert!(err.contains("entry()"), "{err}");
    }
}
