use std::path::PathBuf;

use agenterm_tinyvm::{Val, ValueType, WasmError, WasmExternReference, WasmModule};

fn must_ok<T>(value: Result<T, WasmError>, context: &str) -> T {
    value.unwrap_or_else(|error| panic!("{context}: {}", error.message()))
}

#[test]
#[ignore = "run through smoke-wabt-externref.sh with an independently compiled fixture"]
fn wabt_compiled_externref_matches_tinyvm() {
    let path = std::env::var_os("TINYVM_EXTERNREF_WASM")
        .map(PathBuf::from)
        .expect("TINYVM_EXTERNREF_WASM is set by smoke-wabt-externref.sh");
    let bytes = std::fs::read(path).expect("read independently compiled externref fixture");
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "load externref fixture");
    let expected = must_ok(WasmExternReference::new(), "allocate externref");
    must_ok(
        module.bind_import_typed_in_place("host", "source", move |args, results, _| {
            assert!(args.is_empty());
            results[0] = Val::ExternRef(Some(expected));
            Ok(())
        }),
        "bind externref source",
    );
    must_ok(
        module.bind_import_typed_in_place("host", "sink", move |args, results, _| {
            results[0] = Val::I32(i32::from(args == [Val::ExternRef(Some(expected))]));
            Ok(())
        }),
        "bind externref sink",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate externref fixture");
    assert!(matches!(
        must_ok(
            instance.invoke_by_name("null_is_null", &[]),
            "check null externref"
        )
        .as_slice(),
        [Val::I32(1)]
    ));
    assert!(matches!(
        must_ok(
            instance.invoke_by_name("host_is_not_null", &[]),
            "check non-null host externref"
        )
        .as_slice(),
        [Val::I32(1)]
    ));
    assert!(matches!(
        must_ok(
            instance.invoke_by_name("roundtrip", &[]),
            "roundtrip host externref"
        )
        .as_slice(),
        [Val::I32(1)]
    ));
    let saved = instance
        .exported_global_handle("saved")
        .expect("saved global export");
    assert!(saved.is_mutable());
    assert!(saved.value_type() == ValueType::ExternRef);
    assert!(saved.value() == Val::ExternRef(Some(expected)));

    let other = must_ok(WasmExternReference::new(), "allocate distinct externref");
    must_ok(
        saved.set(Val::ExternRef(Some(other))),
        "set externref global",
    );
    assert!(matches!(
        must_ok(
            instance.invoke_by_name("read_saved", &[]),
            "read changed externref global"
        )
        .as_slice(),
        [Val::I32(0)]
    ));
    must_ok(saved.set(Val::ExternRef(None)), "clear externref global");
    assert!(matches!(saved.value(), Val::ExternRef(None)));
}
