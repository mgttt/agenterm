//! Determinism and content-addressed-store concurrency, matching the rigor
//! of `agenterm-nativecore`'s
//! `verification_and_execution_are_deterministic_across_repeated_runs` and
//! `infinite_loop_pack_join_with_explicit_timeout_never_blocks_forever`.
//! Every test here drives the real pipeline (build `Module` -> `pack::pack`
//! into a real `store::Store` on disk -> `pack::load` -> `verify::verify` ->
//! `eval_core::run`), not hand-constructed mocks of internal state.

use agenterm_dynacore::eval_core::{self, Termination};
use agenterm_dynacore::ir::{Builder, Op, Term};
use agenterm_dynacore::pack;
use agenterm_dynacore::store::Store;
use agenterm_dynacore::verify::{self, OperationParamSchema, OperationSchema};

fn demo_catalog() -> Vec<OperationSchema> {
    vec![OperationSchema {
        id: "demo.echo".to_string(),
        available: true,
        parameters: vec![OperationParamSchema {
            name: "n".to_string(),
            value_type: "uint32".to_string(),
            required: true,
            minimum: Some(0),
            maximum: Some(100),
        }],
    }]
}

fn demo_module() -> agenterm_dynacore::ir::Module {
    let mut b = Builder::new();
    let one = b.konst(10);
    let two = b.konst(32);
    let sum = b.set(Op::Add(one, two));
    let call = b.fleet_call("demo.echo", "{\"n\":3}");
    let combined = b.set(Op::Xor(sum, call));
    b.term(Term::Exit(combined));
    b.finish("determinism_demo", 0)
}

// ============================================================================
// Determinism: verify() and run() applied twice, from two independent
// store-round-trips (never reusing the in-memory Module the first pass
// built), must agree in every observable respect.
// ============================================================================

#[test]
fn build_manifest_hash_is_stable_across_repeated_calls() {
    let m = demo_module();
    let (manifest1, bytes1) = pack::build_manifest(&m);
    let (manifest2, bytes2) = pack::build_manifest(&m);
    assert_eq!(manifest1, manifest2);
    assert_eq!(bytes1, bytes2);
}

#[test]
fn verification_and_execution_are_deterministic_across_repeated_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let catalog = demo_catalog();
    let manifest = pack::pack(&store, &demo_module()).expect("pack");

    let bridge = |_: &str, _: &str| -> Result<String, String> { Ok("{\"echoed\":true}".to_string()) };

    let loaded1 = pack::load(&store, &manifest).expect("load 1");
    let verified1 = verify::verify(&loaded1, &catalog).expect("verify 1");
    let outcome1 = eval_core::run(&verified1, &bridge);

    let loaded2 = pack::load(&store, &manifest).expect("load 2, independent deserialize");
    let verified2 = verify::verify(&loaded2, &catalog).expect("verify 2");
    let outcome2 = eval_core::run(&verified2, &bridge);

    assert_eq!(loaded1, loaded2, "two independent deserializes of the same hash must be byte-for-byte identical modules");
    assert_eq!(outcome1.termination, outcome2.termination);
    assert_eq!(outcome1.termination, Termination::Exited(42 ^ 1));
    assert_eq!(outcome1.calls.len(), outcome2.calls.len());
    assert_eq!(outcome1.calls[0].operation_id, outcome2.calls[0].operation_id);
    assert_eq!(outcome1.calls[0].params_json, outcome2.calls[0].params_json);
}

/// Determinism must hold even when the *bridge* itself is stateful (counts
/// its own invocations) -- the interpreter's own walk (not the bridge) is
/// what must be deterministic; a stateful bridge is a faithful stand-in for
/// a real host binding, not a simplification that would hide non-determinism
/// in `eval_core::run`'s own control flow.
#[test]
fn repeated_runs_of_the_same_verified_module_make_identical_call_sequences() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let catalog = demo_catalog();
    let manifest = pack::pack(&store, &demo_module()).expect("pack");
    let loaded = pack::load(&store, &manifest).expect("load");
    let verified = verify::verify(&loaded, &catalog).expect("verify");

    let calls_seen: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let bridge = |operation_id: &str, params_json: &str| -> Result<String, String> {
        calls_seen.borrow_mut().push(format!("{operation_id}:{params_json}"));
        Ok("{}".to_string())
    };

    let outcome_a = eval_core::run(&verified, &bridge);
    let outcome_b = eval_core::run(&verified, &bridge);
    assert_eq!(outcome_a.termination, outcome_b.termination);
    assert_eq!(
        calls_seen.into_inner(),
        vec!["demo.echo:{\"n\":3}".to_string(), "demo.echo:{\"n\":3}".to_string()],
        "the SAME VerifiedModule run twice must issue the identical call sequence both times"
    );
}

// ============================================================================
// Concurrent/repeated loads from the same content-addressed store.
// ============================================================================

/// The same hash loaded many times in a row from one `Store` handle must
/// always reproduce the identical `Module` -- `store::Store::get` re-verifies
/// the hash on every call (it is not a cache with a "trust the first read"
/// shortcut), so this also indirectly exercises that re-verification path
/// repeatedly rather than once.
#[test]
fn the_same_hash_loaded_repeatedly_always_reproduces_the_identical_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    let manifest = pack::pack(&store, &demo_module()).expect("pack");

    let first = pack::load(&store, &manifest).expect("load 1");
    for i in 0..50 {
        let again = pack::load(&store, &manifest).expect("repeated load");
        assert_eq!(again, first, "load #{i} diverged from the first load of the same hash");
    }
}

/// Multiple threads opening independent `Store` handles over the SAME root
/// directory and loading the SAME hash concurrently must all succeed and all
/// see the identical content -- the store is a plain read-mostly blob
/// directory with no in-process lock, so this is the realistic concurrency
/// shape (independent processes/threads reading a shared store), not an
/// artificial race on one `Store` value.
#[test]
fn concurrent_loads_of_the_same_hash_from_independent_store_handles_agree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_root = dir.path().to_path_buf();
    let manifest = {
        let store = Store::open(&store_root).expect("open store to seed it");
        pack::pack(&store, &demo_module()).expect("pack")
    };

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let root = store_root.clone();
            let manifest = manifest.clone();
            std::thread::spawn(move || {
                let store = Store::open(&root).expect("open store on worker thread");
                pack::load(&store, &manifest).expect("load on worker thread")
            })
        })
        .collect();

    let expected = {
        let store = Store::open(&store_root).expect("reopen store on main thread");
        pack::load(&store, &manifest).expect("load on main thread")
    };

    for h in handles {
        let module = h.join().expect("worker thread must not panic");
        assert_eq!(module, expected, "a concurrently loaded module diverged from the main thread's load");
    }
}

/// Two different-hash packs written to the SAME store from different threads
/// concurrently must not corrupt or cross-contaminate each other's blobs --
/// a stronger, genuinely concurrent (not just interleaved-in-one-thread)
/// version of `pack_lifecycle.rs`'s `two_different_hash_packs_coexist_independently`.
#[test]
fn concurrent_puts_of_distinct_content_do_not_cross_contaminate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_root = dir.path().to_path_buf();

    let handles: Vec<_> = (0u64..8)
        .map(|i| {
            let root = store_root.clone();
            std::thread::spawn(move || {
                let store = Store::open(&root).expect("open store on worker thread");
                let mut b = Builder::new();
                let v = b.konst(1000 + i);
                b.term(Term::Exit(v));
                let module = b.finish(format!("worker_{i}"), 0);
                let manifest = pack::pack(&store, &module).expect("pack on worker thread");
                (i, manifest)
            })
        })
        .collect();

    let store = Store::open(&store_root).expect("open store on main thread");
    let catalog: Vec<OperationSchema> = vec![];
    for h in handles {
        let (i, manifest) = h.join().expect("worker thread must not panic");
        let loaded = pack::load(&store, &manifest).expect("load this worker's own hash back");
        let verified = verify::verify(&loaded, &catalog).expect("well-formed");
        let never_called = |_: &str, _: &str| -> Result<String, String> { panic!("no fleet calls in this module") };
        let outcome = eval_core::run(&verified, &never_called);
        assert_eq!(outcome.termination, Termination::Exited(1000 + i), "worker {i}'s own content must come back, not another worker's");
    }
}
