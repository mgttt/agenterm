use std::path::PathBuf;

use agenterm_tinyvm::{Val, ValueType, WasmError, WasmGlobal, WasmModule};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

fn only_i32(values: Vec<Val>) -> i32 {
    match values.as_slice() {
        [Val::I32(value)] => *value,
        _ => panic!("expected one i32 result"),
    }
}

#[test]
#[ignore = "run through smoke-wabt-imported-globals.sh with an independently compiled fixture"]
fn wabt_compiled_imported_globals_match_tinyvm() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_IMPORTED_GLOBALS_WASM")
            .expect("TINYVM_WABT_IMPORTED_GLOBALS_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced wasm");
    let provider_path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_EXPORTED_GLOBALS_WASM")
            .expect("TINYVM_WABT_EXPORTED_GLOBALS_WASM is set by the smoke script"),
    );
    let provider_bytes = std::fs::read(provider_path).expect("read WABT-produced provider wasm");

    let unbound = must_ok(WasmModule::from_bytes(&bytes), "load unbound module");
    assert!(matches!(
        unbound.instantiate(),
        Err(WasmError::Trap("unbound imported global"))
    ));

    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load imported globals");
    assert_eq!(module.global_imports().len(), 2);
    assert_eq!(module.global_imports()[0].module, "host");
    assert_eq!(module.global_imports()[0].field, "base");
    assert!(module.global_imports()[0].value_type == ValueType::I32);
    assert!(!module.global_imports()[0].mutable);
    assert!(module.global_imports()[1].mutable);

    let wrong = WasmGlobal::new(Val::I64(3), false);
    assert!(matches!(
        module.bind_global_import("host", "base", &wrong),
        Err(WasmError::Trap("global binding type"))
    ));

    let provider = must_ok(
        must_ok(
            WasmModule::from_bytes(&provider_bytes),
            "load exported globals",
        )
        .instantiate(),
        "instantiate exported globals",
    );
    let base = provider
        .exported_global_handle("base")
        .expect("base global export");
    let counter = provider
        .exported_global_handle("counter")
        .expect("counter global export");
    must_ok(
        module.bind_global_import("host", "base", &base),
        "bind base",
    );
    must_ok(
        module.bind_global_import("host", "counter", &counter),
        "bind counter",
    );

    let mut first = must_ok(module.instantiate(), "instantiate first");
    assert_eq!(
        only_i32(must_ok(first.invoke_by_name("run", &[]), "first run")),
        87
    );
    assert!(matches!(counter.value(), Val::I32(11)));

    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load sibling module");
    must_ok(
        module.bind_global_import("host", "base", &base),
        "bind base",
    );
    must_ok(
        module.bind_global_import("host", "counter", &counter),
        "bind shared counter",
    );
    let mut second = must_ok(module.instantiate(), "instantiate sibling");
    assert_eq!(
        only_i32(must_ok(second.invoke_by_name("run", &[]), "sibling run")),
        88
    );
    assert!(matches!(counter.value(), Val::I32(12)));

    must_ok(counter.set(Val::I32(20)), "host updates counter");
    assert_eq!(
        only_i32(must_ok(
            first.invoke_by_name("run", &[]),
            "host-updated run"
        )),
        97
    );
    assert!(matches!(counter.value(), Val::I32(21)));
    assert!(matches!(
        provider.exported_global("counter"),
        Some(Val::I32(21))
    ));
}
