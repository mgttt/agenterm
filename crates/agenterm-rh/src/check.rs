use rhai::{AST, Engine, OptimizationLevel};

use crate::RhError;
use crate::subset::validate_ast;

pub fn parse_rh_ast(source: &str) -> Result<AST, RhError> {
    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);
    engine
        .compile(source)
        .map_err(|err| RhError::Parse(err.to_string()))
}

pub fn check(source: &str) -> Result<(), RhError> {
    let ast = parse_rh_ast(source)?;
    validate_ast(&ast)
}
