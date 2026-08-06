//! End-to-end rh AOT qualification using the canonical fixture.

#[test]
fn fixture_pack_qualifies_on_host() {
    let dir = std::env::temp_dir().join(format!("agenterm-rh-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = include_str!("../fixtures/rh/entry.rh");
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify");
    assert_eq!(receipt.entry_value, 42);
    assert_eq!(receipt.cc_line_count, 2);
    assert_eq!(receipt.api_version, 1);
    let receipt_path = dir.join("qualification.json");
    agenterm_rh::write_receipt(&receipt_path, &receipt).expect("write");
    let loaded = agenterm_rh::RhPack::load(&dir).expect("load");
    assert_eq!(loaded.entry_value(), 42);
    assert_eq!(
        loaded.cc_lines(),
        vec!["rh-aot fixture".to_owned(), "machine-native".to_owned()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fixture_source_hash_is_stable() {
    let source = include_str!("../fixtures/rh/entry.rh");
    let a = agenterm_rh::hash_bytes(source.as_bytes());
    let b = agenterm_rh::hash_bytes(source.as_bytes());
    assert_eq!(a, b);
    assert!(!a.is_empty());
}

#[test]
fn fleet_fixture_qualifies_and_calls_host_shim() {
    let dir = std::env::temp_dir().join(format!("agenterm-rh-fleet-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = include_str!("../fixtures/rh/fleet.rh");
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify");
    assert_eq!(receipt.entry_value, 11);
    assert_eq!(receipt.cc_line_count, 2);
    let native = dir.join(format!("pack.{}", agenterm_rh::compile::native_extension()));
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let calls_for_bridge = std::sync::Arc::clone(&calls);
    let value = agenterm::script_rh_host::call_pack_entry_with_fleet(
        &native,
        Box::new(move |operation_id, params| {
            calls_for_bridge
                .lock()
                .expect("calls")
                .push((operation_id.to_owned(), params.to_owned()));
            Ok("{\"operation_id\":\"protocol.info\"}".to_owned())
        }),
    )
    .expect("entry");
    assert_eq!(value, 11);
    let recorded = calls.lock().expect("calls");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "protocol.info");
    agenterm::script_rh_host::clear_fleet_bridge();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn try_catch_fixture_qualifies_with_native_catch() {
    let dir = std::env::temp_dir().join(format!("agenterm-rh-try-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = include_str!("../fixtures/rh/try-catch.rh");
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify");
    assert_eq!(receipt.entry_value, 99);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn try_ok_fixture_qualifies_without_catch() {
    let dir = std::env::temp_dir().join(format!("agenterm-rh-try-ok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = include_str!("../fixtures/rh/try-ok.rh");
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify");
    assert_eq!(receipt.entry_value, 42);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn while_fixture_qualifies_with_native_loop() {
    let dir = std::env::temp_dir().join(format!("agenterm-rh-while-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = include_str!("../fixtures/rh/while.rh");
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify");
    assert_eq!(receipt.entry_value, 42);
    assert_eq!(receipt.cc_line_count, 2);
    let loaded = agenterm_rh::RhPack::load(&dir).expect("load");
    assert_eq!(loaded.entry_value(), 42);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stdlib_fixture_qualifies_with_host_eval() {
    let dir = std::env::temp_dir().join(format!("agenterm-rh-stdlib-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let source = include_str!("../fixtures/rh/stdlib.rh");
    let receipt = agenterm_rh::qualify_pack_dir(source, &dir).expect("qualify");
    assert_eq!(receipt.entry_value, 42);
    let native = dir.join(format!("pack.{}", agenterm_rh::compile::native_extension()));
    let value = agenterm::script_rh_host::call_pack_entry_with_host(&native, None).expect("entry");
    assert_eq!(value, 42);
    let _ = std::fs::remove_dir_all(&dir);
}
