use rhai::Engine;

use crate::{subset::validate_ast, RhError};

pub fn check(source: &str) -> Result<(), RhError> {
    let ast = Engine::new()
        .compile(source)
        .map_err(|err| RhError::Parse(err.to_string()))?;
    validate_ast(&ast)
}

pub fn compile_native(_source: &str, _output_path: &std::path::Path) -> Result<(), RhError> {
    Err(RhError::Compile(
        "rh AOT link step is not implemented yet (rh-0); use transpile + rustc manually".into(),
    ))
}
