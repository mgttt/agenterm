//! Composed image -> workbench selection -> L2 VM -> Host ABI black box.

#[allow(dead_code)]
#[path = "../src/frontend/chassis_image.rs"]
mod chassis_image;

#[allow(dead_code, unexpected_cfgs)]
#[path = "../crates/agenterm-chassis/src/loader/mod.rs"]
mod chassis_loader;

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agenterm_chassis::CELLS;
use agenterm_chassis::l2_dispatch::HostCallback;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

#[derive(Default)]
struct BoundaryCalls {
    window: Cell<usize>,
    pty: Cell<usize>,
    ipc: Cell<usize>,
    host: Rc<Cell<usize>>,
}

impl BoundaryCalls {
    fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.window.get(),
            self.pty.get(),
            self.ipc.get(),
            self.host.get(),
        )
    }
}

struct RecordingHost {
    calls: Rc<Cell<usize>>,
}

impl HostCallback for RecordingHost {
    fn call(&mut self, capability: &str, parameters: &Value) -> Result<Value, String> {
        assert_eq!(capability, "tabs.active");
        assert_eq!(parameters, &json!({}));
        self.calls.set(self.calls.get() + 1);
        Ok(json!(73))
    }
}

fn execute_selected_image(root: &Path, calls: &BoundaryCalls) -> Result<i64, String> {
    let selected = chassis_image::load_image(root)?;
    assert_eq!(
        selected.native_loader,
        root.join("l1")
            .join(agenterm_chassis::native_cell())
            .join("loader")
    );

    let image = chassis_loader::load_image(&selected.root).map_err(|error| error.to_string())?;
    let (value, _host) = image
        .eval_l2(
            "active-tab.json",
            RecordingHost {
                calls: Rc::clone(&calls.host),
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(value)
}

#[test]
fn composed_image_selects_native_loader_then_dispatches_host_abi_once() {
    let fixture = Fixture::new("valid");
    let calls = BoundaryCalls::default();

    assert_eq!(
        execute_selected_image(fixture.installed(), &calls).expect("end-to-end dispatch"),
        73
    );
    assert_eq!(calls.counts(), (0, 0, 0, 1));
}

#[test]
fn tampered_loader_fails_before_window_pty_ipc_or_host_callback() {
    let fixture = Fixture::new("tampered-loader");
    let native_loader = fixture
        .installed()
        .join("l1")
        .join(agenterm_chassis::native_cell())
        .join("loader");
    fs::write(native_loader, b"tampered-fat-workbench").expect("tamper loader");
    let calls = BoundaryCalls::default();

    let error = execute_selected_image(fixture.installed(), &calls).expect_err("reject tamper");
    assert!(
        error.contains("executable image") || error.contains("SHA-256 mismatch"),
        "{error}"
    );
    assert_eq!(calls.counts(), (0, 0, 0, 0));
}

#[test]
fn unknown_l2_capability_fails_before_window_pty_ipc_or_host_callback() {
    let fixture = Fixture::new("unknown-capability");
    fs::write(
        fixture.installed().join("l2/programs/active-tab.json"),
        json!({
            "caps": ["not.a.capability"],
            "ops": [["call", "not.a.capability"], ["halt"]],
        })
        .to_string(),
    )
    .expect("replace program");
    let calls = BoundaryCalls::default();

    let error = execute_selected_image(fixture.installed(), &calls)
        .expect_err("reject unknown L2 capability");
    assert!(error.contains("unknown cap name"), "{error}");
    assert_eq!(calls.counts(), (0, 0, 0, 0));
}

#[test]
fn fat_candidate_archive_fails_before_window_pty_ipc_or_host_callback() {
    let archive = tempfile::NamedTempFile::new().expect("fat archive");
    fs::write(archive.path(), b"fat-workbench-archive").expect("archive bytes");
    let calls = BoundaryCalls::default();

    let error = execute_selected_image(archive.path(), &calls).expect_err("reject archive");
    assert!(error.contains("not an installed directory"), "{error}");
    assert_eq!(calls.counts(), (0, 0, 0, 0));
}

struct Fixture {
    root: PathBuf,
    installed: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenterm-chassis-e2e-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        let staged = root.join("staged");
        let installed = root.join("installed");

        for cell in CELLS {
            let directory = staged.join("l1").join(cell);
            fs::create_dir_all(&directory).expect("L1 cell");
            let loader = directory.join("loader");
            fs::write(&loader, executable_bytes(cell)).expect("L1 loader");
            make_executable(&loader);
        }
        fs::create_dir_all(staged.join("l2/programs")).expect("L2 programs");
        fs::write(
            staged.join("l2/host-abi.json"),
            include_str!("../crates/agenterm-chassis/l2/host-abi.json"),
        )
        .expect("Host ABI");
        fs::write(
            staged.join("l2/programs/active-tab.json"),
            json!({
                "caps": ["tabs.active"],
                "ops": [["call", "tabs.active"], ["halt"]],
            })
            .to_string(),
        )
        .expect("L2 program");
        fs::create_dir_all(staged.join("l3")).expect("L3");
        fs::write(
            staged.join("l3/app.json"),
            json!({
                "schema": 1,
                "name": "chassis.e2e",
                "capabilities": ["tabs.active"],
            })
            .to_string(),
        )
        .expect("L3 manifest");

        agenterm_chassis::compose(&staged, &installed).expect("compose product image");
        let mut manifest: Value = serde_json::from_slice(
            &fs::read(installed.join("manifest.json")).expect("product manifest"),
        )
        .expect("product manifest JSON");
        manifest["l1_sha256"] = Value::Object(
            CELLS
                .iter()
                .map(|cell| {
                    let bytes = fs::read(installed.join("l1").join(cell).join("loader"))
                        .expect("installed loader");
                    ((*cell).to_owned(), Value::String(sha256_hex(&bytes)))
                })
                .collect(),
        );
        fs::write(
            installed.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("product manifest JSON"),
        )
        .expect("product manifest");

        Self { root, installed }
    }

    fn installed(&self) -> &Path {
        &self.installed
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

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
