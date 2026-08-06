use rhai::{AST, Engine, OptimizationLevel};

use crate::RhError;
use crate::subset::{compat_validate, validate_ast};

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
