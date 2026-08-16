#[allow(dead_code)]
#[path = "../src/frontend/chassis_image.rs"]
mod chassis_image;

use agenterm_chassis::CELLS;
use sha2::{Digest as _, Sha256};
use std::cell::Cell;
use std::fs;
use std::path::Path;

fn executable_bytes(cell: &str) -> Vec<u8> {
    let mut bytes = match cell.split_once('-').map(|(os, _)| os) {
        Some("win") => b"MZ".to_vec(),
        Some("lnx") => b"\x7fELF".to_vec(),
        Some("osx") => vec![0xcf, 0xfa, 0xed, 0xfe],
        _ => unreachable!("canonical cell"),
    };
    bytes.extend_from_slice(format!("thin-loader-{cell}").as_bytes());
    bytes
}

fn write_installed_image(root: &Path) {
    let mut hashes = serde_json::Map::new();
    for cell in CELLS {
        let directory = root.join("l1").join(cell);
        fs::create_dir_all(&directory).expect("cell directory");
        let loader = directory.join("loader");
        let bytes = executable_bytes(cell);
        fs::write(&loader, &bytes).expect("loader");
        make_executable(&loader);
        hashes.insert(
            cell.to_owned(),
            serde_json::Value::String(sha256_hex(&bytes)),
        );
    }
    fs::create_dir_all(root.join("l2/programs")).expect("l2");
    fs::write(
        root.join("l2/host-abi.json"),
        include_str!("../crates/agenterm-chassis/l2/host-abi.json"),
    )
    .expect("host ABI");
    fs::write(
        root.join("l2/programs/active-tab.json"),
        include_str!("../crates/agenterm-chassis/l2/programs/active-tab.json"),
    )
    .expect("active-tab program");
    fs::create_dir_all(root.join("l3")).expect("l3");
    fs::write(
        root.join("l3/app.json"),
        r#"{"schema":1,"name":"workbench","capabilities":["tabs.active"]}"#,
    )
    .expect("app manifest");
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "compile": false,
            "invokes_cargo": false,
            "cells": CELLS,
            "native_cell": null,
            "l1_sha256": hashes,
        }))
        .expect("product manifest"),
    )
    .expect("product manifest");
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("executable");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

fn start_after_image_check(root: &Path, callback: impl FnOnce()) -> Result<(), String> {
    chassis_image::load_image(root)?;
    callback();
    Ok(())
}

#[test]
fn both_host_adapters_share_fail_closed_load_before_start_boundary() {
    for adapter in [
        include_str!("../src/platform/adapters/unix/frontend/mod.rs"),
        include_str!("../src/platform/adapters/windows/frontend.rs"),
    ] {
        let load = adapter
            .find("chassis_image::load_selected_image")
            .expect("shared chassis loader call");
        let startup = adapter[load..]
            .find("attempt_gui_handoff")
            .expect("startup after chassis load");
        assert!(startup > 0);
    }

    let image = tempfile::tempdir().expect("image");
    write_installed_image(image.path());
    let calls = Cell::new(0);
    start_after_image_check(image.path(), || calls.set(calls.get() + 1)).expect("valid image");
    assert_eq!(calls.get(), 1, "valid image starts exactly once");

    let loader = image
        .path()
        .join("l1")
        .join(agenterm_chassis::native_cell())
        .join("loader");
    fs::write(loader, b"tampered-fat-workbench-archive").expect("tamper native loader");
    let calls = Cell::new(0);
    let error = start_after_image_check(image.path(), || calls.set(calls.get() + 1))
        .expect_err("tamper must fail closed");
    assert!(error.contains("executable image") || error.contains("SHA-256 mismatch"));
    assert_eq!(calls.get(), 0, "invalid image never starts");
}

#[test]
fn candidate_archive_path_is_not_mistaken_for_an_installed_image() {
    let candidate = tempfile::NamedTempFile::new().expect("candidate archive");
    let calls = Cell::new(0);
    let error = start_after_image_check(candidate.path(), || calls.set(calls.get() + 1))
        .expect_err("archive must be installed before launch");
    assert!(error.contains("not an installed directory"), "{error}");
    assert_eq!(calls.get(), 0);
}

#[test]
fn native_loader_hash_and_size_are_bound_before_start() {
    let image = tempfile::tempdir().expect("image");
    write_installed_image(image.path());
    let manifest = image.path().join("manifest.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).expect("manifest")).expect("json");
    value["l1_sha256"][agenterm_chassis::native_cell()] = serde_json::Value::String("0".repeat(64));
    fs::write(&manifest, serde_json::to_vec(&value).expect("json")).expect("manifest");
    let error = chassis_image::load_image(image.path()).expect_err("hash mismatch");
    assert!(error.contains("SHA-256 mismatch"), "{error}");

    write_installed_image(image.path());
    let loader = image
        .path()
        .join("l1")
        .join(agenterm_chassis::native_cell())
        .join("loader");
    fs::write(&loader, vec![b'x'; 2 * 1024 * 1024 + 1]).expect("oversize loader");
    let error = chassis_image::load_image(image.path()).expect_err("oversize");
    assert!(error.contains("size must be"), "{error}");
}

#[test]
fn installed_image_cannot_claim_a_different_native_cell() {
    let image = tempfile::tempdir().expect("image");
    write_installed_image(image.path());
    let manifest = image.path().join("manifest.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).expect("manifest")).expect("json");
    value["native_cell"] = serde_json::Value::String(
        CELLS
            .iter()
            .copied()
            .find(|cell| *cell != agenterm_chassis::native_cell())
            .expect("other cell")
            .to_owned(),
    );
    fs::write(&manifest, serde_json::to_vec(&value).expect("json")).expect("manifest");
    let error = chassis_image::load_image(image.path()).expect_err("wrong native cell");
    assert!(error.contains("does not match this host"), "{error}");
}
