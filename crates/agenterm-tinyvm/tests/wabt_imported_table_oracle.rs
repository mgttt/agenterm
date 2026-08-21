use std::path::PathBuf;

use agenterm_tinyvm::{Limits, WasmError, WasmModule};

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
