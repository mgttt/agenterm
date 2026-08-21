use std::path::PathBuf;

use agenterm_tinyvm::{Limits, Val, WasmError, WasmGlobal, WasmModule, WasmStore, WasmTable};

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
    let provider_path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_EXPORTED_TABLE_WASM")
            .expect("TINYVM_WABT_EXPORTED_TABLE_WASM is set by the smoke script"),
    );
    let provider_bytes = std::fs::read(provider_path).expect("read WABT-produced provider wasm");
    let linked_consumer_path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_LINKED_TABLE_CONSUMER_WASM")
            .expect("TINYVM_WABT_LINKED_TABLE_CONSUMER_WASM is set by the smoke script"),
    );
    let linked_consumer_bytes =
        std::fs::read(linked_consumer_path).expect("read WABT-produced linked consumer wasm");
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

    let mut provider = must_ok(
        must_ok(
            WasmModule::from_bytes(&provider_bytes),
            "load table provider",
        )
        .instantiate(),
        "instantiate table provider",
    );
    let table = must_ok(
        provider.exported_table_handle("dispatch"),
        "resolve exported table",
    )
    .expect("dispatch table export");
    let mut linked_consumer = must_ok(
        WasmModule::from_bytes(&linked_consumer_bytes),
        "load linked table consumer",
    );
    must_ok(
        linked_consumer.bind_table_import("host", "dispatch", &table),
        "link provider table",
    );
    let mut linked_consumer = must_ok(
        linked_consumer.instantiate(),
        "instantiate linked table consumer",
    );
    drop(provider);
    assert!(matches!(
        must_ok(
            linked_consumer.invoke_by_name("run", &[]),
            "invoke provider function after handle drop",
        )
        .as_slice(),
        [Val::I32(42)]
    ));
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
        must_ok(
            first.invoke_by_name("run", &[]),
            "cross-instance indirect call"
        )
        .as_slice(),
        [Val::I32(1)]
    ));
    assert!(matches!(
        must_ok(second.invoke_by_name("run", &[]), "second indirect call").as_slice(),
        [Val::I32(2)]
    ));
    drop(second);
    assert!(matches!(
        must_ok(
            first.invoke_by_name("run", &[]),
            "store-owned function after public handle drop"
        )
        .as_slice(),
        [Val::I32(3)]
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

    let first_store = WasmStore::new();
    let second_store = WasmStore::new();
    let first_table = must_ok(first_store.create_table(6, Some(6)), "first table");
    let second_table = must_ok(second_store.create_table(6, Some(6)), "second table");
    let mut split_module = must_ok(
        WasmModule::from_bytes_with(
            &bytes,
            Limits {
                max_table_elems: 12,
                ..Limits::default()
            },
        ),
        "load split-store module",
    );
    must_ok(
        split_module.bind_table_import("host", "a", &first_table),
        "bind split a",
    );
    must_ok(
        split_module.bind_table_import("host", "b", &second_table),
        "bind split b",
    );
    assert!(matches!(
        split_module.instantiate(),
        Err(WasmError::Trap("table imports belong to different stores"))
    ));

    let shared_store = WasmStore::new();
    let first_table = must_ok(shared_store.create_table(6, Some(6)), "same-store a");
    let second_table = must_ok(shared_store.create_table(6, Some(6)), "same-store b");
    let mut same_store_module = must_ok(
        WasmModule::from_bytes_with(
            &bytes,
            Limits {
                max_table_elems: 12,
                ..Limits::default()
            },
        ),
        "load same-store module",
    );
    must_ok(
        same_store_module.bind_table_import("host", "a", &first_table),
        "bind same-store a",
    );
    must_ok(
        same_store_module.bind_table_import("host", "b", &second_table),
        "bind same-store b",
    );
    let same_store_instance = must_ok(
        same_store_module.instantiate(),
        "instantiate same-store tables",
    );
    assert_eq!(same_store_instance.table_elements(), 12);
}

#[test]
#[ignore = "run through smoke-wabt-imported-table.sh with an independently compiled fixture"]
fn cross_instance_cycles_use_the_store_trampoline() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_IMPORTED_TABLE_CYCLE_WASM")
            .expect("TINYVM_WABT_IMPORTED_TABLE_CYCLE_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced cycle wasm");
    let store = WasmStore::new();
    let table = must_ok(store.create_table(2, Some(2)), "create cycle table");
    let limits = Limits {
        max_steps: 100_000,
        max_call_depth: 5_000,
        max_activation_slots: 100_000,
        ..Limits::default()
    };
    let open = |slot: i32| {
        let mut module = must_ok(
            WasmModule::from_bytes_with(&bytes, limits),
            "load cycle module",
        );
        must_ok(
            module.bind_table_import("host", "dispatch", &table),
            "bind cycle table",
        );
        let slot = WasmGlobal::new(Val::I32(slot), false);
        must_ok(
            module.bind_global_import("host", "slot", &slot),
            "bind cycle slot",
        );
        must_ok(module.instantiate(), "instantiate cycle module")
    };
    let mut first = open(0);
    let second = open(1);
    drop(second);
    assert!(matches!(
        must_ok(
            first.invoke_by_name("run", &[Val::I32(4_000)]),
            "deep cross-instance cycle"
        )
        .as_slice(),
        [Val::I32(4_000)]
    ));
    assert_eq!(first.last_peak_call_depth(), 4_001);
    assert_eq!(first.last_peak_activation_slots(), 12_004);
}
