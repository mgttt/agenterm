use std::path::PathBuf;

use agenterm_tinyvm::{Val, WasmError, WasmModule};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
#[ignore = "run through smoke-wabt-multi-value.sh with an independently compiled fixture"]
fn wabt_compiled_multi_value_matches_tinyvm() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_MULTI_VALUE_WASM")
            .expect("TINYVM_WABT_MULTI_VALUE_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced wasm");
    let module = must_ok(WasmModule::from_bytes(&bytes), "load WABT-produced wasm");
    let mut instance = must_ok(module.instantiate(), "instantiate WABT-produced wasm");
    let result = must_ok(
        instance.invoke_by_name("run", &[]),
        "run multi-value fixture",
    );
    assert!(matches!(result.as_slice(), [Val::I32(143)]));
}
