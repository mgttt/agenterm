//! qjs pack manifest: JSON schema `agenterm.qjs-pack-manifest/v1`. Same
//! shape and hashing convention as `agenterm_lua::manifest::LuaPackManifest`
//! (`source_hash`/`bytecode_hash`/file pointers, hex/hash helpers shared via
//! `agenterm_script_common::hex`) — see `compile.rs`'s module doc for what
//! `bytecode_hash` here actually verifies (a real QuickJS module-bytecode
//! fingerprint, not something loaded back at eval time).

use std::path::Path;

use agenterm_script_common::hex::{required_json_string, sha256_hex};

/// qjs pack manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QjsPackManifest {
    pub schema: String,
    pub version: String,
    pub source_hash: String,
    pub bytecode_hash: String,
    pub bytecode_file: String,
    pub entry_file: String,
}

impl QjsPackManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn write(
        path: &Path,
        source_hash: &str,
        bytecode_hash: &str,
        bytecode_file: &str,
        entry_file: &str,
    ) -> Result<(), String> {
        let json = serde_json::json!({
            "schema": "agenterm.qjs-pack-manifest/v1",
            "version": "0.1.0",
            "source_hash": source_hash,
            "bytecode_hash": bytecode_hash,
            "bytecode_file": bytecode_file,
            "entry_file": entry_file,
        });
        let text =
            serde_json::to_string_pretty(&json).map_err(|e| format!("manifest_serialize: {e}"))?;
        std::fs::write(path, text).map_err(|e| format!("manifest_write: {e}"))
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("manifest_read: {e}"))?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| format!("manifest_json: {e}"))?;
        Ok(Self {
            schema: required_json_string(&value, "schema")?,
            version: required_json_string(&value, "version")?,
            source_hash: required_json_string(&value, "source_hash")?,
            bytecode_hash: required_json_string(&value, "bytecode_hash")?,
            bytecode_file: required_json_string(&value, "bytecode_file")?,
            entry_file: required_json_string(&value, "entry_file")?,
        })
    }

    /// Verify the manifest's `bytecode_hash` matches the actual bytecode
    /// file on disk (reproducibility check, not a load-bearing execution
    /// step — see `compile.rs`).
    pub fn verify_bytecode(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(&self.bytecode_file);
        let bytes = std::fs::read(&path).map_err(|e| format!("manifest_verify_bytecode_read: {e}"))?;
        let actual = sha256_hex(&bytes);
        if actual != self.bytecode_hash {
            return Err(format!(
                "manifest_bytecode_hash_mismatch: expected {}, got {actual}",
                self.bytecode_hash
            ));
        }
        Ok(())
    }

    /// Verify the manifest's `source_hash` matches the actual entry source
    /// file on disk — this one *is* load-bearing: `pack.rs`'s `eval` re-runs
    /// this exact source.
    pub fn verify_source(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(&self.entry_file);
        let bytes = std::fs::read(&path).map_err(|e| format!("manifest_verify_source_read: {e}"))?;
        let actual = sha256_hex(&bytes);
        if actual != self.source_hash {
            return Err(format!(
                "manifest_source_hash_mismatch: expected {}, got {actual}",
                self.source_hash
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_and_read_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("manifest.json");
        QjsPackManifest::write(&path, "abc123", "def456", "pack.qjsc", "entry.js").expect("write");
        let m = QjsPackManifest::read(&path).expect("read");
        assert_eq!(m.schema, "agenterm.qjs-pack-manifest/v1");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.source_hash, "abc123");
        assert_eq!(m.bytecode_hash, "def456");
        assert_eq!(m.bytecode_file, "pack.qjsc");
        assert_eq!(m.entry_file, "entry.js");
    }

    #[test]
    fn parse_rejects_missing_fields() {
        let json = r#"{"schema": "x", "version": "1"}"#;
        let err = QjsPackManifest::parse(json.as_bytes()).expect_err("missing fields");
        assert!(err.contains("source_hash"), "{err}");
    }
}
