//! Pack-building mechanics shared by agenterm's bytecode-producing script
//! backends (lua, qjs) — the read-file→SHA256-hex→compare pattern their
//! manifests' `verify_bytecode`/`verify_source` duplicated, the
//! serialize/write + read/parse pattern their qualification receipts
//! duplicated, and the `hash_source` wrapper their `compile.rs` files each
//! hand-rolled independently. rh's native-pack manifest
//! (`agenterm-rh::manifest`) is deliberately NOT built on this module — it
//! uses a different error type (`RhError`, not `String`) and hand-formats
//! its JSON as a literal string rather than via `serde_json::json!`/derived
//! `Serialize`, a genuinely different shape predating and diverging from
//! the lua/qjs convention captured here, not an oversight.

use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::hex::sha256_hex;

/// Read `dir.join(file)`, SHA256-hex it, and compare against
/// `expected_hex`.
///
/// `read_err_prefix` controls the read-failure message verbatim
/// (`"{read_err_prefix}: {io_error}"`) — callers historically used
/// different prefixes here (`manifest_verify_read` in lua's
/// `verify_bytecode`; `manifest_verify_bytecode_read` /
/// `manifest_verify_source_read` in qjs's `verify_bytecode` /
/// `verify_source`, the latter two asserted verbatim by
/// `agenterm-qjs`'s `pack.rs` tests), so this stays parameterized rather
/// than unified onto one literal string.
///
/// `mismatch_kind` names what's being verified (`"bytecode"` or
/// `"source"`) and produces
/// `"manifest_{mismatch_kind}_hash_mismatch: expected {expected}, got
/// {actual}"` — this text was already identical in *shape* across lua and
/// qjs (only the kind word differed), and qjs's
/// `manifest_source_hash_mismatch` wording is asserted verbatim by a test,
/// so the shape is preserved exactly rather than reworded.
pub fn verify_file_hash(
    dir: &Path,
    file: &str,
    expected_hex: &str,
    read_err_prefix: &str,
    mismatch_kind: &str,
) -> Result<(), String> {
    let path = dir.join(file);
    let bytes = std::fs::read(&path).map_err(|err| format!("{read_err_prefix}: {err}"))?;
    let actual = sha256_hex(&bytes);
    if actual != expected_hex {
        return Err(format!(
            "manifest_{mismatch_kind}_hash_mismatch: expected {expected_hex}, got {actual}"
        ));
    }
    Ok(())
}

/// Hash a source string (not bytecode): `sha256_hex` over its UTF-8 bytes.
/// Lua's and qjs's `compile.rs` each hand-rolled an identical wrapper over
/// their own local hex encoder before this crate existed.
pub fn hash_source(source: &str) -> String {
    sha256_hex(source.as_bytes())
}

/// Serialize `value` to pretty JSON and write it to `path` — the
/// `receipt_serialize` / `receipt_write` error-prefix pattern lua's and
/// qjs's `QualificationReceipt::write` shared verbatim.
pub fn write_json_receipt<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|err| format!("receipt_serialize: {err}"))?;
    std::fs::write(path, json).map_err(|err| format!("receipt_write: {err}"))
}

/// Read and parse a JSON receipt written by [`write_json_receipt`] — the
/// `receipt_read` / `receipt_parse` error-prefix pattern lua's and qjs's
/// `QualificationReceipt::read` shared verbatim.
pub fn read_json_receipt<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("receipt_read: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("receipt_parse: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn verify_file_hash_accepts_matching_content() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("f.bin"), b"hello").expect("write");
        let expected = sha256_hex(b"hello");
        verify_file_hash(dir.path(), "f.bin", &expected, "read_err", "bytecode").expect("match");
    }

    #[test]
    fn verify_file_hash_rejects_mismatch_with_kind_in_message() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("f.bin"), b"hello").expect("write");
        let err = verify_file_hash(dir.path(), "f.bin", "deadbeef", "read_err", "bytecode")
            .expect_err("mismatch");
        assert!(err.contains("manifest_bytecode_hash_mismatch"), "{err}");
        assert!(err.contains("expected deadbeef"), "{err}");
    }

    #[test]
    fn verify_file_hash_uses_source_kind_in_mismatch_message() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("f.bin"), b"hello").expect("write");
        let err = verify_file_hash(dir.path(), "f.bin", "deadbeef", "read_err", "source")
            .expect_err("mismatch");
        assert!(err.contains("manifest_source_hash_mismatch"), "{err}");
    }

    #[test]
    fn verify_file_hash_uses_caller_read_prefix_when_file_missing() {
        let dir = TempDir::new().expect("tempdir");
        let err =
            verify_file_hash(dir.path(), "missing.bin", "abc", "custom_read_prefix", "source")
                .expect_err("missing file");
        assert!(err.starts_with("custom_read_prefix: "), "{err}");
    }

    #[test]
    fn hash_source_matches_sha256_hex_of_utf8_bytes() {
        assert_eq!(hash_source("hello"), sha256_hex(b"hello"));
    }

    #[test]
    fn hash_source_deterministic_and_hex() {
        let h1 = hash_source("same");
        let h2 = hash_source("same");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Sample {
        n: i64,
        s: String,
    }

    #[test]
    fn write_json_receipt_then_read_json_receipt_roundtrips() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("receipt.json");
        let sample = Sample {
            n: 7,
            s: "x".into(),
        };
        write_json_receipt(&path, &sample).expect("write");
        let loaded: Sample = read_json_receipt(&path).expect("read");
        assert_eq!(loaded, sample);
    }

    #[test]
    fn read_json_receipt_reports_missing_file() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nope.json");
        let err = read_json_receipt::<Sample>(&path).expect_err("missing");
        assert!(err.starts_with("receipt_read: "), "{err}");
    }

    #[test]
    fn read_json_receipt_reports_parse_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").expect("write");
        let err = read_json_receipt::<Sample>(&path).expect_err("bad json");
        assert!(err.starts_with("receipt_parse: "), "{err}");
    }
}
