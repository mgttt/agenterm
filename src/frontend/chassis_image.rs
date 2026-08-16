//! Shared workbench loader for a pre-composed Chassis-L1/L2/L3 image.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use agenterm_chassis::bytecode::{L2Source, Program, assemble};
use agenterm_chassis::l2_dispatch::{Dispatcher, HostCallback};

const MAX_NATIVE_LOADER_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct ImageIdentity {
    #[serde(default)]
    native_cell: Option<String>,
    l1_sha256: std::collections::BTreeMap<String, String>,
}

#[derive(Debug)]
pub(crate) struct LoadedChassisImage {
    pub(crate) root: PathBuf,
    pub(crate) native_loader: PathBuf,
    pub(crate) l3_name: String,
    active_tab_program: Program,
    host_abi: String,
    declared_capabilities: Vec<String>,
}

static LOADED_IMAGE: OnceLock<LoadedChassisImage> = OnceLock::new();

pub(crate) fn load_selected_image(
    root: Option<&Path>,
) -> Result<Option<&'static LoadedChassisImage>, String> {
    let Some(root) = root else {
        return Ok(None);
    };
    if let Some(loaded) = LOADED_IMAGE.get() {
        if loaded.root == root {
            return Ok(Some(loaded));
        }
        return Err("a different chassis image is already loaded".to_owned());
    }
    let loaded = load_image(root)?;
    LOADED_IMAGE
        .set(loaded)
        .map_err(|_| "chassis image initialization raced".to_owned())?;
    Ok(LOADED_IMAGE.get())
}

pub(crate) fn load_image(root: &Path) -> Result<LoadedChassisImage, String> {
    if !root.is_dir() {
        return Err(format!(
            "chassis image is not an installed directory: {}; extract a Candidate .tgz before launch",
            root.display()
        ));
    }
    agenterm_chassis::check_product_image(root)
        .map_err(|error| format!("chassis image check failed: {error}"))?;
    let native_cell = agenterm_platform::chassis_loader::native_cell()
        .ok_or_else(|| "this OS/ISA has no Chassis-L1 loader cell".to_owned())?;
    let native_loader = root.join("l1").join(native_cell).join("loader");
    if !native_loader.is_file() {
        return Err(format!(
            "chassis image lacks native loader cell {native_cell}"
        ));
    }
    validate_native_loader(root, native_cell, &native_loader)?;
    let app = agenterm_chassis::load_app(&root.join("l3/app.json"))
        .map_err(|error| format!("cannot load chassis L3 manifest: {error}"))?;
    let host_abi = std::fs::read_to_string(root.join("l2/host-abi.json"))
        .map_err(|error| format!("cannot read chassis L2 Host ABI: {error}"))?;
    Dispatcher::from_host_abi_json(&host_abi, &app.capabilities, ValidationOnlyHost)
        .map_err(|error| format!("cannot validate chassis L2 Host ABI: {error}"))?;
    let source: L2Source = serde_json::from_slice(
        &std::fs::read(root.join("l2/programs/active-tab.json"))
            .map_err(|error| format!("cannot read chassis L2 active-tab program: {error}"))?,
    )
    .map_err(|error| format!("cannot parse chassis L2 active-tab program: {error}"))?;
    let active_tab_program = assemble(&source, Some(&app.capabilities))
        .map_err(|error| format!("cannot assemble chassis L2 active-tab program: {error}"))?;
    Ok(LoadedChassisImage {
        root: root.to_path_buf(),
        native_loader,
        l3_name: app.name,
        active_tab_program,
        host_abi,
        declared_capabilities: app.capabilities,
    })
}

/// Run the checked image's first-window L2 artifact against the live product host.
///
/// Callers invoke this only after their real IPC server and first PTY exist.
pub(crate) fn eval_active_tab<H: HostCallback>(
    image: &LoadedChassisImage,
    host: H,
) -> Result<(i64, H), String> {
    let mut dispatcher =
        Dispatcher::from_host_abi_json(&image.host_abi, &image.declared_capabilities, host)
            .map_err(|error| error.to_string())?;
    let value = agenterm_chassis::vm::run(
        &image.active_tab_program,
        &mut dispatcher,
        agenterm_chassis::vm::DEFAULT_MAX_STEPS,
    )
    .map_err(|error| error.to_string())?;
    Ok((value, dispatcher.into_host()))
}

struct ValidationOnlyHost;

impl HostCallback for ValidationOnlyHost {
    fn call(&mut self, _capability: &str, _parameters: &Value) -> Result<Value, String> {
        Err("validation-only host must not be called".to_owned())
    }
}

fn validate_native_loader(root: &Path, cell: &str, loader: &Path) -> Result<(), String> {
    let identity: ImageIdentity = serde_json::from_slice(
        &std::fs::read(root.join("manifest.json"))
            .map_err(|error| format!("cannot read chassis product manifest: {error}"))?,
    )
    .map_err(|error| format!("cannot parse chassis product identity: {error}"))?;
    let expected_sha = identity
        .l1_sha256
        .get(cell)
        .ok_or_else(|| format!("chassis manifest lacks SHA-256 for native loader cell {cell}"))?;
    if let Some(declared_cell) = identity.native_cell.as_deref()
        && declared_cell != cell
    {
        return Err(format!(
            "chassis manifest native cell {declared_cell} does not match this host {cell}"
        ));
    }
    if expected_sha.len() != 64
        || !expected_sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "chassis manifest has invalid SHA-256 for native loader cell {cell}"
        ));
    }

    let metadata = std::fs::symlink_metadata(loader)
        .map_err(|error| format!("cannot inspect native chassis loader: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("native chassis loader must not be a symbolic link".to_owned());
    }
    if metadata.len() == 0 || metadata.len() > MAX_NATIVE_LOADER_BYTES {
        return Err(format!(
            "native chassis loader size must be 1..{MAX_NATIVE_LOADER_BYTES} bytes"
        ));
    }

    let bytes = std::fs::read(loader)
        .map_err(|error| format!("cannot read native chassis loader: {error}"))?;
    agenterm_platform::chassis_loader::validate_executable(loader, &bytes)
        .map_err(|error| format!("native chassis loader for cell {cell}: {error}"))?;
    let actual_sha = sha256_hex(&bytes);
    if &actual_sha != expected_sha {
        return Err(format!(
            "native chassis loader SHA-256 mismatch for cell {cell}"
        ));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenterm_chassis::CELLS;
    use std::fs;

    fn write_image(root: &Path, l3_note: Option<&str>) {
        let mut hashes = serde_json::Map::new();
        for cell in CELLS {
            let dir = root.join("l1").join(cell);
            fs::create_dir_all(&dir).expect("cell");
            let bytes = executable_bytes(cell);
            let loader = dir.join("loader");
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
            include_str!("../../crates/agenterm-chassis/l2/host-abi.json"),
        )
        .expect("abi");
        fs::write(
            root.join("l2/programs/active-tab.json"),
            include_str!("../../crates/agenterm-chassis/l2/programs/active-tab.json"),
        )
        .expect("program");
        fs::create_dir_all(root.join("l3")).expect("l3");
        let note = l3_note.unwrap_or("");
        fs::write(
            root.join("l3/app.json"),
            format!(
                r#"{{"schema":1,"name":"workbench","capabilities":["tabs.active"],"note":"{note}"}}"#
            ),
        )
        .expect("app");
        let manifest = serde_json::json!({
            "schema": 1,
            "compile": false,
            "invokes_cargo": false,
            "cells": CELLS,
            "native_cell": null,
            "l1_sha256": hashes,
        });
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("manifest json"),
        )
        .expect("manifest");
    }

    fn executable_bytes(cell: &str) -> Vec<u8> {
        let mut bytes = if Some(cell) == agenterm_platform::chassis_loader::native_cell() {
            agenterm_platform::chassis_loader::native_executable_header().to_vec()
        } else {
            b"non-native-loader".to_vec()
        };
        bytes.extend_from_slice(format!("thin-loader-{cell}").as_bytes());
        bytes
    }

    fn make_executable(path: &Path) {
        agenterm_platform::chassis_loader::make_executable(path).expect("executable");
    }

    #[test]
    fn loads_native_cell_from_composed_image() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_image(tmp.path(), None);
        let image = load_image(tmp.path()).expect("load");
        assert!(image.native_loader.is_file());
        assert_eq!(image.l3_name, "workbench");
    }

    #[test]
    fn fails_closed_when_l3_names_native_library() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_image(tmp.path(), Some("libc.so.6"));
        let error = load_image(tmp.path()).expect_err("forbidden L3");
        assert!(error.contains("libc.so.6"), "{error}");
    }
}
