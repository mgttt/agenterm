//! Lua bytecode compilation: mlua Chunk::dump → bytes → SHA256 hash.

use sha2::Digest;

/// Compile Lua source to bytecode, returning (bytecode bytes, SHA256 hex hash).
pub fn compile_lua(source: &str) -> Result<(Vec<u8>, String), String> {
    let lua = mlua::Lua::new();
    let bytecode = lua
        .load(source)
        .into_function()
        .map_err(|e| format!("lua_compile_parse: {e}"))?
        .dump(true);
    let hash = sha2::Sha256::digest(&bytecode);
    let hash_hex = hex_encode(&hash);
    Ok((bytecode, hash_hex))
}

/// Hash a source string (not bytecode).
pub fn hash_source(source: &str) -> String {
    let hash = sha2::Sha256::digest(source.as_bytes());
    hex_encode(&hash)
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
    fn compile_returns_bytecode_and_hash() {
        let (bc, hash) = compile_lua("return 42").expect("compile");
        assert!(!bc.is_empty(), "bytecode must not be empty");
        assert_eq!(hash.len(), 64, "SHA256 hex must be 64 chars");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn compile_same_source_same_hash() {
        let (_, h1) = compile_lua("return 1").expect("compile");
        let (_, h2) = compile_lua("return 1").expect("compile");
        assert_eq!(h1, h2, "same source must produce same hash");
    }

    #[test]
    fn compile_different_source_different_hash() {
        let (_, h1) = compile_lua("return 1").expect("compile");
        let (_, h2) = compile_lua("return 2").expect("compile");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_source_deterministic() {
        let h1 = hash_source("hello");
        let h2 = hash_source("hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn compile_rejects_syntax_error() {
        let err = compile_lua("return !!").expect_err("syntax error");
        assert!(err.contains("lua_compile"), "{err}");
    }
}
