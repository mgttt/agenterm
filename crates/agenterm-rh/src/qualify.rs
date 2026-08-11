use std::path::Path;

use serde_json::json;

use crate::{
    RH_VERSION, RhError,
    compile::hash_file,
    pack::{RhPack, build_pack_dir},
};

pub const QUALIFICATION_SCHEMA: &str = "agenterm.rh-qualification/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RhQualificationReceipt {
    pub schema: String,
    pub rh_version: String,
    pub target: String,
    pub source_hash: String,
    pub native_hash: String,
    pub native_file: String,
    pub entry_value: i64,
    pub api_version: u32,
    pub cc_line_count: usize,
}

pub fn qualify_pack_dir(source: &str, dir: &Path) -> Result<RhQualificationReceipt, RhError> {
    let build = build_pack_dir(source, dir)?;
    let pack = RhPack::load(dir)?;
    // Qualification runs the entry, and this crate has no host implementation,
    // so opt into the stub callbacks. Without this the prelude aborts on the
    // first `args.len`/`print` rather than fabricating a value for a callback
    // that was never registered -- see `register_stub_host`.
    pack.register_stub_host()?;
    let cc_lines = pack.cc_lines();
    Ok(RhQualificationReceipt {
        schema: QUALIFICATION_SCHEMA.to_owned(),
        rh_version: RH_VERSION.to_owned(),
        target: host_target_triple(),
        source_hash: build.compile.source_hash,
        native_hash: build.compile.native_hash.clone(),
        native_file: build
            .native_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("pack.so")
            .to_owned(),
        entry_value: pack.entry_value(),
        api_version: pack.api_version(),
        cc_line_count: cc_lines.len(),
    })
}

pub fn write_receipt(path: &Path, receipt: &RhQualificationReceipt) -> Result<(), RhError> {
    let value = json!({
        "schema": receipt.schema,
        "rh_version": receipt.rh_version,
        "target": receipt.target,
        "source_hash": receipt.source_hash,
        "native_hash": receipt.native_hash,
        "native_file": receipt.native_file,
        "entry_value": receipt.entry_value,
        "api_version": receipt.api_version,
        "cc_line_count": receipt.cc_line_count,
    });
    let json =
        serde_json::to_string_pretty(&value).map_err(|err| RhError::Compile(err.to_string()))?;
    std::fs::write(path, json).map_err(|err| RhError::Compile(err.to_string()))
}

pub fn verify_receipt_native(
    receipt: &RhQualificationReceipt,
    native_path: &Path,
) -> Result<(), RhError> {
    let actual = hash_file(native_path)?;
    if actual == receipt.native_hash {
        Ok(())
    } else {
        Err(RhError::Compile(format!(
            "qualification native_hash mismatch: expected {}, got {actual}",
            receipt.native_hash
        )))
    }
}

pub fn host_target_triple() -> String {
    std::env::var("AGENTERM_RH_QUALIFY_TARGET").unwrap_or_else(|_| {
        if cfg!(target_os = "windows") {
            if cfg!(target_arch = "aarch64") {
                "aarch64-pc-windows-msvc".to_owned()
            } else {
                "x86_64-pc-windows-msvc".to_owned()
            }
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "aarch64-apple-darwin".to_owned()
            } else {
                "x86_64-apple-darwin".to_owned()
            }
        } else if cfg!(target_arch = "aarch64") {
            "aarch64-unknown-linux-gnu".to_owned()
        } else {
            "x86_64-unknown-linux-gnu".to_owned()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{QUALIFICATION_SCHEMA, RhQualificationReceipt, qualify_pack_dir, write_receipt};

    #[test]
    fn qualification_receipt_round_trips() {
        let dir = std::env::temp_dir().join(format!("agenterm-rh-qualify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let receipt =
            qualify_pack_dir("fn entry() { 99 }\nfn cc_lines() { [\"qualified\"] }", &dir)
                .expect("qualify");
        assert_eq!(receipt.schema, QUALIFICATION_SCHEMA);
        assert_eq!(receipt.entry_value, 99);
        assert_eq!(receipt.cc_line_count, 1);
        let receipt_path = dir.join("qualification.json");
        write_receipt(&receipt_path, &receipt).expect("write");
        let parsed: RhQualificationReceipt = {
            let bytes = std::fs::read(&receipt_path).expect("read");
            let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
            RhQualificationReceipt {
                schema: value["schema"].as_str().unwrap().to_owned(),
                rh_version: value["rh_version"].as_str().unwrap().to_owned(),
                target: value["target"].as_str().unwrap().to_owned(),
                source_hash: value["source_hash"].as_str().unwrap().to_owned(),
                native_hash: value["native_hash"].as_str().unwrap().to_owned(),
                native_file: value["native_file"].as_str().unwrap().to_owned(),
                entry_value: value["entry_value"].as_i64().unwrap(),
                api_version: value["api_version"].as_u64().unwrap() as u32,
                cc_line_count: value["cc_line_count"].as_u64().unwrap() as usize,
            }
        };
        assert_eq!(parsed.native_hash, receipt.native_hash);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
