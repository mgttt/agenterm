//! qjs pack "compile" step: capture real QuickJS bytecode for a
//! reproducibility fingerprint, aligned in *intent* with
//! `agenterm_lua::compile::compile_lua` (real bytecode + SHA256 hash) but
//! different in what the bytecode is used for afterwards — see `pack.rs`'s
//! module doc for why execution still re-parses from stored source.
//!
//! This reuses exactly the parse path `check.rs` already uses and tests
//! (`Module::declare`, parse/resolve without executing): same acceptance
//! behavior as `check()` for the same source, by construction — not a
//! second, independently-drifting parser. `Module::write` is rquickjs
//! 0.12's only public bytecode-serialization surface and only exists on
//! `Module<'js, _>` (module compilation), which is why `check()` already
//! parses as a module rather than a global script even though `eval_entry`
//! (see `eval.rs`) evaluates as a global script — that split predates this
//! file (see `check.rs`'s module doc "known gap" note) and this module does
//! not change it, only reuses it for a second purpose (hashing).

use rquickjs::{CatchResultExt, Context, Module, Runtime, WriteOptions};
use sha2::Digest;

use crate::error::QjsError;

/// Parse `source` as a module (same as `check::check`) and serialize its
/// compiled bytecode, returning `(bytecode bytes, SHA256 hex hash)`. Fails
/// with `QjsError::Parse` exactly when `check(source, label)` would.
pub fn compile_qjs(source: &str, label: &str) -> Result<(Vec<u8>, String), QjsError> {
    let runtime = Runtime::new().map_err(|err| QjsError::Check(err.to_string()))?;
    let context = Context::full(&runtime).map_err(|err| QjsError::Check(err.to_string()))?;
    let bytecode = context.with(|ctx| {
        let module = Module::declare(ctx.clone(), label, source)
            .catch(&ctx)
            .map_err(|err| QjsError::Parse(err.to_string()))?;
        module
            .write(WriteOptions::default())
            .map_err(|err| QjsError::Check(err.to_string()))
    })?;
    let hash = hash_bytes(&bytecode);
    Ok((bytecode, hash))
}

/// Hash a source string (not bytecode) — mirrors
/// `agenterm_lua::compile::hash_source`.
pub fn hash_source(source: &str) -> String {
    hash_bytes(source.as_bytes())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    hex_encode(&digest)
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
        let (bytecode, hash) = compile_qjs("export default 40 + 2;", "ok.js").expect("compile");
        assert!(!bytecode.is_empty(), "bytecode must not be empty");
        assert_eq!(hash.len(), 64, "SHA256 hex must be 64 chars");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn compile_same_source_same_hash() {
        let (_, h1) = compile_qjs("export default 1;", "a.js").expect("compile");
        let (_, h2) = compile_qjs("export default 1;", "a.js").expect("compile");
        assert_eq!(h1, h2, "same source must produce same hash");
    }

    #[test]
    fn compile_different_source_different_hash() {
        let (_, h1) = compile_qjs("export default 1;", "a.js").expect("compile");
        let (_, h2) = compile_qjs("export default 2;", "a.js").expect("compile");
        assert_ne!(h1, h2);
    }

    #[test]
    fn compile_accepts_plain_entry_scripts() {
        // Pack sources in practice are plain `function entry() {...}`
        // scripts (no `export`), same shape `eval_entry` accepts — modules
        // accept ordinary statements too as long as they're strict-mode
        // clean, which these are.
        let (_, hash) =
            compile_qjs("function entry() { return 42; }", "entry.js").expect("compile");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn compile_rejects_syntax_errors() {
        let err = compile_qjs("this is not valid js (((", "bad.js").expect_err("syntax error");
        assert!(matches!(err, QjsError::Parse(_)));
    }

    #[test]
    fn hash_source_deterministic() {
        let h1 = hash_source("hello");
        let h2 = hash_source("hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }
}
