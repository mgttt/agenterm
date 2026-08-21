use std::path::PathBuf;

use agenterm_tinyvm::{Val, ValueType, WasmError, WasmModule};

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

fn fixture(variable: &str) -> Vec<u8> {
    let path = PathBuf::from(
        std::env::var_os(variable).unwrap_or_else(|| panic!("{variable} is set by smoke script")),
    );
    std::fs::read(path).expect("read WABT-produced wasm")
}

#[test]
#[ignore = "run through smoke-wabt-imported-functions.sh with independently compiled fixtures"]
fn wabt_compiled_exported_functions_link_across_instances() {
    let provider_bytes = fixture("TINYVM_WABT_EXPORTED_FUNCTIONS_WASM");
    let consumer_bytes = fixture("TINYVM_WABT_IMPORTED_FUNCTIONS_WASM");
    let relay_bytes = fixture("TINYVM_WABT_RELINKED_FUNCTION_WASM");

    let provider = must_ok(
        must_ok(WasmModule::from_bytes(&provider_bytes), "load provider").instantiate(),
        "instantiate provider",
    );
    let add = must_ok(
        provider.exported_function_handle("add"),
        "resolve add export",
    )
    .expect("add export");
    let sub = must_ok(
        provider.exported_function_handle("sub"),
        "resolve sub export",
    )
    .expect("sub export");
    let unary = must_ok(
        provider.exported_function_handle("unary"),
        "resolve unary export",
    )
    .expect("unary export");
    let mixed = must_ok(
        provider.exported_function_handle("mixed"),
        "resolve mixed export",
    )
    .expect("mixed export");
    let identity_ref = must_ok(
        provider.exported_function_handle("identity_ref"),
        "resolve identity_ref export",
    )
    .expect("identity_ref export");
    let answer_ref = provider
        .exported_global_handle("answer_ref")
        .expect("answer_ref global export");
    assert_eq!(add.parameter_count(), 2);
    assert_eq!(add.result_count(), 1);
    assert!(add.parameter_type(0) == Some(ValueType::I32));
    assert!(add.parameter_type(2).is_none());
    assert!(add.result_type(0) == Some(ValueType::I32));

    let mut mismatch = must_ok(WasmModule::from_bytes(&consumer_bytes), "load mismatch");
    assert!(matches!(
        mismatch.bind_function_import("provider", "add", &unary),
        Err(WasmError::Trap("function binding type"))
    ));
    let mut consumer = must_ok(WasmModule::from_bytes(&consumer_bytes), "load consumer");
    must_ok(
        consumer.bind_function_import("provider", "add", &add),
        "bind add",
    );
    must_ok(
        consumer.bind_function_import("provider", "sub", &sub),
        "bind sub",
    );
    must_ok(
        consumer.bind_function_import("provider", "mixed", &mixed),
        "bind mixed numeric function",
    );
    must_ok(
        consumer.bind_function_import("provider", "identity_ref", &identity_ref),
        "bind reference identity",
    );
    must_ok(
        consumer.bind_global_import("provider", "answer_ref", &answer_ref),
        "bind reference global",
    );
    let mut consumer = must_ok(consumer.instantiate(), "instantiate consumer");
    drop(provider);
    assert_eq!(
        only_i32(must_ok(consumer.invoke_by_name("run", &[]), "normal call")),
        42
    );
    assert_eq!(
        only_i32(must_ok(
            consumer.invoke_by_name("global_roundtrip", &[]),
            "store-owned global funcref roundtrip"
        )),
        43
    );
    assert_eq!(
        only_i32(must_ok(
            consumer.invoke_by_name("typed", &[]),
            "mixed numeric call"
        )),
        4
    );
    assert_eq!(
        only_i32(must_ok(
            consumer.invoke_by_name("ref_roundtrip", &[]),
            "store-owned funcref roundtrip"
        )),
        42
    );
    assert_eq!(
        only_i32(must_ok(
            consumer.invoke_by_name("tail", &[]),
            "foreign tail call"
        )),
        42
    );

    let reexport = must_ok(
        consumer.exported_function_handle("reexport"),
        "resolve re-export",
    )
    .expect("re-exported function");
    let mut relay = must_ok(WasmModule::from_bytes(&relay_bytes), "load relay");
    must_ok(
        relay.bind_function_import("relay", "function", &reexport),
        "bind re-export",
    );
    let mut relay = must_ok(relay.instantiate(), "instantiate relay");
    drop(consumer);
    assert_eq!(
        only_i32(must_ok(relay.invoke_by_name("run", &[]), "relay call")),
        42
    );

    let mut second_provider = must_ok(
        must_ok(
            WasmModule::from_bytes(&provider_bytes),
            "load second provider",
        )
        .instantiate(),
        "instantiate second provider",
    );
    let second_sub = must_ok(
        second_provider.exported_function_handle("sub"),
        "resolve second sub",
    )
    .expect("second sub export");
    let second_add = must_ok(
        second_provider.exported_function_handle("add"),
        "resolve second add",
    )
    .expect("second add export");
    let second_mixed = must_ok(
        second_provider.exported_function_handle("mixed"),
        "resolve second mixed",
    )
    .expect("second mixed export");
    let second_identity_ref = must_ok(
        second_provider.exported_function_handle("identity_ref"),
        "resolve second identity_ref",
    )
    .expect("second identity_ref export");
    let add_reference = must_ok(add.reference_value(), "export add as funcref");
    assert!(matches!(
        second_provider.invoke_by_name("identity_ref", &[add_reference]),
        Err(WasmError::Trap("funcref belongs to different store"))
    ));
    let stale_reference = {
        let stale_provider = must_ok(
            must_ok(
                WasmModule::from_bytes(&provider_bytes),
                "load stale-reference provider",
            )
            .instantiate(),
            "instantiate stale-reference provider",
        );
        let stale_add = must_ok(
            stale_provider.exported_function_handle("add"),
            "resolve stale-reference add",
        )
        .expect("stale-reference add export");
        must_ok(stale_add.reference_value(), "create stale funcref")
    };
    assert!(matches!(
        second_provider.invoke_by_name("identity_ref", &[stale_reference]),
        Err(WasmError::Trap("funcref belongs to different store"))
    ));
    let mut split_global = must_ok(
        WasmModule::from_bytes(&consumer_bytes),
        "load split-global consumer",
    );
    must_ok(
        split_global.bind_function_import("provider", "add", &second_add),
        "bind second-store add",
    );
    must_ok(
        split_global.bind_function_import("provider", "sub", &second_sub),
        "bind second-store sub",
    );
    must_ok(
        split_global.bind_function_import("provider", "mixed", &second_mixed),
        "bind second-store mixed",
    );
    must_ok(
        split_global.bind_function_import("provider", "identity_ref", &second_identity_ref),
        "bind second-store reference function",
    );
    must_ok(
        split_global.bind_global_import("provider", "answer_ref", &answer_ref),
        "bind first-store reference global",
    );
    assert!(matches!(
        split_global.instantiate(),
        Err(WasmError::Trap("global belongs to different store"))
    ));
    let mut split = must_ok(
        WasmModule::from_bytes(&consumer_bytes),
        "load split consumer",
    );
    must_ok(
        split.bind_function_import("provider", "add", &add),
        "bind first store",
    );
    must_ok(
        split.bind_function_import("provider", "sub", &second_sub),
        "bind second store",
    );
    must_ok(
        split.bind_function_import("provider", "mixed", &mixed),
        "bind first-store mixed function",
    );
    must_ok(
        split.bind_function_import("provider", "identity_ref", &identity_ref),
        "bind first-store reference function",
    );
    must_ok(
        split.bind_global_import("provider", "answer_ref", &answer_ref),
        "bind first-store reference global",
    );
    assert!(matches!(
        split.instantiate(),
        Err(WasmError::Trap(
            "function imports belong to different stores"
        ))
    ));
}
