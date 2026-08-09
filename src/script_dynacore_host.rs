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
use agenterm_dynacore::ir::Module;
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
pub fn run_pack(vm: &VerifiedModule, bridge: &DynacoreFleetBridgeFn) -> RunOutcome {
    agenterm_dynacore::eval_core::run(vm, bridge.as_ref())
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
    use agenterm_dynacore::ir::{Builder, Term};

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
}
