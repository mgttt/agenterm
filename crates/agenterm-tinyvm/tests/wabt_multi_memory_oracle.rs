use std::path::PathBuf;

use agenterm_tinyvm::{Limits, Val, WasmError, WasmModule};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
#[ignore = "run through smoke-wabt-multi-memory.sh with an independently compiled fixture"]
fn wabt_compiled_multi_memory_matches_tinyvm() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_MULTI_MEMORY_WASM")
            .expect("TINYVM_WABT_MULTI_MEMORY_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced wasm");
    let module = must_ok(WasmModule::from_bytes(&bytes), "load WABT-produced wasm");
    let mut instance = must_ok(module.instantiate(), "instantiate WABT-produced wasm");
    assert_eq!(instance.memory_count(), 2);
    assert_eq!(instance.memory_pages_at(0), Some(1));
    assert_eq!(instance.memory_pages_at(1), Some(1));
    let result = must_ok(
        instance.invoke_by_name("run", &[]),
        "run multi-memory fixture",
    );
    assert!(matches!(result.as_slice(), [Val::I32(1225)]));
    assert_eq!(instance.memory_pages_at(0), Some(1));
    assert_eq!(instance.memory_pages_at(1), Some(2));
    assert_eq!(
        &must_ok(instance.memory_at(0), "memory zero").expect("memory zero")[..1],
        b"A"
    );
    assert_eq!(
        &must_ok(instance.memory_at(1), "memory one").expect("memory one")[..3],
        b"BAC"
    );

    let one_page = Limits {
        max_memory_pages: 1,
        ..Limits::default()
    };
    assert!(matches!(
        WasmModule::from_bytes_with(&bytes, one_page),
        Err(WasmError::Trap("memory size"))
    ));

    let two_pages = Limits {
        max_memory_pages: 2,
        ..Limits::default()
    };
    let module = must_ok(
        WasmModule::from_bytes_with(&bytes, two_pages),
        "load at aggregate minimum",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate at aggregate minimum");
    let result = must_ok(
        instance.invoke_by_name("run", &[]),
        "run with aggregate growth refusal",
    );
    assert!(matches!(result.as_slice(), [Val::I32(1222)]));
    assert_eq!(instance.memory_pages_at(0), Some(1));
    assert_eq!(instance.memory_pages_at(1), Some(1));
}
