//! Black-box proof for `plan/design-dynacore-native-core.md` §7's second
//! acceptance scenario: a deliberately constructed nativecore pack whose
//! native-call contract is wrong (an `Intent::SpawnWait` extern declared
//! AND called with 1 arg — self-consistent by the IR's own original arity
//! check, but disagreeing with `SpawnWait`'s REAL Win32-call contract of 0
//! args; the exact F1-class fault
//! `crates/agenterm-nativecore/tests/native_intents.rs`'s own
//! `f1_fix_spawnwait_arg_count_mismatch_is_rejected_before_execution` proves
//! the CRATE'S `verify()` rejects) is rejected by the PRODUCT loader path
//! (`agenterm::script_nativecore_pack::cached_nativecore_pack`) before it
//! ever becomes runnable, and the product invocation entry point
//! (`agenterm::script_backend::try_execute_nativecore_pack_invocation`) does
//! not panic when pointed at it — it falls through cleanly instead.
//!
//! Single `#[test]`, same reasoning as the sibling
//! `nativecore_pack_spawn_echo_live.rs`'s header (process-wide `OnceLock`
//! pinning).

use agenterm::script_backend::try_execute_nativecore_pack_invocation;
use agenterm::script_nativecore_pack::cached_nativecore_pack;
use agenterm::script_protocol::ScriptOperation;
use agenterm_nativecore::{pack, payloads, store::Store};

#[test]
fn bad_contract_pack_is_rejected_before_execution_and_the_product_path_does_not_panic() {
    let dir = std::env::temp_dir().join(format!(
        "agenterm-nativecore-pack-live-bad-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).expect("open a real content-addressed store on disk");
    let module = payloads::spawnwait_arity_mismatch_demo();
    let manifest = pack::pack(&store, &module)
        .expect("packing a malformed IR must still succeed -- packing is not verification");

    unsafe {
        std::env::set_var("AGENTERM_NATIVECORE_PACK_STORE", &dir);
        std::env::set_var("AGENTERM_NATIVECORE_PACK_HASH", &manifest.hash);
    }

    // The product loader rejects it at LOAD time (verify() runs inside
    // `load_nativecore_pack`, called from `cached_nativecore_pack`'s
    // `OnceLock` initializer) and treats that the same as "no pack
    // configured" (mirrors `script_dynacore_pack`'s identical stance,
    // documented on `cached_nativecore_pack`'s own doc) -- so the process
    // never caches an unverified module as runnable, and never panics doing
    // so.
    let cached = std::panic::catch_unwind(cached_nativecore_pack);
    assert!(
        cached.is_ok(),
        "the product loader must not panic on a malformed native-call-contract pack"
    );
    assert!(
        cached.unwrap().is_none(),
        "a pack that fails verify() must be treated as unconfigured, not silently cached as runnable"
    );

    // The invocation entry point itself must also not panic when pointed at
    // this store+hash, and must fall through cleanly (Ok(None)) rather than
    // ever reaching `eval_core::run` on an unverified module.
    let invocation =
        std::panic::catch_unwind(|| try_execute_nativecore_pack_invocation(ScriptOperation::Run));
    assert!(
        invocation.is_ok(),
        "try_execute_nativecore_pack_invocation must not panic on a malformed pack"
    );
    let result = invocation
        .unwrap()
        .expect("a rejected-at-load pack must not surface as an Err here -- it was never cached as configured");
    assert!(
        result.is_none(),
        "a rejected pack must fall through as Ok(None), never execute"
    );

    unsafe {
        std::env::remove_var("AGENTERM_NATIVECORE_PACK_STORE");
        std::env::remove_var("AGENTERM_NATIVECORE_PACK_HASH");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
