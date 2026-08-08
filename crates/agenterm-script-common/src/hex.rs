//! Hex/hash/JSON helpers duplicated verbatim across engine `manifest.rs`
//! files before this crate existed (`agenterm_lua::manifest`,
//! `agenterm_qjs::manifest` both hand-rolled the same hex encoder to avoid
//! a `hex` crate dependency for a two-field job — see either module's old
//! git history). rh keeps its own manifest format (native-binary hash
//! verification via `load::verify_native_hash`, not a bytecode fingerprint)
//! so it doesn't consume this module, but nothing here is lua/qjs-specific.

use sha2::Digest;

/// Hex-encode raw bytes (lowercase, no separator).
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// SHA-256 a byte slice and hex-encode the digest.
pub fn sha256_hex(data: &[u8]) -> String {
    hex_encode(&sha2::Sha256::digest(data))
}

/// Pull a required string field out of a parsed JSON manifest, with the
/// `manifest_missing_field: <key>` error shape both `LuaPackManifest` and
/// `QjsPackManifest` already used independently.
pub fn required_json_string(value: &serde_json::Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned())
        .ok_or_else(|| format!("manifest_missing_field: {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encode_matches_known_vector() {
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        // sha256("") — a standard test vector, not asserting anything about
        // this implementation beyond "it calls through to sha2 correctly".
        let digest = sha256_hex(b"");
        assert_eq!(digest.len(), 64);
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn required_json_string_reports_missing_field() {
        let value = serde_json::json!({"schema": "x"});
        let err = required_json_string(&value, "version").expect_err("missing");
        assert!(err.contains("version"), "{err}");
    }
}
