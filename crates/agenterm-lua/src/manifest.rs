//! Lua pack manifest: JSON schema `agenterm.lua-pack-manifest/v1`.

use std::path::Path;

use sha2::Digest;

/// Lua pack manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaPackManifest {
    pub schema: String,
    pub version: String,
    pub source_hash: String,
    pub bytecode_hash: String,
    pub bytecode_file: String,
}

impl LuaPackManifest {
    pub fn write(
        path: &Path,
        source_hash: &str,
        bytecode_hash: &str,
        bytecode_file: &str,
    ) -> Result<(), String> {
        let json = serde_json::json!({
            "schema": "agenterm.lua-pack-manifest/v1",
            "version": "0.1.0",
            "source_hash": source_hash,
            "bytecode_hash": bytecode_hash,
            "bytecode_file": bytecode_file,
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
            schema: required_string(&value, "schema")?,
            version: required_string(&value, "version")?,
            source_hash: required_string(&value, "source_hash")?,
            bytecode_hash: required_string(&value, "bytecode_hash")?,
            bytecode_file: required_string(&value, "bytecode_file")?,
        })
    }

    /// Verify the manifest's bytecode_hash matches the actual bytecode file.
    pub fn verify_bytecode(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(&self.bytecode_file);
        let bytes =
            std::fs::read(&path).map_err(|e| format!("manifest_verify_read: {e}"))?;
        let hash = sha2::Sha256::digest(&bytes);
        let actual_hex = hex_encode_sha256(&hash);
        if actual_hex != self.bytecode_hash {
            return Err(format!(
                "manifest_bytecode_hash_mismatch: expected {}, got {}",
                self.bytecode_hash, actual_hex
            ));
        }
        Ok(())
    }
}

fn required_string(value: &serde_json::Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned())
        .ok_or_else(|| format!("manifest_missing_field: {key}"))
}

fn hex_encode_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_and_read_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("manifest.json");
        LuaPackManifest::write(
            &path,
            "abc123",
            "def456",
            "pack.luac",
        )
        .expect("write");
        let m = LuaPackManifest::read(&path).expect("read");
        assert_eq!(m.schema, "agenterm.lua-pack-manifest/v1");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.source_hash, "abc123");
        assert_eq!(m.bytecode_hash, "def456");
        assert_eq!(m.bytecode_file, "pack.luac");
    }

    #[test]
    fn parse_rejects_missing_fields() {
        let json = r#"{"schema": "x", "version": "1"}"#;
        let err = LuaPackManifest::parse(json.as_bytes()).expect_err("missing fields");
        assert!(err.contains("source_hash"), "{err}");
    }
}
