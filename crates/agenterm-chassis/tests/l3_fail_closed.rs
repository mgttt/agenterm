use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use agenterm_chassis::CELLS;

#[path = "../src/loader/mod.rs"]
mod loader;

#[test]
fn valid_l3_reaches_presenter_with_checked_metadata() {
    let fixture = Fixture::new("valid");
    write_valid_image(fixture.path());

    let metadata = loader::load_then(fixture.path(), |image| -> Result<_, &'static str> {
        Ok((image.l3_name().to_string(), image.capability_count()))
    })
    .expect("valid L3 reaches presenter");

    assert_eq!(metadata, ("l3.fail-closed.test".to_string(), 1));
}

#[test]
fn missing_l3_manifest_fails_before_presenter_or_host_callback() {
    let fixture = Fixture::new("missing");
    write_valid_image(fixture.path());
    fs::remove_file(fixture.path().join("l3/app.json")).expect("remove L3 manifest");

    assert_rejected_without_callbacks(fixture.path());
}

#[test]
fn tampered_l3_manifest_fails_before_presenter_or_host_callback() {
    let fixture = Fixture::new("tampered");
    write_valid_image(fixture.path());
    fs::write(fixture.path().join("l3/app.json"), b"{\"schema\":1").expect("tamper L3 manifest");

    assert_rejected_without_callbacks(fixture.path());
}

#[test]
fn malicious_l3_entry_fails_before_presenter_or_host_callback() {
    for (label, native_door) in [
        ("dlcall", "dlcall"),
        ("libc", "libc.so.6"),
        ("kernel", "kernel32.dll"),
    ] {
        let fixture = Fixture::new(label);
        write_valid_image(fixture.path());
        fs::write(
            fixture.path().join("l3/entry.txt"),
            format!("native entry: {native_door}"),
        )
        .expect("malicious L3 entry");

        assert_rejected_without_callbacks(fixture.path());
    }
}

fn assert_rejected_without_callbacks(root: &Path) {
    let presenter_calls = Cell::new(0);
    let host_calls = Cell::new(0);

    let result = loader::load_then(root, |_image| -> Result<(), &'static str> {
        presenter_calls.set(presenter_calls.get() + 1);
        host_calls.set(host_calls.get() + 1);
        Ok(())
    });

    assert!(matches!(result, Err(loader::LoadThenError::Image(_))));
    assert_eq!(presenter_calls.get(), 0, "invalid L3 reached presenter");
    assert_eq!(host_calls.get(), 0, "invalid L3 reached host callback");
}

fn write_valid_image(root: &Path) {
    for cell in CELLS {
        let cell_root = root.join("l1").join(cell);
        fs::create_dir_all(&cell_root).expect("L1 cell");
        fs::write(cell_root.join("loader"), format!("frozen-{cell}")).expect("frozen loader");
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
            "name": "l3.fail-closed.test",
            "capabilities": ["tabs.active"],
        })
        .to_string(),
    )
    .expect("L3 manifest");

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

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agenterm-chassis-l3-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
