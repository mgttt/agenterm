use std::cell::Cell;
use std::fs;
use std::path::Path;

use agenterm_chassis::CELLS;

#[path = "../src/loader/mod.rs"]
mod loader;

#[cfg(feature = "loader")]
#[test]
fn native_presenter_contract_compiles_without_opening_a_window() {
    let presenter = loader::present_image;
    let _ = presenter;
}

#[test]
fn valid_composed_image_reaches_headless_presenter() {
    let root = test_root("valid");
    write_layout(&root, "tabs.list");
    let called = Cell::new(false);

    let name = loader::load_then(&root, |image| -> Result<String, &'static str> {
        called.set(true);
        assert_eq!(image.capability_count(), 1);
        Ok(image.l3_name().to_string())
    })
    .expect("valid image");

    assert!(called.get());
    assert_eq!(name, "loader.test");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn native_door_in_l3_fails_before_presenter() {
    for forbidden in ["dlcall", "libSystem.B.dylib", "libc.so.6", "kernel32.dll"] {
        let root = test_root(forbidden.replace('.', "-").as_str());
        write_layout(&root, "tabs.list");
        fs::write(
            root.join("l3/native-door.txt"),
            format!("forbidden native door: {forbidden}"),
        )
        .expect("native-door fixture");
        let called = Cell::new(false);

        let result = loader::load_then(&root, |_image| -> Result<(), &'static str> {
            called.set(true);
            Ok(())
        });

        assert!(
            matches!(result, Err(loader::LoadThenError::Image(_))),
            "{forbidden}: {result:?}"
        );
        assert!(!called.get(), "presenter ran for {forbidden}");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn undeclared_l3_capability_fails_before_presenter() {
    let root = test_root("unknown-capability");
    write_layout(&root, "not.in.host.abi");
    let called = Cell::new(false);

    let result = loader::load_then(&root, |_image| -> Result<(), &'static str> {
        called.set(true);
        Ok(())
    });

    assert!(matches!(result, Err(loader::LoadThenError::Image(_))));
    assert!(!called.get());
    let _ = fs::remove_dir_all(root);
}

fn write_layout(root: &Path, capability: &str) {
    for cell in CELLS {
        let cell_root = root.join("l1").join(cell);
        fs::create_dir_all(&cell_root).expect("L1 cell");
        fs::write(cell_root.join("loader"), format!("frozen-{cell}")).expect("loader");
    }
    fs::create_dir_all(root.join("l2")).expect("L2");
    fs::write(
        root.join("l2/host-abi.json"),
        include_str!("../l2/host-abi.json"),
    )
    .expect("host ABI");
    fs::create_dir_all(root.join("l3")).expect("L3");
    fs::write(
        root.join("l3/app.json"),
        serde_json::json!({
            "schema": 1,
            "name": "loader.test",
            "capabilities": [capability],
        })
        .to_string(),
    )
    .expect("L3 app");
    fs::write(
        root.join("manifest.json"),
        serde_json::json!({
            "schema": 1,
            "compile": false,
            "invokes_cargo": false,
            "cells": CELLS,
        })
        .to_string(),
    )
    .expect("product manifest");
}

fn test_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "chassis-loader-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}
