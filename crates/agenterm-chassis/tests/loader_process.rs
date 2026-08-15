use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use agenterm_chassis::CELLS;

#[test]
fn usage_errors_exit_two() {
    let missing = Command::new(loader()).output().expect("run loader");
    assert_eq!(missing.status.code(), Some(2));

    let extra = Command::new(loader())
        .args(["one", "two"])
        .output()
        .expect("run loader");
    assert_eq!(extra.status.code(), Some(2));
}

#[test]
fn missing_or_legacy_fat_image_exits_three_without_fallback() {
    let fixture = Fixture::new("missing");
    let missing = run(fixture.path());
    assert_image_rejected(&missing);

    fs::write(fixture.path(), b"legacy fat workbench archive").expect("legacy archive");
    let archive = run(fixture.path());
    assert_image_rejected(&archive);
}

#[test]
fn native_cell_is_rejected_before_native_presentation() {
    let fixture = Fixture::new("native-cell");
    write_valid_image(fixture.path());
    let mut manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fixture.path().join("manifest.json")).expect("product manifest"),
    )
    .expect("parse product manifest");
    manifest["native_cell"] = serde_json::Value::String("win-aarch64".to_string());
    fs::write(
        fixture.path().join("manifest.json"),
        serde_json::to_vec(&manifest).expect("serialize manifest"),
    )
    .expect("wrong native cell");

    assert_image_rejected(&run(fixture.path()));
}

#[test]
fn tampered_manifest_is_rejected_before_native_presentation() {
    let fixture = Fixture::new("tampered");
    write_valid_image(fixture.path());
    fs::write(fixture.path().join("l3/app.json"), b"{\"schema\":1").expect("tamper manifest");

    assert_image_rejected(&run(fixture.path()));
}

fn loader() -> &'static str {
    env!("CARGO_BIN_EXE_agenterm-chassis-loader")
}

fn run(root: &Path) -> Output {
    Command::new(loader())
        .arg(root)
        .output()
        .expect("run loader")
}

fn assert_image_rejected(output: &Output) {
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("agenterm-chassis-loader:"), "{stderr}");
    assert!(!stderr.contains("native presentation failed"), "{stderr}");
}

fn write_valid_image(root: &Path) {
    for cell in CELLS {
        let cell_root = root.join("l1").join(cell);
        fs::create_dir_all(&cell_root).expect("L1 cell");
        fs::write(cell_root.join("loader"), format!("frozen-{cell}")).expect("L1 loader");
    }
    fs::create_dir_all(root.join("l2")).expect("L2");
    fs::write(
        root.join("l2/host-abi.json"),
        include_str!("../l2/host-abi.json"),
    )
    .expect("Host ABI");
    fs::create_dir_all(root.join("l3")).expect("L3");
    fs::write(
        root.join("l3/app.json"),
        serde_json::json!({
            "schema": 1,
            "name": "loader.process.test",
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
            "native_cell": agenterm_chassis::native_cell(),
        })
        .to_string(),
    )
    .expect("product manifest");
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agenterm-loader-process-{label}-{}-{:?}",
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
        if self.0.is_dir() {
            let _ = fs::remove_dir_all(&self.0);
        } else {
            let _ = fs::remove_file(&self.0);
        }
    }
}
