//! Lua bytecode cache: source hash → cached bytecode file.

use std::path::PathBuf;

use sha2::Digest;

use crate::compile::{compile_lua, hash_source};

/// Cache directory for compiled Lua bytecode.
fn cache_dir() -> PathBuf {
    let base = dirs_fallback();
    base.join("AgenTerm").join("lua-cache")
}

fn dirs_fallback() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE")
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join("AppData").join("Local")
        })
}

/// Result of cached compilation.
#[derive(Debug)]
pub struct CachedCompileResult {
    /// The bytecode (either freshly compiled or from cache).
    pub bytecode: Vec<u8>,
    /// SHA256 hex hash of the bytecode.
    pub bytecode_hash: String,
    /// True if the bytecode was served from cache.
    pub cache_hit: bool,
}

/// Compile Lua source with caching: if source hash matches an existing cache entry,
/// return cached bytecode. Otherwise compile fresh and store in cache.
pub fn cached_compile(source: &str) -> Result<CachedCompileResult, String> {
    let source_hash = hash_source(source);
    let cache_dir = cache_dir();
    let cache_path = cache_dir.join(format!("{source_hash}.luac"));

    // Check cache
    if cache_path.exists() {
        let bytecode = std::fs::read(&cache_path)
            .map_err(|e| format!("cache_read: {e}"))?;
        let hash = sha2::Sha256::digest(&bytecode);
        let bytecode_hash = hex_encode(&hash);
        return Ok(CachedCompileResult {
            bytecode,
            bytecode_hash,
            cache_hit: true,
        });
    }

    // Compile fresh
    let (bytecode, bytecode_hash) = compile_lua(source)?;

    // Store in cache
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("cache_create_dir: {e}"))?;
    std::fs::write(&cache_path, &bytecode)
        .map_err(|e| format!("cache_write: {e}"))?;

    Ok(CachedCompileResult {
        bytecode,
        bytecode_hash,
        cache_hit: false,
    })
}

/// Clear the cache for a specific source hash.
pub fn clear_cache_for_source(source: &str) {
    let source_hash = hash_source(source);
    let cache_path = cache_dir().join(format!("{source_hash}.luac"));
    let _ = std::fs::remove_file(&cache_path);
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_compile_is_miss() {
        clear_cache_for_source("return 99");
        let result = cached_compile("return 99").expect("compile");
        assert!(!result.cache_hit, "first compile must be a miss");
        assert!(!result.bytecode.is_empty());
        assert_eq!(result.bytecode_hash.len(), 64);
    }

    #[test]
    fn second_compile_is_hit() {
        clear_cache_for_source("return 88");
        let r1 = cached_compile("return 88").expect("compile1");
        assert!(!r1.cache_hit);
        let r2 = cached_compile("return 88").expect("compile2");
        assert!(r2.cache_hit, "second compile must be a hit");
        assert_eq!(r1.bytecode_hash, r2.bytecode_hash);
    }

    #[test]
    fn different_sources_different_cache() {
        clear_cache_for_source("return 1");
        clear_cache_for_source("return 2");
        let r1 = cached_compile("return 1").expect("compile1");
        let r2 = cached_compile("return 2").expect("compile2");
        assert!(!r1.cache_hit, "fresh source 1");
        assert!(!r2.cache_hit, "fresh source 2");
        assert_ne!(r1.bytecode_hash, r2.bytecode_hash);
    }
}
