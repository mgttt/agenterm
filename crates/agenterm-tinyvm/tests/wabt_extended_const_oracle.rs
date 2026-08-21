use std::path::PathBuf;

use agenterm_tinyvm::{Val, WasmError, WasmModule};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
#[ignore = "run through smoke-wabt-extended-const.sh with an independently compiled fixture"]
fn wabt_compiled_extended_const_matches_tinyvm() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_EXTENDED_CONST_WASM")
            .expect("TINYVM_WABT_EXTENDED_CONST_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced wasm");
    let module = must_ok(WasmModule::from_bytes(&bytes), "load WABT-produced wasm");
    let mut instance = must_ok(module.instantiate(), "instantiate WABT-produced wasm");
    let result = must_ok(
        instance.invoke_by_name("run", &[]),
        "run extended-const fixture",
    );
    assert!(matches!(result.as_slice(), [Val::I32(199)]));
}
