//! Lua bytecode compilation: mlua Chunk::dump → bytes → SHA256 hash.

use agenterm_script_common::hex::sha256_hex;

/// Compile Lua source to bytecode, returning (bytecode bytes, SHA256 hex hash).
pub fn compile_lua(source: &str) -> Result<(Vec<u8>, String), String> {
    let lua = mlua::Lua::new();
    let bytecode = lua
        .load(source)
        .into_function()
        .map_err(|e| format!("lua_compile_parse: {e}"))?
        .dump(true);
    let hash_hex = sha256_hex(&bytecode);
    Ok((bytecode, hash_hex))
}

/// Hash a source string (not bytecode).
pub use agenterm_script_common::pack_support::hash_source;

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
