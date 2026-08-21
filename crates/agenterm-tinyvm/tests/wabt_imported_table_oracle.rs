use std::path::PathBuf;

use agenterm_tinyvm::{Limits, Val, WasmError, WasmModule, WasmTable};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
#[ignore = "run through smoke-wabt-imported-table.sh with an independently compiled fixture"]
fn wabt_compiled_imported_table_decodes_in_standard_index_space() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_IMPORTED_TABLE_WASM")
            .expect("TINYVM_WABT_IMPORTED_TABLE_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced wasm");
    let module = must_ok(
        WasmModule::from_bytes_with(
            &bytes,
            Limits {
                max_table_elems: 2,
                ..Limits::default()
            },
        ),
        "load imported table module",
    );

    assert_eq!(module.table_imports().len(), 1);
    let import = &module.table_imports()[0];
    assert_eq!(import.module, "host");
    assert_eq!(import.field, "dispatch");
    assert_eq!(import.min, 1);
    assert_eq!(import.max, Some(3));
    assert_eq!(module.table_export_index("dispatch"), Some(0));
    assert_eq!(module.table_export_index("local"), Some(1));
    assert!(matches!(
        module.instantiate(),
        Err(WasmError::Trap("unbound imported table"))
    ));

    let table = must_ok(WasmTable::new(1, Some(3)), "create host table");
    let open = || {
        let mut module = must_ok(
            WasmModule::from_bytes_with(
                &bytes,
                Limits {
                    max_table_elems: 2,
                    ..Limits::default()
                },
            ),
            "reload imported table module",
        );
        must_ok(
            module.bind_table_import("host", "dispatch", &table),
            "bind host table",
        );
        must_ok(module.instantiate(), "instantiate bound table")
    };
    let mut first = open();
    assert!(matches!(
        must_ok(first.invoke_by_name("run", &[]), "first indirect call").as_slice(),
        [Val::I32(1)]
    ));
    let mut second = open();
    assert!(matches!(
        first.invoke_by_name("run", &[]),
        Err(WasmError::Trap("cross-instance funcref"))
    ));
    assert!(matches!(
        must_ok(second.invoke_by_name("run", &[]), "second indirect call").as_slice(),
        [Val::I32(1)]
    ));
    assert_eq!(
        must_ok(table.is_null(0), "host table visibility"),
        Some(false)
    );

    assert!(matches!(
        WasmModule::from_bytes_with(
            &bytes,
            Limits {
                max_table_elems: 1,
                ..Limits::default()
            }
        ),
        Err(WasmError::Trap("table size"))
    ));
}

#[test]
#[ignore = "run through smoke-wabt-imported-table.sh with an independently compiled fixture"]
fn aliased_import_indices_keep_one_table_identity() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_IMPORTED_TABLE_ALIAS_WASM")
            .expect("TINYVM_WABT_IMPORTED_TABLE_ALIAS_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced alias wasm");
    let table = must_ok(WasmTable::new(6, Some(6)), "create aliased host table");
    let mut module = must_ok(
        WasmModule::from_bytes_with(
            &bytes,
            Limits {
                max_table_elems: 6,
                ..Limits::default()
            },
        ),
        "load aliased table module",
    );
    must_ok(module.bind_table_import("host", "a", &table), "bind a");
    must_ok(module.bind_table_import("host", "b", &table), "bind b");
    let mut instance = must_ok(module.instantiate(), "instantiate aliased table imports");
    assert_eq!(instance.table_elements(), 6);
    assert!(matches!(
        must_ok(instance.invoke_by_name("overlap", &[]), "overlapping copy").as_slice(),
        [Val::I32(16)]
    ));
}
