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
