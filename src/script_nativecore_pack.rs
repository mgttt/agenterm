//! In-process nativecore pack loader — mirrors `script_dynacore_pack.rs`'s
//! shape (process-wide cache, loaded/verified once, env-var triggered) for
//! `agenterm-nativecore`'s binary content-addressed packs.
//!
//! This is the THINNER of the two: `agenterm_nativecore::verify::verify`
//! takes no catalog parameter (native `Intent` contracts are fixed at
//! compile time, not looked up against a runtime `OPERATION_CATALOG` — see
//! `plan/design-dynacore-native-core.md` §4) and
//! `agenterm_nativecore::eval_core::run` takes no fleet bridge (native
//! intents call `seam.rs::do_intent` directly onto real Win32 APIs). So,
//! unlike `script_dynacore_host.rs`, there is no separate host-binding
//! module here: load+verify is done inline below, and run is called
//! directly from `script_backend::try_execute_nativecore_pack_invocation`.
//!
//! Two env vars locate a pack, mirroring `AGENTERM_DYNACORE_PACK_STORE`/
//! `AGENTERM_DYNACORE_PACK_HASH`'s split across what a `Store` needs (a
//! directory) and what a specific pack inside that store needs (its content
//! hash, since `Store` itself has no name->hash lookup):
//!   - `AGENTERM_NATIVECORE_PACK_STORE`: an `agenterm_nativecore::store::Store`
//!     root directory (the directory shape `Store::open` expects).
//!   - `AGENTERM_NATIVECORE_PACK_HASH`: the content hash (as produced by
//!     `agenterm_nativecore::pack::build_manifest`/`pack`) of the pack to
//!     load from that store.

use std::path::Path;
use std::sync::OnceLock;

use agenterm_nativecore::ir::Module;
use agenterm_nativecore::pack::{self, PACK_SCHEMA_VERSION, PackManifest};
use agenterm_nativecore::store::Store;
use agenterm_nativecore::verify;

/// A loaded, load-time-verified nativecore pack, cached for the lifetime of
/// this process. Holds the owned `Module` (not a `VerifiedModule<'_>` — that
/// type borrows the `Module` it verifies, so it cannot itself be cached in a
/// `'static` slot; callers re-verify against the cached `module` on each use,
/// mirroring `verify::verify`'s own doc: "produce-time, no execution, one
/// pass" — cheap enough to repeat per invocation, unlike re-loading/
/// re-parsing pack bytes from disk).
#[derive(Clone, Debug)]
pub struct LoadedNativecorePack {
    pub manifest: PackManifest,
    pub module: Module,
}

static NATIVECORE_PACK: OnceLock<Option<LoadedNativecorePack>> = OnceLock::new();

/// Open `store_dir` as a nativecore `Store`, fetch `hash` from it,
/// deserialize, and verify it (load-time gate, proves well-formedness once —
/// `try_execute_nativecore_pack_invocation` re-verifies per invocation, see
/// that function's doc) before returning.
pub fn load_nativecore_pack(store_dir: &Path, hash: &str) -> Result<LoadedNativecorePack, String> {
    let store = Store::open(store_dir).map_err(|error| {
        format!(
            "failed to open nativecore pack store {}: {error}",
            store_dir.display()
        )
    })?;
    let manifest = PackManifest {
        schema_version: PACK_SCHEMA_VERSION,
        hash: hash.to_owned(),
    };
    let module = pack::load(&store, &manifest)?;
    verify::verify(&module)
        .map_err(|fault| format!("{manifest:?}: rejected by verify() — {fault:?}"))?;
    Ok(LoadedNativecorePack { manifest, module })
}

fn try_load_nativecore_pack_from_env() -> Option<LoadedNativecorePack> {
    let store_dir = std::env::var("AGENTERM_NATIVECORE_PACK_STORE").ok()?;
    let store_dir = store_dir.trim();
    if store_dir.is_empty() {
        return None;
    }
    let hash = std::env::var("AGENTERM_NATIVECORE_PACK_HASH").ok()?;
    let hash = hash.trim();
    if hash.is_empty() {
        return None;
    }
    load_nativecore_pack(Path::new(store_dir), hash).ok()
}

/// Process-wide cached, load-and-verified pack from
/// `AGENTERM_NATIVECORE_PACK_STORE`/`AGENTERM_NATIVECORE_PACK_HASH` (loaded
/// and verified once per process — mirrors
/// `script_dynacore_pack::cached_dynacore_pack()`). Returns `None` when the
/// env vars are unset/empty, or when loading/verification fails (a
/// misconfigured or corrupt pack is treated the same as "no pack
/// configured": the caller falls through to the dynacore/rh/lua/qjs/sql
/// engines rather than hard-failing every invocation in the process).
pub fn cached_nativecore_pack() -> Option<&'static LoadedNativecorePack> {
    NATIVECORE_PACK
        .get_or_init(try_load_nativecore_pack_from_env)
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_spawn_echo_pack() -> Module {
        agenterm_nativecore::payloads::spawn_echo()
    }

    #[test]
    fn load_nativecore_pack_round_trips_from_a_real_store() {
        let dir = std::env::temp_dir().join(format!(
            "agenterm-script-nativecore-pack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).expect("open store");
        let module = build_spawn_echo_pack();
        let manifest = pack::pack(&store, &module).expect("pack module into store");

        let loaded =
            load_nativecore_pack(&dir, &manifest.hash).expect("load+verify a real stored pack");
        assert_eq!(loaded.manifest.hash, manifest.hash);
        assert_eq!(loaded.module.name, module.name);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_nativecore_pack_rejects_a_hash_not_in_the_store() {
        let dir = std::env::temp_dir().join(format!(
            "agenterm-script-nativecore-pack-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Store::open(&dir).expect("open empty store");

        let error = load_nativecore_pack(&dir, "0000000000000000")
            .expect_err("a hash absent from the store must not load");
        assert!(error.contains("not found in store"), "{error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_nativecore_pack_rejects_a_native_call_contract_violation() {
        // The F1-class fault: an extern self-consistently declared AND
        // called with the wrong arity for its Intent's real contract (here,
        // `spawnwait_arity_mismatch_demo` — see
        // `agenterm-nativecore/tests/native_intents.rs`'s own
        // `f1_fix_spawnwait_arg_count_mismatch_is_rejected_before_execution`
        // for the crate-internal proof this is rejected by `verify()`; this
        // test proves the PRODUCT loader path rejects it too, before ever
        // caching it as runnable).
        let dir = std::env::temp_dir().join(format!(
            "agenterm-script-nativecore-pack-badcontract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).expect("open store");
        let module = agenterm_nativecore::payloads::spawnwait_arity_mismatch_demo();
        let manifest = pack::pack(&store, &module).expect("pack module into store (packing is not verification)");

        let error = load_nativecore_pack(&dir, &manifest.hash)
            .expect_err("a native-call-contract violation must not load as runnable");
        assert!(error.contains("IntentArityMismatch"), "{error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cached_nativecore_pack_is_none_without_env() {
        // Only asserts the "unset" branch of try_load_nativecore_pack_from_env
        // directly (not through the OnceLock-backed cached_nativecore_pack(),
        // which — same caveat as script_dynacore_pack's own
        // cached_dynacore_pack() — is permanently pinned by whichever env
        // state was present on this process's first call, and cargo test's
        // single shared test binary means some other test in this process
        // may have already touched it first).
        let prior_store = std::env::var("AGENTERM_NATIVECORE_PACK_STORE").ok();
        let prior_hash = std::env::var("AGENTERM_NATIVECORE_PACK_HASH").ok();
        unsafe {
            std::env::remove_var("AGENTERM_NATIVECORE_PACK_STORE");
            std::env::remove_var("AGENTERM_NATIVECORE_PACK_HASH");
        }
        assert!(try_load_nativecore_pack_from_env().is_none());
        unsafe {
            if let Some(value) = prior_store {
                std::env::set_var("AGENTERM_NATIVECORE_PACK_STORE", value);
            }
            if let Some(value) = prior_hash {
                std::env::set_var("AGENTERM_NATIVECORE_PACK_HASH", value);
            }
        }
    }
}
