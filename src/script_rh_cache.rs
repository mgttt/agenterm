//! Compile rh source to a cached native pack directory keyed by source hash.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use agenterm_rh::{
    RH_CODEGEN_REVISION, RH_HOST_API_VERSION, RhError, RhPack, bundle_project_source,
};

static SOURCE_CACHE: Mutex<Option<(String, PathBuf)>> = Mutex::new(None);
static SOURCE_BUILD_LOCK: Mutex<()> = Mutex::new(());
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn cache_key(source: &str) -> String {
    format!(
        "{}-api{}-cg{}",
        agenterm_rh::hash_bytes(source.as_bytes()),
        RH_HOST_API_VERSION,
        RH_CODEGEN_REVISION,
    )
}

fn effective_source(source: &str, project_root: Option<&Path>) -> Result<String, RhError> {
    let Some(root) = project_root else {
        return Ok(source.to_owned());
    };
    if !source.contains("import ") {
        return Ok(source.to_owned());
    }
    bundle_project_source(root, source)
}

pub fn compile_source_to_cache(source: &str) -> Result<PathBuf, RhError> {
    compile_source_to_cache_with_project(source, None)
}

pub fn compile_source_to_cache_with_project(
    source: &str,
    project_root: Option<&Path>,
) -> Result<PathBuf, RhError> {
    let bundled = effective_source(source, project_root)?;
    let key = cache_key(&bundled);
    if let Some((cached_key, path)) = SOURCE_CACHE.lock().expect("lock").clone()
        && cached_key == key
        && path.is_dir()
    {
        return Ok(path);
    }

    let _build_guard = SOURCE_BUILD_LOCK.lock().expect("lock");
    if let Some((cached_key, path)) = SOURCE_CACHE.lock().expect("lock").clone()
        && cached_key == key
        && path.is_dir()
    {
        return Ok(path);
    }

    match build_or_load_immutable_pack(&bundled, &key) {
        Ok(dir) => {
            *SOURCE_CACHE.lock().expect("lock") = Some((key, dir.clone()));
            Ok(dir)
        }
        Err(_error)
            if bundled != source && project_root.is_some() && source.contains("import ") =>
        {
            let fallback_key = cache_key(source);
            let fallback_dir = build_or_load_immutable_pack(source, &fallback_key)?;
            *SOURCE_CACHE.lock().expect("lock") = Some((fallback_key, fallback_dir.clone()));
            Ok(fallback_dir)
        }
        Err(error) => Err(error),
    }
}

fn build_or_load_immutable_pack(source: &str, key: &str) -> Result<PathBuf, RhError> {
    let dir = std::env::temp_dir().join(format!("agenterm-rh-src-{key}"));
    if dir.is_dir() && RhPack::load(&dir).is_ok() {
        return Ok(dir);
    }
    if dir.exists() {
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(|error| RhError::Compile(error.to_string()))?;
        } else {
            std::fs::remove_file(&dir).map_err(|error| RhError::Compile(error.to_string()))?;
        }
    }

    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let staging = std::env::temp_dir().join(format!(
        "agenterm-rh-src-{key}.staging-{}-{sequence}",
        std::process::id()
    ));
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    if let Err(error) = agenterm_rh::build_pack_dir(source, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }

    match std::fs::rename(&staging, &dir) {
        Ok(()) => {}
        Err(_error) if dir.is_dir() && RhPack::load(&dir).is_ok() => {
            // Another process won the same immutable source-key publication.
            // Its complete pack is byte-equivalent; discard only our staging.
            let _ = std::fs::remove_dir_all(&staging);
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(RhError::Compile(format!(
                "publishing rh source cache {}: {error}",
                dir.display()
            )));
        }
    }

    RhPack::load(&dir)?;
    Ok(dir)
}

pub fn native_path_for_source(source: &str) -> Result<PathBuf, RhError> {
    native_path_for_source_with_project(source, None)
}

pub fn native_path_for_source_with_project(
    source: &str,
    project_root: Option<&Path>,
) -> Result<PathBuf, RhError> {
    let dir = compile_source_to_cache_with_project(source, project_root)?;
    let pack = RhPack::load(&dir)?;
    Ok(dir.join(pack.manifest.native_file))
}

pub fn loaded_pack_for_source(
    source: &str,
) -> Result<crate::script_rh_pack::LoadedRhPack, RhError> {
    loaded_pack_for_source_with_project(source, None)
}

pub fn loaded_pack_for_source_with_project(
    source: &str,
    project_root: Option<&Path>,
) -> Result<crate::script_rh_pack::LoadedRhPack, RhError> {
    let dir = compile_source_to_cache_with_project(source, project_root)?;
    crate::script_rh_pack::load_rh_pack(&dir)
}

#[cfg(test)]
mod tests {
    use super::{
        RH_CODEGEN_REVISION, RH_HOST_API_VERSION, cache_key, compile_source_to_cache,
        native_path_for_source,
    };

    #[test]
    fn source_cache_key_owns_abi_and_codegen_compatibility() {
        let key = cache_key("fn entry() { 1 }");
        // Follow the live constants: a hardcoded pin here went stale on
        // every ABI/codegen bump without adding safety — the point is that
        // BOTH constants appear in the key, not their specific values.
        assert!(
            key.ends_with(&format!(
                "-api{RH_HOST_API_VERSION}-cg{RH_CODEGEN_REVISION}"
            )),
            "{key}"
        );
        assert!(!key.contains(':'));
    }

    #[test]
    fn source_cache_is_stable_for_same_source() {
        let source = "fn entry() { 99 }";
        let a = compile_source_to_cache(source).expect("a");
        let b = compile_source_to_cache(source).expect("b");
        assert_eq!(a, b);
        let native = native_path_for_source(source).expect("native");
        assert!(native.is_file());
    }
}
