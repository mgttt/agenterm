//! In-process rh pack loader for native hosts (Control Center, gateways).

use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct LoadedRhPack {
    pub native_hash: String,
    pub cc_lines: Vec<String>,
}

static RH_PACK: OnceLock<Option<LoadedRhPack>> = OnceLock::new();
static NATIVE_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Loading a pack must NOT run it.
///
/// This used to call `pack.entry_value()` here, which executed the script's
/// `entry()` with NO host callbacks registered -- `register_native_module` runs
/// later, on the run path. An unregistered pack reads `args.len` as -4 and drops
/// every `print` and `rh_fail`, so that first execution silently missed every
/// argument branch and then ran whatever the script does when no branch matches.
/// For most entries that is a swallowed argument check, which is why it went
/// unnoticed; for `fresh-clone-rehearsal` it is a full clone-and-build of the
/// repository, which is what hung the windows unit-tests gate for 900s+ and left
/// a 4.5 GB `target/qualification/fresh-clone-workspace` behind.
///
/// `entry_value` is now produced only by `run_rh_pack_entry`, which registers
/// the host first.
pub fn load_rh_pack(path: &Path) -> Result<LoadedRhPack, agenterm_rh::RhError> {
    let pack = agenterm_rh::RhPack::load(path)?;
    Ok(LoadedRhPack {
        native_hash: pack.manifest.native_hash.clone(),
        cc_lines: pack.cc_lines(),
    })
}

/// Run a pack's entry with the host callbacks registered. Only the explicit
/// pack probes need this; task and run flows go through
/// `script_rh_host::call_pack_entry_with_host_result`.
pub fn run_rh_pack_entry(path: &Path) -> Result<i64, agenterm_rh::RhError> {
    crate::script_rh_host::call_pack_entry_with_host_registration(path)
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
            "rh_pack": rh_pack_document(pack, None),
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

/// `entry_value` is `None` for callers that only inspect a pack. Producing it
/// requires RUNNING the entry, which must never be a side effect of looking.
pub fn rh_pack_document(pack: &LoadedRhPack, entry_value: Option<i64>) -> serde_json::Value {
    serde_json::json!({
        "backend": "rh",
        "native_hash": pack.native_hash,
        "entry_value": entry_value,
        "cc_lines": pack.cc_lines,
    })
}

pub fn print_rh_pack(path: &Path, json: bool) -> Result<(), agenterm_rh::RhError> {
    let pack = load_rh_pack(path)?;
    // Explicit, registered execution -- see `load_rh_pack`.
    let entry_value = run_rh_pack_entry(path)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rh_pack_document(&pack, Some(entry_value)))
                .map_err(|err| { agenterm_rh::RhError::Compile(err.to_string()) })?
        );
    } else {
        println!("rh pack loaded: {}", path.display());
        println!("native_hash={}", pack.native_hash);
        println!("entry={entry_value}");
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
        eprintln!("usage: agenterm cli rh-pack --path PATH [--json]");
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
        assert_eq!(loaded.cc_lines.len(), 2);
        // Loading exposes metadata only; running is explicit and registers the
        // host first, which is the whole point of splitting the two.
        assert_eq!(super::run_rh_pack_entry(&dir).expect("run"), 7);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
