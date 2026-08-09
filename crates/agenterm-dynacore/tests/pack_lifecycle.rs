//! Black-box acceptance tests for the v1 skeleton, against
//! `plan/design-dynacore-logic-pack.md` §4. This crate owns no product
//! name and cannot depend on the root `agenterm` crate's real
//! `OPERATION_CATALOG` (that would be a dependency cycle — the root crate
//! depends on this one, not the reverse), so these tests exercise the
//! machinery end to end against a small synthetic catalog. The real
//! `OPERATION_CATALOG`/`fleet.tabs.list` round trip is covered by the root
//! crate's `tests/dynacore_host.rs`, which owns the product-side host
//! binding (`src/script_dynacore_host.rs`).

use agenterm_dynacore::ir::{Builder, Op, Term};
use agenterm_dynacore::pack;
use agenterm_dynacore::store::Store;
use agenterm_dynacore::verify::{self, IrFault, OperationParamSchema, OperationSchema};

fn demo_catalog() -> Vec<OperationSchema> {
    vec![
        OperationSchema {
            id: "demo.echo".to_string(),
            available: true,
            parameters: vec![OperationParamSchema {
                name: "n".to_string(),
                value_type: "uint32".to_string(),
                required: true,
                minimum: Some(0),
                maximum: Some(100),
            }],
        },
        OperationSchema {
            id: "demo.noop".to_string(),
            available: true,
            parameters: vec![],
        },
    ]
}

fn open_temp_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    (dir, store)
}

/// Acceptance item 2: a pack (constructive content addressing: source
/// content -> hash -> stored) is loaded, verified, and interpreted, and a
/// real (mock) `fleet_call` round-trips operation_id + params + result.
#[test]
fn pack_loads_verifies_and_runs_a_real_fleet_call() {
    let (_dir, store) = open_temp_store();

    let mut b = Builder::new();
    let v = b.fleet_call("demo.echo", "{\"n\":3}");
    b.term(Term::Exit(v));
    let module = b.finish("demo_echo_pack", 0);

    let manifest = pack::pack(&store, &module).expect("pack");
    assert_eq!(manifest.operation_ids, vec!["demo.echo".to_string()]);

    let loaded = pack::load(&store, &manifest).expect("load");
    let catalog = demo_catalog();
    let verified = verify::verify(&loaded, &catalog).expect("verify should accept a well-formed pack");

    let seen = std::cell::RefCell::new(Vec::<(String, String)>::new());
    let bridge = |operation_id: &str, params_json: &str| -> Result<String, String> {
        seen.borrow_mut().push((operation_id.to_string(), params_json.to_string()));
        Ok("{\"ok\":true}".to_string())
    };
    let outcome = agenterm_dynacore::eval_core::run(&verified, &bridge);

    assert_eq!(outcome.result, 1, "FleetCall dest is 1 on Ok");
    assert_eq!(outcome.calls.len(), 1);
    assert_eq!(outcome.calls[0].operation_id, "demo.echo");
    assert_eq!(outcome.calls[0].params_json, "{\"n\":3}");
    assert_eq!(outcome.calls[0].result.as_deref(), Ok("{\"ok\":true}"));
    assert_eq!(
        seen.into_inner(),
        vec![("demo.echo".to_string(), "{\"n\":3}".to_string())],
        "operation_id and params_json reached the bridge exactly as declared"
    );
}

/// Acceptance item 3(a): operation_id not in the catalog is rejected by
/// verify() before any execution — no VerifiedModule is ever produced, so
/// there is no way to reach eval_core::run with it (a type-level guarantee,
/// not just a runtime check), and verify() itself must not panic.
#[test]
fn bad_pack_unknown_operation_is_rejected_before_execution() {
    let mut b = Builder::new();
    let v = b.fleet_call("does.not.exist", "{}");
    b.term(Term::Exit(v));
    let module = b.finish("bad_unknown_op", 0);

    let catalog = demo_catalog();
    let result = verify::verify(&module, &catalog);
    assert_eq!(
        result.err(),
        Some(IrFault::UnknownOperation {
            block: 0,
            id: 0,
            operation_id: "does.not.exist".to_string(),
        })
    );
}

/// Acceptance item 3(b): params that don't match the operation's declared
/// schema (missing required param) are rejected before execution, without
/// panicking.
#[test]
fn bad_pack_missing_required_param_is_rejected_before_execution() {
    let mut b = Builder::new();
    let v = b.fleet_call("demo.echo", "{}"); // missing required "n"
    b.term(Term::Exit(v));
    let module = b.finish("bad_missing_param", 0);

    let catalog = demo_catalog();
    let result = verify::verify(&module, &catalog);
    match result {
        Err(IrFault::ParamsMismatch { operation_id, .. }) => {
            assert_eq!(operation_id, "demo.echo");
        }
        Ok(_) => panic!("expected ParamsMismatch, verify() accepted a malformed pack"),
        Err(other) => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

/// Acceptance item 3(b) (type variant): a param present with the wrong JSON
/// type is rejected before execution, without panicking.
#[test]
fn bad_pack_wrong_param_type_is_rejected_before_execution() {
    let mut b = Builder::new();
    let v = b.fleet_call("demo.echo", "{\"n\":\"not-a-number\"}");
    b.term(Term::Exit(v));
    let module = b.finish("bad_wrong_type", 0);

    let catalog = demo_catalog();
    let result = verify::verify(&module, &catalog);
    match result {
        Err(IrFault::ParamsMismatch { operation_id, .. }) => {
            assert_eq!(operation_id, "demo.echo");
        }
        Ok(_) => panic!("expected ParamsMismatch, verify() accepted a malformed pack"),
        Err(other) => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

/// A pack calling an extern id out of range must also be rejected structurally
/// (IR-internal consistency, the original Q19 property), never panic.
#[test]
fn bad_pack_extern_id_out_of_range_is_rejected() {
    let mut b = Builder::new();
    let dest = b.v();
    b.term(Term::Exit(dest));
    let mut module = b.finish("bad_extern_id", 0);
    // Hand-craft an out-of-range FleetCall the Builder itself cannot produce.
    module.blocks[0]
        .insts
        .push(agenterm_dynacore::ir::Inst::FleetCall(dest, 99));

    let catalog = demo_catalog();
    let result = verify::verify(&module, &catalog);
    assert_eq!(result.err(), Some(IrFault::ExternIdOutOfRange { block: 0, id: 99 }));
}

/// Acceptance item 4: two different-hash packs are loaded/verified/run
/// concurrently-in-effect (interleaved here) and do not interfere with each
/// other's store bytes, verification, or interpreted result.
#[test]
fn two_different_hash_packs_coexist_independently() {
    let (_dir, store) = open_temp_store();
    let catalog = demo_catalog();

    let mut b1 = Builder::new();
    let one = b1.konst(1);
    let two = b1.konst(2);
    let sum = b1.set(Op::Add(one, two));
    b1.term(Term::Exit(sum));
    let module_a = b1.finish("pack_a", 0);

    let mut b2 = Builder::new();
    let call = b2.fleet_call("demo.noop", "{}");
    b2.term(Term::Exit(call));
    let module_b = b2.finish("pack_b", 0);

    let manifest_a = pack::pack(&store, &module_a).expect("pack a");
    let manifest_b = pack::pack(&store, &module_b).expect("pack b");
    assert_ne!(manifest_a.hash, manifest_b.hash, "distinct content must hash distinctly");

    let loaded_a = pack::load(&store, &manifest_a).expect("load a");
    let loaded_b = pack::load(&store, &manifest_b).expect("load b");
    let verified_a = verify::verify(&loaded_a, &catalog).expect("verify a");
    let verified_b = verify::verify(&loaded_b, &catalog).expect("verify b");

    let never_called = |_: &str, _: &str| -> Result<String, String> {
        panic!("pack_a must never call fleet_call")
    };
    let outcome_a = agenterm_dynacore::eval_core::run(&verified_a, &never_called);
    assert_eq!(outcome_a.result, 3);
    assert!(outcome_a.calls.is_empty());

    let bridge_b = |operation_id: &str, _: &str| -> Result<String, String> {
        assert_eq!(operation_id, "demo.noop");
        Ok("{}".to_string())
    };
    let outcome_b = agenterm_dynacore::eval_core::run(&verified_b, &bridge_b);
    assert_eq!(outcome_b.result, 1);
    assert_eq!(outcome_b.calls.len(), 1);

    // Reloading a's bytes from the store independently still reproduces a's
    // own content (not b's) — the store did not conflate the two hashes.
    let reloaded_a = pack::load(&store, &manifest_a).expect("reload a");
    assert_eq!(reloaded_a, loaded_a);
}
