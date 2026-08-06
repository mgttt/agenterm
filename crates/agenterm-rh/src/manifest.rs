use std::path::Path;

use crate::{RH_VERSION, RhError, load::verify_native_hash};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RhPackManifest {
    pub schema: String,
    pub rh_version: String,
    pub source_hash: String,
    pub native_hash: String,
    pub native_file: String,
    pub entry_symbol: String,
    pub cc_line_count: Option<u32>,
}

impl RhPackManifest {
    pub fn write(
        path: &Path,
        source_hash: &str,
        native_hash: &str,
        native_file: &str,
        cc_line_count: Option<u32>,
    ) -> Result<(), RhError> {
        let cc_line_field = match cc_line_count {
            Some(count) => format!(",\n  \"cc_line_count\": {count}"),
            None => String::new(),
        };
        let json = format!(
            "{{\n  \"schema\": \"agenterm.rh-pack-manifest/v1\",\n  \
             \"rh_version\": \"{RH_VERSION}\",\n  \
             \"source_hash\": \"{source_hash}\",\n  \
             \"native_hash\": \"{native_hash}\",\n  \
             \"native_file\": \"{native_file}\",\n  \
             \"entry_symbol\": \"rh_entry\"{cc_line_field}\n}}\n"
        );
        std::fs::write(path, json).map_err(|err| RhError::Compile(err.to_string()))
    }

    pub fn read(path: &Path) -> Result<Self, RhError> {
        let bytes = std::fs::read(path).map_err(|err| RhError::Compile(err.to_string()))?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, RhError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|err| RhError::Compile(err.to_string()))?;
        Ok(Self {
            schema: required_string(&value, "schema")?,
            rh_version: required_string(&value, "rh_version")?,
            source_hash: required_string(&value, "source_hash")?,
            native_hash: required_string(&value, "native_hash")?,
            native_file: required_string(&value, "native_file")?,
            entry_symbol: required_string(&value, "entry_symbol")?,
            cc_line_count: value
                .get("cc_line_count")
                .and_then(serde_json::Value::as_u64)
                .and_then(|count| u32::try_from(count).ok()),
        })
    }

    pub fn verify_native(&self, native_path: &Path) -> Result<(), RhError> {
        verify_native_hash(native_path, &self.native_hash)
    }
}

fn required_string(value: &serde_json::Value, field: &str) -> Result<String, RhError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| RhError::Compile(format!("manifest missing `{field}`")))
}

pub fn manifest_path_for(native_path: &Path) -> std::path::PathBuf {
    native_path.with_extension("manifest.json")
}

pub fn default_pack_layout(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let native = dir.join(format!("pack.{}", crate::compile::native_extension()));
    let manifest = dir.join("manifest.json");
    (native, manifest)
}

#[cfg(test)]
mod tests {
    use super::RhPackManifest;

    #[test]
    fn round_trips_manifest_json() {
        let json = r#"{
  "schema": "agenterm.rh-pack-manifest/v1",
  "rh_version": "0.1.15",
  "source_hash": "abc",
  "native_hash": "def",
  "native_file": "pack.so",
  "entry_symbol": "rh_entry",
  "cc_line_count": 2
}"#;
        let manifest = RhPackManifest::parse(json.as_bytes()).expect("parse");
        assert_eq!(manifest.cc_line_count, Some(2));
        assert_eq!(manifest.native_file, "pack.so");
    }
}
