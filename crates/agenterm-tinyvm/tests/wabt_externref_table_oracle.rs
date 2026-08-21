use std::path::PathBuf;

use agenterm_tinyvm::{Val, ValueType, WasmError, WasmExternReference, WasmModule, WasmStore};

fn must_ok<T>(value: Result<T, WasmError>, context: &str) -> T {
    value.unwrap_or_else(|error| panic!("{context}: {}", error.message()))
}

#[test]
#[ignore = "run through smoke-wabt-externref-table.sh with an independently compiled fixture"]
fn wabt_compiled_externref_tables_preserve_host_identity() {
    let path = std::env::var_os("TINYVM_EXTERNREF_TABLE_WASM")
        .map(PathBuf::from)
        .expect("TINYVM_EXTERNREF_TABLE_WASM is set by the smoke script");
    let bytes = std::fs::read(path).expect("read independently compiled externref-table fixture");

    let store = WasmStore::new();
    let shared = must_ok(
        store.create_externref_table(2, Some(6)),
        "create host externref table",
    );
    assert!(shared.element_type() == ValueType::ExternRef);

    let first = must_ok(WasmExternReference::new(), "allocate first externref");
    let second = must_ok(WasmExternReference::new(), "allocate second externref");
    must_ok(
        shared.set(0, Val::ExternRef(Some(second))),
        "host table set",
    );
    assert!(shared.get(0) == Ok(Some(Val::ExternRef(Some(second)))));

    let mut module = must_ok(
        WasmModule::from_bytes(&bytes),
        "load externref table fixture",
    );
    assert!(module.table_imports()[0].element_type == ValueType::ExternRef);
    must_ok(
        module.bind_table_import("host", "refs", &shared),
        "bind host externref table",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate externref table fixture");

    must_ok(
        instance.invoke_by_name("seed", &[Val::ExternRef(Some(first))]),
        "seed local and imported tables",
    );
    assert!(matches!(
        must_ok(instance.invoke_by_name("get_local", &[]), "get local table").as_slice(),
        [Val::ExternRef(Some(value))] if *value == first
    ));
    assert!(shared.get(1) == Ok(Some(Val::ExternRef(Some(first)))));

    must_ok(
        instance.invoke_by_name("copy_local_to_shared", &[]),
        "copy between externref tables",
    );
    assert!(shared.get(0) == Ok(Some(Val::ExternRef(Some(first)))));

    assert!(matches!(
        must_ok(
            instance.invoke_by_name("grow_local", &[Val::ExternRef(Some(second)), Val::I32(2)]),
            "grow externref table"
        )
        .as_slice(),
        [Val::I32(3)]
    ));
    let local = must_ok(
        instance.exported_table_handle("local"),
        "export local externref table",
    )
    .expect("local table export");
    assert!(local.element_type() == ValueType::ExternRef);
    assert!(local.len() == 5);
    assert!(local.get(3) == Ok(Some(Val::ExternRef(Some(second)))));
    assert!(local.get(4) == Ok(Some(Val::ExternRef(Some(second)))));

    must_ok(
        instance.invoke_by_name("fill_local", &[Val::ExternRef(Some(first))]),
        "fill externref table",
    );
    assert!(local.get(1) == Ok(Some(Val::ExternRef(Some(first)))));
    assert!(local.get(2) == Ok(Some(Val::ExternRef(Some(first)))));
    must_ok(
        instance.invoke_by_name("init_nulls", &[]),
        "initialize externref table from passive segment",
    );
    assert!(local.is_null(1) == Ok(Some(true)));
    assert!(local.is_null(2) == Ok(Some(true)));

    let mut sibling_module = must_ok(WasmModule::from_bytes(&bytes), "reload sibling fixture");
    must_ok(
        sibling_module.bind_table_import("host", "refs", &local),
        "bind exported externref table to sibling",
    );
    let mut sibling = must_ok(sibling_module.instantiate(), "instantiate sibling");
    drop(instance);
    must_ok(
        sibling.invoke_by_name("seed", &[Val::ExternRef(Some(second))]),
        "mutate exported table after provider drop",
    );
    assert!(local.get(1) == Ok(Some(Val::ExternRef(Some(second)))));

    let funcref = must_ok(store.create_table(2, Some(6)), "create funcref table");
    let mut wrong = must_ok(WasmModule::from_bytes(&bytes), "reload fixture");
    assert!(matches!(
        wrong.bind_table_import("host", "refs", &funcref),
        Err(WasmError::Trap("table element type"))
    ));
    assert!(matches!(
        shared.set(0, Val::FuncRef(None)),
        Err(WasmError::Trap("table element type"))
    ));
}
