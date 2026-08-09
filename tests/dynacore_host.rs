//! Black-box acceptance tests for `plan/design-dynacore-logic-pack.md` §4,
//! against the real `OPERATION_CATALOG` (not a synthetic catalog — that
//! coverage lives in `crates/agenterm-dynacore/tests/pack_lifecycle.rs`,
//! which cannot see this crate's catalog without a dependency cycle). Every
//! test drives the public `agenterm::script_dynacore_host` binding the same
//! way a real embedder would: build a pack with `agenterm_dynacore::ir`,
//! store/load it by content hash, verify it against `OPERATION_CATALOG`, and
//! interpret it through a mock `DynacoreFleetBridgeFn` (no real GUI —
//! per the task, proving the call path is complete does not require one).

use std::sync::Arc;

use agenterm::operations::OPERATION_CATALOG;
use agenterm::script_dynacore_host::{self, DynacoreFleetBridgeFn};
use agenterm_dynacore::ir::{Builder, Term};
use agenterm_dynacore::store::Store;
use agenterm_dynacore::verify::IrFault;

fn temp_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path()).expect("open store");
    (dir, store)
}

/// `fleet.tabs.list` is the accepted v1 verification target
/// (`plan/design-dynacore-logic-pack.md`): Observe class, no side effects,
/// `NO_PARAMETERS`.
fn tabs_list_operation_id() -> &'static str {
    let op = OPERATION_CATALOG
        .iter()
        .find(|op| op.script_surface == "fleet.tabs.list")
        .expect("fleet.tabs.list must be registered in OPERATION_CATALOG");
    assert!(op.available, "fleet.tabs.list must be available for this to be a meaningful v1 target");
    assert!(op.parameters.is_empty(), "fleet.tabs.list must take no parameters");
    op.id
}

/// Acceptance item 2: a pack (constructive content addressing: source
/// content -> hash -> stored in `Store`) is loaded, verified against the
/// real `OPERATION_CATALOG`, and interpreted, making a real call path to
/// `fleet.tabs.list` — operation_id passed correctly, params passed
/// correctly, result passed back correctly — through a mock bridge.
#[test]
fn pack_round_trips_a_real_fleet_tabs_list_call() {
    let (_dir, store) = temp_store();
    let operation_id = tabs_list_operation_id();

    let mut b = Builder::new();
    let v = b.fleet_call(operation_id, "{}");
    b.term(Term::Exit(v));
    let module = b.finish("tabs_list_pack", 0);

    let manifest = script_dynacore_host::pack_module(&store, &module).expect("pack");
    assert_eq!(manifest.operation_ids, vec![operation_id.to_string()]);

    let loaded = script_dynacore_host::load_and_verify_pack(&store, &manifest).expect("load_and_verify");
    let verified = script_dynacore_host::verify_pack(&loaded).expect("verify against OPERATION_CATALOG");

    let seen: Arc<std::sync::Mutex<Vec<(String, String)>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_for_bridge = Arc::clone(&seen);
    let bridge: DynacoreFleetBridgeFn = Arc::new(move |op_id: &str, params: &str| {
        seen_for_bridge.lock().unwrap().push((op_id.to_string(), params.to_string()));
        Ok("{\"tabs\":[]}".to_string())
    });

    let outcome = script_dynacore_host::run_pack(&verified, &bridge);

    assert_eq!(outcome.result, 1, "FleetCall dest word is 1 on Ok");
    assert_eq!(outcome.calls.len(), 1);
    assert_eq!(outcome.calls[0].operation_id, operation_id);
    assert_eq!(outcome.calls[0].params_json, "{}");
    assert_eq!(outcome.calls[0].result.as_deref(), Ok("{\"tabs\":[]}"));
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[(operation_id.to_string(), "{}".to_string())],
        "the mock bridge observed exactly one real call: operation_id and params round-tripped"
    );
}

/// Acceptance item 3(a): a pack naming an `operation_id` absent from
/// `OPERATION_CATALOG` is rejected by `verify_pack` before any execution —
/// there is no way to obtain a `VerifiedModule` for it, so `run_pack` can
/// never be reached, and `verify_pack` itself must not panic.
#[test]
fn bad_pack_operation_id_not_in_catalog_is_rejected() {
    let mut b = Builder::new();
    let v = b.fleet_call("definitely.not.a.real.operation", "{}");
    b.term(Term::Exit(v));
    let module = b.finish("bad_op_pack", 0);

    let err = script_dynacore_host::verify_pack(&module)
        .err()
        .expect("unknown operation_id must be rejected");
    match err {
        IrFault::UnknownOperation { operation_id, .. } => {
            assert_eq!(operation_id, "definitely.not.a.real.operation");
        }
        other => panic!("expected UnknownOperation, got {other:?}"),
    }
}

/// Acceptance item 3(b): a pack whose params don't match the real
/// operation's declared parameter schema — here, any params at all for
/// `fleet.tabs.list`'s `NO_PARAMETERS` — is rejected before execution,
/// without panicking.
#[test]
fn bad_pack_params_mismatch_against_real_schema_is_rejected() {
    let operation_id = tabs_list_operation_id();
    let mut b = Builder::new();
    let v = b.fleet_call(operation_id, "{\"unexpected\":\"param\"}");
    b.term(Term::Exit(v));
    let module = b.finish("bad_params_pack", 0);

    let err = script_dynacore_host::verify_pack(&module)
        .err()
        .expect("a param tabs.list does not declare must be rejected");
    match err {
        IrFault::ParamsMismatch { operation_id: got, .. } => assert_eq!(got, operation_id),
        other => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

/// A pack with an operation that exists but is declared unavailable (or, as
/// exercised structurally here, one an out-of-range extern id points at)
/// must also be rejected, not panic. Uses `ui.hello`'s required `minimum`
/// parameter with the wrong JSON type to exercise the real numeric-type
/// path (not just presence/absence) against a live catalog entry.
#[test]
fn bad_pack_wrong_param_type_against_real_schema_is_rejected() {
    let ui_hello = OPERATION_CATALOG
        .iter()
        .find(|op| op.id == "ui.hello")
        .expect("ui.hello must be registered");
    assert!(
        ui_hello.parameters.iter().any(|p| p.name == "minimum" && p.required),
        "test assumption: ui.hello requires a numeric 'minimum' param"
    );

    let mut b = Builder::new();
    let v = b.fleet_call("ui.hello", "{\"minimum\":\"not-a-number\",\"maximum\":5}");
    b.term(Term::Exit(v));
    let module = b.finish("bad_type_pack", 0);

    let err = script_dynacore_host::verify_pack(&module)
        .err()
        .expect("wrong JSON type for a declared param must be rejected");
    match err {
        IrFault::ParamsMismatch { operation_id, .. } => assert_eq!(operation_id, "ui.hello"),
        other => panic!("expected ParamsMismatch, got {other:?}"),
    }
}

/// Acceptance item 4: two different-hash packs are loaded/verified/run
/// independently in one store without interfering with each other.
#[test]
fn two_different_hash_packs_coexist_independently_against_real_catalog() {
    let (_dir, store) = temp_store();
    let operation_id = tabs_list_operation_id();

    let mut b1 = Builder::new();
    let v1 = b1.fleet_call(operation_id, "{}");
    b1.term(Term::Exit(v1));
    let module_a = b1.finish("pack_a", 0);

    let mut b2 = Builder::new();
    let one = b2.konst(1);
    let two = b2.konst(2);
    let call = b2.fleet_call(operation_id, "{}");
    let sum = b2.set(agenterm_dynacore::ir::Op::Add(one, two));
    let _ = call; // call result also flows into vals, unused arithmetically on purpose
    b2.term(Term::Exit(sum));
    let module_b = b2.finish("pack_b", 0);

    let manifest_a = script_dynacore_host::pack_module(&store, &module_a).expect("pack a");
    let manifest_b = script_dynacore_host::pack_module(&store, &module_b).expect("pack b");
    assert_ne!(manifest_a.hash, manifest_b.hash);

    let loaded_a = script_dynacore_host::load_and_verify_pack(&store, &manifest_a).expect("load a");
    let loaded_b = script_dynacore_host::load_and_verify_pack(&store, &manifest_b).expect("load b");
    let verified_a = script_dynacore_host::verify_pack(&loaded_a).expect("verify a");
    let verified_b = script_dynacore_host::verify_pack(&loaded_b).expect("verify b");

    let calls_a: Arc<std::sync::Mutex<u32>> = Arc::new(std::sync::Mutex::new(0));
    let calls_a_for_bridge = Arc::clone(&calls_a);
    let bridge_a: DynacoreFleetBridgeFn = Arc::new(move |_: &str, _: &str| {
        *calls_a_for_bridge.lock().unwrap() += 1;
        Ok("{}".to_string())
    });
    let outcome_a = script_dynacore_host::run_pack(&verified_a, &bridge_a);
    assert_eq!(outcome_a.result, 1);
    assert_eq!(*calls_a.lock().unwrap(), 1);

    let calls_b: Arc<std::sync::Mutex<u32>> = Arc::new(std::sync::Mutex::new(0));
    let calls_b_for_bridge = Arc::clone(&calls_b);
    let bridge_b: DynacoreFleetBridgeFn = Arc::new(move |_: &str, _: &str| {
        *calls_b_for_bridge.lock().unwrap() += 1;
        Ok("{}".to_string())
    });
    let outcome_b = script_dynacore_host::run_pack(&verified_b, &bridge_b);
    assert_eq!(outcome_b.result, 3, "pack_b's final value is the arithmetic sum, unaffected by pack_a's run");
    assert_eq!(*calls_b.lock().unwrap(), 1, "pack_b made its own single call, not pack_a's");

    // Reloading each hash independently still reproduces its own content.
    let reloaded_a = script_dynacore_host::load_and_verify_pack(&store, &manifest_a).expect("reload a");
    let reloaded_b = script_dynacore_host::load_and_verify_pack(&store, &manifest_b).expect("reload b");
    assert_eq!(reloaded_a, loaded_a);
    assert_eq!(reloaded_b, loaded_b);
}
