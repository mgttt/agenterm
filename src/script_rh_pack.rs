//! In-process rh pack loader for native hosts (Control Center, gateways).

use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct LoadedRhPack {
    pub native_hash: String,
    pub entry_value: i64,
    pub cc_lines: Vec<String>,
}

static RH_PACK: OnceLock<Option<LoadedRhPack>> = OnceLock::new();
static NATIVE_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

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

pub fn cached_native_path() -> Option<&'static Path> {
    NATIVE_PATH
        .get_or_init(|| {
            let path = std::env::var("AGENTERM_RH_PACK").ok()?;
            let path = path.trim();
            if path.is_empty() {
                return None;
            }
            let path = Path::new(path);
            if path.is_dir() {
                let manifest =
                    agenterm_rh::RhPackManifest::read(&path.join("manifest.json")).ok()?;
                Some(path.join(manifest.native_file))
            } else {
                Some(path.to_path_buf())
            }
        })
        .as_ref()
        .map(std::path::PathBuf::as_path)
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

pub fn print_rh_pack(path: &Path, json: bool) -> Result<(), agenterm_rh::RhError> {
    let pack = load_rh_pack(path)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rh_pack_document(&pack))
                .map_err(|err| { agenterm_rh::RhError::Compile(err.to_string()) })?
        );
    } else {
        println!("rh pack loaded: {}", path.display());
        println!("native_hash={}", pack.native_hash);
        println!("entry={}", pack.entry_value);
        for line in &pack.cc_lines {
            println!("cc: {line}");
        }
    }
    Ok(())
}

pub fn run_rh_pack_cli(args: &[String]) -> i32 {
    let mut json = false;
    let mut path = None;
    let mut position = 1;
    while position < args.len() {
        match args[position].as_str() {
            "--json" => json = true,
            "--path" => {
                position += 1;
                if position >= args.len() {
                    eprintln!("rh-pack: --path requires a value");
                    return 2;
                }
                path = Some(args[position].clone());
            }
            value if path.is_none() && !value.starts_with('-') => path = Some(value.to_owned()),
            unknown => {
                eprintln!("rh-pack: unknown argument `{unknown}`");
                return 2;
            }
        }
        position += 1;
    }
    let Some(path) = path else {
        eprintln!("usage: agenterm-cli rh-pack --path PATH [--json]");
        return 2;
    };
    match print_rh_pack(Path::new(path.as_str()), json) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LoadedRhPack, append_cc_lines, load_rh_pack};

    #[test]
    fn rh_pack_observability_without_env() {
        let value = super::rh_pack_observability();
        assert_eq!(value["script_backend"], "rh");
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
        let dir =
            std::env::temp_dir().join(format!("agenterm-script-rh-pack-{}", std::process::id()));
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
