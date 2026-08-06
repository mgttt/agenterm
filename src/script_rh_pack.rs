//! In-process rh pack loader for native hosts (Control Center, gateways).

use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct LoadedRhPack {
    pub native_hash: String,
    pub entry_value: i64,
    pub cc_lines: Vec<String>,
}

static RH_PACK: OnceLock<Option<LoadedRhPack>> = OnceLock::new();

pub fn load_rh_pack(path: &Path) -> Result<LoadedRhPack, agenterm_rh::RhError> {
    let pack = agenterm_rh::RhPack::load(path)?;
    Ok(LoadedRhPack {
        native_hash: pack.manifest.native_hash.clone(),
        entry_value: pack.entry_value(),
        cc_lines: pack.cc_lines(),
    })
}

pub fn try_load_rh_pack_from_env() -> Option<LoadedRhPack> {
    let path = std::env::var("AGENTERM_RH_PACK").ok()?;
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    load_rh_pack(Path::new(path)).ok()
}

/// Process-wide cached pack from `AGENTERM_RH_PACK` (dlopen once per process).
pub fn cached_rh_pack() -> Option<&'static LoadedRhPack> {
    RH_PACK.get_or_init(try_load_rh_pack_from_env).as_ref()
}

pub fn rh_pack_observability() -> serde_json::Value {
    match cached_rh_pack() {
        Some(pack) => serde_json::json!({
            "script_backend": crate::script_backend::ScriptBackend::from_env().as_str(),
            "rh_pack": rh_pack_document(pack),
        }),
        None => serde_json::json!({
            "script_backend": crate::script_backend::ScriptBackend::from_env().as_str(),
            "rh_pack": serde_json::Value::Null,
        }),
    }
}

pub fn append_cc_lines(mut lines: Vec<String>, pack: &LoadedRhPack) -> Vec<String> {
    if pack.cc_lines.is_empty() {
        return lines;
    }
    lines.push("── rh pack ──".to_owned());
    lines.extend(pack.cc_lines.iter().cloned());
    lines
}

pub fn rh_pack_document(pack: &LoadedRhPack) -> serde_json::Value {
    serde_json::json!({
        "backend": "rh",
        "native_hash": pack.native_hash,
        "entry_value": pack.entry_value,
        "cc_lines": pack.cc_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::{append_cc_lines, load_rh_pack, LoadedRhPack};

    #[test]
    fn rh_pack_observability_without_env() {
        let value = super::rh_pack_observability();
        assert_eq!(value["script_backend"], "rhai");
        assert!(value["rh_pack"].is_null());
    }

    #[test]
    fn append_cc_lines_inserts_banner() {
        let lines = append_cc_lines(
            vec!["cockpit".to_owned()],
            &LoadedRhPack {
                native_hash: "abc".to_owned(),
                entry_value: 1,
                cc_lines: vec!["pack line".to_owned()],
            },
        );
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1], "── rh pack ──");
    }

    #[test]
    fn load_pack_dir_round_trips_cc_lines() {
        let dir = std::env::temp_dir().join(format!(
            "agenterm-script-rh-pack-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        agenterm_rh::build_pack_dir(
            "fn entry() { 7 }\nfn cc_lines() { [\"from native\", \"machine code\"] }",
            &dir,
        )
        .expect("build");
        let loaded = load_rh_pack(&dir).expect("load");
        assert_eq!(loaded.entry_value, 7);
        assert_eq!(loaded.cc_lines.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
