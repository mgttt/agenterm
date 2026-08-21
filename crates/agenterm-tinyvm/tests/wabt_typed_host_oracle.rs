use std::path::PathBuf;

use agenterm_tinyvm::{Val, WasmError, WasmModule};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
#[ignore = "run through smoke-wabt-typed-host.sh with an independently compiled fixture"]
fn wabt_compiled_typed_host_import_matches_tinyvm() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_TYPED_HOST_WASM")
            .expect("TINYVM_WABT_TYPED_HOST_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced wasm");
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load WABT-produced wasm");
    must_ok(
        module.bind_import_typed_in_place("host", "mix", |args, results, _| {
            assert!(args == [Val::I64(40), Val::F32(1.5), Val::F64(2.5)]);
            results[0] = Val::F64(4.5);
            results[1] = Val::I64(42);
            results[2] = Val::F32(3.5);
            Ok(())
        }),
        "bind WABT typed import",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate WABT-produced wasm");
    let result = must_ok(
        instance.invoke_by_name("run", &[]),
        "run typed host fixture",
    );
    assert!(result == [Val::F64(4.5), Val::I64(42), Val::F32(3.5)]);
}
