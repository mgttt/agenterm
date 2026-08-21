//! AgenTerm-only fleet-shape validation layered on the language subset.
//!
//! `subset.rs` is Language 1 and knows nothing about `fleet`. This module is
//! the workbench flavour: it installs a root-expression hook that enforces
//! `RH_SUBSET_FLEET_SHAPE` and re-exports the `validate_ast` entry point the
//! AOT pipeline has always used.
//!
//! Per `plan/design-rh-standalone-product.md` §8 this module and `fleet.rs`
//! **stay in AgenTerm**; they must not follow `subset.rs` into `rh-lang`.

use rhai::{AST, Expr};

use crate::RhError;
use crate::fleet::{expr_uses_fleet, parse_fleet_call, validate_fleet_call};
use crate::subset::{SubsetPolicy, validate_ast_with};

/// The AgenTerm subset policy: language roots plus `fleet`, plus fleet shape.
pub const AGENTERM_POLICY: SubsetPolicy = SubsetPolicy::agenterm(fleet_root_expr);

/// Reject any root expression that mentions `fleet` but is not a supported
/// `fleet.*` call. Installed as the `subset::RootExprHook` for AgenTerm.
pub fn fleet_root_expr(expr: &Expr) -> Option<RhError> {
    if let Some(call) = parse_fleet_call(expr) {
        return validate_fleet_call(&call).err();
    }
    if expr_uses_fleet(expr) && parse_fleet_call(expr).is_none() {
        return Some(RhError::Subset {
            code: "RH_SUBSET_FLEET_SHAPE",
            detail: "fleet expression must be a supported fleet.* call".to_owned(),
        });
    }
    None
}

/// AgenTerm subset validation: Language 1 **plus** fleet shape.
///
/// This is the entry point `check.rs` and `transpile.rs` use. The product
/// `Engine::check` calls [`crate::subset::validate_ast_lang`] instead.
pub fn validate_ast(ast: &AST) -> Result<(), RhError> {
    validate_ast_with(ast, AGENTERM_POLICY)
}

#[cfg(test)]
mod tests {
    use rhai::Engine;

    use super::validate_ast;
    use crate::subset::validate_ast_lang;

    #[test]
    fn accepts_fleet_protocol_info() {
        let ast = Engine::new()
            .compile("fn entry() { fleet.protocol.info(); 1 }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn agenterm_policy_rejects_malformed_fleet_shape() {
        let ast = Engine::new()
            .compile("fn entry() { let handle = fleet; 1 }")
            .expect("compile");
        let error = validate_ast(&ast).expect_err("fleet shape");
        assert!(
            error.to_string().contains("RH_SUBSET_FLEET_SHAPE"),
            "{error}"
        );
    }

    /// The split's whole point: the same source that trips
    /// `RH_SUBSET_FLEET_SHAPE` under the AgenTerm policy must not trip it
    /// under Language 1, because Language 1 has no fleet grammar at all.
    #[test]
    fn language_policy_never_emits_fleet_shape() {
        let ast = Engine::new()
            .compile("fn entry() { let handle = fleet; 1 }")
            .expect("compile");
        if let Err(error) = validate_ast_lang(&ast) {
            assert!(
                !error.to_string().contains("RH_SUBSET_FLEET_SHAPE"),
                "Language 1 must not run fleet-shape validation: {error}"
            );
        }
    }
}
