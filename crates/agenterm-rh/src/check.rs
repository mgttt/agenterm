use rhai::Engine;

use crate::RhError;
use crate::subset::validate_ast;

pub fn check(source: &str) -> Result<(), RhError> {
    let ast = Engine::new()
        .compile(source)
        .map_err(|err| RhError::Parse(err.to_string()))?;
    validate_ast(&ast)
}
