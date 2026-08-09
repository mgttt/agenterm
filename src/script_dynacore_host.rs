//! dynacore host bridge — wires `agenterm-dynacore`'s produce-time verify
//! gate and interpreter to this crate's real `OPERATION_CATALOG`, and to the
//! same `fleet_call(operation_id, params_json)` bridge shape
//! `src/script_rh_host.rs` (`FleetBridgeFn`) and `src/script_lua_host.rs`
//! (`LuaFleetBridgeFn`) already established for the three script engines.
//! Not a new binding shape — the fourth consumer of the same contract.
//!
//! `agenterm-dynacore` owns no product name (same posture as
//! `agenterm-platform`) and does not depend on this crate's
//! `OPERATION_CATALOG`, so `operation_catalog_schema` is the one-time
//! conversion from `operations::OperationSpec`/`OperationParameterSpec`
//! into the crate's generic `verify::OperationSchema`/`OperationParamSchema`
//! mirror.

use std::sync::{Arc, OnceLock};

use agenterm_dynacore::eval_core::RunOutcome;
use agenterm_dynacore::ir::{Builder, Module, Term};
use agenterm_dynacore::pack::{self, PackManifest};
use agenterm_dynacore::store::Store;
use agenterm_dynacore::verify::{self, IrFault, OperationParamSchema, OperationSchema, VerifiedModule};

use crate::operations::OPERATION_CATALOG;

/// Fleet bridge injected into a dynacore pack run: (operation_id,
/// params_json) → result JSON. Same shape as `script_rh_host::FleetBridgeFn`
/// and `script_lua_host::LuaFleetBridgeFn`; `Arc` (not `Box`) because a pack
/// run has no thread-local FFI boundary to own the closure across, matching
/// `LuaFleetBridgeFn`'s posture rather than rh's.
pub type DynacoreFleetBridgeFn = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

fn operation_catalog_schema() -> &'static [OperationSchema] {
    static SCHEMA: OnceLock<Vec<OperationSchema>> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        OPERATION_CATALOG
            .iter()
            .map(|operation| OperationSchema {
                id: operation.id.to_string(),
                available: operation.available,
                parameters: operation
                    .parameters
                    .iter()
                    .map(|parameter| OperationParamSchema {
                        name: parameter.name.to_string(),
                        value_type: parameter.value_type.to_string(),
                        required: parameter.required,
                        minimum: parameter.minimum,
                        maximum: parameter.maximum,
                    })
                    .collect(),
            })
            .collect()
    })
}

/// Verify `module` against the real `OPERATION_CATALOG` (produce-time gate;
/// no execution). This is the only way to obtain a `VerifiedModule` through
/// this host binding.
pub fn verify_pack(module: &Module) -> Result<VerifiedModule<'_>, IrFault> {
    verify::verify(module, operation_catalog_schema())
}

/// Interpret a verified pack, routing every `fleet_call` through `bridge`.
/// Bounded at `agenterm_dynacore::eval_core::DEFAULT_MAX_STEPS` (v1.1 safety
/// hardening — see that module's doc) so a pack whose control flow never
/// falls out of a loop cannot wedge the host thread.
pub fn run_pack(vm: &VerifiedModule, bridge: &DynacoreFleetBridgeFn) -> RunOutcome {
    agenterm_dynacore::eval_core::run(vm, bridge.as_ref())
}

/// `run_pack`, with a caller-chosen step ceiling instead of the host default.
/// The step limit is a HOST knob, never a pack-declared one (see
/// `eval_core::run_with_step_limit`'s doc for why) — this exists for
/// embedders/tests that want a tighter bound than `DEFAULT_MAX_STEPS`
/// without waiting out the full default budget.
pub fn run_pack_with_step_limit(vm: &VerifiedModule, bridge: &DynacoreFleetBridgeFn, max_steps: u64) -> RunOutcome {
    agenterm_dynacore::eval_core::run_with_step_limit(vm, bridge.as_ref(), max_steps)
}

/// A runnable, non-trivial demo pack (`agenterm-cli dynacore-run --demo`):
/// two `fleet.tabs.list` calls with a real `BrCond` between them, branching
/// on whether the FIRST call's `FleetCall` dest word came back `1` (Ok) or
/// `0` (Err) — the one branch predicate this crate's IR can express today
/// (`ir::Inst::FleetCall`'s own doc: "directly usable by `Term::BrCond`").
///
/// Deliberately NOT attempted: branching on something pulled out of the
/// fleet call's JSON *result* (e.g. "how many tabs came back"). There is no
/// `Op`/`Inst` in this crate that parses a `fleet_call` `Result<String,
/// String>` payload into a `Val` — v1's `Op` set is closed over `Val`s
/// produced by `Const`/arithmetic/`FleetCall`'s own Ok/Err dest, nothing
/// reads `params_json`/result strings. `plan/design-dynacore-logic-pack.md`
/// names this gap directly (no expression language for consuming
/// `fleet_call` results); this demo uses the Ok/Err dest branch it actually
/// has rather than inventing a new op just to make the demo look richer than
/// the shipped IR is.
///
/// Shape: call `tabs.list` → branch on Ok/Err → (Ok arm) call `tabs.list`
/// again and exit with ITS dest word; (Err arm) exit with the sentinel `999`
/// without a second call. Both arms are real, reachable blocks — against a
/// live, reachable agenterm server `tabs.list` always succeeds, so a real run
/// exercises the Ok arm (two real fleet calls observed); a disconnected
/// server exercises the Err arm instead (one call attempted, one failure
/// observed, no second call).
pub fn demo_pack_module() -> Module {
    let operation_id = OPERATION_CATALOG
        .iter()
        .find(|operation| operation.script_surface == "fleet.tabs.list")
        .expect("fleet.tabs.list must be registered in OPERATION_CATALOG")
        .id;

    let mut b = Builder::new();
    let first = b.fleet_call(operation_id, "{}");
    b.term(Term::BrCond(first, 1, 2)); // block 0: nz(Ok) -> block 1, z(Err) -> block 2
    let second = b.fleet_call(operation_id, "{}");
    b.term(Term::Exit(second)); // block 1 (Ok arm): a second real call, exit with its dest
    let sentinel = b.konst(999);
    b.term(Term::Exit(sentinel)); // block 2 (Err arm): no second call, exit with a sentinel
    b.finish("dynacore_demo_pack", 0)
}

/// Build-time step: serialize `module`, store it under its content hash, and
/// return the manifest a loader should hold onto.
pub fn pack_module(store: &Store, module: &Module) -> std::io::Result<PackManifest> {
    pack::pack(store, module)
}

/// Run-time step: fetch `manifest.hash` from `store`, deserialize it, and
/// verify it against the real `OPERATION_CATALOG`. Returns the loaded module
/// (owned, so the caller can keep it alive alongside the `VerifiedModule`
/// borrow) or a description of the first failure — never panics on
/// malformed store content or a malformed pack.
pub fn load_and_verify_pack(store: &Store, manifest: &PackManifest) -> Result<Module, String> {
    let module = pack::load(store, manifest)?;
    verify_pack(&module).map_err(|fault| format!("{manifest:?}: rejected by verify() — {fault:?}"))?;
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_catalog_schema_covers_tabs_list() {
        let schema = operation_catalog_schema();
        let tabs_list = schema
            .iter()
            .find(|op| op.id == "tabs.list")
            .expect("tabs.list must be in the converted catalog");
        assert!(tabs_list.available);
        assert!(
            tabs_list.parameters.is_empty(),
            "fleet.tabs.list is declared with NO_PARAMETERS"
        );
    }

    #[test]
    fn verify_pack_accepts_a_real_tabs_list_call() {
        let mut b = Builder::new();
        let v = b.fleet_call("tabs.list", "{}");
        b.term(Term::Exit(v));
        let module = b.finish("tabs_list_pack", 0);

        verify_pack(&module).expect("tabs.list with empty params must verify against the real catalog");
    }

    #[test]
    fn verify_pack_rejects_unknown_operation() {
        let mut b = Builder::new();
        let v = b.fleet_call("does.not.exist", "{}");
        b.term(Term::Exit(v));
        let module = b.finish("unknown_op_pack", 0);

        let err = verify_pack(&module).err().expect("unknown operation_id must be rejected");
        match err {
            IrFault::UnknownOperation { operation_id, .. } => assert_eq!(operation_id, "does.not.exist"),
            other => panic!("expected UnknownOperation, got {other:?}"),
        }
    }

    #[test]
    fn verify_pack_rejects_extra_param_tabs_list_does_not_declare() {
        let mut b = Builder::new();
        let v = b.fleet_call("tabs.list", "{\"unexpected\":1}");
        b.term(Term::Exit(v));
        let module = b.finish("tabs_list_extra_param_pack", 0);

        let err = verify_pack(&module)
            .err()
            .expect("a param tabs.list does not declare must be rejected");
        match err {
            IrFault::ParamsMismatch { operation_id, .. } => assert_eq!(operation_id, "tabs.list"),
            other => panic!("expected ParamsMismatch, got {other:?}"),
        }
    }

    #[test]
    fn demo_pack_module_verifies_against_the_real_catalog() {
        let module = demo_pack_module();
        assert_eq!(module.blocks.len(), 3, "entry + Ok arm + Err arm");
        verify_pack(&module).expect("the demo pack must be well-formed against the real catalog");
    }

    #[test]
    fn demo_pack_module_ok_arm_makes_two_real_calls_and_exits_with_the_second_dest() {
        let module = demo_pack_module();
        let verified = verify_pack(&module).expect("verify");
        let seen: std::cell::RefCell<u32> = std::cell::RefCell::new(0);
        let bridge = |_: &str, _: &str| -> Result<String, String> {
            *seen.borrow_mut() += 1;
            Ok("{\"tabs\":[]}".to_string())
        };
        let outcome = agenterm_dynacore::eval_core::run(&verified, &bridge);
        assert_eq!(outcome.result(), Some(1), "second call's dest is 1 on Ok");
        assert_eq!(outcome.calls.len(), 2, "the Ok arm makes exactly two fleet calls");
        assert_eq!(*seen.borrow(), 2);
    }

    #[test]
    fn demo_pack_module_err_arm_makes_one_call_and_exits_with_the_sentinel() {
        let module = demo_pack_module();
        let verified = verify_pack(&module).expect("verify");
        let bridge = |_: &str, _: &str| -> Result<String, String> { Err("unreachable".to_string()) };
        let outcome = agenterm_dynacore::eval_core::run(&verified, &bridge);
        assert_eq!(outcome.result(), Some(999), "the Err arm exits with the sentinel");
        assert_eq!(outcome.calls.len(), 1, "the Err arm makes exactly one fleet call");
    }
}
