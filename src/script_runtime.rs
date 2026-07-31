//! Shared construction policy for every Rhai execution surface.
//!
//! Callers own transport, cancellation, output capture, and session lifecycle;
//! this module keeps the language limits and registered API graph identical.

use rhai::Engine;

use crate::{script_fleet, script_protocol::ScriptBudgets, script_stdlib};

pub const MAX_SESSION_VARIABLES: usize = 1_024;
pub const MAX_SESSION_FUNCTIONS: usize = 1_024;
pub const MAX_SESSION_MODULES: usize = 256;

pub fn configure_engine(engine: &mut Engine, budgets: &ScriptBudgets) {
    engine.set_max_operations(budgets.operations);
    engine.set_max_call_levels(budgets.call_depth);
    engine.set_max_expr_depths(budgets.expression_depth, budgets.expression_depth);
    engine.set_max_array_size(budgets.collection_items);
    engine.set_max_map_size(budgets.collection_items);
    engine.set_max_string_size(budgets.string_bytes);
    engine.set_max_variables(MAX_SESSION_VARIABLES);
    engine.set_max_functions(MAX_SESSION_FUNCTIONS);
    engine.set_max_modules(MAX_SESSION_MODULES);
    script_stdlib::register_local(engine);
    script_fleet::register(engine);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_engine_exposes_local_and_fleet_prelude() {
        let mut engine = Engine::new();
        configure_engine(&mut engine, &ScriptBudgets::default());
        assert!(
            engine
                .compile("let path = std::path::PathBuf::from(\".\");")
                .is_ok()
        );
        assert!(engine.compile("let value = fleet.tabs;").is_ok());
    }
}
