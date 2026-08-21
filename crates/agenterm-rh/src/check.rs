use std::path::Path;

use rhai::{AST, Engine, OptimizationLevel};

use crate::RhError;
use crate::api_validate::validate_available_apis;
use crate::fleet_subset::validate_ast;
use crate::project_import::validate_project_imports;
use crate::subset::compat_validate;

/// Match the ordinary Script worker expression budget so large task scripts parse.
pub const RH_MAX_EXPR_DEPTH: usize = 512;

pub fn parse_rh_ast(source: &str) -> Result<AST, RhError> {
    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);
    engine.set_max_expr_depths(RH_MAX_EXPR_DEPTH, RH_MAX_EXPR_DEPTH);
    engine
        .compile(source)
        .map_err(|err| RhError::Parse(err.to_string()))
}

pub fn check(source: &str) -> Result<(), RhError> {
    let ast = parse_rh_ast(source)?;
    if let Err(error) = validate_ast(&ast) {
        compat_validate(source, &ast).map_err(|_| error)?;
    }
    Ok(())
}

/// rh subset check plus optional project import tree and shipped API validation.
pub fn check_with_project_validation(
    source: &str,
    project_root: Option<&Path>,
) -> Result<(), RhError> {
    check(source)?;
    if let Some(root) = project_root {
        let module_sources = validate_project_imports(root, source).map_err(RhError::Compile)?;
        for module_source in module_sources {
            validate_available_apis(&module_source).map_err(api_validate_error_to_rh)?;
        }
    }
    validate_available_apis(source).map_err(api_validate_error_to_rh)
}

fn api_validate_error_to_rh(error: crate::api_validate::ApiValidateError) -> RhError {
    RhError::Compile(format!("{}: {}", error.code, error.message))
}

#[cfg(test)]
mod tests {
    use super::check_with_project_validation;

    #[test]
    fn check_with_project_validation_rejects_unknown_api() {
        let error = check_with_project_validation("fn entry() { std::fs::not_shipped(`x`) }", None)
            .expect_err("unknown API");
        assert!(error.to_string().contains("script_api_unknown"));
    }
}
