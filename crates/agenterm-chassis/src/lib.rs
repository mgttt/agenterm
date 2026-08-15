//! Independent Chassis-L1 / L2 / L3 product image.
//!
//! This crate does not depend on the workbench `agenterm` package. Compose
//! copies frozen L1 loader bytes and validates that L3 only names L2
//! capabilities.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CELLS: [&str; 6] = [
    "win-x86_64",
    "win-aarch64",
    "lnx-x86_64",
    "lnx-aarch64",
    "osx-x86_64",
    "osx-aarch64",
];

const FORBIDDEN_IN_L3: [&str; 5] = [
    "libSystem.B.dylib",
    "libc.so.6",
    "kernel32.dll",
    "dlcall",
    "GetCurrentProcessId",
];

#[derive(Debug)]
pub enum ChassisError {
    Io(io::Error),
    Json(serde_json::Error),
    Check(String),
    Usage(String),
}

impl std::fmt::Display for ChassisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::Check(err) | Self::Usage(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ChassisError {}

impl From<io::Error> for ChassisError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for ChassisError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostAbi {
    pub schema: u32,
    pub version: u32,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppManifest {
    pub schema: u32,
    pub name: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductManifest {
    pub schema: u32,
    pub compile: bool,
    pub invokes_cargo: bool,
    pub cells: Vec<String>,
    pub native_cell: String,
}

pub fn native_cell() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "win-x86_64",
        ("windows", "aarch64") => "win-aarch64",
        ("linux", "x86_64") => "lnx-x86_64",
        ("linux", "aarch64") => "lnx-aarch64",
        ("macos", "x86_64") => "osx-x86_64",
        ("macos", "aarch64") => "osx-aarch64",
        _ => "unknown",
    }
}

pub fn bundled_l2_abi() -> Result<HostAbi, ChassisError> {
    let raw = include_str!("../l2/host-abi.json");
    Ok(serde_json::from_str(raw)?)
}

pub fn bundled_l3_app() -> Result<AppManifest, ChassisError> {
    let raw = include_str!("../l3/example-app.json");
    Ok(serde_json::from_str(raw)?)
}

pub fn load_abi(path: &Path) -> Result<HostAbi, ChassisError> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn load_app(path: &Path) -> Result<AppManifest, ChassisError> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn compose(from: &Path, out: &Path) -> Result<ProductManifest, ChassisError> {
    let l1 = from.join("l1");
    for cell in CELLS {
        let loader = l1.join(cell).join("loader");
        if !loader.is_file() {
            return Err(ChassisError::Check(format!(
                "missing frozen L1 loader {}",
                loader.display()
            )));
        }
    }
    let l2 = from.join("l2").join("host-abi.json");
    if !l2.is_file() {
        return Err(ChassisError::Check("missing l2/host-abi.json".into()));
    }
    let l3 = from.join("l3").join("app.json");
    if !l3.is_file() {
        return Err(ChassisError::Check("missing l3/app.json".into()));
    }
    check_layout(from)?;

    copy_tree(&from.join("l1"), &out.join("l1"))?;
    copy_tree(&from.join("l2"), &out.join("l2"))?;
    copy_tree(&from.join("l3"), &out.join("l3"))?;

    let manifest = ProductManifest {
        schema: 1,
        compile: false,
        invokes_cargo: false,
        cells: CELLS.iter().map(|cell| (*cell).to_string()).collect(),
        native_cell: native_cell().to_string(),
    };
    fs::create_dir_all(out)?;
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

pub fn check_layout(root: &Path) -> Result<(), ChassisError> {
    let abi = load_abi(&root.join("l2/host-abi.json"))?;
    let app = load_app(&root.join("l3/app.json"))?;
    let allowed: std::collections::BTreeSet<_> =
        abi.capabilities.iter().map(|cap| cap.id.as_str()).collect();
    for name in &app.capabilities {
        if !allowed.contains(name.as_str()) {
            return Err(ChassisError::Check(format!(
                "L3 capability `{name}` is not in the L2 host ABI"
            )));
        }
    }
    for entry in walk_files(&root.join("l3"))? {
        let text = fs::read_to_string(&entry)?;
        for needle in FORBIDDEN_IN_L3 {
            if text.contains(needle) {
                return Err(ChassisError::Check(format!(
                    "L3 file {} names native door `{needle}`",
                    entry.display()
                )));
            }
        }
    }
    for cell in CELLS {
        if !root.join("l1").join(cell).join("loader").is_file() {
            return Err(ChassisError::Check(format!("missing L1 cell {cell}")));
        }
    }
    Ok(())
}

pub fn inspect(root: &Path) -> Result<serde_json::Value, ChassisError> {
    check_layout(root)?;
    let abi = load_abi(&root.join("l2/host-abi.json"))?;
    let app = load_app(&root.join("l3/app.json"))?;
    Ok(serde_json::json!({
        "native_cell": native_cell(),
        "l1_cells": CELLS,
        "l2_capabilities": abi.capabilities.iter().map(|cap| &cap.id).collect::<Vec<_>>(),
        "l3_name": app.name,
        "l3_capabilities": app.capabilities,
        "compile": false,
    }))
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), ChassisError> {
    for file in walk_files(from)? {
        let rel = file
            .strip_prefix(from)
            .map_err(|err| ChassisError::Check(err.to_string()))?;
        let dest = to.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&file, &dest)?;
    }
    Ok(())
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>, ChassisError> {
    let mut out = Vec::new();
    visit(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn visit(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), ChassisError> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_ok_layout(root: &Path) {
        for (i, cell) in CELLS.iter().enumerate() {
            let dir = root.join("l1").join(cell);
            fs::create_dir_all(&dir).expect("l1 cell");
            fs::write(dir.join("loader"), format!("frozen-{cell}-{i}")).expect("loader");
        }
        fs::create_dir_all(root.join("l2")).expect("l2");
        fs::write(
            root.join("l2/host-abi.json"),
            include_str!("../l2/host-abi.json"),
        )
        .expect("abi");
        fs::create_dir_all(root.join("l3")).expect("l3");
        fs::write(
            root.join("l3/app.json"),
            include_str!("../l3/example-app.json"),
        )
        .expect("app");
    }

    #[test]
    fn bundled_l3_is_subset_of_bundled_l2() {
        let abi = bundled_l2_abi().expect("abi");
        let app = bundled_l3_app().expect("app");
        let allowed: std::collections::BTreeSet<_> =
            abi.capabilities.iter().map(|cap| cap.id.as_str()).collect();
        for name in app.capabilities {
            assert!(allowed.contains(name.as_str()), "{name}");
        }
    }

    #[test]
    fn compose_copies_l1_bytes_and_refuses_os_libraries_in_l3() {
        let tmp = std::env::temp_dir().join(format!("chassis-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("tmp");
        write_ok_layout(&tmp.join("from"));
        let image = compose(&tmp.join("from"), &tmp.join("out")).expect("compose");
        assert!(!image.compile);
        assert!(!image.invokes_cargo);
        for cell in CELLS {
            let src = fs::read(tmp.join("from/l1").join(cell).join("loader")).expect("src");
            let dst = fs::read(tmp.join("out/l1").join(cell).join("loader")).expect("dst");
            assert_eq!(src, dst);
        }
        fs::write(
            tmp.join("from/l3/app.json"),
            r#"{"schema":1,"name":"bad","capabilities":["tabs.list"],"note":"libSystem.B.dylib"}"#,
        )
        .expect("poison");
        let err = check_layout(&tmp.join("from")).expect_err("os lib");
        assert!(format!("{err}").contains("libSystem.B.dylib"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unknown_l3_capability_is_rejected() {
        let tmp = std::env::temp_dir().join(format!("chassis-unknown-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        write_ok_layout(&tmp);
        fs::write(
            tmp.join("l3/app.json"),
            r#"{"schema":1,"name":"bad","capabilities":["not.a.capability"]}"#,
        )
        .expect("write");
        let err = check_layout(&tmp).expect_err("unknown");
        assert!(format!("{err}").contains("not.a.capability"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn native_cell_is_one_of_the_six_or_unknown() {
        let cell = native_cell();
        assert!(CELLS.contains(&cell) || cell == "unknown");
    }
}
