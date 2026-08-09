//! In-process dynacore pack loader — mirrors `script_rh_pack.rs`'s shape
//! (process-wide cache, loaded/verified once, env-var triggered) for
//! dynacore's binary content-addressed packs.
//!
//! dynacore packs are NOT human-authored text source the way rh/lua/qjs/sql
//! sources are — they are produced by `agenterm_dynacore::pack::pack` and
//! addressed by content hash in a `agenterm_dynacore::store::Store`
//! (`store.rs`'s own header: "hash 由调用方给定，store 只做给 hash 取内容").
//! That is the same shape as rh's own *native* pack (`script_rh_pack.rs`'s
//! `AGENTERM_RH_PACK`-triggered `cached_rh_pack()`), not the same shape as
//! `ScriptEngineBackend::check(source)`/`execute(source)`'s human-text-source
//! contract — so this module follows `script_rh_pack.rs`'s pattern, not
//! `script_engine.rs`'s trait.
//!
//! Two env vars locate a pack, mirroring `AGENTERM_RH_PACK`'s single-var
//! convention split across what a `Store` needs (a directory) and what a
//! specific pack inside that store needs (its content hash, since `Store`
//! itself has no name->hash lookup — see `store.rs`'s header):
//!   - `AGENTERM_DYNACORE_PACK_STORE`: a `agenterm_dynacore::store::Store`
//!     root directory (same directory shape `Store::open` expects — a
//!     `store/` subdirectory of content-hashed `.bin` blobs).
//!   - `AGENTERM_DYNACORE_PACK_HASH`: the content hash (as produced by
//!     `agenterm_dynacore::pack::build_manifest`/`pack`) of the pack to load
//!     from that store.

use std::path::Path;
use std::sync::OnceLock;

use agenterm_dynacore::ir::Module;
use agenterm_dynacore::pack::{PACK_SCHEMA_VERSION, PackManifest};
use agenterm_dynacore::store::Store;

/// A loaded, load-time-verified dynacore pack, cached for the lifetime of
/// this process. Holds the owned `Module` (not a `VerifiedModule<'_>` —
/// that type borrows the `Module` it verifies, so it cannot itself be
/// cached in a `'static` slot; callers re-verify against the cached
/// `module` on each use, mirroring `verify::verify`'s own doc: "produce-time,
/// no execution, one pass" — cheap enough to repeat per invocation, unlike
/// re-loading/re-parsing pack bytes from disk).
#[derive(Clone, Debug)]
pub struct LoadedDynacorePack {
    pub manifest: PackManifest,
    pub module: Module,
}

static DYNACORE_PACK: OnceLock<Option<LoadedDynacorePack>> = OnceLock::new();

/// Open `store_dir` as a dynacore `Store`, fetch `hash` from it, deserialize,
/// and verify it against the real `OPERATION_CATALOG` (via
/// `script_dynacore_host::load_and_verify_pack` — the real host binding's
/// load+verify step, not a hand-rolled shortcut around it) before returning.
/// `operation_ids` in the constructed manifest is left empty: `pack::load`
/// only ever reads `manifest.hash` (see `pack.rs`'s `load` doc) — that field
/// exists for a build-time producer's own audit bookkeeping, not for
/// loading, so a hash-only caller (this function's whole contract) has
/// nothing to put there.
pub fn load_dynacore_pack(store_dir: &Path, hash: &str) -> Result<LoadedDynacorePack, String> {
    let store = Store::open(store_dir).map_err(|error| {
        format!(
            "failed to open dynacore pack store {}: {error}",
            store_dir.display()
        )
    })?;
    let manifest = PackManifest {
        schema_version: PACK_SCHEMA_VERSION,
        hash: hash.to_owned(),
        operation_ids: Vec::new(),
    };
    let module = crate::script_dynacore_host::load_and_verify_pack(&store, &manifest)?;
    Ok(LoadedDynacorePack { manifest, module })
}

fn try_load_dynacore_pack_from_env() -> Option<LoadedDynacorePack> {
    let store_dir = std::env::var("AGENTERM_DYNACORE_PACK_STORE").ok()?;
    let store_dir = store_dir.trim();
    if store_dir.is_empty() {
        return None;
    }
    let hash = std::env::var("AGENTERM_DYNACORE_PACK_HASH").ok()?;
    let hash = hash.trim();
    if hash.is_empty() {
        return None;
    }
    load_dynacore_pack(Path::new(store_dir), hash).ok()
}

/// Process-wide cached, load-and-verified pack from
/// `AGENTERM_DYNACORE_PACK_STORE`/`AGENTERM_DYNACORE_PACK_HASH` (loaded and
/// verified once per process — mirrors `script_rh_pack::cached_rh_pack()`).
/// Returns `None` when the env vars are unset/empty, or when loading/
/// verification fails (a misconfigured or corrupt pack is treated the same
/// as "no pack configured": the caller falls through to the rh/lua/qjs/sql
/// engines rather than hard-failing every invocation in the process).
pub fn cached_dynacore_pack() -> Option<&'static LoadedDynacorePack> {
    DYNACORE_PACK
        .get_or_init(try_load_dynacore_pack_from_env)
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_tabs_list_pack_bytes() -> agenterm_dynacore::ir::Module {
        use agenterm_dynacore::ir::{Builder, Term};
        let mut builder = Builder::new();
        let value = builder.fleet_call("tabs.list", "{}");
        builder.term(Term::Exit(value));
        builder.finish("test_tabs_list_pack", 0)
    }

    #[test]
    fn load_dynacore_pack_round_trips_from_a_real_store() {
        let dir = std::env::temp_dir().join(format!(
            "agenterm-script-dynacore-pack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).expect("open store");
        let module = build_tabs_list_pack_bytes();
        let manifest =
            agenterm_dynacore::pack::pack(&store, &module).expect("pack module into store");

        let loaded =
            load_dynacore_pack(&dir, &manifest.hash).expect("load+verify a real stored pack");
        assert_eq!(loaded.manifest.hash, manifest.hash);
        assert_eq!(loaded.module.name, "test_tabs_list_pack");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_dynacore_pack_rejects_a_hash_not_in_the_store() {
        let dir = std::env::temp_dir().join(format!(
            "agenterm-script-dynacore-pack-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Store::open(&dir).expect("open empty store");

        let error = load_dynacore_pack(&dir, "0000000000000000")
            .expect_err("a hash absent from the store must not load");
        assert!(error.contains("not found in store"), "{error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cached_dynacore_pack_is_none_without_env() {
        // Only asserts the "unset" branch of try_load_dynacore_pack_from_env
        // directly (not through the OnceLock-backed cached_dynacore_pack(),
        // which — same caveat as script_rh_pack's own cached_rh_pack() — is
        // permanently pinned by whichever env state was present on this
        // process's first call, and cargo test's single shared test binary
        // means some other test in this process may have already touched
        // it first).
        let prior_store = std::env::var("AGENTERM_DYNACORE_PACK_STORE").ok();
        let prior_hash = std::env::var("AGENTERM_DYNACORE_PACK_HASH").ok();
        unsafe {
            std::env::remove_var("AGENTERM_DYNACORE_PACK_STORE");
            std::env::remove_var("AGENTERM_DYNACORE_PACK_HASH");
        }
        assert!(try_load_dynacore_pack_from_env().is_none());
        unsafe {
            if let Some(value) = prior_store {
                std::env::set_var("AGENTERM_DYNACORE_PACK_STORE", value);
            }
            if let Some(value) = prior_hash {
                std::env::set_var("AGENTERM_DYNACORE_PACK_HASH", value);
            }
        }
    }
}
