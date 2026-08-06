//! Compile rh source to a cached native pack directory keyed by source hash.

use std::path::PathBuf;
use std::sync::Mutex;

use agenterm_rh::{RhError, RhPack, RH_HOST_API_VERSION};

static SOURCE_CACHE: Mutex<Option<(String, PathBuf)>> = Mutex::new(None);

fn cache_key(source: &str) -> String {
    format!(
        "{}:{}",
        agenterm_rh::hash_bytes(source.as_bytes()),
        RH_HOST_API_VERSION
    )
}

pub fn compile_source_to_cache(source: &str) -> Result<PathBuf, RhError> {
    let key = cache_key(source);
    if let Some((cached_key, path)) = SOURCE_CACHE.lock().expect("lock").clone()
        && cached_key == key
        && path.is_dir()
    {
        return Ok(path);
    }

    let dir = std::env::temp_dir().join(format!("agenterm-rh-src-{key}"));
    if dir.is_dir() {
        if RhPack::load(&dir).is_ok() {
            *SOURCE_CACHE.lock().expect("lock") = Some((key.clone(), dir.clone()));
            return Ok(dir);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    agenterm_rh::build_pack_dir(source, &dir)?;
    *SOURCE_CACHE.lock().expect("lock") = Some((key, dir.clone()));
    Ok(dir)
}

pub fn native_path_for_source(source: &str) -> Result<PathBuf, RhError> {
    let dir = compile_source_to_cache(source)?;
    let pack = RhPack::load(&dir)?;
    Ok(dir.join(pack.manifest.native_file))
}

pub fn loaded_pack_for_source(
    source: &str,
) -> Result<crate::script_rh_pack::LoadedRhPack, RhError> {
    let dir = compile_source_to_cache(source)?;
    crate::script_rh_pack::load_rh_pack(&dir)
}

#[cfg(test)]
mod tests {
    use super::{compile_source_to_cache, native_path_for_source};

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
