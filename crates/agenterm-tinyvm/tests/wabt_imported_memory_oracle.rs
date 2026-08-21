use std::path::PathBuf;

use agenterm_tinyvm::{Limits, Val, WasmError, WasmMemory, WasmModule};

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
#[ignore = "run through smoke-wabt-imported-memory.sh with an independently compiled fixture"]
fn wabt_compiled_imported_memory_matches_tinyvm() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_IMPORTED_MEMORY_WASM")
            .expect("TINYVM_WABT_IMPORTED_MEMORY_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced wasm");
    let provider_path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_EXPORTED_MEMORY_WASM")
            .expect("TINYVM_WABT_EXPORTED_MEMORY_WASM is set by the smoke script"),
    );
    let provider_bytes = std::fs::read(provider_path).expect("read WABT-produced provider wasm");

    let unbound = must_ok(WasmModule::from_bytes(&bytes), "load unbound module");
    assert!(matches!(
        unbound.instantiate(),
        Err(WasmError::Trap("unbound imported memory"))
    ));

    let provider_module = must_ok(
        WasmModule::from_bytes_with(
            &provider_bytes,
            Limits {
                max_memory_pages: 3,
                ..Limits::default()
            },
        ),
        "load memory provider",
    );
    let mut provider = must_ok(provider_module.instantiate(), "instantiate memory provider");
    let memory = must_ok(
        provider.exported_memory_handle("ram"),
        "resolve exported memory",
    )
    .expect("ram memory export");
    let open = || {
        let mut module = must_ok(
            WasmModule::from_bytes_with(
                &bytes,
                Limits {
                    max_memory_pages: 3,
                    ..Limits::default()
                },
            ),
            "load imported memory",
        );
        assert_eq!(module.memory_imports().len(), 1);
        assert_eq!(module.memory_imports()[0].module, "host");
        assert_eq!(module.memory_imports()[0].field, "ram");
        assert_eq!(module.memory_imports()[0].min, 1);
        assert_eq!(module.memory_imports()[0].max, Some(3));
        assert_eq!(module.memory_export_index("ram"), Some(0));
        must_ok(
            module.bind_memory_import("host", "ram", &memory),
            "bind shared memory",
        );
        must_ok(module.instantiate(), "instantiate imported memory")
    };

    let mut first = open();
    let mut second = open();
    assert_eq!(
        must_ok(first.exported_memory("ram"), "exported imported memory")
            .expect("exported imported memory")
            .len(),
        65_536
    );
    assert_eq!(
        only_i32(must_ok(first.invoke_by_name("run", &[]), "first run")),
        166
    );
    assert_eq!(
        only_i32(must_ok(second.invoke_by_name("run", &[]), "sibling run")),
        167
    );
    must_ok(memory.view_mut(), "host memory write")[0] = 70;
    assert_eq!(
        only_i32(must_ok(
            first.invoke_by_name("run", &[]),
            "host-updated run"
        )),
        171
    );
    assert_eq!(
        only_i32(must_ok(first.invoke_by_name("grow", &[]), "shared grow")),
        1
    );
    assert_eq!(
        only_i32(must_ok(second.invoke_by_name("size", &[]), "sibling size")),
        2
    );
    assert_eq!(memory.pages(), 2);
    assert_eq!(
        must_ok(provider.exported_memory("ram"), "provider memory")
            .expect("provider memory")
            .len(),
        2 * 65_536
    );

    let wrong_min = must_ok(WasmMemory::new(0, Some(3)), "small memory");
    let mut module = must_ok(WasmModule::from_bytes(&bytes), "reload for wrong minimum");
    assert!(matches!(
        module.bind_memory_import("host", "ram", &wrong_min),
        Err(WasmError::Trap("memory binding limits"))
    ));
    let wrong_max = must_ok(WasmMemory::new(1, None), "unbounded memory");
    assert!(matches!(
        module.bind_memory_import("host", "ram", &wrong_max),
        Err(WasmError::Trap("memory binding limits"))
    ));
}

#[test]
#[ignore = "run through smoke-wabt-imported-memory.sh with an independently compiled fixture"]
fn aliased_import_indices_keep_one_memory_identity() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_IMPORTED_MEMORY_ALIAS_WASM")
            .expect("TINYVM_WABT_IMPORTED_MEMORY_ALIAS_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced alias wasm");
    let memory = must_ok(WasmMemory::new(1, Some(3)), "allocate aliased memory");
    let mut module = must_ok(
        WasmModule::from_bytes_with(
            &bytes,
            Limits {
                max_memory_pages: 1,
                ..Limits::default()
            },
        ),
        "load alias module",
    );
    must_ok(
        module.bind_memory_import("host", "a", &memory),
        "bind alias a",
    );
    must_ok(
        module.bind_memory_import("host", "b", &memory),
        "bind alias b",
    );
    let mut instance = must_ok(module.instantiate(), "instantiate aliased imports once");

    let host_read = must_ok(memory.view(), "hold host read view");
    assert!(matches!(
        instance.invoke_by_name("overlap", &[]),
        Err(WasmError::Trap("memory is already borrowed"))
    ));
    drop(host_read);

    assert_eq!(
        only_i32(must_ok(
            instance.invoke_by_name("overlap", &[]),
            "overlapping aliased copy"
        )),
        593
    );
    assert_eq!(
        &must_ok(memory.view(), "inspect alias result")[..6],
        b"aabcdf"
    );
}
