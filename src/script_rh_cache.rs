//! Compile rh source to a cached native pack directory keyed by source hash.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agenterm_rh::{RH_CODEGEN_REVISION, RH_HOST_API_VERSION, RhError, RhPack, bundle_project_source};

static SOURCE_CACHE: Mutex<Option<(String, PathBuf)>> = Mutex::new(None);

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
    let source = effective_source(source, project_root)?;
    let key = cache_key(&source);
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
    agenterm_rh::build_pack_dir(&source, &dir)?;
    *SOURCE_CACHE.lock().expect("lock") = Some((key, dir.clone()));
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
    use super::{cache_key, compile_source_to_cache, native_path_for_source};

    #[test]
    fn source_cache_key_owns_abi_and_codegen_compatibility() {
        let key = cache_key("fn entry() { 1 }");
        assert!(key.ends_with("-api9-cg16"), "{key}");
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
