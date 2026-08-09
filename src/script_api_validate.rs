//! Static API surface validation for script sources (shared by the Rh engine and rh check paths).

use crate::script_protocol::{ScriptFailure, ScriptFailureCategory};

fn script_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    ScriptFailure {
        code: code.into(),
        message: message.into(),
        category: ScriptFailureCategory::Script,
    }
}

pub fn validate_available_apis(source: &str) -> Result<(), ScriptFailure> {
    agenterm_rh::api_validate::validate_available_apis(source)
        .map_err(|error| script_error(error.code, error.message))
}

pub use agenterm_rh::api_validate::{
    agent_method_calls, external_function_calls, fleet_method_calls, qualified_function_calls,
    ApiValidateError,
};
pub use agenterm_rh::shipped_surfaces::SHIPPED_SURFACE_PATHS;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::script_catalog::{entries, ScriptApiStatus};

    use super::qualified_function_calls;

    #[test]
    fn shipped_surface_paths_match_catalog() {
        let catalog: HashSet<_> = entries()
            .into_iter()
            .filter(|entry| entry.status == ScriptApiStatus::Shipped)
            .map(|entry| entry.surface_path)
            .collect();
        let shipped: HashSet<_> = super::SHIPPED_SURFACE_PATHS.iter().copied().collect();
        if catalog != shipped {
            let only_catalog: Vec<_> = catalog.difference(&shipped).copied().collect();
            let only_shipped: Vec<_> = shipped.difference(&catalog).copied().collect();
            panic!(
                "agenterm-rh shipped surface list diverged from script_catalog\nonly catalog: {only_catalog:?}\nonly shipped: {only_shipped:?}"
            );
        }
    }

    #[test]
    fn qualified_function_calls_ignore_strings_and_comments() {
        assert!(
            qualified_function_calls(r#""std::fs::hidden()"; // rhai::json::hidden()"#).is_empty()
        );
    }
}
