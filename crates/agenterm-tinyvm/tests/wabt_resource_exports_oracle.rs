use std::path::PathBuf;

use agenterm_tinyvm::{Val, WasmError, WasmModule};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

#[test]
#[ignore = "run through smoke-wabt-resource-exports.sh with an independently compiled fixture"]
fn wabt_compiled_resource_exports_match_tinyvm() {
    let path = PathBuf::from(
        std::env::var_os("TINYVM_WABT_RESOURCE_EXPORTS_WASM")
            .expect("TINYVM_WABT_RESOURCE_EXPORTS_WASM is set by the smoke script"),
    );
    let bytes = std::fs::read(path).expect("read WABT-produced wasm");
    let module = must_ok(WasmModule::from_bytes(&bytes), "load resource exports");
    assert_eq!(module.table_export_index("dispatch"), Some(0));
    assert_eq!(module.memory_export_index("ram"), Some(0));
    assert_eq!(module.global_export_index("counter"), Some(0));
    assert_eq!(module.global_export_index("fixed"), Some(1));

    let mut instance = must_ok(module.instantiate(), "instantiate resource exports");
    assert_eq!(instance.exported_table_elements("dispatch"), Some(2));
    assert_eq!(
        must_ok(instance.exported_memory("ram"), "exported ram").map(|memory| memory[0]),
        Some(b'A')
    );
    must_ok(instance.exported_memory_mut("ram"), "exported ram mut").expect("exported ram")[1] =
        b'B';
    must_ok(
        instance.set_exported_global("counter", Val::I32(11)),
        "set exported counter",
    );
    assert!(matches!(
        instance.exported_global("fixed"),
        Some(Val::I64(9))
    ));
    assert!(matches!(
        must_ok(instance.invoke_by_name("read", &[]), "read exports").as_slice(),
        [Val::I32(76)]
    ));
}
